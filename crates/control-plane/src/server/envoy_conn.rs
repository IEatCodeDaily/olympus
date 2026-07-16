//! Envoy connection manager + RemoteRuntime — the Hall-side counterparts to
//! the proto wire protocol (ADR 0008 §1, milestone S3).
//!
//! Each connected envoy gets an [`EnvoyConnection`] holding the write half of
//! its UDS stream plus a pending-request table and per-session event channels.
//! Hall sends [`HallFrame`]s to drive session ops; the envoy replies with
//! [`EnvoyFrame::Resp`] and streams [`EnvoyFrame::Event`] frames back. The UDS
//! read loop in `node.rs` dispatches inbound frames to the matching connection.
//!
//! [`RemoteRuntime`] implements [`AgentRuntime`] backed by an
//! `EnvoyConnection`, so Hall's existing `post_message` drain loop can drive a
//! session on a remote envoy identically to an in-process runtime.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::stream::Stream;
use olympus_envoy::bridge::{AgentCommand, AgentEvent, AgentRuntime};
use olympus_proto::frames::HallFrame;
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, Mutex, RwLock};

/// A boxed, thread-safe async writer — transport-agnostic (UDS OwnedWriteHalf
/// or iroh QUIC SendStream). This lets `EnvoyConnection` work identically over
/// either transport (ADR 0008 §1).
pub type BoxedWriter = Box<dyn tokio::io::AsyncWrite + Send + Unpin>;

/// A pending request awaiting an envoy `Resp` frame.
type PendingSlot = tokio::sync::oneshot::Sender<EnvoyResp>;

/// The structured result extracted from an `EnvoyFrame::Resp`.
#[derive(Debug)]
pub struct EnvoyResp {
    pub ok: bool,
    pub error: Option<String>,
    pub result: Option<serde_json::Value>,
}

/// One envoy's connection state: the write half + pending requests + per-session
/// event channels.
pub struct EnvoyConnection {
    pub(crate) epoch: u64,
    shutdown: tokio::sync::watch::Sender<bool>,
    /// Buffered writer to the transport stream (UDS or iroh QUIC). Guarded so
    /// Hall can send from any task. Transport-agnostic via [`BoxedWriter`].
    writer: Mutex<tokio::io::BufWriter<BoxedWriter>>,
    /// Pending requests keyed by reqId, awaiting `EnvoyFrame::Resp`.
    pending: Mutex<HashMap<u64, PendingSlot>>,
    /// Next Hall-assigned reqId.
    next_req_id: AtomicU64,
    /// Per-session event broadcast channels. Each RemoteRuntime subscribes to
    /// its session's channel; Event frames arriving on the UDS are forwarded
    /// here by the read loop.
    ///
    /// Uses `std::sync::Mutex` because the critical section is a non-blocking
    /// HashMap lookup + broadcast send/subscribe. This lets `events()` (a sync
    /// trait method) subscribe without an `.await`.
    event_channels: std::sync::Mutex<HashMap<String, broadcast::Sender<AgentEvent>>>,
    /// Per-terminal output channels (ADR 0021 cockpit). Keyed by Hall-issued
    /// `terminal_id`; the operator WebSocket subscribes, the read loop forwards
    /// `TerminalOutput`/`TerminalExited` here. Separate from `event_channels`
    /// so shell bytes never touch the session event plane.
    terminal_channels: std::sync::Mutex<HashMap<String, broadcast::Sender<TerminalFrame>>>,
    /// Hermes session ids captured from `ensure_runtime` responses, keyed by
    /// Olympus session id. RemoteRuntime reads this to implement
    /// `hermes_session_id()`.
    hermes_ids: std::sync::Mutex<HashMap<String, String>>,
    log: Option<Arc<crate::log::Log>>,
}

/// A terminal output/exit event forwarded from an envoy to the operator WS.
#[derive(Debug, Clone)]
pub enum TerminalFrame {
    Output { data_b64: String },
    Exited { exit_code: Option<i32> },
}

