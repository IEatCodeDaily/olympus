//! Node registry — tracks fleet nodes (envoys) connected via UDS.
//!
//! The control plane listens on a Unix domain socket
//! (`~/.olympus/control.sock`). Each node (envoy) connects and speaks a
//! JSON-lines protocol:
//!
//! ```text
//! → {"kind":"hello","nodeId":"worker-1","hostname":"talos","slotsTotal":4,"version":"0.1"}
//! ← {"kind":"welcome","status":"ok"}
//! → {"kind":"heartbeat","nodeId":"worker-1","slotsUsed":2}
//! ← {"kind":"ack","status":"ok"}
//! → {"kind":"bye","nodeId":"worker-1"}
//! ```
//!
//! Liveness: a node that misses heartbeats for `HEARTBEAT_TIMEOUT` (30s)
//! transitions to `offline` and is evicted after `EVICTION_TIMEOUT` (60s).
//! When the socket disconnects, the node is immediately removed.
//!
//! The local (in-process) node auto-registers at boot via `register_local()`
//! — it has no UDS connection but is always `online`. This is the ADR 0005
//! §3 "boundary is preserved so multi-node is additive" pattern: the local
//! envoy is just another node in the registry.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::server::agents::AgentInfo;

/// Node status as the control plane tracks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    /// Active, heartbeating.
    Online,
    /// Draining — no new sessions assigned.
    Draining,
    /// Missed heartbeats; will be evicted.
    Offline,
}

/// How a node is connected to the Hall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeTransport {
    /// In-process (the Hall's own host).
    Local,
    /// Unix domain socket (same host, separate process).
    Uds,
    /// iroh QUIC tunnel (remote host).
    Iroh,
}

/// A registered node in the fleet.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    /// Unique node identifier (hostname or operator-chosen slug).
    pub node_id: String,
    /// Hostname or address of the node.
    pub hostname: String,
    /// Current liveness status.
    pub status: NodeStatus,
    /// Active agent sessions on this node.
    pub slots_used: u32,
    /// Total agent session capacity.
    pub slots_total: u32,
    /// Envoy version string.
    pub version: String,
    /// Whether this is the local (in-process) node.
    pub local: bool,
    /// Seconds since last heartbeat.
    pub last_heartbeat_ago_secs: u64,
    /// How this node is connected (local / uds / iroh).
    pub transport: NodeTransport,
    /// The node's iroh public key (set for iroh-connected envoys; used to
    /// revoke it from the allowlist on Remove).
    pub iroh_node_id: Option<String>,
    /// Agents this node's envoy has discovered on its host (Hermes profiles +
    /// installed CLI harnesses). Populated by the node's envoy — for the local
    /// node, by in-process discovery at boot and on manual refresh. Empty until
    /// a remote envoy reports its agents.
    pub agents: Vec<AgentInfo>,
}

/// Internal tracking entry (not serialized directly; `NodeInfo` is the wire shape).
struct NodeEntry {
    node_id: String,
    hostname: String,
    status: NodeStatus,
    slots_used: u32,
    slots_total: u32,
    version: String,
    local: bool,
    last_heartbeat: Instant,
    transport: NodeTransport,
    iroh_node_id: Option<String>,
    agents: Vec<AgentInfo>,
}

/// Heartbeat timeout: a node is `offline` if no heartbeat for this long.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(30);
/// Eviction timeout: an offline node is removed after this long.
const EVICTION_TIMEOUT: Duration = Duration::from_secs(60);

/// Thread-safe in-memory node registry.
#[derive(Clone)]
pub struct NodeRegistry {
    nodes: Arc<RwLock<HashMap<String, NodeEntry>>>,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register or re-register a node (hello handshake). Updates all fields.
    /// `agents` is the node's envoy-discovered agent list (empty for a remote
    /// node until it reports; the local node passes its in-process discovery).
    /// `iroh_node_id` is the peer's iroh public key for iroh-connected envoys.
    #[allow(clippy::too_many_arguments)]
    pub async fn register(
        &self,
        node_id: &str,
        hostname: &str,
        slots_total: u32,
        version: &str,
        local: bool,
        transport: NodeTransport,
        iroh_node_id: Option<String>,
        agents: Vec<AgentInfo>,
    ) {
        let now = Instant::now();
        let mut nodes = self.nodes.write().await;
        nodes.insert(
            node_id.to_string(),
            NodeEntry {
                node_id: node_id.to_string(),
                hostname: hostname.to_string(),
                status: NodeStatus::Online,
                slots_used: 0,
                slots_total,
                version: version.to_string(),
                local,
                last_heartbeat: now,
                transport,
                iroh_node_id,
                agents,
            },
        );
    }

