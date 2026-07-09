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
use olympus_proto::frames::{EnvoyFrame, HallFrame};
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, Mutex, RwLock};

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
    /// Buffered writer to the UDS stream (guarded so Hall can send from any task).
    writer: Mutex<tokio::io::BufWriter<tokio::net::unix::OwnedWriteHalf>>,
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
    /// Hermes session ids captured from `ensure_runtime` responses, keyed by
    /// Olympus session id. RemoteRuntime reads this to implement
    /// `hermes_session_id()`.
    hermes_ids: std::sync::Mutex<HashMap<String, String>>,
}

impl EnvoyConnection {
    fn new(writer: tokio::net::unix::OwnedWriteHalf) -> Arc<Self> {
        Arc::new(Self {
            writer: Mutex::new(tokio::io::BufWriter::new(writer)),
            pending: Mutex::new(HashMap::new()),
            next_req_id: AtomicU64::new(1),
            event_channels: std::sync::Mutex::new(HashMap::new()),
            hermes_ids: std::sync::Mutex::new(HashMap::new()),
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
            | HallFrame::Probe { .. } => {
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
            HallFrame::Ack { .. } | HallFrame::ResumeFrom { .. } => {
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
        // Fire-and-forget frames pass through unchanged.
        other => other,
    }
}

/// Thread-safe map of `node_id → EnvoyConnection`. The UDS read loop stores
/// connections here on hello and removes them on disconnect.
#[derive(Clone, Default)]
pub struct EnvoyConnections {
    inner: Arc<RwLock<HashMap<String, Arc<EnvoyConnection>>>>,
}

impl EnvoyConnections {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a connection for a node. Returns the connection.
    pub async fn insert(
        &self,
        node_id: &str,
        writer: tokio::net::unix::OwnedWriteHalf,
    ) -> Arc<EnvoyConnection> {
        let conn = EnvoyConnection::new(writer);
        self.inner
            .write()
            .await
            .insert(node_id.to_string(), conn.clone());
        conn
    }

    /// Remove a connection (returns it so the caller can fail its pending reqs).
    pub async fn remove(&self, node_id: &str) -> Option<Arc<EnvoyConnection>> {
        self.inner.write().await.remove(node_id)
    }

    /// Get a connection by node_id.
    pub async fn get(&self, node_id: &str) -> Option<Arc<EnvoyConnection>> {
        self.inner.read().await.get(node_id).cloned()
    }

    /// Whether a node has a registered envoy connection (i.e. is remote).
    pub async fn is_remote_node(&self, node_id: &str) -> bool {
        self.inner.read().await.contains_key(node_id)
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
}

impl RemoteRuntime {
    /// Create a new remote runtime for a session on the given envoy connection.
    pub fn new(conn: Arc<EnvoyConnection>, session_id: String) -> Self {
        Self {
            conn,
            session_id,
            hermes_id: tokio::sync::Mutex::new(None),
        }
    }

    /// Create an Arc-wrapped remote runtime (the common case for the
    /// AgentRuntime trait).
    pub fn new_arc(conn: Arc<EnvoyConnection>, session_id: String) -> Arc<dyn AgentRuntime> {
        Arc::new(Self::new(conn, session_id))
    }
}

#[async_trait::async_trait]
impl AgentRuntime for RemoteRuntime {
    async fn start(&self, session_id: Option<&str>) -> Result<()> {
        // EnsureRuntime: tell the envoy to spawn (or resume) a runtime for
        // this session. The envoy replies with the captured Hermes session id.
        let resume_id = session_id.filter(|s| !s.is_empty()).map(String::from);
        let spec = olympus_proto::RuntimeSpec::default();
        let frame = HallFrame::EnsureRuntime {
            req_id: 0, // Hall assigns the real id
            session_id: self.session_id.clone(),
            spec,
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
    fn test_conn() -> Arc<EnvoyConnection> {
        use std::os::unix::net::UnixStream as StdStream;
        let (s1, _s2) = StdStream::pair().unwrap();
        s1.set_nonblocking(true).unwrap();
        let stream = tokio::net::UnixStream::from_std(s1).unwrap();
        let (_reader, writer) = stream.into_split();
        EnvoyConnection::new(writer)
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
}
