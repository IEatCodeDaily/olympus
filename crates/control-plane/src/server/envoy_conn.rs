//! Envoy connection manager — the Hall-side counterpart to the proto wire
//! protocol (ADR 0008 §1, milestone S3).
//!
//! Each connected envoy gets an [`EnvoyConnection`] holding the write half of
//! its UDS stream plus a pending-request table. Hall sends [`HallFrame`]s to
//! drive session ops; the envoy replies with [`EnvoyFrame::Resp`] and streams
//! [`EnvoyFrame::Event`] frames back. The UDS read loop in `node.rs` dispatches
//! inbound frames to the matching connection.
//!
//! Session events arriving on an envoy connection are forwarded into Hall's
//! broadcast `deltas` channel (via a sink closure) so the WS fanout + event log
//! apply path sees them identically to in-process events.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use olympus_proto::agent::AgentEvent;
use olympus_proto::frames::{EnvoyFrame, HallFrame};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

/// A pending request awaiting an envoy `Resp` frame.
type PendingSlot = tokio::sync::oneshot::Sender<EnvoyResp>;

/// The structured result extracted from an `EnvoyFrame::Resp`.
#[derive(Debug)]
pub struct EnvoyResp {
    pub ok: bool,
    pub error: Option<String>,
    pub result: Option<serde_json::Value>,
}

/// One envoy's connection state: the write half + a map of pending requests.
pub struct EnvoyConnection {
    /// Buffered writer to the UDS stream (guarded so Hall can send from any task).
    writer: Mutex<tokio::io::BufWriter<tokio::net::unix::OwnedWriteHalf>>,
    /// Pending requests keyed by reqId, awaiting `EnvoyFrame::Resp`.
    pending: Mutex<HashMap<u64, PendingSlot>>,
    /// Next Hall-assigned reqId.
    next_req_id: AtomicU64,
    /// Read-loop handle, stored so we can abort it on disconnect.
    _read_handle: Option<JoinHandle<()>>,
}

impl EnvoyConnection {
    fn new(writer: tokio::net::unix::OwnedWriteHalf, read_handle: JoinHandle<()>) -> Arc<Self> {
        Arc::new(Self {
            writer: Mutex::new(tokio::io::BufWriter::new(writer)),
            pending: Mutex::new(HashMap::new()),
            next_req_id: AtomicU64::new(1),
            _read_handle: Some(read_handle),
        })
    }

    /// Send a HallFrame to this envoy. For request frames (those with a
    /// `reqId`), registers a pending slot and returns a receiver for the
    /// matching `Resp`. Fire-and-forget frames (ack/resume_from) return None.
    pub async fn send_request(
        &self,
        frame: HallFrame,
    ) -> anyhow::Result<Option<tokio::sync::oneshot::Receiver<EnvoyResp>>> {
        let rx = match &frame {
            HallFrame::EnsureRuntime { .. }
            | HallFrame::Prompt { .. }
            | HallFrame::Steer { .. }
            | HallFrame::Cancel { .. }
            | HallFrame::Stop { .. }
            | HallFrame::RespondPermission { .. }
            | HallFrame::Drain { .. }
            | HallFrame::Probe { .. } => {
                let id = self.next_req_id.fetch_add(1, Ordering::SeqCst);
                // We need to inject the reqId into the frame. Rebuild with the
                // allocated id.
                let frame_with_id = inject_req_id(frame, id);
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.pending.lock().await.insert(id, tx);
                let json = serde_json::to_string(&frame_with_id)
                    .map_err(|e| anyhow::anyhow!("serializing HallFrame: {e}"))?;
                let mut w = self.writer.lock().await;
                w.write_all(json.as_bytes()).await?;
                w.write_all(b"\n").await?;
                w.flush().await?;
                Some(rx)
            }
            // Fire-and-forget frames: no reqId, no pending slot.
            HallFrame::Ack { .. } | HallFrame::ResumeFrom { .. } => {
                let json = serde_json::to_string(&frame)
                    .map_err(|e| anyhow::anyhow!("serializing HallFrame: {e}"))?;
                let mut w = self.writer.lock().await;
                w.write_all(json.as_bytes()).await?;
                w.write_all(b"\n").await?;
                w.flush().await?;
                None
            }
        };
        Ok(rx)
    }

    /// Resolve a pending request with the envoy's response (called from the
    /// UDS read loop when an `EnvoyFrame::Resp` arrives).
    pub async fn resolve(&self, req_id: u64, resp: EnvoyResp) {
        if let Some(slot) = self.pending.lock().await.remove(&req_id) {
            let _ = slot.send(resp);
        }
    }

    /// Clean up: fail all pending requests (envoy disconnected).
    async fn fail_all(&self) {
        let mut pending = self.pending.lock().await;
        pending.clear(); // oneshot senders drop → receivers get a RecvError
    }
}

/// Inject a Hall-assigned reqId into a request frame (overwriting whatever was
/// there). This keeps the `send_request` API simple — callers build frames with
/// a dummy reqId (0) and Hall allocates the real one.
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

    /// Register a connection for a node.
    pub async fn insert(&self, node_id: &str, conn: Arc<EnvoyConnection>) {
        self.inner.write().await.insert(node_id.to_string(), conn);
    }

    /// Remove a connection (returns it so the caller can fail its pending reqs).
    pub async fn remove(&self, node_id: &str) -> Option<Arc<EnvoyConnection>> {
        self.inner.write().await.remove(node_id)
    }

    /// Get a connection by node_id.
    pub async fn get(&self, node_id: &str) -> Option<Arc<EnvoyConnection>> {
        self.inner.read().await.get(node_id).cloned()
    }

    /// Fail all pending requests on all connections (graceful shutdown).
    pub async fn fail_all(&self) {
        let conns: Vec<_> = self.inner.read().await.values().cloned().collect();
        for c in conns {
            c.fail_all().await;
        }
    }
}

/// A sink closure: how the UDS read loop forwards session events into Hall's
/// broadcast channel + event-log apply path. The closure receives
/// (session_id, turn_id, seq, event).
pub type EventSink = Arc<dyn Fn(&str, &str, u64, AgentEvent) + Send + Sync>;