impl EnvoyConnection {
    fn new(writer: BoxedWriter, log: Option<Arc<crate::log::Log>>) -> Arc<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Self::new_with_epoch(writer, log, 0, shutdown)
    }

    fn new_with_epoch(
        writer: BoxedWriter,
        log: Option<Arc<crate::log::Log>>,
        epoch: u64,
        shutdown: tokio::sync::watch::Sender<bool>,
    ) -> Arc<Self> {
        Arc::new(Self {
            epoch,
            shutdown,
            writer: Mutex::new(tokio::io::BufWriter::new(writer)),
            pending: Mutex::new(HashMap::new()),
            next_req_id: AtomicU64::new(1),
            event_channels: std::sync::Mutex::new(HashMap::new()),
            terminal_channels: std::sync::Mutex::new(HashMap::new()),
            hermes_ids: std::sync::Mutex::new(HashMap::new()),
            log,
        })
    }

    /// Send a HallFrame to this envoy. For request frames (those with a
    /// `reqId`), registers a pending slot and returns a receiver for the
    /// matching `Resp`. Fire-and-forget frames (ack/resume_from) return None.
    ///
    /// Lock ordering: pending lock is acquired and released BEFORE the writer
    /// lock (review R4 fix). This avoids holding pending across a network I/O
    /// await.
    pub async fn send_request(
        &self,
        frame: HallFrame,
    ) -> Result<Option<tokio::sync::oneshot::Receiver<EnvoyResp>>> {
        match &frame {
            HallFrame::EnsureRuntime { .. }
            | HallFrame::Prompt { .. }
            | HallFrame::Steer { .. }
            | HallFrame::Cancel { .. }
            | HallFrame::Stop { .. }
            | HallFrame::RespondPermission { .. }
            | HallFrame::Drain { .. }
            | HallFrame::Probe { .. }
            | HallFrame::DispatchJob { .. }
            | HallFrame::CancelJob { .. }
            | HallFrame::TerminalOpen { .. } => {
                let id = self.next_req_id.fetch_add(1, Ordering::SeqCst);
                let frame_with_id = inject_req_id(frame, id);
                let (tx, rx) = tokio::sync::oneshot::channel();

                // Register the pending slot BEFORE writing so the resp can be
                // resolved as soon as it arrives (short critical section, no
                // I/O await inside the lock).
                self.pending.lock().await.insert(id, tx);

                // Serialize + write (writer lock held only during write).
                let json = serde_json::to_string(&frame_with_id)
                    .map_err(|e| anyhow::anyhow!("serializing HallFrame: {e}"))?;
                {
                    let mut w = self.writer.lock().await;
                    w.write_all(json.as_bytes()).await?;
                    w.write_all(b"\n").await?;
                    w.flush().await?;
                }
                Ok(Some(rx))
            }
            HallFrame::Ack { .. }
            | HallFrame::ResumeFrom { .. }
            | HallFrame::HeartbeatAck
            | HallFrame::ReRegister
            | HallFrame::TerminalInput { .. }
            | HallFrame::TerminalResize { .. }
            | HallFrame::TerminalClose { .. } => {
                let json = serde_json::to_string(&frame)
                    .map_err(|e| anyhow::anyhow!("serializing HallFrame: {e}"))?;
                {
                    let mut w = self.writer.lock().await;
                    w.write_all(json.as_bytes()).await?;
                    w.write_all(b"\n").await?;
                    w.flush().await?;
                }
                Ok(None)
            }
        }
    }

    /// Resolve a pending request with the envoy's response (called from the
    /// UDS read loop when an `EnvoyFrame::Resp` arrives).
    pub async fn resolve(&self, req_id: u64, resp: EnvoyResp) {
        if let Some(slot) = self.pending.lock().await.remove(&req_id) {
            let _ = slot.send(resp);
        }
    }

    /// Forward a session event into the per-session broadcast channel (called
    /// from the UDS read loop when an `EnvoyFrame::Event` arrives). Synchronous
    /// because the event_channels map uses std::sync::Mutex with a non-blocking
    /// critical section.
    pub fn forward_event(&self, session_id: &str, event: AgentEvent) {
        if let Ok(channels) = self.event_channels.lock() {
            if let Some(tx) = channels.get(session_id) {
                // send errors only when there are no receivers — that's fine,
                // it means nobody is draining this session's events yet.
                let _ = tx.send(event);
            }
        }
    }

    /// Durably gate the sequence, forward a new event once, and acknowledge
    /// the resulting high-water mark. Duplicates are acked without forwarding;
    /// gaps fail closed and remain unacked.
    pub async fn apply_event(&self, session_id: &str, seq: u64, event: AgentEvent) -> Result<bool> {
        let is_new = match &self.log {
            Some(log) => log.accept_envoy_seq(session_id, seq)?,
            None => true,
        };
        if is_new {
            self.forward_event(session_id, event);
        }
        self.send_request(HallFrame::Ack {
            session_id: session_id.to_owned(),
            seq,
        })
        .await?;
        Ok(is_new)
    }

    pub async fn apply_observed(
        &self,
        transport_session_id: &str,
        seq: u64,
        payload: olympus_proto::frames::ObservedEvent,
    ) -> Result<bool> {
        use olympus_proto::frames::ObservedEvent;
        let (hermes_id, message_id, event) = match payload {
            ObservedEvent::Session {
                hermes_id,
                source,
                model,
                title,
                started_at,
                message_count,
                input_tokens,
                output_tokens,
                archived: _,
            } => {
                let event = crate::event::Event::SessionCreated {
                    session_id: hermes_id.clone(),
                    hermes_id: hermes_id.clone(),
                    source,
                    model,
                    title,
                    started_at,
                    message_count,
                    input_tokens,
                    output_tokens,
                    agent: None,
                    node: None,
                };
                (hermes_id, None, event)
            }
            ObservedEvent::Message {
                hermes_id,
                message_id,
                role,
                content,
                tool_name,
                tool_calls,
                reasoning,
                timestamp,
                token_count,
                finish_reason,
            } => {
                let event = crate::event::Event::MessageAppended {
                    session_id: hermes_id.clone(),
                    hermes_session_id: hermes_id.clone(),
                    message_id,
                    role,
                    content,
                    tool_name,
                    tool_calls,
                    reasoning,
                    timestamp,
                    token_count,
                    finish_reason,
                };
                (hermes_id, Some(message_id), event)
            }
        };
        let is_new = match &self.log {
            Some(log) => {
                log.accept_observed(transport_session_id, seq, &hermes_id, message_id, &event)?
            }
            None => true,
        };
        self.send_request(HallFrame::Ack {
            session_id: transport_session_id.to_owned(),
            seq,
        })
        .await?;
        Ok(is_new)
    }

    pub fn watermark(&self, session_id: &str) -> Result<Option<u64>> {
        self.log
            .as_ref()
            .map_or(Ok(None), |log| log.envoy_watermark(session_id))
    }

    /// Get or create the broadcast sender for a session's event channel, then
    /// return a fresh receiver. Synchronous (see forward_event doc).
    pub fn subscribe_events(&self, session_id: &str) -> broadcast::Receiver<AgentEvent> {
        let mut channels = self.event_channels.lock().unwrap();
        let tx = channels
            .entry(session_id.to_string())
            .or_insert_with(|| broadcast::channel::<AgentEvent>(256).0)
            .clone();
        tx.subscribe()
    }

    /// Subscribe to a terminal's output stream (ADR 0021). The operator WS
    /// calls this before sending `TerminalOpen`; the read loop forwards
    /// `TerminalOutput`/`TerminalExited` via `forward_terminal`.
    pub fn subscribe_terminal(&self, terminal_id: &str) -> broadcast::Receiver<TerminalFrame> {
        let mut channels = self.terminal_channels.lock().unwrap();
        let tx = channels
            .entry(terminal_id.to_string())
            .or_insert_with(|| broadcast::channel::<TerminalFrame>(1024).0)
            .clone();
        tx.subscribe()
    }

    /// Forward a terminal frame from the envoy read loop to any operator WS
    /// subscribed to this terminal. No subscribers is fine (operator detached).
    pub fn forward_terminal(&self, terminal_id: &str, frame: TerminalFrame) {
        let tx = {
            let channels = self.terminal_channels.lock().unwrap();
            channels.get(terminal_id).cloned()
        };
        if let Some(tx) = tx {
            let _ = tx.send(frame);
        }
    }

    /// Drop a terminal's channel once it has exited/closed.
    pub fn drop_terminal(&self, terminal_id: &str) {
        self.terminal_channels.lock().unwrap().remove(terminal_id);
    }

    /// Store the Hermes session id captured from an `ensure_runtime` response.
    pub fn store_hermes_id(&self, session_id: &str, hermes_id: &str) {
        if let Ok(mut ids) = self.hermes_ids.lock() {
            ids.insert(session_id.to_string(), hermes_id.to_string());
        }
    }

    /// Retrieve the stored Hermes session id for a session.
    pub fn hermes_id(&self, session_id: &str) -> Option<String> {
        self.hermes_ids.lock().ok()?.get(session_id).cloned()
    }

    /// Clean up: fail all pending requests (envoy disconnected).
    pub async fn fail_all(&self) {
        let mut pending = self.pending.lock().await;
        pending.clear(); // oneshot senders drop → receivers get a RecvError
    }

    pub async fn close(&self) {
        let _ = self.shutdown.send(true);
        self.fail_all().await;
        let _ = self.writer.lock().await.shutdown().await;
    }
}

