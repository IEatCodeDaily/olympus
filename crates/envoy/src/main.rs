//! Olympus Envoy — the runtime-holder binary (ADR 0008 S3).
//!
//! Connects to Hall's UDS socket, registers (hello with protocolVersion +
//! BuildVersion + discovered agents), heartbeats, and dispatches Hall session
//! frames (`ensure_runtime`, `prompt`, `steer`, `cancel`, `stop`,
//! `respond_permission`, `probe`) to a [`RuntimeTable`]. Agent events are
//! streamed back as `EnvoyFrame::Event` frames with per-session monotonic `seq`.
//!
//! Production spawns real agent children (`hermes acp` / claude-code-acp /
//! codex-acp); `--mock` swaps in an echo runtime so the UDS round-trip can be
//! exercised in CI without a real agent.
//!
//! Usage:
//!   olympus-envoy [--socket <path>] [--node-id <id>] [--mock]
//! Defaults: socket = `$OLYMPUS_CONTROL_SOCKET` or `~/.olympus/control.sock`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::StreamExt;
use olympus_envoy::{
    bridge::{AgentCommand, AgentEvent, AgentRuntime},
    discovery,
    mock_runtime::MockAgentRuntime,
    runtime_table::RuntimeTable,
};
use olympus_proto::{
    frames::{EnvoyFrame, HallFrame},
    runtime::RuntimeSpec,
    version::{BuildVersion, PROTOCOL_VERSION},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

/// Heartbeat interval.
const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    let socket = resolve_socket()?;
    let mock = flag_present("--mock");
    let node_id = arg_value("--node-id").unwrap_or_else(|| {
        std::env::var("OLYMPUS_NODE_ID").unwrap_or_else(|_| {
            hostname::get()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| "envoy".to_string())
        })
    });
    let hostname_val = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "localhost".to_string());

    tracing::info!(socket = %socket.display(), node = %node_id, mock, "olympus-envoy starting");

    let agents = discovery::discover_local_agents();
    tracing::info!(count = agents.len(), "discovered local agents");

    let table = if mock {
        RuntimeTable::with_factory(Arc::new(|_spec: &RuntimeSpec| {
            MockAgentRuntime::new_arc() as Arc<dyn AgentRuntime>
        }))
    } else {
        production_factory()
    };

    // Reconnect loops below: Hall restarts must NOT kill this envoy
    // (ADR 0008 §2). The runtime table (and its ACP children) survives across
    // reconnects; each new connection re-sends hello.
    let table = Arc::new(table);

    // Transport selection: `--hall iroh:<node-id>` connects via iroh (public
    // n0 relays, ADR 0008 §1 / S7); otherwise UDS (default local path).
    let hall = arg_value("--hall").or_else(|| std::env::var("OLYMPUS_HALL").ok());
    if let Some(target) = hall.as_deref().and_then(|h| h.strip_prefix("iroh:")) {
        let state_dir = envoy_state_dir(&node_id)?;
        let secret = olympus_envoy::transport::load_or_create_secret(&state_dir)?;
        let my_id = secret.public();
        tracing::info!(envoy_node_id = %my_id, hall = %target, "connecting to Hall via iroh");
        println!("envoy iroh node id: {my_id}  (add to hall.toml allowed_envoys)");
        let endpoint = olympus_envoy::transport::bind_endpoint(secret).await?;
        loop {
            match olympus_envoy::transport::connect_to_hall(&endpoint, target).await {
                Ok((send, recv)) => {
                    tracing::info!("connected to Hall via iroh");
                    if let Err(e) = run_connection(
                        recv,
                        send,
                        table.clone(),
                        &node_id,
                        &hostname_val,
                        agents.clone(),
                    )
                    .await
                    {
                        tracing::warn!(
                            error = format!("{e:#}"),
                            "iroh connection ended with error"
                        );
                    }
                    tracing::warn!("Hall iroh connection lost; reconnecting…");
                }
                Err(e) => {
                    tracing::warn!(error = format!("{e:#}"), "iroh connect failed, retrying…");
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }

    let stream = connect_with_retry(&socket).await?;
    tracing::info!("connected to Hall UDS");

    let mut stream = Some(stream);
    loop {
        let s = match stream.take() {
            Some(s) => s,
            None => {
                let s = connect_forever(&socket).await;
                tracing::info!("reconnected to Hall UDS");
                s
            }
        };
        let (reader, writer) = s.into_split();
        if let Err(e) = run_connection(
            reader,
            writer,
            table.clone(),
            &node_id,
            &hostname_val,
            agents.clone(),
        )
        .await
        {
            tracing::warn!(error = format!("{e:#}"), "connection ended with error");
        }
        tracing::warn!("Hall connection lost; reconnecting…");
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// Per-envoy state dir (iroh key lives here): ~/.olympus/envoy/<node-id>/.
fn envoy_state_dir(node_id: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join(".olympus")
        .join("envoy")
        .join(node_id))
}

/// Type-erased writer — UDS write half locally, iroh QUIC SendStream remotely.
type BoxedWriter = Box<dyn tokio::io::AsyncWrite + Send + Unpin>;

/// Shared connection state: the writer (mutex-guarded), the runtime table, and
/// per-session counters.
struct Conn {
    writer: Mutex<BufWriter<BoxedWriter>>,
    table: Arc<RuntimeTable>,
    seq: Mutex<HashMap<String, u64>>,
    turn: Mutex<HashMap<String, u64>>,
}

impl Conn {
    async fn send_frame(&self, frame: &EnvoyFrame) -> Result<()> {
        let json = serde_json::to_string(frame).context("serializing envoy frame")?;
        let mut w = self.writer.lock().await;
        w.write_all(json.as_bytes()).await?;
        w.write_all(b"\n").await?;
        w.flush().await?;
        Ok(())
    }

    /// Next per-session event seq (starts at 0).
    async fn next_seq(&self, session_id: &str) -> u64 {
        let mut m = self.seq.lock().await;
        let v = m.entry(session_id.to_string()).or_insert(0);
        let current = *v;
        *v += 1;
        current
    }

    /// Next per-session turn id (monotonic string).
    async fn next_turn(&self, session_id: &str) -> String {
        let mut m = self.turn.lock().await;
        let v = m.entry(session_id.to_string()).or_insert(0);
        *v += 1;
        format!("turn-{}", v)
    }

    /// Send a `resp` frame.
    async fn send_resp(&self, req_id: u64, ok: bool, error: Option<&str>) {
        let frame = EnvoyFrame::Resp {
            req_id,
            ok,
            error: error.map(String::from),
            result: None,
        };
        if let Err(e) = self.send_frame(&frame).await {
            tracing::error!(error = %e, "failed to send resp frame");
        }
    }

    /// Send a `resp` frame with a result payload.
    async fn send_resp_result(&self, req_id: u64, result: serde_json::Value) {
        let frame = EnvoyFrame::Resp {
            req_id,
            ok: true,
            error: None,
            result: Some(result),
        };
        if let Err(e) = self.send_frame(&frame).await {
            tracing::error!(error = %e, "failed to send resp frame");
        }
    }
}

/// Run the full connection lifecycle: hello → heartbeat loop + read loop.
async fn run_connection<R, W>(
    reader: R,
    writer: W,
    table: Arc<RuntimeTable>,
    node_id: &str,
    hostname: &str,
    agents: Vec<discovery::AgentInfo>,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
    W: tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    let conn = Arc::new(Conn {
        writer: Mutex::new(BufWriter::new(Box::new(writer) as BoxedWriter)),
        table,
        seq: Mutex::new(HashMap::new()),
        turn: Mutex::new(HashMap::new()),
    });

    // Hello handshake.
    let agents_json = serde_json::to_value(&agents).unwrap_or_default();
    let hello = EnvoyFrame::Hello {
        node_id: node_id.to_string(),
        hostname: hostname.to_string(),
        slots_total: 4,
        protocol_version: PROTOCOL_VERSION,
        version: BuildVersion::for_binary(env!("CARGO_PKG_VERSION")),
        agents: Some(agents_json),
        runtimes: Vec::new(),
    };
    conn.send_frame(&hello).await?;
    tracing::info!("hello sent");

    // Heartbeat loop — store handle so we can abort it when the read loop exits.
    let hb_handle = {
        let hb_conn = conn.clone();
        let hb_node = node_id.to_string();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(HEARTBEAT_INTERVAL).await;
                let hb = EnvoyFrame::Heartbeat {
                    node_id: hb_node.clone(),
                    slots_used: 0,
                };
                if hb_conn.send_frame(&hb).await.is_err() {
                    break;
                }
            }
        })
    };

    // Read loop — dispatch HallFrames.
    let mut lines = BufReader::new(reader).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(l)) => l,
            Ok(None) => {
                tracing::info!("Hall disconnected");
                break;
            }
            Err(e) => {
                tracing::error!(error = %e, "read error");
                break;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let frame: HallFrame = match serde_json::from_str(line) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(error = %e, "unparseable HallFrame, skipping");
                continue;
            }
        };

        let conn2 = conn.clone();
        tokio::spawn(async move {
            if let Err(e) = dispatch_frame(conn2, frame).await {
                tracing::error!(error = %e, "frame dispatch error");
            }
        });
    }

    // Read loop exited (Hall disconnected or error). Abort the heartbeat task
    // so it doesn't leak holding an Arc<Conn> clone across reconnect cycles.
    hb_handle.abort();

    Ok(())
}