    /// Replace a node's discovered agent list (manual "detect agents" refresh,
    /// or a remote envoy re-reporting). Returns the updated list, or an error if
    /// the node is unknown.
    pub async fn set_agents(
        &self,
        node_id: &str,
        agents: Vec<AgentInfo>,
    ) -> Result<Vec<AgentInfo>, NodeError> {
        let mut nodes = self.nodes.write().await;
        let entry = nodes
            .get_mut(node_id)
            .ok_or(NodeError::UnknownNode(node_id.to_string()))?;
        entry.agents = agents.clone();
        Ok(agents)
    }

    /// Get a node's discovered agents.
    pub async fn agents(&self, node_id: &str) -> Result<Vec<AgentInfo>, NodeError> {
        let nodes = self.nodes.read().await;
        nodes
            .get(node_id)
            .map(|e| e.agents.clone())
            .ok_or(NodeError::UnknownNode(node_id.to_string()))
    }

    /// All agents across every node, deduped by (node_id is dropped) agent id.
    /// Used by the flat /api/agents list for backward compatibility.
    pub async fn all_agents(&self) -> Vec<AgentInfo> {
        let nodes = self.nodes.read().await;
        let mut seen = std::collections::BTreeMap::new();
        for e in nodes.values() {
            for a in &e.agents {
                seen.entry(a.id.clone()).or_insert_with(|| a.clone());
            }
        }
        seen.into_values().collect()
    }

    /// Update a node's heartbeat and slot usage.
    pub async fn heartbeat(&self, node_id: &str, slots_used: u32) -> Result<(), NodeError> {
        let mut nodes = self.nodes.write().await;
        let entry = nodes
            .get_mut(node_id)
            .ok_or(NodeError::UnknownNode(node_id.to_string()))?;
        entry.last_heartbeat = Instant::now();
        entry.slots_used = slots_used;
        if entry.status == NodeStatus::Offline {
            entry.status = NodeStatus::Online;
        }
        Ok(())
    }

    /// Mark a node as draining (no new sessions).
    pub async fn set_draining(&self, node_id: &str) -> Result<(), NodeError> {
        let mut nodes = self.nodes.write().await;
        let entry = nodes
            .get_mut(node_id)
            .ok_or(NodeError::UnknownNode(node_id.to_string()))?;
        entry.status = NodeStatus::Draining;
        Ok(())
    }

    /// Remove a node from the registry (bye or disconnect).
    pub async fn deregister(&self, node_id: &str) {
        self.nodes.write().await.remove(node_id);
    }

    /// List all nodes with current status, evicting stale ones.
    /// This is the function the `/api/nodes` handler calls.
    pub async fn list(&self) -> Vec<NodeInfo> {
        let now = Instant::now();
        let mut nodes = self.nodes.write().await;

        // Evict nodes that have been offline too long.
        nodes.retain(|_, e| {
            if e.local {
                return true; // local node never evicted
            }
            let elapsed = now.duration_since(e.last_heartbeat);
            if elapsed > EVICTION_TIMEOUT {
                tracing::warn!(node = %e.node_id, "evicting stale node");
                return false;
            }
            true
        });

        // Mark timed-out nodes as offline.
        for entry in nodes.values_mut() {
            if entry.local {
                continue;
            }
            let elapsed = now.duration_since(entry.last_heartbeat);
            if elapsed > HEARTBEAT_TIMEOUT && entry.status == NodeStatus::Online {
                tracing::warn!(node = %entry.node_id, "node went offline (heartbeat timeout)");
                entry.status = NodeStatus::Offline;
            }
        }

        // Project to wire DTO.
        nodes
            .values()
            .map(|e| NodeInfo {
                node_id: e.node_id.clone(),
                hostname: e.hostname.clone(),
                status: e.status,
                slots_used: e.slots_used,
                slots_total: e.slots_total,
                version: e.version.clone(),
                local: e.local,
                last_heartbeat_ago_secs: now.duration_since(e.last_heartbeat).as_secs(),
                transport: e.transport,
                iroh_node_id: e.iroh_node_id.clone(),
                agents: e.agents.clone(),
            })
            .collect()
    }