/// Inject a Hall-assigned reqId into a request frame (overwriting whatever was
/// there). Callers build frames with a dummy reqId (0) and Hall allocates the
/// real one.
fn inject_req_id(frame: HallFrame, req_id: u64) -> HallFrame {
    match frame {
        HallFrame::EnsureRuntime {
            session_id,
            spec,
            resume_id,
            ..
        } => HallFrame::EnsureRuntime {
            req_id,
            session_id,
            spec,
            resume_id,
        },
        HallFrame::Prompt {
            session_id,
            text,
            model,
            ..
        } => HallFrame::Prompt {
            req_id,
            session_id,
            text,
            model,
        },
        HallFrame::Steer {
            session_id, text, ..
        } => HallFrame::Steer {
            req_id,
            session_id,
            text,
        },
        HallFrame::Cancel { session_id, .. } => HallFrame::Cancel { req_id, session_id },
        HallFrame::Stop { session_id, .. } => HallFrame::Stop { req_id, session_id },
        HallFrame::RespondPermission {
            session_id,
            request_id,
            option_id,
            ..
        } => HallFrame::RespondPermission {
            req_id,
            session_id,
            request_id,
            option_id,
        },
        HallFrame::Drain { to_node, .. } => HallFrame::Drain { req_id, to_node },
        HallFrame::Probe { .. } => HallFrame::Probe { req_id },
        HallFrame::DispatchJob {
            job_id,
            argv,
            env_allowlist,
            cwd,
            timeout_secs,
            max_output_bytes,
            ..
        } => HallFrame::DispatchJob {
            req_id,
            job_id,
            argv,
            env_allowlist,
            cwd,
            timeout_secs,
            max_output_bytes,
        },
        HallFrame::CancelJob { job_id, .. } => HallFrame::CancelJob { req_id, job_id },
        // Fire-and-forget frames pass through unchanged.
        other => other,
    }
}

