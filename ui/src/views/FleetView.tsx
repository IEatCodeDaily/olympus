// FleetView — fleet + agents operator view (roadmap U4, revamped).
//
// Renders two sub-views behind a tab strip:
//   • Fleet  — responsive grid of node cards (status / transport / slots /
//              heartbeat); clicking one slides in a detail drawer with the
//              node's agents, its live sessions, and Drain / Remove actions.
//              "Add node" mints an enroll token and shows the one-line
//              curl-able setup command for a remote envoy.
//   • Agents — agents grouped by node, each row showing icon, name, model.
//
// Data: /api/nodes (10s poll). Node sessions: /api/sessions?node=<id>.
// Enrollment: POST /api/enroll → { command } (short-lived capability token).
import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Icon } from "../components/Icon";
import { BrandIcon, agentBrand } from "../components/BrandIcons";
import { useNodes, useSessions } from "../hooks/queries";
import { refreshNodeAgents, mintEnroll, drainNode, removeNode } from "../api";
import { relativeTime } from "../lib/format";
import type { AgentInfo, EnrollResponse, NodeInfo, NodeStatus } from "../types";

// ── Local fleet model ──────────────────────────────
// FleetNode maps the backend NodeInfo to the display fields the drawer needs.
type FleetNode = NodeInfo;

type SubView = "fleet" | "agents";

// ── Helpers ────────────────────────────────────────

function statusTagClass(status: NodeStatus): string {
  if (status === "online") return "gtag ok";
  if (status === "draining") return "gtag warn";
  return "gtag err";
}

function statusDotColor(status: NodeStatus): string {
  if (status === "online") return "var(--green)";
  if (status === "draining") return "var(--amber)";
  return "var(--red)";
}

/** Transport label for the badge: how the node reaches the Hall. */
function transportLabel(node: FleetNode): string {
  if (node.local) return "local";
  return node.transport; // "uds" | "iroh"
}

function slotPct(used: number, total: number): number {
  if (total <= 0) return 0;
  return Math.max(0, Math.min(100, (used / total) * 100));
}

/** Heartbeat label with second precision (the 10s nodes poll makes this meaningful). */
function heartbeatLabel(epochSecAgo: number): string {
  if (epochSecAgo < 60) return `${epochSecAgo}s ago`;
  return relativeTime(Math.floor(Date.now() / 1000) - epochSecAgo);
}

// ── Sub-components ─────────────────────────────────

function TabStrip({
  sub,
  onChange,
}: {
  sub: SubView;
  onChange: (s: SubView) => void;
}) {
  return (
    <div className="nodes-toolbar" role="tablist" aria-label="Fleet sub-view">
      <div className="nodes-filter-group">
        {(["fleet", "agents"] as const).map((key) => (
          <button
            key={key}
            role="tab"
            aria-selected={sub === key}
            type="button"
            className={`nodes-filter ${sub === key ? "active" : ""}`}
            onClick={() => onChange(key)}
          >
            {key === "fleet" ? "Fleet" : "Agents"}
          </button>
        ))}
      </div>
    </div>
  );
}

function NodeCard({
  node,
  selected,
  onClick,
}: {
  node: FleetNode;
  selected: boolean;
  onClick: () => void;
}) {
  const pct = slotPct(node.slotsUsed, node.slotsTotal);
  return (
    <div
      role="button"
      tabIndex={0}
      data-node-id={node.nodeId}
      className={`gcard click ${selected ? "selected" : ""}`}
      style={selected ? { borderColor: "var(--border-strong)" } : undefined}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick();
        }
      }}
    >
      <div className="grow" style={{ marginBottom: 10 }}>
        <span className="gtitle" style={{ fontSize: 13, fontFamily: "var(--mono)" }}>
          {node.nodeId}
        </span>
        <span style={{ display: "inline-flex", gap: 4 }}>
          <span className="gtag">{transportLabel(node)}</span>
          <span className={statusTagClass(node.status)}>{node.status}</span>
        </span>
      </div>
      <div className="grow" style={{ fontSize: 12, color: "var(--dim)", marginBottom: 6 }}>
        <span>slots</span>
        <span>
          {node.slotsUsed} / {node.slotsTotal}
        </span>
      </div>
      <div className="gbar">
        <i style={{ width: `${pct}%` }} />
      </div>
      <div className="grow" style={{ fontSize: 11, color: "var(--faint)", marginTop: 9 }}>
        <span>heartbeat</span>
        <span>{heartbeatLabel(node.lastHeartbeatAgoSecs)}</span>
      </div>
    </div>
  );
}