    /// Get a single node by id.
    pub async fn get(&self, node_id: &str) -> Option<NodeInfo> {
        let nodes = self.nodes.read().await;
        nodes.get(node_id).map(|e| NodeInfo {
            node_id: e.node_id.clone(),
            hostname: e.hostname.clone(),
            status: e.status,
            slots_used: e.slots_used,
            slots_total: e.slots_total,
            version: e.version.clone(),
            local: e.local,
            last_heartbeat_ago_secs: Instant::now().duration_since(e.last_heartbeat).as_secs(),
            transport: e.transport,
            iroh_node_id: e.iroh_node_id.clone(),
            agents: e.agents.clone(),
        })
    }

    /// Count of online nodes.
    pub async fn online_count(&self) -> usize {
        let nodes = self.nodes.read().await;
        nodes
            .values()
            .filter(|e| e.status == NodeStatus::Online)
            .count()
    }
}

impl Default for NodeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeError {
    UnknownNode(String),
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownNode(id) => write!(f, "unknown node: {id}"),
        }
    }
}

impl std::error::Error for NodeError {}

// ── UDS Protocol Messages ──────────────────────────

/// Inbound message from an envoy over the UDS.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NodeMessage {
    Hello {
        #[serde(rename = "nodeId")]
        node_id: String,
        hostname: String,
        #[serde(default = "default_slots", rename = "slotsTotal")]
        slots_total: u32,
        #[serde(default)]
        version: String,
    },
    Heartbeat {
        #[serde(rename = "nodeId")]
        node_id: String,
        #[serde(default, rename = "slotsUsed")]
        slots_used: u32,
    },
    Bye {
        #[serde(rename = "nodeId")]
        node_id: String,
    },
}

/// Outbound response from the control plane.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NodeResponse {
    Welcome { status: &'static str },
    Ack { status: &'static str },
    Error { message: String },
}

fn default_slots() -> u32 {
    4
}

/// Bind and run the UDS listener. Each accepted connection speaks JSON-lines
/// (one message per line, newline-delimited). The connection stays open for
/// the lifetime of the envoy — heartbeats arrive on the same socket.
///
/// ADR 0008 S3: connections now speak the `EnvoyFrame` protocol (hello/resp/
/// event/heartbeat/bye/runtimes). Old envoys that still send legacy
/// `NodeMessage` (hello/heartbeat/bye-only) are handled by falling back to
/// the legacy dispatch. On disconnect, the node is deregistered and its
/// EnvoyConnection (if any) is removed.
///
/// `envoy_conns` holds the per-node write halves for RemoteRuntime; `registry`
/// holds the node metadata. Both are shared clones.
pub async fn run_uds_listener(
    path: std::path::PathBuf,
    registry: NodeRegistry,
    envoy_conns: crate::server::envoy_conn::EnvoyConnections,
) {
    // Remove stale socket from a previous run.
    let _ = std::fs::remove_file(&path);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let listener = match tokio::net::UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(path = %path.display(), error = %e, "failed to bind UDS listener");
            return;
        }
    };
    tracing::info!(path = %path.display(), "node UDS listener started");

    while let Ok((stream, _)) = listener.accept().await {
        let reg = registry.clone();
        let conns = envoy_conns.clone();
        tokio::spawn(handle_uds_conn(stream, reg, conns));
    }
}

/// Handle a single UDS connection (one envoy's lifecycle).
///
/// Supports two protocol generations on the same socket:
/// - **v2 (ADR 0008):** `EnvoyFrame`-tagged JSON-lines (hello/heartbeat/bye/
///   resp/event/runtimes). On hello, validates `protocol_version == 2` (fail
///   closed) and registers an `EnvoyConnection` so RemoteRuntime can drive
///   sessions on this envoy.
/// - **v1 (legacy):** `NodeMessage`-tagged JSON-lines (hello/heartbeat/bye).
///   Kept for backward compatibility with old envoys.
///
/// The dispatch tries `EnvoyFrame` first; if the `kind` field doesn't match
/// any EnvoyFrame variant, it falls back to `NodeMessage`.
async fn handle_uds_conn(
    stream: tokio::net::UnixStream,
    registry: NodeRegistry,
    envoy_conns: crate::server::envoy_conn::EnvoyConnections,
) {
    let (reader, writer) = stream.into_split();
    handle_envoy_conn(
        reader,
        writer,
        registry,
        envoy_conns,
        NodeTransport::Uds,
        None,
    )
    .await;
}

