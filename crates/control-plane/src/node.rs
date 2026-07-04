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
    pub async fn register(
        &self,
        node_id: &str,
        hostname: &str,
        slots_total: u32,
        version: &str,
        local: bool,
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
/// On disconnect, the node is deregistered. On `bye`, the node is deregistered
/// and the connection closes.
pub async fn run_uds_listener(path: std::path::PathBuf, registry: NodeRegistry) {
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
        tokio::spawn(handle_uds_conn(stream, reg));
    }
}

/// Handle a single UDS connection (one envoy's lifecycle).
async fn handle_uds_conn(stream: tokio::net::UnixStream, registry: NodeRegistry) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let mut connected_node: Option<String> = None;

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

        let msg: NodeMessage = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                let resp = NodeResponse::Error {
                    message: format!("bad json: {e}"),
                };
                let _ = writer
                    .write_all(format!("{}\n", serde_json::to_string(&resp).unwrap()).as_bytes())
                    .await;
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
                tracing::info!(node = %node_id, hostname = %hostname, "node registered");
                registry
                    .register(&node_id, &hostname, slots_total, &version, false, Vec::new())
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
                let resp = NodeResponse::Ack { status: "ok" };
                let _ = writer
                    .write_all(format!("{}\n", serde_json::to_string(&resp).unwrap()).as_bytes())
                    .await;
                break;
            }
        };

        let _ = writer
            .write_all(format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes())
            .await;
    }

    // Connection closed — deregister the node if it was registered.
    if let Some(node_id) = connected_node {
        tracing::info!(node = %node_id, "node disconnected, deregistering");
        registry.deregister(&node_id).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn register_and_list() {
        let reg = NodeRegistry::new();
        reg.register("node-1", "host-1", 4, "0.1", false, vec![]).await;

        let nodes = reg.list().await;
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_id, "node-1");
        assert_eq!(nodes[0].status, NodeStatus::Online);
        assert_eq!(nodes[0].slots_total, 4);
    }

    #[tokio::test]
    async fn heartbeat_updates_slots() {
        let reg = NodeRegistry::new();
        reg.register("node-1", "host-1", 4, "0.1", false, vec![]).await;

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
        reg.register("node-1", "host-1", 4, "0.1", false, vec![]).await;
        assert_eq!(reg.list().await.len(), 1);

        reg.deregister("node-1").await;
        assert_eq!(reg.list().await.len(), 0);
    }

    #[tokio::test]
    async fn re_register_updates_fields() {
        let reg = NodeRegistry::new();
        reg.register("node-1", "host-1", 2, "0.1", false, vec![]).await;
        // Re-register with updated capacity
        reg.register("node-1", "host-1", 8, "0.2", false, vec![]).await;

        let nodes = reg.list().await;
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].slots_total, 8);
        assert_eq!(nodes[0].version, "0.2");
    }

    #[tokio::test]
    async fn local_node_never_evicted() {
        let reg = NodeRegistry::new();
        reg.register("local", "localhost", 4, "0.1", true, vec![]).await;

        // Even after a long wait, local node stays.
        sleep(Duration::from_millis(50)).await;
        let nodes = reg.list().await;
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].status, NodeStatus::Online);
    }

    #[tokio::test]
    async fn set_draining_changes_status() {
        let reg = NodeRegistry::new();
        reg.register("node-1", "host-1", 4, "0.1", false, vec![]).await;

        reg.set_draining("node-1").await.unwrap();
        let nodes = reg.list().await;
        assert_eq!(nodes[0].status, NodeStatus::Draining);
    }

    #[tokio::test]
    async fn online_count() {
        let reg = NodeRegistry::new();
        reg.register("n1", "h1", 4, "0.1", false, vec![]).await;
        reg.register("n2", "h2", 4, "0.1", false, vec![]).await;
        reg.set_draining("n2").await.unwrap();

        assert_eq!(reg.online_count().await, 1);
    }

    #[tokio::test]
    async fn get_single_node() {
        let reg = NodeRegistry::new();
        reg.register("n1", "h1", 4, "0.1", false, vec![]).await;

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
