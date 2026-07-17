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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    pub enrolled_at: Option<u64>,
    pub last_seen: Option<u64>,
    pub last_version: String,
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
    connection_epoch: Option<u64>,
    enrolled_at: Option<u64>,
    last_seen: Option<u64>,
    persisted_last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DurableNode {
    node_id: String,
    iroh_node_id: String,
    enrolled_at: u64,
    last_seen: Option<u64>,
    #[serde(default)]
    last_version: String,
}

/// Heartbeat timeout: a node is `offline` if no heartbeat for this long.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(30);
/// Eviction timeout: an offline node is removed after this long.
const EVICTION_TIMEOUT: Duration = Duration::from_secs(60);

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Thread-safe in-memory node registry.
#[derive(Clone)]
pub struct NodeRegistry {
    nodes: Arc<RwLock<HashMap<String, NodeEntry>>>,
    roles: Arc<RwLock<HashMap<String, Vec<olympus_proto::frames::NodeRole>>>>,
    inventory_path: Option<Arc<std::path::PathBuf>>,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            roles: Arc::new(RwLock::new(HashMap::new())),
            inventory_path: None,
        }
    }

    pub fn with_inventory(home: &std::path::Path) -> anyhow::Result<Self> {
        let path = home.join("nodes.json");
        let mut durable: Vec<DurableNode> = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(nodes) => nodes,
                Err(error) => {
                    let corrupt = path.with_extension("json.corrupt");
                    tracing::warn!(
                        path = %path.display(),
                        backup = %corrupt.display(),
                        %error,
                        "quarantining corrupt node inventory"
                    );
                    if let Err(rename_error) = std::fs::rename(&path, &corrupt) {
                        tracing::warn!(%rename_error, "failed to quarantine corrupt node inventory");
                    }
                    Vec::new()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "node inventory unreadable; rebuilding from allowlist");
                Vec::new()
            }
        };
        let now = unix_now();
        for key in crate::enroll::allowlist_list(home) {
            if !durable.iter().any(|node| node.iroh_node_id == key) {
                durable.push(DurableNode {
                    node_id: key.clone(),
                    iroh_node_id: key,
                    enrolled_at: now,
                    last_seen: None,
                    last_version: String::new(),
                });
            }
        }
        let nodes = durable
            .into_iter()
            .map(|node| {
                let persisted_last_seen = node.last_seen.unwrap_or(0);
                (
                    node.node_id.clone(),
                    NodeEntry {
                        node_id: node.node_id.clone(),
                        hostname: node.node_id,
                        status: NodeStatus::Offline,
                        slots_used: 0,
                        slots_total: 0,
                        version: node.last_version,
                        local: false,
                        last_heartbeat: Instant::now(),
                        transport: NodeTransport::Iroh,
                        iroh_node_id: Some(node.iroh_node_id),
                        agents: vec![],
                        connection_epoch: None,
                        enrolled_at: Some(node.enrolled_at),
                        last_seen: node.last_seen,
                        persisted_last_seen,
                    },
                )
            })
            .collect();
        Ok(Self {
            nodes: Arc::new(RwLock::new(nodes)),
            roles: Arc::new(RwLock::new(HashMap::new())),
            inventory_path: Some(Arc::new(path)),
        })
    }

    fn iroh_peer_is_allowlisted(&self, peer: Option<&str>) -> bool {
        let Some(peer) = peer else {
            return true;
        };
        let Some(home) = self
            .inventory_path
            .as_deref()
            .and_then(|path| path.parent())
        else {
            return true;
        };
        crate::enroll::allowlist_list(home)
            .iter()
            .any(|key| key == peer)
    }

    pub async fn enroll(&self, node_id: &str, iroh_node_id: &str) -> anyhow::Result<()> {
        let now = unix_now();
        let mut nodes = self.nodes.write().await;

        // First durable-inventory boot seeds legacy allowlist entries under the
        // raw iroh key because the old Hall never persisted node names. When
        // enrollment supplies the name, migrate that placeholder instead of
        // creating a second fleet row for the same authenticated identity.
        let duplicate_ids: Vec<_> = nodes
            .iter()
            .filter(|(id, node)| {
                id.as_str() != node_id && node.iroh_node_id.as_deref() == Some(iroh_node_id)
            })
            .map(|(id, _)| id.clone())
            .collect();
        let mut migrated = None;
        for duplicate_id in duplicate_ids {
            if let Some(entry) = nodes.remove(&duplicate_id) {
                migrated.get_or_insert(entry);
            }
        }

        let entry = nodes.entry(node_id.to_owned()).or_insert_with(|| {
            migrated.unwrap_or_else(|| NodeEntry {
                node_id: node_id.to_owned(),
                hostname: node_id.to_owned(),
                status: NodeStatus::Offline,
                slots_used: 0,
                slots_total: 0,
                version: String::new(),
                local: false,
                last_heartbeat: Instant::now(),
                transport: NodeTransport::Iroh,
                iroh_node_id: Some(iroh_node_id.to_owned()),
                agents: vec![],
                connection_epoch: None,
                enrolled_at: Some(now),
                last_seen: None,
                persisted_last_seen: 0,
            })
        });
        let placeholder_hostname = entry.hostname == entry.node_id;
        entry.node_id = node_id.to_owned();
        if placeholder_hostname {
            entry.hostname = node_id.to_owned();
        }
        entry.iroh_node_id = Some(iroh_node_id.to_owned());
        entry.enrolled_at.get_or_insert(now);

        self.persist_inventory(&nodes)
    }

    fn persist_inventory(&self, nodes: &HashMap<String, NodeEntry>) -> anyhow::Result<()> {
        let Some(path) = &self.inventory_path else {
            return Ok(());
        };
        let durable: Vec<_> = nodes
            .values()
            .filter_map(|node| {
                Some(DurableNode {
                    node_id: node.node_id.clone(),
                    iroh_node_id: node.iroh_node_id.clone()?,
                    enrolled_at: node.enrolled_at?,
                    last_seen: node.last_seen,
                    last_version: node.version.clone(),
                })
            })
            .collect();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&durable)?)?;
        std::fs::rename(tmp, path.as_ref())?;
        Ok(())
    }

    /// Register or re-register a node outside a connection lifecycle.
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
        self.register_inner(
            node_id,
            hostname,
            slots_total,
            version,
            local,
            transport,
            iroh_node_id,
            agents,
            None,
        )
        .await;
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn register_connection(
        &self,
        node_id: &str,
        hostname: &str,
        slots_total: u32,
        version: &str,
        transport: NodeTransport,
        iroh_node_id: Option<String>,
        agents: Vec<AgentInfo>,
        epoch: u64,
    ) -> bool {
        self.register_inner(
            node_id,
            hostname,
            slots_total,
            version,
            false,
            transport,
            iroh_node_id,
            agents,
            Some(epoch),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn register_inner(
        &self,
        node_id: &str,
        hostname: &str,
        slots_total: u32,
        version: &str,
        local: bool,
        transport: NodeTransport,
        iroh_node_id: Option<String>,
        agents: Vec<AgentInfo>,
        epoch: Option<u64>,
    ) -> bool {
        let mut nodes = self.nodes.write().await;
        let enrolled = iroh_node_id.as_ref().and_then(|key| {
            nodes
                .iter()
                .find(|(_, entry)| entry.iroh_node_id.as_ref() == Some(key))
                .map(|(id, entry)| {
                    (
                        id.clone(),
                        entry.enrolled_at,
                        entry.last_seen,
                        entry.persisted_last_seen,
                        entry.connection_epoch,
                    )
                })
        });
        if let Some(new) = epoch {
            let current = nodes
                .get(node_id)
                .and_then(|entry| entry.connection_epoch)
                .or_else(|| enrolled.as_ref().and_then(|entry| entry.4));
            if current.is_some_and(|current| new < current) {
                return false;
            }
        }
        if let Some((old_id, _, _, _, _)) = &enrolled {
            if old_id != node_id {
                nodes.remove(old_id);
            }
        }
        let now = unix_now();
        let (_, enrolled_at, previous_seen, persisted_last_seen, _) =
            enrolled.unwrap_or_else(|| (node_id.to_owned(), None, None, 0, None));
        nodes.insert(
            node_id.to_owned(),
            NodeEntry {
                node_id: node_id.to_owned(),
                hostname: hostname.to_owned(),
                status: NodeStatus::Online,
                slots_used: 0,
                slots_total,
                version: version.to_owned(),
                local,
                last_heartbeat: Instant::now(),
                transport,
                iroh_node_id,
                agents,
                connection_epoch: epoch,
                enrolled_at,
                last_seen: enrolled_at.map(|_| now).or(previous_seen),
                persisted_last_seen,
            },
        );
        let _ = self.persist_inventory(&nodes);
        true
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

    /// Per-node agent catalog for node-aware session creation. Unlike
    /// `all_agents`, this preserves duplicate agent ids on different nodes.
    pub async fn agent_catalog(&self) -> Vec<NodeInfo> {
        self.list().await
    }

    /// Update a node's heartbeat and slot usage.
    pub async fn heartbeat(
        &self,
        node_id: &str,
        slots_used: u32,
        epoch: Option<u64>,
    ) -> Result<(), NodeError> {
        let mut nodes = self.nodes.write().await;
        let now = unix_now();
        let persist = {
            let entry = nodes
                .get_mut(node_id)
                .ok_or(NodeError::UnknownNode(node_id.to_string()))?;
            if epoch.is_some() && entry.connection_epoch != epoch {
                return Err(NodeError::StaleConnection(node_id.to_string()));
            }
            entry.last_heartbeat = Instant::now();
            entry.slots_used = slots_used;
            entry.last_seen = entry.enrolled_at.map(|_| now);
            if entry.status == NodeStatus::Offline {
                entry.status = NodeStatus::Online;
            }
            let persist =
                entry.enrolled_at.is_some() && now.saturating_sub(entry.persisted_last_seen) >= 60;
            if persist {
                entry.persisted_last_seen = now;
            }
            persist
        };
        if persist {
            let _ = self.persist_inventory(&nodes);
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

    /// Remove a node identity from the registry (operator action/tests).
    pub async fn deregister(&self, node_id: &str) {
        let mut nodes = self.nodes.write().await;
        nodes.remove(node_id);
        let _ = self.persist_inventory(&nodes);
        self.roles.write().await.remove(node_id);
    }

    /// Tear down only the registration owned by `epoch`. Durable enrolled
    /// identity remains visible offline; a stale epoch is a no-op.
    pub async fn deregister_connection(&self, node_id: &str, epoch: u64) -> bool {
        let mut nodes = self.nodes.write().await;
        let Some(entry) = nodes.get(node_id) else {
            return false;
        };
        if entry.connection_epoch != Some(epoch) {
            return false;
        }
        if entry.enrolled_at.is_some() {
            let entry = nodes.get_mut(node_id).unwrap();
            entry.status = NodeStatus::Offline;
            entry.connection_epoch = None;
            entry.slots_used = 0;
        } else {
            nodes.remove(node_id);
        }
        let _ = self.persist_inventory(&nodes);
        self.roles.write().await.remove(node_id);
        true
    }

    pub async fn set_roles(&self, node_id: &str, roles: Vec<olympus_proto::frames::NodeRole>) {
        self.roles.write().await.insert(node_id.to_owned(), roles);
    }

    pub async fn has_role(&self, node_id: &str, role: olympus_proto::frames::NodeRole) -> bool {
        self.roles
            .read()
            .await
            .get(node_id)
            .is_some_and(|roles| roles.contains(&role))
    }

    /// List all nodes with current status, evicting stale ones.
    /// This is the function the `/api/nodes` handler calls.
    pub async fn list(&self) -> Vec<NodeInfo> {
        let now = Instant::now();
        let mut nodes = self.nodes.write().await;

        // Evict nodes that have been offline too long.
        nodes.retain(|_, e| {
            if e.local || e.enrolled_at.is_some() {
                return true; // durable identity remains visible while offline
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
                enrolled_at: e.enrolled_at,
                last_seen: e.last_seen,
                last_version: e.version.clone(),
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
            enrolled_at: e.enrolled_at,
            last_seen: e.last_seen,
            last_version: e.version.clone(),
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
    StaleConnection(String),
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownNode(id) => write!(f, "unknown node: {id}"),
            Self::StaleConnection(id) => write!(f, "stale connection for node: {id}"),
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
/// Supports the protocol-v1 `EnvoyFrame` wire contract and the unversioned legacy
/// `NodeMessage` registration frames kept for old local envoys.
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
    let epoch = envoy_conns.allocate_epoch();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let mut connected_node: Option<String> = None;
    // The EnvoyConnection (set on hello). All writes to the envoy go through
    // its buffered writer. For legacy connections, we fall back to writing
    // directly via the raw writer.
    let mut conn: Option<Arc<crate::server::envoy_conn::EnvoyConnection>> = None;
    let mut legacy_writer: Option<crate::server::envoy_conn::BoxedWriter> = Some(Box::new(writer));

    loop {
        let next = tokio::select! {
            line = lines.next_line() => line,
            _ = shutdown_rx.changed() => break,
        };
        let line = match next {
            Ok(Some(l)) => l,
            Ok(None) => break, // EOF — peer disconnected
            Err(_) => break,   // read error
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        // Try parsing as EnvoyFrame first. EnvoyFrame and
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
                        epoch,
                        shutdown_tx.clone(),
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
            let outcome = handle_envoy_frame(
                frame,
                &registry,
                &envoy_conns,
                &mut conn,
                transport,
                peer_iroh_id.clone(),
                epoch,
            )
            .await;
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
                if !registry
                    .register_connection(
                        &node_id,
                        &hostname,
                        slots_total,
                        &version,
                        transport,
                        peer_iroh_id.clone(),
                        Vec::new(),
                        epoch,
                    )
                    .await
                {
                    break;
                }
                connected_node = Some(node_id);
                NodeResponse::Welcome { status: "ok" }
            }
            NodeMessage::Heartbeat {
                node_id,
                slots_used,
            } => {
                if let Err(e) = registry.heartbeat(&node_id, slots_used, Some(epoch)).await {
                    NodeResponse::Error {
                        message: e.to_string(),
                    }
                } else {
                    NodeResponse::Ack { status: "ok" }
                }
            }
            NodeMessage::Bye { node_id } => {
                tracing::info!(node = %node_id, "node deregistered (bye)");
                registry.deregister_connection(&node_id, epoch).await;
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
        registry.deregister_connection(&node_id, epoch).await;
        // Fail all pending requests on this envoy's connection and remove it.
        if let Some(conn) = envoy_conns.remove_epoch(&node_id, epoch).await {
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

/// Handle an EnvoyFrame::Hello: validate protocol version, register the node,
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
    epoch: u64,
    shutdown: tokio::sync::watch::Sender<bool>,
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
        roles,
        job_attempts,
    } = frame
    else {
        unreachable!("handle_envoy_hello called with non-Hello frame")
    };

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
        "envoy registered"
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

    if !registry
        .register_connection(
            &node_id,
            &hostname,
            slots_total,
            &version_str,
            transport,
            peer_iroh_id,
            agents_parsed,
            epoch,
        )
        .await
    {
        return HelloOutcome::Rejected;
    }
    registry.set_roles(&node_id, roles).await;

    // Publish the new epoch before closing the superseded connection.
    let (conn, old) = match envoy_conns
        .insert_epoch(&node_id, writer, epoch, shutdown)
        .await
    {
        Ok(inserted) => inserted,
        Err(stale) => {
            stale.close().await;
            return HelloOutcome::Rejected;
        }
    };
    if let Some(old) = old {
        tokio::spawn(async move { old.close().await });
    }
    let pending_dispatches = conn
        .pending_job_dispatches(&node_id, &job_attempts)
        .unwrap_or_default();
    if let Err(error) = conn.reconcile_jobs(&node_id, &job_attempts) {
        tracing::error!(node = %node_id, %error, "reconciling durable job attempts");
    }
    for frame in pending_dispatches {
        if let Err(error) = conn.send_request(frame).await {
            tracing::error!(node = %node_id, %error, "replaying durable job dispatch intent");
        }
    }
    for attempt in &job_attempts {
        let identity = crate::jobs::wire_id(&attempt.job_id, attempt.attempt_epoch);
        let watermark = conn.watermark(&identity).ok().flatten().unwrap_or(u64::MAX);
        if let Err(error) = conn
            .send_request(olympus_proto::frames::HallFrame::ResumeFrom {
                session_id: identity,
                seq: watermark,
            })
            .await
        {
            tracing::error!(job = %attempt.job_id, %error, "requesting durable job spool replay");
        }
    }
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

async fn reregister_hello(
    frame: olympus_proto::frames::EnvoyFrame,
    registry: &NodeRegistry,
    transport: NodeTransport,
    peer_iroh_id: Option<String>,
    epoch: u64,
) -> bool {
    use olympus_proto::frames::EnvoyFrame;
    use olympus_proto::version::PROTOCOL_VERSION;

    let EnvoyFrame::Hello {
        node_id,
        hostname,
        slots_total,
        version: build_version,
        protocol_version,
        agents,
        roles,
        ..
    } = frame
    else {
        return false;
    };
    if protocol_version != PROTOCOL_VERSION
        || !registry.iroh_peer_is_allowlisted(peer_iroh_id.as_deref())
    {
        return false;
    }
    let version = if build_version.git_hash != "unknown" {
        format!("{} ({})", build_version.semver, build_version.git_hash)
    } else {
        build_version.semver
    };
    let agents = agents
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    let accepted = registry
        .register_connection(
            &node_id,
            &hostname,
            slots_total,
            &version,
            transport,
            peer_iroh_id,
            agents,
            epoch,
        )
        .await;
    if accepted {
        registry.set_roles(&node_id, roles).await;
    }
    accepted
}

/// Dispatch a parsed EnvoyFrame (ADR 0008 protocol v1) — all variants except
/// the initial Hello. Resp and Event frames route through `conn`.
async fn handle_envoy_frame(
    frame: olympus_proto::frames::EnvoyFrame,
    registry: &NodeRegistry,
    envoy_conns: &crate::server::envoy_conn::EnvoyConnections,
    conn: &mut Option<Arc<crate::server::envoy_conn::EnvoyConnection>>,
    transport: NodeTransport,
    peer_iroh_id: Option<String>,
    epoch: u64,
) -> FrameOutcome {
    use olympus_proto::frames::EnvoyFrame;

    match frame {
        hello @ EnvoyFrame::Hello { .. } => {
            if !reregister_hello(hello, registry, transport, peer_iroh_id, epoch).await {
                return FrameOutcome::Disconnect;
            }
        }
        EnvoyFrame::Heartbeat {
            node_id,
            slots_used,
        } => {
            let Some(conn) = conn else {
                return FrameOutcome::Disconnect;
            };
            let reply = match registry.heartbeat(&node_id, slots_used, Some(epoch)).await {
                Ok(()) => olympus_proto::frames::HallFrame::HeartbeatAck,
                Err(e) if !registry.iroh_peer_is_allowlisted(peer_iroh_id.as_deref()) => {
                    tracing::warn!(node = %node_id, error = %e, "closing unrepairable envoy heartbeat");
                    return FrameOutcome::Disconnect;
                }
                Err(e) => {
                    tracing::warn!(node = %node_id, error = %e, "heartbeat registration mismatch; requesting registration");
                    olympus_proto::frames::HallFrame::ReRegister
                }
            };
            if conn.send_request(reply).await.is_err() {
                return FrameOutcome::Disconnect;
            }
        }
        EnvoyFrame::Bye { node_id } => {
            tracing::info!(node = %node_id, "envoy deregistered (bye)");
            registry.deregister_connection(&node_id, epoch).await;
            if let Some(conn) = envoy_conns.remove_epoch(&node_id, epoch).await {
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
        EnvoyFrame::JobOutput {
            job_id,
            attempt_epoch,
            seq,
            stream,
            data,
        } => {
            if let Some(c) = conn {
                if let Err(error) = c
                    .apply_job_output(&job_id, attempt_epoch, seq, stream, data)
                    .await
                {
                    tracing::warn!(job = %job_id, attempt_epoch, seq, %error, "job output rejected; leaving unacked");
                }
            }
        }
        EnvoyFrame::JobResult {
            job_id,
            attempt_epoch,
            seq,
            exit_code,
            truncated,
            timed_out,
            cancelled,
        } => {
            if let Some(c) = conn {
                if let Err(error) = c
                    .apply_job_result(
                        &job_id,
                        attempt_epoch,
                        seq,
                        exit_code,
                        truncated,
                        timed_out,
                        cancelled,
                    )
                    .await
                {
                    tracing::warn!(job = %job_id, attempt_epoch, seq, %error, "job result rejected; leaving unacked");
                }
            }
        }
        EnvoyFrame::TerminalOutput {
            terminal_id,
            data_b64,
        } => {
            // Operator terminal output (ADR 0021): forward to any operator WS
            // subscribed to this terminal. Never logged (shell bytes).
            if let Some(c) = conn {
                c.forward_terminal(
                    &terminal_id,
                    crate::server::envoy_conn::TerminalFrame::Output { data_b64 },
                );
            }
        }
        EnvoyFrame::TerminalExited {
            terminal_id,
            exit_code,
        } => {
            if let Some(c) = conn {
                c.forward_terminal(
                    &terminal_id,
                    crate::server::envoy_conn::TerminalFrame::Exited { exit_code },
                );
                c.drop_terminal(&terminal_id);
            }
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
    use olympus_proto::version::{BuildVersion, PROTOCOL_VERSION};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    use tokio::time::sleep;

    fn test_agent(id: &str) -> AgentInfo {
        AgentInfo {
            id: id.into(),
            provider: Some("test-provider".into()),
            model: Some("test-model".into()),
            models: vec![],
            version: None,
            kind: "hermes".into(),
            is_default: id == "default",
            ready: None,
        }
    }

    fn hello(node_id: &str) -> olympus_proto::frames::EnvoyFrame {
        hello_version(node_id, PROTOCOL_VERSION)
    }

    fn hello_version(node_id: &str, protocol_version: u32) -> olympus_proto::frames::EnvoyFrame {
        olympus_proto::frames::EnvoyFrame::Hello {
            node_id: node_id.into(),
            hostname: "host".into(),
            slots_total: 4,
            protocol_version,
            version: BuildVersion::for_binary("test"),
            agents: None,
            runtimes: vec![],
            roles: vec![],
            job_attempts: vec![],
        }
    }

    #[tokio::test]
    async fn agent_catalog_preserves_duplicate_agent_ids_per_node() {
        let reg = NodeRegistry::new();
        reg.register(
            "local",
            "localhost",
            4,
            "0.1",
            true,
            NodeTransport::Local,
            None,
            vec![test_agent("default")],
        )
        .await;
        reg.register(
            "fx-zephyrus",
            "zephyrus",
            4,
            "0.1",
            false,
            NodeTransport::Iroh,
            None,
            vec![test_agent("default"), test_agent("codex")],
        )
        .await;

        let catalog = reg.agent_catalog().await;
        let pairs: Vec<_> = catalog
            .iter()
            .flat_map(|node| {
                node.agents
                    .iter()
                    .map(move |agent| format!("{}:{}", node.node_id, agent.id))
            })
            .collect();

        assert!(pairs.contains(&"local:default".to_string()));
        assert!(pairs.contains(&"fx-zephyrus:default".to_string()));
        assert!(pairs.contains(&"fx-zephyrus:codex".to_string()));
        assert_eq!(reg.all_agents().await.len(), 2);
    }

    #[tokio::test]
    async fn stale_connection_death_does_not_deregister_newer_epoch() {
        let reg = NodeRegistry::new();
        assert!(
            reg.register_connection(
                "node",
                "old",
                4,
                "v1",
                NodeTransport::Iroh,
                Some("key".into()),
                vec![],
                1
            )
            .await
        );
        assert!(
            reg.register_connection(
                "node",
                "new",
                4,
                "v2",
                NodeTransport::Iroh,
                Some("key".into()),
                vec![],
                2
            )
            .await
        );
        assert!(
            !reg.register_connection(
                "renamed",
                "old",
                4,
                "v1",
                NodeTransport::Iroh,
                Some("key".into()),
                vec![],
                1
            )
            .await
        );

        assert!(!reg.deregister_connection("node", 1).await);
        let node = reg.get("node").await.expect("new registration survives");
        assert_eq!(node.hostname, "new");
        assert_eq!(node.version, "v2");
    }

    #[tokio::test]
    async fn newer_hello_supersedes_and_closes_old_connection() {
        let registry = NodeRegistry::new();
        let conns = crate::server::envoy_conn::EnvoyConnections::new();
        let (old_hall, mut old_envoy) = tokio::io::duplex(4096);
        let (old_reader, old_writer) = tokio::io::split(old_hall);
        let old_task = tokio::spawn(handle_envoy_conn(
            old_reader,
            old_writer,
            registry.clone(),
            conns.clone(),
            NodeTransport::Iroh,
            Some("key".into()),
        ));
        old_envoy
            .write_all(format!("{}\n", serde_json::to_string(&hello("node")).unwrap()).as_bytes())
            .await
            .unwrap();
        while registry.get("node").await.is_none() {
            tokio::task::yield_now().await;
        }

        let (new_hall, mut new_envoy) = tokio::io::duplex(4096);
        let (new_reader, new_writer) = tokio::io::split(new_hall);
        let new_task = tokio::spawn(handle_envoy_conn(
            new_reader,
            new_writer,
            registry.clone(),
            conns.clone(),
            NodeTransport::Iroh,
            Some("key".into()),
        ));
        new_envoy
            .write_all(format!("{}\n", serde_json::to_string(&hello("node")).unwrap()).as_bytes())
            .await
            .unwrap();

        tokio::time::timeout(Duration::from_secs(1), old_task)
            .await
            .expect("superseded connection must be actively closed")
            .unwrap();
        assert!(registry.get("node").await.is_some());
        drop(new_envoy);
        new_task.await.unwrap();
    }

    #[tokio::test]
    async fn unknown_node_heartbeat_requests_reregistration() {
        let registry = NodeRegistry::new();
        let conns = crate::server::envoy_conn::EnvoyConnections::new();
        let (hall, envoy) = tokio::io::duplex(4096);
        let (hall_reader, hall_writer) = tokio::io::split(hall);
        let (envoy_reader, mut envoy_writer) = tokio::io::split(envoy);
        let task = tokio::spawn(handle_envoy_conn(
            hall_reader,
            hall_writer,
            registry.clone(),
            conns,
            NodeTransport::Iroh,
            Some("key".into()),
        ));
        envoy_writer
            .write_all(format!("{}\n", serde_json::to_string(&hello("node")).unwrap()).as_bytes())
            .await
            .unwrap();
        while registry.get("node").await.is_none() {
            tokio::task::yield_now().await;
        }
        registry.deregister("node").await;
        let heartbeat = olympus_proto::frames::EnvoyFrame::Heartbeat {
            node_id: "node".into(),
            slots_used: 0,
        };
        envoy_writer
            .write_all(format!("{}\n", serde_json::to_string(&heartbeat).unwrap()).as_bytes())
            .await
            .unwrap();

        let mut lines = tokio::io::BufReader::new(envoy_reader).lines();
        let line = tokio::time::timeout(Duration::from_secs(1), lines.next_line())
            .await
            .expect("Hall must repair unknown heartbeat")
            .unwrap()
            .unwrap();
        assert!(matches!(
            serde_json::from_str::<olympus_proto::frames::HallFrame>(&line).unwrap(),
            olympus_proto::frames::HallFrame::ReRegister
        ));
        drop(envoy_writer);
        task.abort();
    }

    #[tokio::test]
    async fn stale_connection_heartbeat_does_not_mutate_newer_registration() {
        let registry = NodeRegistry::new();
        for epoch in [1, 2] {
            assert!(
                registry
                    .register_connection(
                        "node",
                        "host",
                        4,
                        "v3",
                        NodeTransport::Iroh,
                        Some("key".into()),
                        vec![],
                        epoch,
                    )
                    .await
            );
        }

        let error = registry.heartbeat("node", 3, Some(1)).await.unwrap_err();
        assert_eq!(error, NodeError::StaleConnection("node".into()));
        assert_eq!(registry.get("node").await.unwrap().slots_used, 0);
    }

    #[tokio::test]
    async fn revoked_iroh_connection_cannot_reregister() {
        let dir = tempfile::tempdir().unwrap();
        let key = "83141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499320";
        crate::enroll::allowlist_add(dir.path(), key).unwrap();
        let registry = NodeRegistry::with_inventory(dir.path()).unwrap();
        registry.enroll("node", key).await.unwrap();
        crate::enroll::allowlist_remove(dir.path(), key).unwrap();
        registry.deregister("node").await;

        assert!(
            !reregister_hello(
                hello("node"),
                &registry,
                NodeTransport::Iroh,
                Some(key.into()),
                1,
            )
            .await
        );
        assert!(registry.get("node").await.is_none());
    }

    #[tokio::test]
    async fn hello_rejects_non_v1_protocols() {
        for protocol_version in [0, 2] {
            let registry = NodeRegistry::new();
            let conns = crate::server::envoy_conn::EnvoyConnections::new();
            let (hall, mut envoy) = tokio::io::duplex(4096);
            let (reader, writer) = tokio::io::split(hall);
            let task = tokio::spawn(handle_envoy_conn(
                reader,
                writer,
                registry.clone(),
                conns,
                NodeTransport::Uds,
                None,
            ));
            envoy
                .write_all(
                    format!(
                        "{}\n",
                        serde_json::to_string(&hello_version("node", protocol_version)).unwrap()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();

            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .expect("protocol mismatch must close the connection")
                .unwrap();
            assert!(registry.get("node").await.is_none());
        }
    }

    #[test]
    fn corrupt_inventory_is_quarantined_instead_of_bricking_startup() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("nodes.json"), b"not json").unwrap();

        NodeRegistry::with_inventory(dir.path()).expect("rebuildable inventory must fail open");
        assert!(dir.path().join("nodes.json.corrupt").exists());
    }

    #[tokio::test]
    async fn enrollment_name_replaces_allowlist_key_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let key = "83141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499320";
        crate::enroll::allowlist_add(dir.path(), key).unwrap();
        let registry = NodeRegistry::with_inventory(dir.path()).unwrap();
        assert!(registry.get(key).await.is_some());

        registry.enroll("talos", key).await.unwrap();
        assert!(registry.get(key).await.is_none());
        assert_eq!(
            registry.get("talos").await.unwrap().iroh_node_id.as_deref(),
            Some(key)
        );
        drop(registry);

        let restarted = NodeRegistry::with_inventory(dir.path()).unwrap();
        let nodes = restarted.list().await;
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_id, "talos");
        assert_eq!(nodes[0].status, NodeStatus::Offline);
    }

    #[tokio::test]
    async fn enrolled_offline_node_survives_registry_restart() {
        let dir = tempfile::tempdir().unwrap();
        let registry = NodeRegistry::with_inventory(dir.path()).unwrap();
        registry
            .enroll(
                "node",
                "83141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499320",
            )
            .await
            .unwrap();
        drop(registry);

        let restarted = NodeRegistry::with_inventory(dir.path()).unwrap();
        let node = restarted.get("node").await.expect("durable enrolled node");
        assert_eq!(node.status, NodeStatus::Offline);
        assert_eq!(node.version, "");
    }

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

        reg.heartbeat("node-1", 2, None).await.unwrap();

        let nodes = reg.list().await;
        assert_eq!(nodes[0].slots_used, 2);
    }

    #[tokio::test]
    async fn heartbeat_unknown_node_fails() {
        let reg = NodeRegistry::new();
        let err = reg.heartbeat("ghost", 1, None).await.unwrap_err();
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