/// Transport-generic envoy connection handler: the same JSON-lines dispatch
/// runs over UDS (local) and iroh QUIC streams (remote, S7). ADR 0008 §1.
/// `transport` + `peer_iroh_id` describe HOW the envoy reached us (shown in
/// the Fleet view; the iroh id also enables allowlist revocation on Remove).
pub async fn handle_envoy_conn<R, W>(
    reader: R,
    writer: W,
    registry: NodeRegistry,
    envoy_conns: crate::server::envoy_conn::EnvoyConnections,
    transport: NodeTransport,
    peer_iroh_id: Option<String>,
) where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
    W: tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let mut lines = BufReader::new(reader).lines();
    let mut connected_node: Option<String> = None;
    // The EnvoyConnection (set on hello). All writes to the envoy go through
    // its buffered writer. For legacy v1 connections, we fall back to writing
    // directly via the raw writer.
    let mut conn: Option<Arc<crate::server::envoy_conn::EnvoyConnection>> = None;
    let mut legacy_writer: Option<crate::server::envoy_conn::BoxedWriter> = Some(Box::new(writer));

    loop {
        let line = match lines.next_line().await {
            Ok(Some(l)) => l,
            Ok(None) => break, // EOF — peer disconnected
            Err(_) => break,   // read error
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        // Try parsing as EnvoyFrame (v2 protocol) first. EnvoyFrame and
        // NodeMessage share `kind`-tagged JSON, but their variant names differ
        // (EnvoyFrame uses snake_case: hello, heartbeat, bye, resp, event,
        // runtimes). NodeMessage uses lowercase: hello, heartbeat, bye.
        let parsed_envoy: Result<olympus_proto::frames::EnvoyFrame, _> =
            serde_json::from_str(&line);
        if let Ok(frame) = parsed_envoy {
            // On the first Hello, move the writer into an EnvoyConnection.
            if matches!(frame, olympus_proto::frames::EnvoyFrame::Hello { .. }) {
                if let Some(w) = legacy_writer.take() {
                    let hello_frame = match frame {
                        olympus_proto::frames::EnvoyFrame::Hello { .. } => frame,
                        _ => unreachable!(),
                    };
                    let new_conn = handle_envoy_hello(
                        hello_frame,
                        &registry,
                        &envoy_conns,
                        w,
                        &mut connected_node,
                        transport,
                        peer_iroh_id.clone(),
                    )
                    .await;
                    match new_conn {
                        HelloOutcome::Accepted(c) => {
                            conn = Some(c);
                        }
                        HelloOutcome::Rejected => break, // protocol mismatch → disconnect
                    }
                    continue;
                }
            }
            let outcome = handle_envoy_frame(frame, &registry, &envoy_conns, &mut conn).await;
            if outcome == FrameOutcome::Disconnect {
                break;
            }
            continue;
        }

        // Fall back to legacy NodeMessage (v1 protocol).
        let msg: NodeMessage = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                if let Some(ref mut w) = legacy_writer {
                    let resp = NodeResponse::Error {
                        message: format!("bad json: {e}"),
                    };
                    let _ = w
                        .write_all(
                            format!("{}\n", serde_json::to_string(&resp).unwrap()).as_bytes(),
                        )
                        .await;
                }
                continue;
            }
        };

        let response = match msg {
            NodeMessage::Hello {
                node_id,
                hostname,
                slots_total,
                version,
            } => {
                tracing::info!(node = %node_id, hostname = %hostname, "node registered (legacy v1)");
                registry
                    .register(
                        &node_id,
                        &hostname,
                        slots_total,
                        &version,
                        false,
                        transport,
                        peer_iroh_id.clone(),
                        Vec::new(),
                    )
                    .await;
                connected_node = Some(node_id);
                NodeResponse::Welcome { status: "ok" }
            }
            NodeMessage::Heartbeat {
                node_id,
                slots_used,
            } => {
                if let Err(e) = registry.heartbeat(&node_id, slots_used).await {
                    NodeResponse::Error {
                        message: e.to_string(),
                    }
                } else {
                    NodeResponse::Ack { status: "ok" }
                }
            }
            NodeMessage::Bye { node_id } => {
                tracing::info!(node = %node_id, "node deregistered (bye)");
                registry.deregister(&node_id).await;
                if let Some(ref mut w) = legacy_writer {
                    let resp = NodeResponse::Ack { status: "ok" };
                    let _ = w
                        .write_all(
                            format!("{}\n", serde_json::to_string(&resp).unwrap()).as_bytes(),
                        )
                        .await;
                }
                break;
            }
        };

        if let Some(ref mut w) = legacy_writer {
            let _ = w
                .write_all(format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes())
                .await;
        }
    }

    // Connection closed — deregister the node if it was registered.
    if let Some(node_id) = connected_node {
        tracing::info!(node = %node_id, "node disconnected, deregistering");
        registry.deregister(&node_id).await;
        // Fail all pending requests on this envoy's connection and remove it.
        if let Some(conn) = envoy_conns.remove(&node_id).await {
            conn.fail_all().await;
        }
    }
}

