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
import { useMemo, useState } from "react";
import { Icon } from "../components/Icon";
import { useAgents, useHealth } from "../hooks/queries";
import { relativeTime } from "../lib/format";
import type { AgentInfo } from "../types";

// ── Local fleet model ──────────────────────────────
// Mirrors the eventual NodeInfo contract but carries the extra display fields
// (bind, version) the drawer needs. The health probe is the source of truth
// for liveness; configured agents fill the slot count.
type NodeStatus = "online" | "draining" | "offline";

interface FleetNode {
  id: string;
  status: NodeStatus;
  bind: string;
  slotsUsed: number;
  slotsTotal: number;
  /** epoch seconds of the last heartbeat. */
  heartbeat: number;
  version: string;
  /** true for the synthesized local node (no remove action). */
  local: boolean;
}

type SubView = "fleet" | "agents";

const API_BASE = (import.meta.env.VITE_API_BASE as string | undefined) ?? "";
const APP_VERSION = "v0.3.1";
/** Capacity assumed for the local single-node MVP. */
const LOCAL_SLOTS_TOTAL = 4;

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

/** "acp" for the root ACP profile, "cli" for the rest — matches the reference tag set. */
function agentTypeTag(agent: AgentInfo): string {
  return agent.isDefault ? "acp" : "cli";
}

/** Reduce VITE_API_BASE (URL or host:port) to a host:port label. */
function bindLabel(): string {
  if (!API_BASE) return "127.0.0.1:8787";
  try {
    // Full URL form, e.g. http://127.0.0.1:8787
    return new URL(API_BASE).host;
  } catch {
    // Bare host:port form.
    return API_BASE;
  }
}

/** Heartbeat label with second precision (the 15s health poll makes this meaningful). */
function heartbeatLabel(epochSec: number): string {
  const diff = Math.max(0, Math.floor(Date.now() / 1000 - epochSec));
  if (diff < 60) return `${diff}s ago`;
  return relativeTime(epochSec);
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
          {node.id}
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
        <span>{heartbeatLabel(node.heartbeat)}</span>
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
    <aside className="drawer on" role="dialog" aria-label={`Node ${node.id}`}>
      <div className="dr-head">
        <span className="dr-title">{node.id}</span>
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
          <span className="k">BIND</span>
          <span className="v">{node.bind}</span>
        </div>
        <div className="kv">
          <span className="k">SLOTS</span>
          <span className="v">
            {node.slotsUsed} / {node.slotsTotal}
          </span>
        </div>
        <div className="kv">
          <span className="k">HEARTBEAT</span>
          <span className="v">{heartbeatLabel(node.heartbeat)}</span>
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
    <div className="agrow">
      <Icon name="bot" size={14} />
      <span className="nm">{agent.id}</span>
      <span className="gtag">{agentTypeTag(agent)}</span>
      <span className="sp" />
      <span className="gk">{agent.model ?? "—"}</span>
      <span className="gtag">configured</span>
    </div>
  );
}

function NodeSection({
  node,
  agents,
}: {
  node: FleetNode;
  agents: AgentInfo[];
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
          {node.id}
        </span>
        <span className="gk">
          {node.bind} · {agents.length} agent{agents.length === 1 ? "" : "s"}
        </span>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {agents.length > 0 ? (
          agents.map((a) => <AgentRow key={a.id} agent={a} />)
        ) : (
          <div className="mono" style={{ fontSize: 11, color: "var(--faint)" }}>
            no agents configured
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

  const healthQ = useHealth();
  const agentsQ = useAgents();

  const health = healthQ.data;
  const agents = agentsQ.data?.agents ?? [];

  // Synthesize the local node from the health probe. `dataUpdatedAt` (epoch ms
  // of the last successful fetch) stands in for the heartbeat until the real
  // fleet endpoint ships.
  const nodes: FleetNode[] = useMemo(() => {
    const local: FleetNode = {
      id: health?.hermesProfile || "local",
      status: health?.status === "ok" ? "online" : "offline",
      bind: bindLabel(),
      slotsUsed: Math.min(agents.length, LOCAL_SLOTS_TOTAL),
      slotsTotal: LOCAL_SLOTS_TOTAL,
      heartbeat: Math.floor((healthQ.dataUpdatedAt || Date.now()) / 1000),
      version: APP_VERSION,
      local: true,
    };
    return [local];
  }, [health, agents.length, healthQ.dataUpdatedAt]);

  const selectedNode =
    nodes.find((n) => n.id === selectedId) ?? (selectedId ? null : nodes[0] ?? null);

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
                  key={node.id}
                  node={node}
                  selected={selectedNode?.id === node.id}
                  onClick={() => setSelectedId(node.id)}
                />
              ))}
            </div>

            {nodes.length === 1 && (
              <div style={{ marginTop: 16 }}>
                <EmptyState
                  title="Single-node fleet"
                  message="No other nodes registered. Additional operators appear here once fleet orchestration (Epic L) is live."
                />
              </div>
            )}

            {selectedNode && (
              <Drawer
                node={selectedNode}
                agents={selectedNode.local ? agents : []}
                onClose={() => setSelectedId(null)}
              />
            )}
          </div>
        ) : (
          <div style={{ maxWidth: 680, display: "flex", flexDirection: "column", gap: 20 }}>
            {nodes.map((node) => (
              <NodeSection
                key={node.id}
                node={node}
                agents={node.local ? agents : []}
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
