//! HermesAgentRuntime — [`AgentRuntime`] backed by a real `hermes acp` child process.
//!
//! Spawns `hermes acp` over stdio, performs the ACP handshake (`initialize` →
//! `session/new`), and bridges [`AgentCommand`]s / [`AgentEvent`]s via newline-
//! delimited JSON-RPC (the transport `hermes acp` actually uses — see
//! `docs/reviews/acp-wire-spike.md` §"Verdict").
//!
//! # Wire transport
//!
//! Despite the ACP spec mentioning Content-Length framing, Hermes' ACP adapter
//! uses **newline-delimited JSON-RPC 2.0**: one compact JSON object per line,
//! terminated by `\n`. The spike confirmed this by reading the Python ACP
//! library source (`acp/sender.py:33`):
//! ```text
//! data = (json.dumps(payload, separators=(",", ":")) + "\n").encode("utf-8")
//! ```
//!
//! The [`NlFrame`] codec below implements this transport. The Content-Length
//! [`Frame`](crate::bridge::acp::Frame) codec in `acp.rs` remains available for
//! spec-compliant peers that prefer it.

use std::env;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::stream::Stream;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;
use tracing::debug;

use super::{AgentCommand, AgentEvent, AgentRuntime};
use crate::bridge::acp::{AcpId, AcpMessage, AcpNotification, AcpRequest};

// ---------------------------------------------------------------------------
// Newline-delimited JSON-RPC codec (the transport `hermes acp` actually uses)
// ---------------------------------------------------------------------------

/// Newline-delimited JSON-RPC frame codec.
///
/// Wire format: one compact JSON object per line, terminated by `\n`.
pub struct NlFrame;

impl NlFrame {
    /// Encode a JSON value into a newline-terminated byte buffer.
    pub fn encode_value(msg: &Value) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec(msg).context("serialize JSON-RPC")?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Encode a typed [`AcpMessage`] into a newline-terminated byte buffer.
    pub fn encode(msg: &AcpMessage) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec(msg).context("serialize JSON-RPC")?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Decode a single line (without trailing newline) into an [`AcpMessage`].
    ///
    /// Returns `None` for blank / whitespace-only lines (the receiver simply
    /// skips them rather than erroring).
    pub fn decode_line(line: &[u8]) -> Option<AcpMessage> {
        let trimmed = std::str::from_utf8(line).ok()?.trim();
        if trimmed.is_empty() {
            return None;
        }
        let msg: AcpMessage = serde_json::from_str(trimmed).ok()?;
        Some(msg)
    }
}

// ---------------------------------------------------------------------------
// Handshake request builders
// ---------------------------------------------------------------------------