/// Outcome of handling an EnvoyFrame: Continue or Disconnect (bye).
#[derive(PartialEq)]
enum FrameOutcome {
    Continue,
    Disconnect,
}

/// Outcome of handling an EnvoyFrame Hello: Accepted (with the EnvoyConnection)
/// or Rejected (protocol mismatch → connection closing).
enum HelloOutcome {
    Accepted(Arc<crate::server::envoy_conn::EnvoyConnection>),
    Rejected,
}

/// Handle a v2 EnvoyFrame::Hello: validate protocol version, register the node,
/// create the EnvoyConnection with the writer, and return the connection.
#[allow(clippy::too_many_arguments)]
async fn handle_envoy_hello(
    frame: olympus_proto::frames::EnvoyFrame,
    registry: &NodeRegistry,
    envoy_conns: &crate::server::envoy_conn::EnvoyConnections,
    writer: crate::server::envoy_conn::BoxedWriter,
    connected_node: &mut Option<String>,
    transport: NodeTransport,
    peer_iroh_id: Option<String>,
) -> HelloOutcome {
    use olympus_proto::frames::EnvoyFrame;
    use olympus_proto::version::PROTOCOL_VERSION;

    let EnvoyFrame::Hello {
        node_id,
        hostname,
        slots_total,
        protocol_version,
        version: build_version,
        agents,
        runtimes,
    } = frame
    else {
        unreachable!("handle_envoy_hello called with non-Hello frame")
    };

    // Fail closed: reject incompatible protocol versions (ADR 0008 §1).
    if protocol_version != PROTOCOL_VERSION {
        tracing::warn!(
            node = %node_id,
            got = protocol_version,
            expected = PROTOCOL_VERSION,
            "rejecting envoy: protocol version mismatch"
        );
        // We can't write to the writer anymore (it's consumed by insert).
        // The rejection is logged; the connection is closed.
        return HelloOutcome::Rejected;
    }

    tracing::info!(
        node = %node_id,
        hostname = %hostname,
        version = %build_version.semver,
        git = %build_version.git_hash,
        "envoy registered (v2)"
    );

    // Parse the agents JSON into AgentInfo (best-effort; the envoy sends
    // harness-native JSON that serde tolerates with unknown fields).
    let agents_parsed: Vec<crate::server::agents::AgentInfo> = agents
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    // Build a display version from the BuildVersion semver + git hash.
    let version_str = if build_version.git_hash != "unknown" {
        format!("{} ({})", build_version.semver, build_version.git_hash)
    } else {
        build_version.semver.clone()
    };

    registry
        .register(
            &node_id,
            &hostname,
            slots_total,
            &version_str,
            false,
            transport,
            peer_iroh_id,
            agents_parsed,
        )
        .await;

    // Create the EnvoyConnection and store it for RemoteRuntime.
    let conn = envoy_conns.insert(&node_id, writer).await;
    for runtime in runtimes {
        let watermark = match conn.watermark(&runtime.session_id) {
            Ok(Some(seq)) => seq,
            Ok(None) => u64::MAX,
            Err(error) => {
                tracing::error!(session = %runtime.session_id, error = %error, "reading envoy replay watermark");
                continue;
            }
        };
        if let Err(error) = conn
            .send_request(olympus_proto::frames::HallFrame::ResumeFrom {
                session_id: runtime.session_id,
                seq: watermark,
            })
            .await
        {
            tracing::error!(error = %error, "requesting envoy spool replay");
        }
    }
    *connected_node = Some(node_id);

    HelloOutcome::Accepted(conn)
}