/// Thread-safe map of `node_id → EnvoyConnection`. The UDS read loop stores
/// connections here on hello and removes them on disconnect.
#[derive(Clone, Default)]
pub struct EnvoyConnections {
    inner: Arc<RwLock<HashMap<String, Arc<EnvoyConnection>>>>,
    log: Option<Arc<crate::log::Log>>,
    next_epoch: Arc<AtomicU64>,
}

impl EnvoyConnections {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_log(log: Arc<crate::log::Log>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            log: Some(log),
            next_epoch: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn allocate_epoch(&self) -> u64 {
        self.next_epoch.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Register a connection for a node. Returns the connection.
    pub async fn insert(&self, node_id: &str, writer: BoxedWriter) -> Arc<EnvoyConnection> {
        let conn = EnvoyConnection::new(writer, self.log.clone());
        self.inner
            .write()
            .await
            .insert(node_id.to_string(), conn.clone());
        conn
    }

    pub async fn insert_epoch(
        &self,
        node_id: &str,
        writer: BoxedWriter,
        epoch: u64,
        shutdown: tokio::sync::watch::Sender<bool>,
    ) -> Result<(Arc<EnvoyConnection>, Option<Arc<EnvoyConnection>>), Arc<EnvoyConnection>> {
        let conn = EnvoyConnection::new_with_epoch(writer, self.log.clone(), epoch, shutdown);
        let mut inner = self.inner.write().await;
        if inner.get(node_id).is_some_and(|old| old.epoch > epoch) {
            return Err(conn);
        }
        let old = inner.insert(node_id.to_owned(), conn.clone());
        Ok((conn, old))
    }

    /// Remove a connection (returns it so the caller can fail its pending reqs).
    pub async fn remove(&self, node_id: &str) -> Option<Arc<EnvoyConnection>> {
        self.inner.write().await.remove(node_id)
    }

    pub async fn remove_epoch(&self, node_id: &str, epoch: u64) -> Option<Arc<EnvoyConnection>> {
        let mut inner = self.inner.write().await;
        if inner.get(node_id).is_some_and(|conn| conn.epoch == epoch) {
            inner.remove(node_id)
        } else {
            None
        }
    }

    /// Get a connection by node_id.
    pub async fn get(&self, node_id: &str) -> Option<Arc<EnvoyConnection>> {
        self.inner.read().await.get(node_id).cloned()
    }

    /// Whether a node has a registered envoy connection (i.e. is remote).
    pub async fn is_remote_node(&self, node_id: &str) -> bool {
        self.inner.read().await.contains_key(node_id)
    }

    /// The first connected envoy's node id (for default routing when a session
    /// has no explicit node). Returns None when no envoys are connected.
    pub async fn first_node(&self) -> Option<String> {
        self.inner.read().await.keys().next().cloned()
    }

    /// Fail all pending requests on all connections (graceful shutdown).
    pub async fn fail_all(&self) {
        let conns: Vec<_> = self.inner.read().await.values().cloned().collect();
        for c in conns {
            c.fail_all().await;
        }
    }
}

// ── RemoteRuntime ──────────────────────────────────────────────────────

/// An [`AgentRuntime`] backed by a remote envoy connection. Hall sends
/// [`HallFrame`]s over the UDS; the envoy drives the real agent child and
/// streams events back as [`EnvoyFrame::Event`] frames. The UDS read loop
/// forwards those events into the per-session broadcast channel, which
/// `events()` drains — so Hall's existing `post_message` drain loop works
/// identically for remote sessions.
pub struct RemoteRuntime {
    conn: Arc<EnvoyConnection>,
    session_id: String,
    hermes_id: tokio::sync::Mutex<Option<String>>,
    /// The spawn configuration sent to the envoy in EnsureRuntime so it
    /// knows which agent/cwd/mcp to use. Defaults to empty (the envoy's
    /// factory picks its own cwd + default agent).
    spec: olympus_proto::RuntimeSpec,
}

impl RemoteRuntime {
    /// Create a new remote runtime for a session on the given envoy connection.
    pub fn new(conn: Arc<EnvoyConnection>, session_id: String) -> Self {
        Self {
            conn,
            session_id,
            hermes_id: tokio::sync::Mutex::new(None),
            spec: olympus_proto::RuntimeSpec::default(),
        }
    }

    /// Create a remote runtime with an explicit spawn spec. The spec flows
    /// into the EnsureRuntime frame so the envoy's factory knows which agent
    /// to run and in which cwd.
    pub fn with_spec(
        conn: Arc<EnvoyConnection>,
        session_id: String,
        spec: olympus_proto::RuntimeSpec,
    ) -> Self {
        Self {
            conn,
            session_id,
            hermes_id: tokio::sync::Mutex::new(None),
            spec,
        }
    }

    /// Create an Arc-wrapped remote runtime (the common case for the
    /// AgentRuntime trait).
    pub fn new_arc(conn: Arc<EnvoyConnection>, session_id: String) -> Arc<dyn AgentRuntime> {
        Arc::new(Self::new(conn, session_id))
    }

    /// Create an Arc-wrapped remote runtime with a spawn spec.
    pub fn arc_with_spec(
        conn: Arc<EnvoyConnection>,
        session_id: String,
        spec: olympus_proto::RuntimeSpec,
    ) -> Arc<dyn AgentRuntime> {
        Arc::new(Self::with_spec(conn, session_id, spec))
    }
}

#[async_trait::async_trait]
impl AgentRuntime for RemoteRuntime {
    async fn start(&self, session_id: Option<&str>) -> Result<()> {
        // EnsureRuntime: tell the envoy to spawn (or resume) a runtime for
        // this session. The envoy replies with the captured Hermes session id.
        let resume_id = session_id.filter(|s| !s.is_empty()).map(String::from);
        let frame = HallFrame::EnsureRuntime {
            req_id: 0, // Hall assigns the real id
            session_id: self.session_id.clone(),
            spec: self.spec.clone(),
            resume_id,
        };
        let rx = self
            .conn
            .send_request(frame)
            .await?
            .context("EnsureRuntime should produce a Resp")?;
        let resp = rx
            .await
            .map_err(|_| anyhow::anyhow!("envoy disconnected before EnsureRuntime resp"))?;
        if !resp.ok {
            anyhow::bail!(
                "ensure_runtime failed: {}",
                resp.error.unwrap_or_else(|| "unknown error".into())
            );
        }
        // Extract the Hermes session id from the result.
        if let Some(result) = &resp.result {
            if let Some(hid) = result.get("hermesId").and_then(|v| v.as_str()) {
                let hid = hid.to_string();
                *self.hermes_id.lock().await = Some(hid.clone());
                self.conn.store_hermes_id(&self.session_id, &hid);
            }
        }
        Ok(())
    }

    async fn fork_session(&self, _session_id: &str) -> Result<()> {
        // Fork over UDS is a future enhancement (S5 drain/handover). For now,
        // remote sessions don't support fork.
        anyhow::bail!("fork_session not supported for remote runtimes");
    }

    async fn send(&self, cmd: AgentCommand) -> Result<()> {
        let frame = match cmd {
            AgentCommand::Prompt { text, model } => HallFrame::Prompt {
                req_id: 0,
                session_id: self.session_id.clone(),
                text,
                model,
            },
            AgentCommand::Steer { text } => HallFrame::Steer {
                req_id: 0,
                session_id: self.session_id.clone(),
                text,
            },
            AgentCommand::Cancel => HallFrame::Cancel {
                req_id: 0,
                session_id: self.session_id.clone(),
            },
            AgentCommand::Stop => HallFrame::Stop {
                req_id: 0,
                session_id: self.session_id.clone(),
            },
            // Slash / SwitchModel are Hermes-CLI-specific; for the remote path,
            // map them to a prompt (the envoy's real runtime handles them).
            AgentCommand::Slash { command } => HallFrame::Prompt {
                req_id: 0,
                session_id: self.session_id.clone(),
                text: format!("/{command}"),
                model: None,
            },
            AgentCommand::SwitchModel { model } => HallFrame::Prompt {
                req_id: 0,
                session_id: self.session_id.clone(),
                text: format!("/model {model}"),
                model: None,
            },
        };
        let rx = self
            .conn
            .send_request(frame)
            .await?
            .context("command frame should produce a Resp")?;
        let resp = rx
            .await
            .map_err(|_| anyhow::anyhow!("envoy disconnected before command resp"))?;
        if !resp.ok {
            anyhow::bail!(
                "command failed: {}",
                resp.error.unwrap_or_else(|| "unknown error".into())
            );
        }
        Ok(())
    }

    fn events(&self) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send>> {
        // Subscribe to the per-session broadcast channel. Each call gets its
        // own fresh receiver, so each turn's drain loop sees that turn's events.
        // (Same pattern as HermesAgentRuntime::events.)
        use tokio_stream::StreamExt as _;
        let rx = self.conn.subscribe_events(&self.session_id);
        Box::pin(tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(|res| res.ok()))
    }

    async fn stop(&self) -> Result<()> {
        // Send Stop and best-effort await the Resp (the envoy closes the child).
        let frame = HallFrame::Stop {
            req_id: 0,
            session_id: self.session_id.clone(),
        };
        if let Some(rx) = self.conn.send_request(frame).await? {
            let _ = rx.await; // best-effort: envoy may disconnect immediately
        }
        Ok(())
    }

    async fn hermes_session_id(&self) -> Option<String> {
        // Check local cache first, then the shared store (populated by start()).
        if let Some(hid) = self.hermes_id.lock().await.clone() {
            return Some(hid);
        }
        self.conn.hermes_id(&self.session_id)
    }

    async fn resumable(&self) -> bool {
        // The capability is parsed by the envoy from the adapter's initialize
        // response. Hall can query it via a probe or runtimes-table frame in a
        // future milestone. For now, fail closed.
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test EnvoyConnection backed by a real UDS socketpair (the
    /// write half goes to the conn; the read half is dropped — tests don't
    /// check the wire, only the in-memory event/pending maps).
    fn test_writer() -> BoxedWriter {
        use std::os::unix::net::UnixStream as StdStream;
        let (s1, _s2) = StdStream::pair().unwrap();
        s1.set_nonblocking(true).unwrap();
        let stream = tokio::net::UnixStream::from_std(s1).unwrap();
        let (_reader, writer) = stream.into_split();
        Box::new(writer)
    }

    fn test_conn() -> Arc<EnvoyConnection> {
        EnvoyConnection::new(test_writer(), None)
    }

    #[tokio::test]
    async fn envoy_connections_insert_get_remove() {
        let conns = EnvoyConnections::new();
        assert!(!conns.is_remote_node("n1").await);
        // Insert directly via the internal map (can't create a real
        // OwnedWriteHalf without a socket).
        conns.inner.write().await.insert("n1".into(), test_conn());
        assert!(conns.is_remote_node("n1").await);
        assert!(conns.get("n1").await.is_some());
        conns.remove("n1").await;
        assert!(!conns.is_remote_node("n1").await);
    }

    #[tokio::test]
    async fn older_epoch_cannot_replace_newer_connection() {
        let conns = EnvoyConnections::new();
        let (shutdown, _) = tokio::sync::watch::channel(false);
        assert!(conns
            .insert_epoch("n1", test_writer(), 2, shutdown)
            .await
            .is_ok());
        let (shutdown, _) = tokio::sync::watch::channel(false);
        assert!(conns
            .insert_epoch("n1", test_writer(), 1, shutdown)
            .await
            .is_err());
        assert_eq!(conns.get("n1").await.unwrap().epoch, 2);
    }

    #[tokio::test]
    async fn event_channel_forwards_events() {
        let conn = test_conn();
        let mut rx = conn.subscribe_events("s1");
        conn.forward_event("s1", AgentEvent::Text("hello".into()));
        let event = rx.recv().await.unwrap();
        assert_eq!(event, AgentEvent::Text("hello".into()));
    }

    #[tokio::test]
    async fn event_channel_multiple_subscribers() {
        let conn = test_conn();
        let mut rx1 = conn.subscribe_events("s1");
        let mut rx2 = conn.subscribe_events("s1");
        conn.forward_event("s1", AgentEvent::Text("hi".into()));
        assert_eq!(rx1.recv().await.unwrap(), AgentEvent::Text("hi".into()));
        assert_eq!(rx2.recv().await.unwrap(), AgentEvent::Text("hi".into()));
    }

    #[tokio::test]
    async fn hermes_id_store_and_retrieve() {
        let conn = test_conn();
        assert!(conn.hermes_id("s1").is_none());
        conn.store_hermes_id("s1", "hermes-abc");
        assert_eq!(conn.hermes_id("s1").as_deref(), Some("hermes-abc"));
    }

    #[tokio::test]
    async fn pending_resolve_delivers_response() {
        let conn = test_conn();
        let (tx, rx) = tokio::sync::oneshot::channel();
        conn.pending.lock().await.insert(42, tx);
        conn.resolve(
            42,
            EnvoyResp {
                ok: true,
                error: None,
                result: None,
            },
        )
        .await;
        let resp = rx.await.unwrap();
        assert!(resp.ok);
    }

    #[tokio::test]
    async fn fail_all_drops_pending() {
        let conn = test_conn();
        let (tx, rx) = tokio::sync::oneshot::channel();
        conn.pending.lock().await.insert(1, tx);
        conn.fail_all().await;
        // Receiver should get an error (sender dropped).
        assert!(rx.await.is_err());
    }

    #[tokio::test]
    async fn durable_watermark_drops_duplicates_across_hall_restart() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("hall.db");
        let log = Arc::new(crate::log::Log::open(&db).unwrap());
        let (writer, mut peer) = tokio::io::duplex(4096);
        let conn = EnvoyConnection::new(Box::new(writer), Some(log.clone()));
        let mut events = conn.subscribe_events("s1");

        assert!(conn
            .apply_event("s1", 0, AgentEvent::Text("zero".into()))
            .await
            .unwrap());
        assert!(!conn
            .apply_event("s1", 0, AgentEvent::Text("duplicate".into()))
            .await
            .unwrap());
        assert!(conn
            .apply_event("s1", 1, AgentEvent::Text("one".into()))
            .await
            .unwrap());
        assert_eq!(
            events.recv().await.unwrap(),
            AgentEvent::Text("zero".into())
        );
        assert_eq!(events.recv().await.unwrap(), AgentEvent::Text("one".into()));
        assert!(events.try_recv().is_err());
        assert_eq!(log.envoy_watermark("s1").unwrap(), Some(1));

        use tokio::io::AsyncReadExt as _;
        let mut ack_bytes = vec![0; 512];
        let count = peer.read(&mut ack_bytes).await.unwrap();
        assert_eq!(
            String::from_utf8_lossy(&ack_bytes[..count])
                .matches("\"kind\":\"ack\"")
                .count(),
            3
        );
        drop(conn);
        drop(log);

        let reopened = Arc::new(crate::log::Log::open(&db).unwrap());
        let (writer, _peer) = tokio::io::duplex(4096);
        let conn = EnvoyConnection::new(Box::new(writer), Some(reopened.clone()));
        let mut events = conn.subscribe_events("s1");
        assert!(!conn
            .apply_event("s1", 1, AgentEvent::Text("replayed-one".into()))
            .await
            .unwrap());
        assert!(conn
            .apply_event("s1", 2, AgentEvent::Text("two".into()))
            .await
            .unwrap());
        assert_eq!(events.recv().await.unwrap(), AgentEvent::Text("two".into()));
        assert!(events.try_recv().is_err());
        assert_eq!(reopened.envoy_watermark("s1").unwrap(), Some(2));
    }

    #[tokio::test]
    async fn observed_state_db_rows_survive_hall_restart_exactly_once() {
        use olympus_envoy::observer::StateDbObserver;
        use olympus_envoy::spool::EventSpool;
        use olympus_proto::frames::{EnvoyFrame, ObservedEvent};
        use rusqlite::Connection;

        let dir = tempfile::tempdir().unwrap();
        let state_db = dir.path().join("state.db");
        let db = Connection::open(&state_db).unwrap();
        db.execute_batch("CREATE TABLE sessions(id TEXT PRIMARY KEY,source TEXT,model TEXT,title TEXT,started_at REAL,message_count INTEGER,input_tokens INTEGER,output_tokens INTEGER,archived INTEGER); CREATE TABLE messages(id INTEGER PRIMARY KEY AUTOINCREMENT,session_id TEXT,role TEXT,content TEXT,tool_name TEXT,tool_calls TEXT,reasoning TEXT,timestamp REAL,token_count INTEGER,finish_reason TEXT,active INTEGER,compacted INTEGER); INSERT INTO sessions VALUES('seed','cli',NULL,NULL,0,0,0,0,0);").unwrap();
        drop(db);
        let mut observer = StateDbObserver::open(&state_db).unwrap();
        let spool = EventSpool::open(&dir.path().join("envoy")).unwrap();
        let hall_db = dir.path().join("hall.db");

        let insert = |session: &str, content: &str, timestamp: f64| {
            let db = Connection::open(&state_db).unwrap();
            db.execute(
                "INSERT OR IGNORE INTO sessions VALUES(?1,'telegram','m','title',?2,1,0,0,0)",
                rusqlite::params![session, timestamp],
            )
            .unwrap();
            db.execute(
                "INSERT INTO messages(session_id,role,content,timestamp,active,compacted) VALUES(?1,'user',?2,?3,1,0)",
                rusqlite::params![session, content, timestamp],
            )
            .unwrap();
        };
        let spool_poll = |observer: &mut StateDbObserver, spool: &EventSpool| {
            for payload in observer.poll(100).unwrap() {
                let hermes_id = match &payload {
                    ObservedEvent::Session { hermes_id, .. }
                    | ObservedEvent::Message { hermes_id, .. } => hermes_id,
                };
                let transport_id = format!("observed:{hermes_id}");
                let frame = EnvoyFrame::Observed {
                    seq: spool.next_seq(&transport_id).unwrap(),
                    session_id: transport_id,
                    payload,
                };
                spool.append(&frame).unwrap();
            }
        };

        insert("remote", "one", 1.0);
        spool_poll(&mut observer, &spool);
        let log = Arc::new(crate::log::Log::open(&hall_db).unwrap());
        let (writer, _peer) = tokio::io::duplex(4096);
        let conn = EnvoyConnection::new(Box::new(writer), Some(log.clone()));
        for frame in spool.read("observed:remote", None).unwrap() {
            if let EnvoyFrame::Observed {
                session_id,
                seq,
                payload,
            } = frame
            {
                conn.apply_observed(&session_id, seq, payload)
                    .await
                    .unwrap();
            }
        }
        assert_eq!(log.recent_messages("remote", 10).unwrap().len(), 1);
        drop(conn);
        drop(log);

        insert("remote", "two", 2.0);
        spool_poll(&mut observer, &spool);
        let reopened = Arc::new(crate::log::Log::open(&hall_db).unwrap());
        let (writer, _peer) = tokio::io::duplex(4096);
        let conn = EnvoyConnection::new(Box::new(writer), Some(reopened.clone()));
        for _ in 0..2 {
            for frame in spool.read("observed:remote", None).unwrap() {
                if let EnvoyFrame::Observed {
                    session_id,
                    seq,
                    payload,
                } = frame
                {
                    conn.apply_observed(&session_id, seq, payload)
                        .await
                        .unwrap();
                }
            }
        }
        let messages = reopened.recent_messages("remote", 10).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content.as_deref(), Some("one"));
        assert_eq!(messages[1].content.as_deref(), Some("two"));
        reopened.retain_native().unwrap();
        assert_eq!(reopened.recent_messages("remote", 10).unwrap().len(), 2);
    }

    #[tokio::test]
    async fn sequence_gap_is_not_forwarded_or_acknowledged() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let log = Arc::new(crate::log::Log::open(file.path()).unwrap());
        let (writer, mut peer) = tokio::io::duplex(512);
        let conn = EnvoyConnection::new(Box::new(writer), Some(log.clone()));
        let mut events = conn.subscribe_events("s1");

        let error = conn
            .apply_event("s1", 2, AgentEvent::Text("gap".into()))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("expected 0, got 2"));
        assert!(events.try_recv().is_err());
        assert_eq!(log.envoy_watermark("s1").unwrap(), None);
        use tokio::io::AsyncReadExt as _;
        let mut byte = [0];
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), peer.read(&mut byte))
                .await
                .is_err()
        );
    }
}
