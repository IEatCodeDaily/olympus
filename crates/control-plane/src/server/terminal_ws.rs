//! Operator terminal WebSocket (ADR 0021 cockpit).
//!
//! A DEDICATED operator channel — deliberately NOT the `/ws` delta firehose —
//! so raw shell bytes never enter the session event plane, the debug ring, or
//! any client that isn't the operator driving this specific terminal.
//!
//! `GET /ws/operator/terminals/:terminalId?node=<nodeId>` upgrades to a
//! bidirectional byte relay for one operator shell:
//!
//! - target `node == "hall"` (default): a shell on the Hall host, run by the
//!   Hall-local [`HallTerminals`] manager (Hall has no EnvoyConnection to
//!   itself). Reuses the node-agnostic `olympus_envoy::pty::PtyManager`.
//! - target `node == <envoy node id>`: Hall sends `TerminalOpen` to that
//!   node's [`EnvoyConnection`], subscribes to its terminal channel, and relays
//!   frames both ways.
//!
//! Client→server WS text frames (JSON): `{kind:"input",dataB64}`,
//! `{kind:"resize",cols,rows}`. Server→client: `{kind:"output",dataB64}`,
//! `{kind:"exited",exitCode}`, `{kind:"attached",persistent:bool}`.
//!
//! WebSocket close with code 1000/1001 (normal/going-away) detaches the PTY
//! reader but leaves the tmux session alive for reattach. Close with code 4000
//! (explicit close) kills the tmux session permanently.
//!
//! This is operator-only: the route sits behind the same auth gate as `/ws`
//! and is never exposed to an agent runtime.

use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::envoy_conn::TerminalFrame;
use super::AppState;
use olympus_envoy::pty::{b64_encode, ChannelSink, PtyManager, TerminalMsg};

/// Hall-local terminals: a `PtyManager` for shells on the Hall host, plus the
/// receiver drained per-attachment. Because `PtyManager`'s sink is fixed at
/// construction, `HallTerminals` builds one manager whose output is fanned out
/// to per-terminal subscribers via broadcast channels (mirrors the envoy
/// connection's terminal_channels).
pub struct HallTerminals {
    mgr: Arc<PtyManager>,
    channels: std::sync::Mutex<
        std::collections::HashMap<String, tokio::sync::broadcast::Sender<TerminalFrame>>,
    >,
    /// The sink receiver, taken by the fan-out task on first use. Held here so
    /// construction stays runtime-free (no `tokio::spawn` in `new`) — a
    /// synchronous test can build AppState without a Tokio reactor.
    rx: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<TerminalMsg>>>,
    fan_started: std::sync::atomic::AtomicBool,
}

impl HallTerminals {
    pub fn new() -> Arc<Self> {
        let (sink, rx) = ChannelSink::new();
        Arc::new(Self {
            mgr: PtyManager::new(sink),
            channels: std::sync::Mutex::new(std::collections::HashMap::new()),
            rx: std::sync::Mutex::new(Some(rx)),
            fan_started: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Start the background fan-out task that routes PTY output from the shared
    /// sink to per-terminal broadcast channels. Idempotent; must be called from
    /// an async (Tokio) context — always true on the operator WS path.
    fn ensure_fan(self: &Arc<Self>) {
        use std::sync::atomic::Ordering;
        if self.fan_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(mut rx) = self.rx.lock().unwrap().take() else {
            return;
        };
        let fan = self.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let (tid, frame) = match msg {
                    TerminalMsg::Output {
                        terminal_id,
                        data_b64,
                    } => (terminal_id, TerminalFrame::Output { data_b64 }),
                    TerminalMsg::Exited {
                        terminal_id,
                        exit_code,
                    } => (terminal_id, TerminalFrame::Exited { exit_code }),
                };
                let tx = fan.channels.lock().unwrap().get(&tid).cloned();
                if let Some(tx) = tx {
                    let _ = tx.send(frame);
                }
            }
        });
    }