/// Dispatch a parsed EnvoyFrame (ADR 0008 v2 protocol) — all variants except
/// Hello (handled by handle_envoy_hello). The `conn` is set after a successful
/// hello; Resp and Event frames route through it.
async fn handle_envoy_frame(
    frame: olympus_proto::frames::EnvoyFrame,
    registry: &NodeRegistry,
    envoy_conns: &crate::server::envoy_conn::EnvoyConnections,
    conn: &mut Option<Arc<crate::server::envoy_conn::EnvoyConnection>>,
) -> FrameOutcome {
    use olympus_proto::frames::EnvoyFrame;

    match frame {
        EnvoyFrame::Hello { .. } => {
            // A second hello on the same connection is a no-op (the first was
            // handled by handle_envoy_hello in the read loop).
        }
        EnvoyFrame::Heartbeat {
            node_id,
            slots_used,
        } => {
            if let Err(e) = registry.heartbeat(&node_id, slots_used).await {
                tracing::warn!(node = %node_id, error = %e, "heartbeat for unknown node");
            }
        }
        EnvoyFrame::Bye { node_id } => {
            tracing::info!(node = %node_id, "envoy deregistered (bye)");
            registry.deregister(&node_id).await;
            if let Some(conn) = envoy_conns.remove(&node_id).await {
                conn.fail_all().await;
            }
            return FrameOutcome::Disconnect;
        }
        EnvoyFrame::Resp {
            req_id,
            ok,
            error,
            result,
        } => {
            if let Some(c) = conn {
                c.resolve(
                    req_id,
                    crate::server::envoy_conn::EnvoyResp { ok, error, result },
                )
                .await;
            }
        }
        EnvoyFrame::Event {
            session_id,
            turn_id: _,
            seq,
            payload,
        } => {
            if let Some(c) = conn {
                if let Err(error) = c.apply_event(&session_id, seq, payload).await {
                    tracing::warn!(session = %session_id, seq, error = %error, "envoy event rejected; leaving unacked");
                }
            }
        }
        EnvoyFrame::Observed {
            session_id,
            seq,
            payload,
        } => {
            if let Some(c) = conn {
                if let Err(error) = c.apply_observed(&session_id, seq, payload).await {
                    tracing::warn!(session = %session_id, seq, error = %error, "envoy observation rejected; leaving unacked");
                }
            }
        }
        EnvoyFrame::Runtimes { runtimes: _ } => {
            tracing::debug!("runtimes table update received (S4 will process)");
        }
    }
    FrameOutcome::Continue
}

// ── Iroh listener (remote envoys, ADR 0008 §1 / S7) ────────────────────

/// Hall-side allowlist config, loaded from `<home>/hall.toml`:
///
/// ```toml
/// allowed_envoys = ["<iroh-node-id>", ...]
/// ```
#[derive(Debug, Default, serde::Deserialize)]
struct HallConfig {
    #[serde(default)]
    allowed_envoys: Vec<String>,
}

fn load_allowlist(home: &std::path::Path) -> Vec<iroh::PublicKey> {
    let path = home.join("hall.toml");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new(); // no file → empty allowlist → no remote envoys (fail closed)
    };
    let cfg: HallConfig = match toml::from_str(&raw) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, path = %path.display(), "hall.toml parse failed — remote envoys disabled");
            return Vec::new();
        }
    };
    cfg.allowed_envoys
        .iter()
        .filter_map(|s| match s.parse::<iroh::PublicKey>() {
            Ok(k) => Some(k),
            Err(e) => {
                tracing::warn!(id = %s, error = %e, "invalid node id in hall.toml allowlist, skipping");
                None
            }
        })
        .collect()
}