/// Dispatch a single HallFrame.
async fn dispatch_frame(conn: Arc<Conn>, frame: HallFrame) -> Result<()> {
    match frame {
        HallFrame::EnsureRuntime {
            req_id,
            session_id,
            spec,
            resume_id,
        } => {
            let result = conn
                .table
                .ensure_runtime(&session_id, &spec, resume_id.as_deref())
                .await;
            match result {
                Ok((_rt, hermes_id)) => {
                    conn.send_resp_result(req_id, serde_json::json!({ "hermesId": hermes_id }))
                        .await;
                }
                Err(e) => {
                    conn.send_resp(req_id, false, Some(&format!("{e:#}"))).await;
                }
            }
        }
        HallFrame::Prompt {
            req_id,
            session_id,
            text,
            model,
        } => {
            let outcome =
                send_and_stream(&conn, &session_id, AgentCommand::Prompt { text, model }).await;
            match outcome {
                Ok(()) => conn.send_resp(req_id, true, None).await,
                Err(e) => conn.send_resp(req_id, false, Some(&format!("{e:#}"))).await,
            }
        }
        HallFrame::Steer {
            req_id,
            session_id,
            text,
        } => {
            let outcome = send_and_stream(&conn, &session_id, AgentCommand::Steer { text }).await;
            match outcome {
                Ok(()) => conn.send_resp(req_id, true, None).await,
                Err(e) => conn.send_resp(req_id, false, Some(&format!("{e:#}"))).await,
            }
        }
        HallFrame::Cancel { req_id, session_id } => {
            // Cancel does not start a new event stream; just forward + ack.
            let outcome = conn.table.send(&session_id, AgentCommand::Cancel).await;
            match outcome {
                Ok(()) => conn.send_resp(req_id, true, None).await,
                Err(e) => conn.send_resp(req_id, false, Some(&format!("{e:#}"))).await,
            }
        }
        HallFrame::Stop { req_id, session_id } => {
            let outcome = conn.table.stop(&session_id).await;
            match outcome {
                Ok(()) => conn.send_resp(req_id, true, None).await,
                Err(e) => conn.send_resp(req_id, false, Some(&format!("{e:#}"))).await,
            }
        }
        HallFrame::RespondPermission {
            req_id,
            session_id,
            request_id,
            option_id,
        } => {
            let runtime = conn.table.get(&session_id).await;
            match runtime {
                Some(rt) => {
                    let outcome = rt
                        .respond_permission(&request_id, option_id.as_deref())
                        .await;
                    match outcome {
                        Ok(()) => conn.send_resp(req_id, true, None).await,
                        Err(e) => conn.send_resp(req_id, false, Some(&format!("{e:#}"))).await,
                    }
                }
                None => {
                    conn.send_resp(req_id, false, Some("no runtime for session"))
                        .await;
                }
            }
        }
        HallFrame::Drain { req_id, .. } => {
            // S4 implements the drain state machine; S3 acknowledges.
            tracing::info!("drain requested (S4 will implement handover)");
            conn.send_resp(req_id, true, None).await;
        }
        HallFrame::Probe { req_id } => {
            let agents = discovery::discover_local_agents();
            let result = serde_json::json!({ "agents": agents });
            conn.send_resp_result(req_id, result).await;
        }
        HallFrame::Ack { session_id, seq } => {
            tracing::debug!(session = %session_id, seq, "ack from Hall (spool truncation — S4)");
        }
        HallFrame::ResumeFrom { session_id, seq } => {
            tracing::debug!(session = %session_id, seq, "resume_from from Hall (replay — S4)");
        }
    }
    Ok(())
}
/// Send a command to a session's runtime and drain its event stream into
/// `EnvoyFrame::Event` frames with per-session monotonic seq.
async fn send_and_stream(conn: &Conn, session_id: &str, cmd: AgentCommand) -> Result<()> {
    let runtime = conn
        .table
        .get(session_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("no runtime for session"))?;

    // Assign the turn id before sending so events are grouped.
    let _ = conn.next_turn(session_id).await;

    // Subscribe BEFORE sending so fast runtimes cannot emit and finish the
    // whole turn before the drain loop is listening (broadcast only delivers
    // to existing subscribers). Mirrors the monolith's post_message ordering.
    let mut events = runtime.events();

    runtime
        .send(cmd)
        .await
        .context("sending command to runtime")?;

    // Drain the event stream, forwarding each as an EnvoyFrame::Event.
    // Break on terminal events (Done/Error) — the broadcast channel is never
    // closed (it lives for the runtime's lifetime), so without a break this
    // loop hangs forever after the turn completes.
    while let Some(event) = events.next().await {
        let turn_id = conn.next_turn_id_for_event(session_id).await;
        let seq = conn.next_seq(session_id).await;
        let frame = EnvoyFrame::Event {
            session_id: session_id.to_string(),
            turn_id,
            seq,
            payload: event.clone(),
        };
        if let Err(e) = conn.send_frame(&frame).await {
            tracing::error!(error = %e, "failed to send event frame");
            return Err(e);
        }
        // Terminal events end the drain — the broadcast stream itself never ends.
        if matches!(&event, AgentEvent::Done { .. } | AgentEvent::Error(_)) {
            break;
        }
    }

    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────

impl Conn {
    /// The turn id to stamp on event frames for the current turn. We reuse the
    /// turn counter's current value (assigned in send_and_stream) so all events
    /// within one turn share the same turn id.
    async fn next_turn_id_for_event(&self, session_id: &str) -> String {
        let m = self.turn.lock().await;
        m.get(session_id)
            .map(|v| format!("turn-{v}"))
            .unwrap_or_else(|| "turn-0".to_string())
    }
}

/// Resolve the Hall UDS socket path: `--socket` arg → `OLYMPUS_CONTROL_SOCKET`
/// → `~/.olympus/control.sock`.
fn resolve_socket() -> Result<PathBuf> {
    if let Some(p) = arg_value("--socket") {
        return Ok(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("OLYMPUS_CONTROL_SOCKET") {
        return Ok(PathBuf::from(p));
    }
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".olympus").join("control.sock"))
}

/// Connect to the UDS socket, retrying forever (used by the reconnect loop —
/// a downed Hall must never terminate a running envoy, ADR 0008 §2).
async fn connect_forever(path: &PathBuf) -> UnixStream {
    loop {
        match UnixStream::connect(path).await {
            Ok(s) => return s,
            Err(e) => {
                tracing::warn!(error = %e, path = %path.display(), "reconnect failed, retrying…");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }
}

async fn connect_with_retry(path: &PathBuf) -> Result<UnixStream> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match UnixStream::connect(path).await {
            Ok(s) => return Ok(s),
            Err(e) if std::time::Instant::now() < deadline => {
                tracing::warn!(error = %e, path = %path.display(), "connect failed, retrying…");
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Err(e) => return Err(e).with_context(|| format!("connecting to {}", path.display())),
        }
    }
}

/// Production runtime factory: spawns a real agent child via the ACP bridge.
fn production_factory() -> RuntimeTable {
    use olympus_envoy::bridge::hermes::{
        acp_command_for_agent, acp_framing_for_agent, HermesAgentRuntime, HermesRuntimeConfig,
    };

    RuntimeTable::with_factory(Arc::new(|spec: &RuntimeSpec| {
        let cwd = spec
            .cwd
            .as_deref()
            .filter(|c| !c.is_empty())
            .map(String::from)
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| ".".into())
            });
        let env = spec.env.clone();
        let command = acp_command_for_agent(spec.agent.as_deref());
        let framing = acp_framing_for_agent(spec.agent.as_deref());
        let config = HermesRuntimeConfig {
            command,
            cwd,
            session_source: Some("olympus".into()),
            event_buffer: 256,
            start_timeout_secs: 30,
            mcp_servers: spec.mcp_servers.clone(),
            env,
            framing,
        };
        HermesAgentRuntime::new_arc(config) as Arc<dyn AgentRuntime>
    }))
}

/// Check if a boolean flag is present in argv.
fn flag_present(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

/// Extract the value of a `--key value` arg from argv.
fn arg_value(key: &str) -> Option<String> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == key {
            return args.next();
        }
    }
    None
}