function EmptyState({ title, message }: { title: string; message: string }) {
  return (
    <div className="empty-state">
      <Icon name="server" size={28} />
      <div className="empty-state-title">{title}</div>
      <div className="empty-state-msg">{message}</div>
    </div>
  );
}

/** "Add node" modal — mints an enroll token and shows the one-liner. */
function AddNodeModal({ onClose }: { onClose: () => void }) {
  const [enroll, setEnroll] = useState<EnrollResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let cancelled = false;
    mintEnroll()
      .then((r) => {
        if (!cancelled) setEnroll(r);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const copy = async () => {
    if (!enroll) return;
    try {
      await navigator.clipboard.writeText(enroll.command);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // clipboard unavailable (http origin) — the command is selectable text
    }
  };

  return (
    <div
      className="ol-overlay"
      role="dialog"
      aria-modal="true"
      aria-label="Add node"
      onClick={onClose}
    >
      <div className="ol-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="ol-dialog-head">
          <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
            <Icon name="server" size={18} />
            <div className="ol-dialog-title">Add node</div>
          </div>
          <button
            type="button"
            className="icobtn"
            onClick={onClose}
            title="Close"
            aria-label="Close"
          >
            <Icon name="x" size={14} />
          </button>
        </div>
        <div className="ol-dialog-body">
          {error ? (
            <div style={{ color: "var(--red)" }}>{error}</div>
          ) : !enroll ? (
            <div className="mono" style={{ fontSize: 12, color: "var(--faint)" }}>
              Minting enroll token…
            </div>
          ) : (
            <>
              <p style={{ marginTop: 0 }}>
                Run this on the target host (Linux x86_64, as a regular user).
                It installs the envoy, registers it with this Hall over iroh,
                and starts it under systemd:
              </p>
              <div
                style={{
                  display: "flex",
                  gap: 6,
                  alignItems: "center",
                  background: "var(--chrome)",
                  border: "1px solid var(--line)",
                  borderRadius: 6,
                  padding: "8px 10px",
                }}
              >
                <code
                  className="mono"
                  data-testid="enroll-command"
                  style={{
                    fontSize: 11,
                    whiteSpace: "nowrap",
                    overflowX: "auto",
                    flex: 1,
                  }}
                >
                  {enroll.command}
                </code>
                <button
                  type="button"
                  className="icobtn"
                  title="Copy command"
                  aria-label="Copy command"
                  onClick={copy}
                >
                  <Icon name={copied ? "check" : "copy"} size={13} />
                </button>
              </div>
              <p style={{ fontSize: 11, color: "var(--faint)", marginBottom: 0 }}>
                Token is single-use and expires in{" "}
                {Math.round(enroll.expiresInSecs / 60)} minutes. The node
                appears in the fleet automatically once it connects.
              </p>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/** Live sessions pinned to a node (drawer section). */
function NodeSessions({ nodeId }: { nodeId: string }) {
  const sessionsQ = useSessions({ node: nodeId, limit: 8 });
  const sessions = sessionsQ.data?.sessions ?? [];
  if (sessions.length === 0) {
    return (
      <div className="mono" style={{ fontSize: 11, color: "var(--faint)" }}>
        No live sessions pinned to this node.
      </div>
    );
  }
  return (
    <div
      className="mono"
      style={{ fontSize: 11, color: "var(--dim)", display: "flex", flexDirection: "column", gap: 4 }}
    >
      {sessions.map((s) => (
        <span key={s.id} title={s.id}>
          {s.title ?? s.id} · {s.liveness}
        </span>
      ))}
    </div>
  );
}

function Drawer({
  node,
  agents,
  onClose,
  onDrain,
  onRemove,
  busy,
}: {
  node: FleetNode;
  agents: AgentInfo[];
  onClose: () => void;
  onDrain: () => void;
  onRemove: () => void;
  busy: boolean;
}) {
  return (
    <aside className="drawer on" role="dialog" aria-label={`Node ${node.nodeId}`}>
      <div className="dr-head">
        <span className="dr-title">{node.nodeId}</span>
        <button
          type="button"
          className="icobtn"
          title="Close"
          aria-label="Close drawer"
          onClick={onClose}
        >
          <Icon name="chevron-right" size={14} />
        </button>
      </div>
      <div className="dr-body">
        <div className="kv">
          <span className="k">STATUS</span>
          <span className="v">
            <span className={statusTagClass(node.status)}>{node.status}</span>
          </span>
        </div>
        <div className="kv">
          <span className="k">TRANSPORT</span>
          <span className="v">
            <span className="gtag">{transportLabel(node)}</span>
          </span>
        </div>
        <div className="kv">
          <span className="k">HOST</span>
          <span className="v">{node.hostname}</span>
        </div>
        <div className="kv">
          <span className="k">SLOTS</span>
          <span className="v">
            {node.slotsUsed} / {node.slotsTotal}
          </span>
        </div>
        <div className="kv">
          <span className="k">HEARTBEAT</span>
          <span className="v">{heartbeatLabel(node.lastHeartbeatAgoSecs)}</span>
        </div>
        <div className="kv">
          <span className="k">VERSION</span>
          <span className="v">{node.version}</span>
        </div>
        {node.irohNodeId && (
          <div className="kv" title={node.irohNodeId}>
            <span className="k">IROH ID</span>
            <span className="v mono" style={{ fontSize: 10 }}>
              {node.irohNodeId.slice(0, 16)}…
            </span>
          </div>
        )}

        <div>
          <div className="gk" style={{ marginBottom: 6 }}>
            agents on node
          </div>
          {agents.length > 0 ? (
            <div
              className="mono"
              style={{
                fontSize: 11,
                color: "var(--dim)",
                display: "flex",
                flexDirection: "column",
                gap: 4,
              }}
            >
              {agents.map((a) => (
                <span key={a.id}>
                  {a.id} · {a.model ?? "—"}
                </span>
              ))}
            </div>
          ) : (
            <div className="mono" style={{ fontSize: 11, color: "var(--faint)" }}>
              none configured
            </div>
          )}
        </div>

        <div>
          <div className="gk" style={{ marginBottom: 6 }}>
            sessions
          </div>
          <NodeSessions nodeId={node.nodeId} />
        </div>

        <div className="dr-actions">
          <button
            type="button"
            className="btn"
            disabled={node.local || busy || node.status === "draining"}
            title={node.local ? "The local node cannot be drained" : "Stop routing new sessions to this node"}
            onClick={onDrain}
          >
            Drain
          </button>
          <button
            type="button"
            className="btn"
            disabled={node.local || busy}
            title={
              node.local
                ? "Local node cannot be removed"
                : "Deregister and revoke this node's allowlist entry"
            }
            onClick={onRemove}
          >
            Remove
          </button>
        </div>
      </div>
    </aside>
  );
}

function AgentRow({ agent }: { agent: AgentInfo }) {
  return (
    <div className="agrow" title={`${agent.provider ?? agent.kind} · ${agent.model ?? "—"}`}>
      <BrandIcon name={agentBrand(agent.kind, agent.provider)} size={15} />
      <span className="nm">{agent.id}</span>
      <span className="sp" />
      <span className="gk">{agent.model ?? "—"}</span>
    </div>
  );
}

function NodeSection({
  node,
  agents,
  onDetect,
  detecting,
}: {
  node: FleetNode;
  agents: AgentInfo[];
  onDetect: (nodeId: string) => void;
  detecting: boolean;
}) {
  return (
    <div style={node.status === "offline" ? { opacity: 0.6 } : undefined}>
      <div className="nodehead">
        <span
          style={{
            width: 7,
            height: 7,
            borderRadius: 999,
            background: statusDotColor(node.status),
          }}
        />
        <span className="mono" style={{ fontSize: 12, fontWeight: 600 }}>
          {node.nodeId}
        </span>
        <span className="gk">
          {node.hostname} · {agents.length} agent{agents.length === 1 ? "" : "s"}
        </span>
        <span style={{ flex: 1 }} />
        <button
          type="button"
          className="btn"
          title="Re-detect agents installed on this node"
          disabled={detecting}
          onClick={() => onDetect(node.nodeId)}
        >
          <Icon name="activity" size={12} />
          {detecting ? "Detecting…" : "Detect agents"}
        </button>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {agents.length > 0 ? (
          agents.map((a) => <AgentRow key={a.id} agent={a} />)
        ) : (
          <div className="mono" style={{ fontSize: 11, color: "var(--faint)" }}>
            no agents detected
          </div>
        )}
      </div>
    </div>
  );
}

// ── Main ───────────────────────────────────────────

export default function FleetView() {
  const [sub, setSub] = useState<SubView>("fleet");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detectingNode, setDetectingNode] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [actionBusy, setActionBusy] = useState(false);

  const nodesQ = useNodes();
  const queryClient = useQueryClient();

  // Nodes come directly from the backend now — envoys register via UDS
  // (same host) or iroh (remote). Each node carries its OWN envoy-discovered
  // agents (per-node, not a global control-plane probe).
  const nodes: FleetNode[] = nodesQ.data?.nodes ?? [];

  const selectedNode = selectedId
    ? (nodes.find((n) => n.nodeId === selectedId) ?? null)
    : null;

  const invalidateNodes = async () => {
    await queryClient.invalidateQueries({ queryKey: ["nodes"] });
    await queryClient.invalidateQueries({ queryKey: ["agents"] });
  };

  const handleDetect = async (nodeId: string) => {
    setDetectingNode(nodeId);
    try {
      await refreshNodeAgents(nodeId);
      // Re-fetch nodes so the refreshed per-node agent list shows.
      await invalidateNodes();
    } catch {
      // best-effort; a remote node without an envoy returns 501
    } finally {
      setDetectingNode(null);
    }
  };

  const handleDrain = async () => {
    if (!selectedNode) return;
    setActionBusy(true);
    try {
      await drainNode(selectedNode.nodeId);
      await invalidateNodes();
    } catch {
      // node may have vanished; the poll will reconcile
    } finally {
      setActionBusy(false);
    }
  };

  const handleRemove = async () => {
    if (!selectedNode) return;
    // eslint-disable-next-line no-alert
    if (!window.confirm(`Remove node "${selectedNode.nodeId}" from the fleet? Its allowlist entry is revoked.`)) {
      return;
    }
    setActionBusy(true);
    try {
      await removeNode(selectedNode.nodeId);
      setSelectedId(null);
      await invalidateNodes();
    } catch {
      // surfaced via the poll; keep the drawer open on failure
    } finally {
      setActionBusy(false);
    }
  };

  return (
    <>
      <div className="gv-head">
        <span className="gv-title">{sub === "fleet" ? "Fleet" : "Agents"}</span>
        <span className="gv-sub">
          {sub === "fleet" ? "· nodes" : "· configured per node"}
        </span>
        <div className="gv-actions">
          {sub === "fleet" && (
            <button
              type="button"
              className="btn"
              title="Enroll a new remote node (one-line setup)"
              data-testid="add-node"
              onClick={() => setAddOpen(true)}
            >
              <Icon name="plus" size={12} />
              Add node
            </button>
          )}
          {sub === "agents" && (
            <button type="button" className="icobtn" title="Add agent" aria-label="Add agent">
              <Icon name="plus" size={14} />
            </button>
          )}
        </div>
      </div>

      <div className="gv-body">
        <TabStrip sub={sub} onChange={setSub} />

        {sub === "fleet" ? (
          <div className="gv-wrap">
            <div
              className="ggrid"
              data-testid="fleet-grid"
              style={{ gridTemplateColumns: "repeat(auto-fill,minmax(230px,1fr))" }}
            >
              {nodes.map((node) => (
                <NodeCard
                  key={node.nodeId}
                  node={node}
                  selected={selectedNode?.nodeId === node.nodeId}
                  onClick={() => setSelectedId(node.nodeId)}
                />
              ))}
            </div>

            {nodes.length <= 1 && (
              <div style={{ marginTop: 16 }}>
                <EmptyState
                  title="Single-node fleet"
                  message="No other nodes registered. Click “Add node” to get a one-line setup command for a remote envoy."
                />
              </div>
            )}

            {selectedNode && (
              <Drawer
                node={selectedNode}
                agents={selectedNode.agents ?? []}
                onClose={() => setSelectedId(null)}
                onDrain={handleDrain}
                onRemove={handleRemove}
                busy={actionBusy}
              />
            )}

            {addOpen && <AddNodeModal onClose={() => setAddOpen(false)} />}
          </div>
        ) : (
          <div style={{ maxWidth: 680, display: "flex", flexDirection: "column", gap: 20 }}>
            {nodes.map((node) => (
              <NodeSection
                key={node.nodeId}
                node={node}
                agents={node.agents ?? []}
                onDetect={handleDetect}
                detecting={detectingNode === node.nodeId}
              />
            ))}

            {nodes.length <= 1 && (
              <EmptyState
                title="No other nodes registered"
                message="Agents above run on the local node. Enroll remote envoys from the Fleet tab to see their agents here."
              />
            )}
          </div>
        )}
      </div>
    </>
  );
}