/// Bind the hall's iroh endpoint (public n0 relays) and return it + the node
/// id. The node id is needed at boot for `AppState` (GET /api/nodes/hall-identity)
/// and for the operator log line. The accept loop is [`run_iroh_accept_loop`].
///
/// The hall's iroh identity persists at `<home>/iroh.key`.
pub async fn create_iroh_endpoint(
    home: &std::path::Path,
) -> anyhow::Result<(iroh::Endpoint, iroh::EndpointId)> {
    let secret = olympus_envoy::transport::load_or_create_secret(home)?;
    let endpoint = olympus_envoy::transport::bind_endpoint(secret).await?;
    let node_id = endpoint.id();
    tracing::info!(iroh_node_id = %node_id, "hall iroh endpoint listening (remote envoys)");
    Ok((endpoint, node_id))
}

/// Run the iroh accept loop: each accepted + allowlisted connection speaks the
/// identical JSON-lines protocol via [`handle_envoy_conn`]. The allowlist is
/// re-read from `hall.toml` per connection so additions don't need a restart.
pub async fn run_iroh_accept_loop(
    home: std::path::PathBuf,
    endpoint: iroh::Endpoint,
    registry: NodeRegistry,
    envoy_conns: crate::server::envoy_conn::EnvoyConnections,
) -> anyhow::Result<()> {
    while let Some(incoming) = endpoint.accept().await {
        let conn = match incoming.await {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(error = %e, "iroh handshake failed");
                continue;
            }
        };
        let peer = conn.remote_id();
        // Fail closed: only allowlisted node ids proceed to the protocol.
        let allowlist = load_allowlist(&home);
        if !allowlist.contains(&peer) {
            tracing::warn!(peer = %peer, "rejecting non-allowlisted iroh envoy");
            conn.close(1u32.into(), b"not allowlisted");
            continue;
        }
        tracing::info!(peer = %peer, "iroh envoy connected");
        let reg = registry.clone();
        let conns = envoy_conns.clone();
        tokio::spawn(async move {
            // The envoy opens the bi-stream; hall accepts it.
            match conn.accept_bi().await {
                Ok((send, recv)) => {
                    handle_envoy_conn(
                        recv,
                        send,
                        reg,
                        conns,
                        NodeTransport::Iroh,
                        Some(peer.to_string()),
                    )
                    .await;
                }
                Err(e) => {
                    tracing::debug!(error = %e, "iroh accept_bi failed");
                }
            }
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn register_and_list() {
        let reg = NodeRegistry::new();
        reg.register(
            "node-1",
            "host-1",
            4,
            "0.1",
            false,
            NodeTransport::Uds,
            None,
            vec![],
        )
        .await;

        let nodes = reg.list().await;
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_id, "node-1");
        assert_eq!(nodes[0].status, NodeStatus::Online);
        assert_eq!(nodes[0].slots_total, 4);
    }

    #[tokio::test]
    async fn heartbeat_updates_slots() {
        let reg = NodeRegistry::new();
        reg.register(
            "node-1",
            "host-1",
            4,
            "0.1",
            false,
            NodeTransport::Uds,
            None,
            vec![],
        )
        .await;

        reg.heartbeat("node-1", 2).await.unwrap();

        let nodes = reg.list().await;
        assert_eq!(nodes[0].slots_used, 2);
    }

    #[tokio::test]
    async fn heartbeat_unknown_node_fails() {
        let reg = NodeRegistry::new();
        let err = reg.heartbeat("ghost", 1).await.unwrap_err();
        assert_eq!(err, NodeError::UnknownNode("ghost".into()));
    }

    #[tokio::test]
    async fn deregister_removes_node() {
        let reg = NodeRegistry::new();
        reg.register(
            "node-1",
            "host-1",
            4,
            "0.1",
            false,
            NodeTransport::Uds,
            None,
            vec![],
        )
        .await;
        assert_eq!(reg.list().await.len(), 1);

        reg.deregister("node-1").await;
        assert_eq!(reg.list().await.len(), 0);
    }

    #[tokio::test]
    async fn re_register_updates_fields() {
        let reg = NodeRegistry::new();
        reg.register(
            "node-1",
            "host-1",
            2,
            "0.1",
            false,
            NodeTransport::Uds,
            None,
            vec![],
        )
        .await;
        // Re-register with updated capacity
        reg.register(
            "node-1",
            "host-1",
            8,
            "0.2",
            false,
            NodeTransport::Uds,
            None,
            vec![],
        )
        .await;

        let nodes = reg.list().await;
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].slots_total, 8);
        assert_eq!(nodes[0].version, "0.2");
    }

    #[tokio::test]
    async fn local_node_never_evicted() {
        let reg = NodeRegistry::new();
        reg.register(
            "local",
            "localhost",
            4,
            "0.1",
            true,
            NodeTransport::Local,
            None,
            vec![],
        )
        .await;

        // Even after a long wait, local node stays.
        sleep(Duration::from_millis(50)).await;
        let nodes = reg.list().await;
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].status, NodeStatus::Online);
    }

    #[tokio::test]
    async fn set_draining_changes_status() {
        let reg = NodeRegistry::new();
        reg.register(
            "node-1",
            "host-1",
            4,
            "0.1",
            false,
            NodeTransport::Uds,
            None,
            vec![],
        )
        .await;

        reg.set_draining("node-1").await.unwrap();
        let nodes = reg.list().await;
        assert_eq!(nodes[0].status, NodeStatus::Draining);
    }

    #[tokio::test]
    async fn online_count() {
        let reg = NodeRegistry::new();
        reg.register(
            "n1",
            "h1",
            4,
            "0.1",
            false,
            NodeTransport::Uds,
            None,
            vec![],
        )
        .await;
        reg.register(
            "n2",
            "h2",
            4,
            "0.1",
            false,
            NodeTransport::Uds,
            None,
            vec![],
        )
        .await;
        reg.set_draining("n2").await.unwrap();

        assert_eq!(reg.online_count().await, 1);
    }

    #[tokio::test]
    async fn get_single_node() {
        let reg = NodeRegistry::new();
        reg.register(
            "n1",
            "h1",
            4,
            "0.1",
            false,
            NodeTransport::Uds,
            None,
            vec![],
        )
        .await;

        let node = reg.get("n1").await.unwrap();
        assert_eq!(node.node_id, "n1");
        assert_eq!(node.hostname, "h1");

        assert!(reg.get("ghost").await.is_none());
    }

    #[tokio::test]
    async fn message_deserialize_hello() {
        let json =
            r#"{"kind":"hello","nodeId":"w1","hostname":"talos","slotsTotal":4,"version":"0.1"}"#;
        let msg: NodeMessage = serde_json::from_str(json).unwrap();
        match msg {
            NodeMessage::Hello {
                node_id,
                hostname,
                slots_total,
                version,
            } => {
                assert_eq!(node_id, "w1");
                assert_eq!(hostname, "talos");
                assert_eq!(slots_total, 4);
                assert_eq!(version, "0.1");
            }
            _ => panic!("expected Hello"),
        }
    }

    #[tokio::test]
    async fn message_deserialize_heartbeat() {
        let json = r#"{"kind":"heartbeat","nodeId":"w1","slotsUsed":2}"#;
        let msg: NodeMessage = serde_json::from_str(json).unwrap();
        match msg {
            NodeMessage::Heartbeat {
                node_id,
                slots_used,
            } => {
                assert_eq!(node_id, "w1");
                assert_eq!(slots_used, 2);
            }
            _ => panic!("expected Heartbeat"),
        }
    }

    #[tokio::test]
    async fn message_deserialize_bye() {
        let json = r#"{"kind":"bye","nodeId":"w1"}"#;
        let msg: NodeMessage = serde_json::from_str(json).unwrap();
        match msg {
            NodeMessage::Bye { node_id } => assert_eq!(node_id, "w1"),
            _ => panic!("expected Bye"),
        }
    }

    #[tokio::test]
    async fn message_deserialize_defaults() {
        // Missing optional fields should use defaults.
        let json = r#"{"kind":"hello","nodeId":"w1","hostname":"talos"}"#;
        let msg: NodeMessage = serde_json::from_str(json).unwrap();
        match msg {
            NodeMessage::Hello {
                slots_total,
                version,
                ..
            } => {
                assert_eq!(slots_total, 4); // default
                assert_eq!(version, ""); // default
            }
            _ => panic!("expected Hello"),
        }
    }

    #[tokio::test]
    async fn message_response_serialize() {
        let welcome = NodeResponse::Welcome { status: "ok" };
        let json = serde_json::to_string(&welcome).unwrap();
        assert!(json.contains("\"kind\":\"welcome\""));
        assert!(json.contains("\"status\":\"ok\""));

        let err = NodeResponse::Error {
            message: "bad request".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"kind\":\"error\""));
        assert!(json.contains("\"message\":\"bad request\""));
    }
}
