// FleetView — fleet + agents operator view (roadmap U4).
//
// Renders two sub-views behind a tab strip:
//   • Fleet  — responsive grid of node cards; clicking one slides in a detail
//              drawer (status / bind / slots / heartbeat / version, agents on
//              node, sessions, and Drain/Restart/Remove actions).
//   • Agents — agents grouped by node, each row showing icon, name, type tag
//              (acp/cli), model, and status.
//
// Data: the real /api/nodes contract lands with backend Epic L. Until then we
// synthesize the single "local" node from the health probe + configured agents
// and show an honest "no other nodes registered" empty state. When the fleet
// endpoint ships, swap `useFleetNodes()` for the real hook.
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Icon } from "../components/Icon";
import { BrandIcon, agentBrand } from "../components/BrandIcons";
import { useNodes } from "../hooks/queries";
import { refreshNodeAgents } from "../api";
import { relativeTime } from "../lib/format";
import type { AgentInfo, NodeInfo, NodeStatus } from "../types";

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
        <span className={statusTagClass(node.status)}>{node.status}</span>
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

function Drawer({
  node,
  agents,
  onClose,
}: {
  node: FleetNode;
  agents: AgentInfo[];
  onClose: () => void;
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
          <div className="mono" style={{ fontSize: 11, color: "var(--faint)" }}>
            No live sessions pinned to this node.
          </div>
        </div>

        <div className="dr-actions">
          <button type="button" className="btn" disabled={node.local}>
            Drain
          </button>
          <button type="button" className="btn">
            Restart
          </button>
          <button type="button" className="btn" disabled={node.local} title={node.local ? "Local node cannot be removed" : undefined}>
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

  const nodesQ = useNodes();
  const queryClient = useQueryClient();

  // Nodes come directly from the backend now — the local node auto-registers
  // at boot; remote envoys register via UDS. Each node carries its OWN
  // envoy-discovered agents (per-node, not a global control-plane probe).
  const nodes: FleetNode[] = nodesQ.data?.nodes ?? [];

  const selectedNode =
    nodes.find((n) => n.nodeId === selectedId) ?? (selectedId ? null : nodes[0] ?? null);

  const handleDetect = async (nodeId: string) => {
    setDetectingNode(nodeId);
    try {
      await refreshNodeAgents(nodeId);
      // Re-fetch nodes so the refreshed per-node agent list shows.
      await queryClient.invalidateQueries({ queryKey: ["nodes"] });
      await queryClient.invalidateQueries({ queryKey: ["agents"] });
    } catch {
      // best-effort; a remote node without an envoy returns 501
    } finally {
      setDetectingNode(null);
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
                  message="No other nodes registered. Additional envoys appear here once they connect via UDS."
                />
              </div>
            )}

            {selectedNode && (
              <Drawer
                node={selectedNode}
                agents={selectedNode.agents ?? []}
                onClose={() => setSelectedId(null)}
              />
            )}
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
                message="Agents above run on the local node. Remote nodes and their agents appear here once fleet orchestration (Epic L) is live."
              />
            )}
          </div>
        )}
      </div>
    </>
  );
}