    fn subscribe(self: &Arc<Self>, terminal_id: &str) -> tokio::sync::broadcast::Receiver<TerminalFrame> {
        self.ensure_fan();
        let mut channels = self.channels.lock().unwrap();
        channels
            .entry(terminal_id.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel::<TerminalFrame>(1024).0)
            .subscribe()
    }

    fn drop_terminal(&self, terminal_id: &str) {
        self.channels.lock().unwrap().remove(terminal_id);
    }

    pub fn manager(&self) -> Arc<PtyManager> {
        self.mgr.clone()
    }
}

#[derive(Debug, Deserialize)]
pub struct TerminalQuery {
    /// Target node id; `"hall"` (default) runs on the Hall host.
    #[serde(default)]
    node: Option<String>,
    /// Token for browsers that cannot set the WS Authorization header.
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    cols: Option<u16>,
    #[serde(default)]
    rows: Option<u16>,
}

/// Client→server terminal control frames.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ClientTerm {
    #[serde(rename_all = "camelCase")]
    Input { data_b64: String },
    Resize { cols: u16, rows: u16 },
}

/// Custom close code: client signals "I'm closing this tab permanently — kill
/// the session." Normal close (1000/1001) just detaches.
const CLOSE_CODE_EXPLICIT: u16 = 4000;

