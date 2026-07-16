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

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::StreamExt;
use olympus_envoy::{
    bridge::{AgentCommand, AgentEvent, AgentRuntime},
    discovery,
    job_table::{JobSpec, JobTable},
    mock_runtime::MockAgentRuntime,
    pty,
    runtime_table::RuntimeTable,
    spool::EventSpool,
};
use olympus_proto::{
    frames::{EnvoyFrame, HallFrame, NodeRole},
    runtime::RuntimeSpec,
    version::{BuildVersion, PROTOCOL_VERSION},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

/// Heartbeat interval.
const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

/// How often the idle reaper runs.
const IDLE_REAP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(120);

/// Sessions idle longer than this are terminated by the reaper.
/// 30 minutes — generous enough for a developer reading output between prompts,
/// aggressive enough to prevent 4-hour zombie sessions eating memory.
const IDLE_REAP_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(1800);

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    let node_id = arg_value("--node-id").unwrap_or_else(|| {
        std::env::var("OLYMPUS_NODE_ID").unwrap_or_else(|_| {
            hostname::get()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| "envoy".to_string())
        })
    });

    // `--print-node-id`: generate/load the iroh identity for this node id and
    // print ONLY the public key on stdout, then exit. Used by the one-line
    // bootstrap installer to register the id with the Hall's allowlist BEFORE
    // the service starts (fail-closed enrollment). Must run BEFORE tracing
    // init — the fmt subscriber writes to stdout and would pollute the
    // captured output.
    if flag_present("--print-node-id") {
        let state_dir = envoy_state_dir(&node_id)?;
        let secret = olympus_envoy::transport::load_or_create_secret(&state_dir)?;
        println!("{}", secret.public());
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    let socket = resolve_socket()?;
    let mock = flag_present("--mock");
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
    let state_dir = envoy_state_dir(&node_id)?;
    let spool = Arc::new(EventSpool::open(&state_dir)?);
    let roles = configured_roles()?;
    let job_root = arg_value("--job-root")
        .or_else(|| std::env::var("OLYMPUS_JOB_ROOT").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| state_dir.join("jobs"));
    let jobs = Arc::new(JobTable::new(job_root)?);
    if let Some(state_db) = observer_state_db()? {
        let observer_spool = spool.clone();
        std::thread::Builder::new()
            .name("olympus-statedb-observer".into())
            .spawn(move || run_observer(state_db, observer_spool))
            .context("spawning state.db observer")?;
    }

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
                        spool.clone(),
                        jobs.clone(),
                        roles.clone(),
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
            spool.clone(),
            jobs.clone(),
            roles.clone(),
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

/// Shared connection state: writer, runtime table, durable event spool, and
/// per-session turn counters.
struct Conn {
    writer: Mutex<BufWriter<BoxedWriter>>,
    table: Arc<RuntimeTable>,
    spool: Arc<EventSpool>,
    jobs: Arc<JobTable>,
    turn: Mutex<std::collections::HashMap<String, u64>>,
    /// Operator terminal (PTY) manager (ADR 0021 cockpit). Operator-only;
    /// driven solely by HallFrame::Terminal* frames.
    pty: Arc<crate::pty::PtyManager>,
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
#[allow(clippy::too_many_arguments)] // connection wiring: every arg is a distinct owned subsystem
async fn run_connection<R, W>(
    reader: R,
    writer: W,
    table: Arc<RuntimeTable>,
    node_id: &str,
    hostname: &str,
    agents: Vec<discovery::AgentInfo>,
    spool: Arc<EventSpool>,
    jobs: Arc<JobTable>,
    roles: Vec<NodeRole>,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
    W: tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    // Operator terminal (PTY) manager (ADR 0021). The sink pushes output/exit
    // into an mpsc that a forwarder task drains into the Hall connection.
    let (pty_sink, mut pty_rx) = pty::ChannelSink::new();
    let pty_mgr = pty::PtyManager::new(pty_sink);

    let conn = Arc::new(Conn {
        writer: Mutex::new(BufWriter::new(Box::new(writer) as BoxedWriter)),
        table: table.clone(),
        spool,
        jobs,
        turn: Mutex::new(std::collections::HashMap::new()),
        pty: pty_mgr,
    });

    // Forward PTY output/exit frames to Hall over this connection.
    let pty_fwd_handle = {
        let fwd_conn = conn.clone();
        tokio::spawn(async move {
            while let Some(msg) = pty_rx.recv().await {
                let frame = match msg {
                    pty::TerminalMsg::Output {
                        terminal_id,
                        data_b64,
                    } => EnvoyFrame::TerminalOutput {
                        terminal_id,
                        data_b64,
                    },
                    pty::TerminalMsg::Exited {
                        terminal_id,
                        exit_code,
                    } => EnvoyFrame::TerminalExited {
                        terminal_id,
                        exit_code,
                    },
                };
                if fwd_conn.send_frame(&frame).await.is_err() {
                    break;
                }
            }
        })
    };

    // Hello handshake.
    let agents_json = serde_json::to_value(&agents).unwrap_or_default();
    let mut runtimes = Vec::new();
    for session_id in conn.spool.sessions()? {
        runtimes.push(olympus_proto::frames::RuntimeStatus {
            last_seq: conn.spool.last_seq(&session_id)?.unwrap_or(0),
            session_id,
            hermes_id: None,
            state: "spooled".into(),
            resumable: false,
        });
    }
    let hello = EnvoyFrame::Hello {
        node_id: node_id.to_string(),
        hostname: hostname.to_string(),
        slots_total: 4,
        protocol_version: PROTOCOL_VERSION,
        version: BuildVersion::for_binary(env!("CARGO_PKG_VERSION")),
        agents: Some(agents_json),
        runtimes,
        roles,
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

    let replay_handle = {
        let replay_conn = conn.clone();
        tokio::spawn(async move {
            // Send each frame at most once per live connection unless Hall
            // explicitly requests replay with ResumeFrom. Blindly re-reading
            // the full spool every second turns one gap into a log/CPU storm.
            let mut sent_through = std::collections::HashMap::<String, u64>::new();
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let sessions = match replay_conn.spool.sessions() {
                    Ok(sessions) => sessions,
                    Err(_) => break,
                };
                for session_id in sessions {
                    let frames = match replay_conn.spool.read(
                        &session_id,
                        periodic_replay_after(&sent_through, &session_id),
                    ) {
                        Ok(frames) => frames,
                        Err(_) => continue,
                    };
                    for frame in frames {
                        if replay_conn.send_frame(&frame).await.is_err() {
                            return;
                        }
                        mark_periodic_replay_sent(&mut sent_through, &session_id, &frame);
                    }
                }
            }
        })
    };

    // Idle session reaper — terminates agent sessions that have been idle
    // longer than IDLE_REAP_THRESHOLD. Sessions can be resumed later via
    // ensure_runtime with a resume_hermes_id. Frees memory and child
    // processes on resource-constrained envoys.
    let reaper_handle = {
        let reaper_table = table.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(IDLE_REAP_INTERVAL).await;
                let reaped = reaper_table.reap_idle(IDLE_REAP_THRESHOLD).await;
                if reaped > 0 {
                    tracing::info!(reaped, "idle session reaper terminated sessions");
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

        // ACK truncation is ordered and cheap enough to keep inline. A
        // ResumeFrom may replay hundreds of frames and must not monopolize the
        // socket read loop: Hall sends ACKs on that same full-duplex stream.
        // Replaying inline can fill both socket buffers (Envoy waits to write
        // replay, Hall waits to write ACK) and starve heartbeats indefinitely.
        if dispatch_inline(&frame) {
            if let Err(e) = dispatch_frame(conn.clone(), frame).await {
                tracing::error!(error = %e, "frame dispatch error");
            }
        } else {
            let conn2 = conn.clone();
            tokio::spawn(async move {
                if let Err(e) = dispatch_frame(conn2, frame).await {
                    tracing::error!(error = %e, "frame dispatch error");
                }
            });
        }
    }

    // Read loop exited (Hall disconnected or error). Abort the heartbeat and
    // reaper tasks so they don't leak holding Arc clones across reconnects.
    hb_handle.abort();
    replay_handle.abort();
    reaper_handle.abort();
    pty_fwd_handle.abort();

    Ok(())
}

fn frame_sequence(frame: &EnvoyFrame) -> Option<u64> {
    match frame {
        EnvoyFrame::Event { seq, .. }
        | EnvoyFrame::Observed { seq, .. }
        | EnvoyFrame::JobOutput { seq, .. }
        | EnvoyFrame::JobResult { seq, .. } => Some(*seq),
        _ => None,
    }
}

fn periodic_replay_after(
    sent_through: &std::collections::HashMap<String, u64>,
    session_id: &str,
) -> Option<u64> {
    sent_through.get(session_id).copied()
}

fn mark_periodic_replay_sent(
    sent_through: &mut std::collections::HashMap<String, u64>,
    session_id: &str,
    frame: &EnvoyFrame,
) {
    if let Some(seq) = frame_sequence(frame) {
        sent_through.insert(session_id.to_owned(), seq);
    }
}

fn dispatch_inline(frame: &HallFrame) -> bool {
    matches!(frame, HallFrame::Ack { .. })
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
        HallFrame::DispatchJob {
            req_id,
            job_id,
            argv,
            env_allowlist,
            cwd,
            timeout_secs,
            max_output_bytes,
        } => {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let output_conn = conn.clone();
            tokio::spawn(async move {
                while let Some(mut frame) = rx.recv().await {
                    if output_conn.spool.append_next(&mut frame).is_ok() {
                        let _ = output_conn.send_frame(&frame).await;
                    }
                }
            });
            let result = conn
                .jobs
                .spawn(
                    JobSpec {
                        job_id,
                        argv,
                        env_allowlist,
                        cwd,
                        timeout_secs,
                        max_output_bytes,
                    },
                    tx,
                )
                .await;
            match result {
                Ok(()) => conn.send_resp(req_id, true, None).await,
                Err(error) => {
                    conn.send_resp(req_id, false, Some(&format!("{error:#}")))
                        .await
                }
            }
        }
        HallFrame::CancelJob { req_id, job_id } => match conn.jobs.cancel(&job_id).await {
            Ok(()) => conn.send_resp(req_id, true, None).await,
            Err(error) => {
                conn.send_resp(req_id, false, Some(&format!("{error:#}")))
                    .await
            }
        },
        HallFrame::Ack { session_id, seq } => {
            conn.spool.acknowledge(&session_id, seq)?;
            tracing::debug!(session = %session_id, seq, "ack from Hall truncated spool");
        }
        HallFrame::ResumeFrom { session_id, seq } => {
            // u64::MAX is the wire sentinel for "Hall has no watermark yet";
            // seq 0 is a valid first event and must be replayed.
            let after = (seq != u64::MAX).then_some(seq);
            for frame in conn.spool.read(&session_id, after)? {
                conn.send_frame(&frame).await?;
            }
            tracing::debug!(session = %session_id, seq, "replayed spool from Hall watermark");
        }
        HallFrame::TerminalOpen {
            req_id,
            terminal_id,
            cols,
            rows,
            cwd,
        } => {
            let outcome = conn
                .pty
                .open(&terminal_id, cols, rows, cwd.as_deref())
                .await;
            match outcome {
                Ok(()) => conn.send_resp(req_id, true, None).await,
                Err(e) => conn.send_resp(req_id, false, Some(&format!("{e:#}"))).await,
            }
        }
        HallFrame::TerminalInput {
            terminal_id,
            data_b64,
        } => {
            if let Err(e) = conn.pty.input(&terminal_id, &data_b64).await {
                tracing::debug!(terminal = %terminal_id, error = %e, "terminal input dropped");
            }
        }
        HallFrame::TerminalResize {
            terminal_id,
            cols,
            rows,
        } => {
            let _ = conn.pty.resize(&terminal_id, cols, rows).await;
        }
        HallFrame::TerminalClose { terminal_id } => {
            let _ = conn.pty.close(&terminal_id).await;
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
        let mut frame = EnvoyFrame::Event {
            session_id: session_id.to_string(),
            turn_id,
            seq: 0,
            payload: event.clone(),
        };
        conn.spool.append_next(&mut frame)?;
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

fn observer_state_db() -> Result<Option<PathBuf>> {
    let path = if let Some(path) = arg_value("--state-db") {
        PathBuf::from(path)
    } else if let Ok(path) = std::env::var("HERMES_STATE_DB") {
        PathBuf::from(path)
    } else {
        let home = std::env::var("HOME").context("HOME is not set")?;
        PathBuf::from(home).join(".hermes").join("state.db")
    };
    Ok(path.exists().then_some(path))
}

fn run_observer(path: PathBuf, spool: Arc<EventSpool>) {
    let mut observer = match olympus_envoy::observer::StateDbObserver::open(&path) {
        Ok(observer) => observer,
        Err(error) => {
            tracing::error!(error = %error, db = %path.display(), "state.db observer failed to start");
            return;
        }
    };
    loop {
        match observer.poll(1_000) {
            Ok(events) => {
                let active = !events.is_empty();
                for payload in events {
                    let hermes_id = match &payload {
                        olympus_proto::frames::ObservedEvent::Session { hermes_id, .. }
                        | olympus_proto::frames::ObservedEvent::Message { hermes_id, .. } => {
                            hermes_id
                        }
                    };
                    let session_id = format!("observed:{hermes_id}");
                    let mut frame = EnvoyFrame::Observed {
                        seq: 0,
                        session_id,
                        payload,
                    };
                    if let Err(error) = spool.append_next(&mut frame) {
                        tracing::error!(error = %error, "spooling state.db observation");
                    }
                }
                std::thread::sleep(if active {
                    std::time::Duration::from_secs(2)
                } else {
                    std::time::Duration::from_secs(30)
                });
            }
            Err(error) => {
                tracing::warn!(error = %error, "state.db observation poll failed");
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
        }
    }
}

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
        let model_set_style =
            olympus_envoy::bridge::hermes::model_set_style_for_agent(spec.agent.as_deref());
        let config = HermesRuntimeConfig {
            command,
            cwd,
            session_source: Some("olympus".into()),
            event_buffer: 256,
            start_timeout_secs: 30,
            mcp_servers: spec.mcp_servers.clone(),
            env,
            framing,
            model_set_style,
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

fn configured_roles() -> Result<Vec<NodeRole>> {
    let raw = arg_value("--roles").or_else(|| std::env::var("OLYMPUS_ENVOY_ROLES").ok());
    // Every envoy hosts operator terminals by default (ADR 0021 cockpit) — the
    // cockpit's node picker spawns a shell on any node. TerminalHost is an
    // operator-only capability; agents never reach the PTY manager.
    let mut roles = vec![NodeRole::AgentRuntime, NodeRole::TerminalHost];
    if let Some(raw) = raw {
        for role in raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            match role {
                "agent_runtime" | "agent-runtime" => {}
                "job_runner" | "job-runner" => roles.push(NodeRole::JobRunner),
                "terminal_host" | "terminal-host" => roles.push(NodeRole::TerminalHost),
                other => anyhow::bail!("unknown envoy role: {other}"),
            }
        }
    }
    roles.sort_by_key(|role| match role {
        NodeRole::AgentRuntime => 0,
        NodeRole::JobRunner => 1,
        NodeRole::TerminalHost => 2,
    });
    roles.dedup();
    Ok(roles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_replay_never_blocks_the_socket_read_loop() {
        assert!(!dispatch_inline(&HallFrame::ResumeFrom {
            session_id: "session".into(),
            seq: u64::MAX,
        }));
        assert!(dispatch_inline(&HallFrame::Ack {
            session_id: "session".into(),
            seq: 42,
        }));
    }

    #[test]
    fn periodic_replay_advances_once_per_connection() {
        let mut cursor = std::collections::HashMap::new();
        let frame = EnvoyFrame::Event {
            session_id: "session".into(),
            turn_id: "turn".into(),
            seq: 7,
            payload: AgentEvent::Done {
                finish_reason: Some("end_turn".into()),
            },
        };

        assert_eq!(periodic_replay_after(&cursor, "session"), None);
        mark_periodic_replay_sent(&mut cursor, "session", &frame);
        assert_eq!(periodic_replay_after(&cursor, "session"), Some(7));

        // Reconnection creates a fresh cursor and permits one durable replay.
        // Explicit Hall ResumeFrom bypasses this periodic cursor entirely.
        let reconnected = std::collections::HashMap::new();
        assert_eq!(periodic_replay_after(&reconnected, "session"), None);
    }
}