/// Build the ACP `initialize` request.
pub fn build_initialize_request(id: AcpId) -> AcpRequest {
    AcpRequest {
        jsonrpc: "2.0".into(),
        id,
        method: "initialize".into(),
        params: json!({
            "protocolVersion": 1,
            "clientCapabilities": {
                "fs": {
                    "readTextFile": true,
                    "writeTextFile": true,
                }
            },
            "clientInfo": {
                "name": "olympus-control-plane",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }),
    }
}

/// Build the ACP `session/new` request.
pub fn build_session_new_request(cwd: &str, id: AcpId) -> AcpRequest {
    AcpRequest {
        jsonrpc: "2.0".into(),
        id,
        method: "session/new".into(),
        params: json!({
            "cwd": cwd,
            "mcpServers": [],
        }),
    }
}

/// Build the ACP `session/resume` request.
pub fn build_session_resume_request(session_id: &str, cwd: &str, id: AcpId) -> AcpRequest {
    AcpRequest {
        jsonrpc: "2.0".into(),
        id,
        method: "session/resume".into(),
        params: json!({
            "sessionId": session_id,
            "cwd": cwd,
            "mcpServers": [],
        }),
    }
}

/// Build the ACP `session/fork` request.
pub fn build_session_fork_request(session_id: &str, cwd: &str, id: AcpId) -> AcpRequest {
    AcpRequest {
        jsonrpc: "2.0".into(),
        id,
        method: "session/fork".into(),
        params: json!({
            "sessionId": session_id,
            "cwd": cwd,
            "mcpServers": [],
        }),
    }
}

// ---------------------------------------------------------------------------
// Message → AgentEvent mapping
// ---------------------------------------------------------------------------

/// Map any [`AcpMessage`] (as read from the wire) into an optional [`AgentEvent`].
///
/// - Notifications are mapped via [`AgentEvent::from_notification`].
/// - Responses are mapped via [`AgentEvent::from_response`].
/// - Requests (never sent by the agent) produce `None`.
pub fn map_message_to_event(msg: &AcpMessage) -> Option<AgentEvent> {
    match msg {
        AcpMessage::Notification(notif) => AgentEvent::from_notification(notif),
        AcpMessage::Response(resp) => AgentEvent::from_response(resp),
        AcpMessage::Request(_) => None,
    }
}

// ---------------------------------------------------------------------------
// HermesAgentRuntime
// ---------------------------------------------------------------------------

/// Internal state held behind a lock so `&self` trait methods work.
struct RuntimeState {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    session_id: Option<String>,
}

impl RuntimeState {
    fn new() -> Self {
        Self {
            child: None,
            stdin: None,
            session_id: None,
        }
    }
}

/// Configuration for [`HermesAgentRuntime`].
#[derive(Clone)]
pub struct HermesRuntimeConfig {
    /// Command to invoke `hermes acp` (default: `["hermes", "acp"]`).
    pub command: Vec<String>,
    /// Working directory for the child + ACP session cwd.
    pub cwd: String,
    /// If set, overrides `HERMES_ACP_SESSION_SOURCE` on the child.
    pub session_source: Option<String>,
    /// Channel capacity for the event stream (default 256).
    pub event_buffer: usize,
    /// How long `start()` waits for the `session/new` response (the ACP adapter
    /// can take several seconds to boot). Default 30s.
    pub start_timeout_secs: u64,
}

impl Default for HermesRuntimeConfig {
    fn default() -> Self {
        Self {
            command: vec!["hermes".into(), "acp".into()],
            cwd: env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".into()),
            session_source: Some("olympus".into()),
            event_buffer: 256,
            start_timeout_secs: 30,
        }
    }
}

/// An [`AgentRuntime`] backed by a real `hermes acp` child process.
///
/// Spawns the agent over stdio, performs the ACP handshake, and bridges
/// [`AgentCommand`]s to JSON-RPC requests/notifications. Events from the agent
/// arrive as a [`Stream`] of [`AgentEvent`].
pub struct HermesAgentRuntime {
    config: HermesRuntimeConfig,
    state: Mutex<RuntimeState>,
    next_id: AtomicI64,
    event_tx: tokio::sync::broadcast::Sender<AgentEvent>,
    /// Active ACP session id, shared with the stdout reader task so it can
    /// capture the id from the `session/new` response while `send()` reads it.
    session_id_shared: Arc<Mutex<Option<String>>>,
}

impl HermesAgentRuntime {
    /// Create a new runtime (Arc-wrapped, the common case).
    pub fn new_arc(config: HermesRuntimeConfig) -> Arc<Self> {
        // broadcast (not mpsc): each `events()` call subscribes and gets its
        // own fresh receiver, so multiple turns on the same runtime each see
        // the turn's events. The old mpsc take-once receiver was consumed by
        // the first turn's drain loop, leaving subsequent turns with an empty
        // stream and silently dropping their assistant reply.
        let (tx, _rx) = tokio::sync::broadcast::channel(config.event_buffer);
        Arc::new(Self {
            config,
            state: Mutex::new(RuntimeState::new()),
            next_id: AtomicI64::new(1),
            event_tx: tx,
            session_id_shared: Arc::new(Mutex::new(None)),
        })
    }

    fn alloc_id(&self) -> AcpId {
        AcpId::from(self.next_id.fetch_add(1, Ordering::SeqCst))
    }

    /// Write a raw JSON-RPC message to the child's stdin (newline-delimited).
    async fn write_message(&self, msg: &AcpMessage) -> Result<()> {
        let bytes = NlFrame::encode(msg)?;
        let mut state = self.state.lock().await;
        let stdin = state
            .stdin
            .as_mut()
            .context("child stdin not open — was start() called?")?;
        stdin.write_all(&bytes).await?;
        stdin.flush().await?;
        Ok(())
    }

    /// Spawn the stdout reader task that parses JSON-RPC lines, captures the
    /// session id from `session/new`/`session/resume` responses, and pushes
    /// [`AgentEvent`]s into the channel. Holds only the pieces it needs (tx +
    /// the shared session-id cell), so it does not pin the whole runtime alive.
    fn spawn_reader(
        stdout: ChildStdout,
        tx: tokio::sync::broadcast::Sender<AgentEvent>,
        session_id_shared: Arc<Mutex<Option<String>>>,
    ) {
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                debug!(target: "olympus.bridge.hermes", line = %line, "ACP recv");
                let Some(msg) = NlFrame::decode_line(line.as_bytes()) else {
                    continue;
                };
                // Capture session id from session/new|resume responses.
                if let AcpMessage::Response(resp) = &msg {
                    if let Some(sid) = resp.result.get("sessionId").and_then(|v| v.as_str()) {
                        *session_id_shared.lock().await = Some(sid.to_string());
                        debug!(target: "olympus.bridge.hermes", session_id = %sid, "captured session id");
                    }
                }
                if let Some(event) = map_message_to_event(&msg) {
                    // broadcast::send is synchronous; Err means no receivers
                    // are subscribed (the drain task may have ended). That's
                    // fine — we keep reading so a later turn's subscriber sees
                    // its events. We do NOT break on Err (that would stop the
                    // reader and kill all future turns).
                    let _ = tx.send(event);
                }
            }
            debug!(target: "olympus.bridge.hermes", "ACP stdout reader closed");
        });
    }

    /// Get the active session ID, or error if not started.
    /// Prefers the id captured by the reader from the `session/new` response;
    /// falls back to an explicitly-provided resume id stored in state.
    async fn session_id_or_default(&self) -> Result<String> {
        if let Some(sid) = self.session_id_shared.lock().await.clone() {
            return Ok(sid);
        }
        let state = self.state.lock().await;
        Ok(state.session_id.clone().unwrap_or_default())
    }
}