/// `GET /ws/operator/terminals/:terminalId`.
pub async fn terminal_ws_handler(
    State(state): State<AppState>,
    Path(terminal_id): Path<String>,
    Query(q): Query<TerminalQuery>,
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !super::ws::authorize_operator(&state, q.token.as_deref(), &headers) {
        return (axum::http::StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let node = q.node.clone().unwrap_or_else(|| "hall".to_string());
    let cols = q.cols.unwrap_or(80);
    let rows = q.rows.unwrap_or(24);
    ws.on_upgrade(move |socket| relay(socket, state, terminal_id, node, cols, rows))
}

async fn relay(
    mut socket: WebSocket,
    state: AppState,
    terminal_id: String,
    node: String,
    cols: u16,
    rows: u16,
) {
    // Subscribe to output BEFORE opening so no bytes are missed.
    let mut output_rx = if node == "hall" {
        state.hall_pty.subscribe(&terminal_id)
    } else {
        match state.envoy_conns.get(&node).await {
            Some(conn) => conn.subscribe_terminal(&terminal_id),
            None => {
                let _ = socket
                    .send(Message::Text(
                        format!("{{\"kind\":\"exited\",\"error\":\"node {node} not connected\"}}")
                            .into(),
                    ))
                    .await;
                return;
            }
        }
    };

    // Open the shell (or re-attach to existing tmux session).
    let persistent = if node == "hall" {
        match state.hall_pty.manager().open(&terminal_id, cols, rows, None).await {
            Ok(p) => p,
            Err(_) => {
                let _ = socket
                    .send(Message::Text(
                        "{\"kind\":\"exited\",\"error\":\"open failed\"}".into(),
                    ))
                    .await;
                return;
            }
        }
    } else {
        match state.envoy_conns.get(&node).await {
            Some(conn) => {
                match conn
                    .send_request(olympus_proto::frames::HallFrame::TerminalOpen {
                        req_id: 0,
                        terminal_id: terminal_id.clone(),
                        cols,
                        rows,
                        cwd: None,
                    })
                    .await
                {
                    Ok(_) => true, // envoy persistence is its own concern
                    Err(_) => {
                        let _ = socket
                            .send(Message::Text(
                                "{\"kind\":\"exited\",\"error\":\"open failed\"}".into(),
                            ))
                            .await;
                        return;
                    }
                }
            }
            None => false,
        }
    };

    // Tell the client whether this session is persistent (tmux-backed).
    let _ = socket
        .send(Message::Text(
            format!("{{\"kind\":\"attached\",\"persistent\":{persistent}}}").into(),
        ))
        .await;

    // Bidirectional relay.
    let mut explicit_close = false;
    loop {
        tokio::select! {
            // PTY → browser.
            out = output_rx.recv() => {
                match out {
                    Ok(TerminalFrame::Output { data_b64 }) => {
                        let msg = format!("{{\"kind\":\"output\",\"dataB64\":\"{data_b64}\"}}");
                        if socket.send(Message::Text(msg.into())).await.is_err() { break; }
                    }
                    Ok(TerminalFrame::Exited { exit_code }) => {
                        let code = exit_code.map(|c| c.to_string()).unwrap_or_else(|| "null".into());
                        let _ = socket
                            .send(Message::Text(
                                format!("{{\"kind\":\"exited\",\"exitCode\":{code}}}").into(),
                            ))
                            .await;
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            // browser → PTY.
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientTerm>(&text) {
                            Ok(ClientTerm::Input { data_b64 }) => {
                                send_input(&state, &node, &terminal_id, &data_b64).await;
                            }
                            Ok(ClientTerm::Resize { cols, rows }) => {
                                send_resize(&state, &node, &terminal_id, cols, rows).await;
                            }
                            Err(_) => {}
                        }
                    }
                    Some(Ok(Message::Close(code))) => {
                        // Code 4000 = explicit tab close → kill session.
                        // None / 1000 / 1001 = normal disconnect → detach only.
                        if let Some(cf) = code {
                            if cf.code == CLOSE_CODE_EXPLICIT {
                                explicit_close = true;
                            }
                        }
                        break;
                    }
                    None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }

    // Socket closed. Explicit close → kill the session permanently.
    // Normal close → detach (tmux session stays alive for reattach).
    if explicit_close {
        close_terminal(&state, &node, &terminal_id).await;
    } else {
        detach_terminal(&state, &node, &terminal_id).await;
    }
    let _ = b64_encode(b""); // keep b64 import used if select above changes
}

async fn send_input(state: &AppState, node: &str, terminal_id: &str, data_b64: &str) {
    if node == "hall" {
        let _ = state.hall_pty.manager().input(terminal_id, data_b64).await;
    } else if let Some(conn) = state.envoy_conns.get(node).await {
        let _ = conn
            .send_request(olympus_proto::frames::HallFrame::TerminalInput {
                terminal_id: terminal_id.to_string(),
                data_b64: data_b64.to_string(),
            })
            .await;
    }
}

async fn send_resize(state: &AppState, node: &str, terminal_id: &str, cols: u16, rows: u16) {
    if node == "hall" {
        let _ = state.hall_pty.manager().resize(terminal_id, cols, rows).await;
    } else if let Some(conn) = state.envoy_conns.get(node).await {
        let _ = conn
            .send_request(olympus_proto::frames::HallFrame::TerminalResize {
                terminal_id: terminal_id.to_string(),
                cols,
                rows,
            })
            .await;
    }
}

/// Detach: abort the PTY reader but leave the tmux session alive.
async fn detach_terminal(state: &AppState, node: &str, terminal_id: &str) {
    if node == "hall" {
        let _ = state.hall_pty.manager().detach(terminal_id).await;
    }
    // For envoy nodes: Hall can't detach on the remote — the envoy's PtyManager
    // handles it when the TerminalOpen resp/reader EOFs. Just drop the channel.
    state.hall_pty.drop_terminal(terminal_id);
}

/// Close permanently: kill the tmux session / bare shell.
async fn close_terminal(state: &AppState, node: &str, terminal_id: &str) {
    if node == "hall" {
        let _ = state.hall_pty.manager().close(terminal_id).await;
        state.hall_pty.drop_terminal(terminal_id);
    } else if let Some(conn) = state.envoy_conns.get(node).await {
        let _ = conn
            .send_request(olympus_proto::frames::HallFrame::TerminalClose {
                terminal_id: terminal_id.to_string(),
            })
            .await;
        conn.drop_terminal(terminal_id);
    }
}