#[async_trait::async_trait]
impl AgentRuntime for HermesAgentRuntime {
    async fn start(&self, session_id: Option<&str>) -> Result<()> {
        let mut state = self.state.lock().await;

        if state.child.is_some() {
            anyhow::bail!("runtime already started");
        }

        // Build the child command with env
        let mut cmd = tokio::process::Command::new(&self.config.command[0]);
        cmd.args(&self.config.command[1..]);
        cmd.current_dir(&self.config.cwd);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit()); // logging goes to our stderr
        if let Some(source) = &self.config.session_source {
            cmd.env("HERMES_ACP_SESSION_SOURCE", source);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawning {:?}", self.config.command))?;
        let stdin = child
            .stdin
            .take()
            .context("child stdin pipe was not captured")?;
        let stdout = child
            .stdout
            .take()
            .context("child stdout pipe was not captured")?;

        // Store session_id for resume
        if let Some(sid) = session_id {
            state.session_id = Some(sid.to_string());
            *self.session_id_shared.lock().await = Some(sid.to_string());
        }
        state.child = Some(child);
        state.stdin = Some(stdin);
        drop(state); // release before we call write_message

        // Spawn the stdout reader so streamed session/update events flow into
        // events() and the session id from session/new is captured.
        Self::spawn_reader(
            stdout,
            self.event_tx.clone(),
            Arc::clone(&self.session_id_shared),
        );

        // --- ACP handshake: initialize ---
        let init_req = build_initialize_request(self.alloc_id());
        debug!(target: "olympus.bridge.hermes", method = %init_req.method, "ACP send");
        self.write_message(&AcpMessage::Request(init_req)).await?;

        // --- session/new or session/resume ---
        let req = if let Some(sid) = session_id {
            build_session_resume_request(sid, &self.config.cwd, self.alloc_id())
        } else {
            build_session_new_request(&self.config.cwd, self.alloc_id())
        };
        debug!(target: "olympus.bridge.hermes", method = %req.method, "ACP send");
        self.write_message(&AcpMessage::Request(req)).await?;

        // Wait for the reader to capture the session id from the session/new
        // (or session/resume) response before returning, so a caller can
        // immediately send() without racing the handshake. The ACP adapter can
        // take a few seconds to boot (loads .env, MCP servers) before it replies.
        let deadline =
            std::time::Instant::now() + Duration::from_secs(self.config.start_timeout_secs);
        loop {
            if self.session_id_shared.lock().await.is_some() {
                break;
            }
            if std::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "timed out after {}s waiting for ACP session/new response",
                    self.config.start_timeout_secs
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Ok(())
    }

    async fn fork_session(&self, session_id: &str) -> Result<()> {
        let mut state = self.state.lock().await;

        if state.child.is_some() {
            anyhow::bail!("runtime already started");
        }

        let mut cmd = tokio::process::Command::new(&self.config.command[0]);
        cmd.args(&self.config.command[1..]);
        cmd.current_dir(&self.config.cwd);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit());
        if let Some(source) = &self.config.session_source {
            cmd.env("HERMES_ACP_SESSION_SOURCE", source);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawning {:?}", self.config.command))?;
        let stdin = child
            .stdin
            .take()
            .context("child stdin pipe was not captured")?;
        let stdout = child
            .stdout
            .take()
            .context("child stdout pipe was not captured")?;

        *self.session_id_shared.lock().await = None;
        state.child = Some(child);
        state.stdin = Some(stdin);
        drop(state);

        Self::spawn_reader(
            stdout,
            self.event_tx.clone(),
            Arc::clone(&self.session_id_shared),
        );

        let init_req = build_initialize_request(self.alloc_id());
        debug!(target: "olympus.bridge.hermes", method = %init_req.method, "ACP send");
        self.write_message(&AcpMessage::Request(init_req)).await?;

        let fork_req = build_session_fork_request(session_id, &self.config.cwd, self.alloc_id());
        debug!(target: "olympus.bridge.hermes", method = %fork_req.method, "ACP send");
        self.write_message(&AcpMessage::Request(fork_req)).await?;

        let deadline =
            std::time::Instant::now() + Duration::from_secs(self.config.start_timeout_secs);
        loop {
            if let Some(sid) = self.session_id_shared.lock().await.clone() {
                self.state.lock().await.session_id = Some(sid);
                break;
            }
            if std::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "timed out after {}s waiting for ACP session/fork response",
                    self.config.start_timeout_secs
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Ok(())
    }

    async fn send(&self, cmd: AgentCommand) -> Result<()> {
        match &cmd {
            AgentCommand::Stop => {
                self.stop().await?;
            }
            AgentCommand::Cancel => {
                let session_id = self.session_id_or_default().await?;
                if session_id.is_empty() {
                    anyhow::bail!("no active session — was start() called?");
                }
                let notif = AcpNotification::from_command(&cmd, &session_id)?;
                let msg = AcpMessage::Notification(notif);
                self.write_message(&msg).await?;
            }
            AgentCommand::Prompt {
                model: Some(model), ..
            } => {
                let session_id = self.session_id_or_default().await?;
                if session_id.is_empty() {
                    anyhow::bail!("no active session — was start() called?");
                }
                let switch = AgentCommand::SwitchModel {
                    model: model.clone(),
                };
                let switch_req = AcpRequest::from_command(&switch, &session_id, self.alloc_id())?;
                self.write_message(&AcpMessage::Request(switch_req)).await?;
                let req = AcpRequest::from_command(&cmd, &session_id, self.alloc_id())?;
                self.write_message(&AcpMessage::Request(req)).await?;
            }
            _ => {
                let session_id = self.session_id_or_default().await?;
                if session_id.is_empty() {
                    anyhow::bail!("no active session — was start() called?");
                }
                let req = AcpRequest::from_command(&cmd, &session_id, self.alloc_id())?;
                self.write_message(&AcpMessage::Request(req)).await?;
            }
        }
        Ok(())
    }

    fn events(&self) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send>> {
        // Each call subscribes and gets its OWN fresh receiver, so every turn's
        // drain loop sees that turn's events. (broadcast, not the old take-once
        // mpsc receiver that was consumed by the first turn.) BroadcastStream
        // yields Result<Item, Lagged> — we drop lag errors, keeping only Ok.
        use tokio_stream::StreamExt as _;
        Box::pin(
            tokio_stream::wrappers::BroadcastStream::new(self.event_tx.subscribe())
                .filter_map(|res| res.ok()),
        )
    }

    async fn stop(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        if let Some(mut stdin) = state.stdin.take() {
            let _ = stdin.shutdown().await;
        }
        if let Some(mut child) = state.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        state.session_id = None;
        Ok(())
    }

    async fn hermes_session_id(&self) -> Option<String> {
        self.session_id_shared.lock().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::acp::{AcpMessage, AcpResponse};
    use serde_json::json;

    // ---- Newline-framed codec ----

    #[test]
    fn newline_encode_produces_json_plus_newline() {
        let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
        let bytes = NlFrame::encode_value(&msg).expect("encode");
        assert!(bytes.ends_with(b"\n"));
        // body before \n must be valid JSON
        let body = std::str::from_utf8(&bytes[..bytes.len() - 1]).expect("utf8");
        let parsed: serde_json::Value = serde_json::from_str(body).expect("parse");
        assert_eq!(parsed["method"], "initialize");
    }

    #[test]
    fn newline_decode_parses_a_single_json_line() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#;
        let msg = NlFrame::decode_line(line.as_bytes()).expect("decode");
        assert!(matches!(msg, AcpMessage::Notification(_)));
    }

    #[test]
    fn newline_decode_rejects_blank_line() {
        let msg = NlFrame::decode_line(b"   \n");
        assert!(msg.is_none(), "blank line should produce no message");
    }

    // ---- Handshake request builders ----

    #[test]
    fn initialize_request_has_correct_shape() {
        let req = build_initialize_request(AcpId::from(1));
        assert_eq!(req.method, "initialize");
        assert_eq!(req.params["protocolVersion"], 1);
        assert_eq!(
            req.params["clientCapabilities"]["fs"]["readTextFile"],
            serde_json::Value::Bool(true)
        );
        assert!(req.params["clientInfo"]["name"].is_string());
    }

    #[test]
    fn session_new_request_includes_cwd() {
        let req = build_session_new_request("/tmp/work", AcpId::from(2));
        assert_eq!(req.method, "session/new");
        assert_eq!(req.params["cwd"], "/tmp/work");
    }

    // ---- Event mapping from raw JSON-RPC messages ----

    #[test]
    fn agent_message_chunk_line_maps_to_text_event() {
        let notif = spike_chunk_notif("P");
        let msg = AcpMessage::Notification(notif);
        let event = map_message_to_event(&msg);
        assert_eq!(event, Some(AgentEvent::Text("P".into())));
    }

    #[test]
    fn final_response_line_maps_to_done_event() {
        let resp = AcpResponse {
            jsonrpc: "2.0".into(),
            id: 3.into(),
            result: json!({"stopReason": "end_turn"}),
            error: None,
        };
        let msg = AcpMessage::Response(resp);
        let event = map_message_to_event(&msg);
        assert_eq!(
            event,
            Some(AgentEvent::Done {
                finish_reason: Some("end_turn".into())
            })
        );
    }

    #[test]
    fn request_message_produces_no_event() {
        let req = crate::bridge::acp::AcpRequest {
            jsonrpc: "2.0".into(),
            id: 99.into(),
            method: "session/prompt".into(),
            params: json!({}),
        };
        let msg = AcpMessage::Request(req);
        let event = map_message_to_event(&msg);
        assert!(event.is_none());
    }

    // ---- helpers ----

    fn spike_chunk_notif(text: &str) -> crate::bridge::acp::AcpNotification {
        crate::bridge::acp::AcpNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: json!({
                "sessionId": "s-1",
                "update": {
                    "content": {"text": text, "type": "text"},
                    "sessionUpdate": "agent_message_chunk",
                },
            }),
        }
    }
}
