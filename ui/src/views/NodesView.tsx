// NodesView — fleet management (roadmap U4). Backend /api/nodes lands with Epic L.
// Mock-first against MSW so the UI shape is stable before the backend exists.
import { useEffect, useMemo, useState } from "react";
import {
  Badge,
  EmptyState,
  PageHeader,
  PlaceholderBadge,
  StatPill,
} from "../components/shell";
import { relativeTime } from "../lib/format";
import type { NodeInfo, NodeStatus, NodesResponse } from "../types";

const BASE = import.meta.env.VITE_API_BASE as string;
const TOKEN = import.meta.env.VITE_API_TOKEN as string;

const NODE_SESSIONS: Record<string, string[]> = {
  local: ["Board sync watcher", "Search index rebuild"],
  "gpu-box": ["Batch eval: claude-sonnet-4", "LoRA smoke test", "Image queue drain"],
  "edge-mini": [],
};

const FILTERS: Array<{ key: "all" | NodeStatus; label: string }> = [
  { key: "all", label: "All" },
  { key: "online", label: "Online" },
  { key: "draining", label: "Draining" },
  { key: "offline", label: "Offline" },
];

function badgeKind(status: NodeStatus): string | undefined {
  if (status === "online") return "running";
  if (status === "draining") return "warning";
  if (status === "offline") return "blocked";
  return undefined;
}

function authHeaders(): Record<string, string> {
  return { Authorization: `Bearer ${TOKEN}` };
}

function slotFill(node: NodeInfo): number {
  if (node.slotsTotal <= 0) return 0;
  return Math.max(0, Math.min(100, (node.slotsUsed / node.slotsTotal) * 100));
}

function NodesSkeleton() {
  return (
    <div className="node-grid" aria-label="Loading nodes">
      {Array.from({ length: 3 }, (_, i) => (
        <div key={i} className="node-card node-card-skeleton">
          <div className="node-skel node-skel-title" />
          <div className="node-skel node-skel-badge" />
          <div className="node-skel node-skel-bar" />
          <div className="node-skel node-skel-line" />
          <div className="node-skel node-skel-line short" />
        </div>
      ))}
    </div>
  );
}

export default function NodesView() {
  const [nodes, setNodes] = useState<NodeInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<"all" | NodeStatus>("all");
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function loadNodes() {
      try {
        setLoading(true);
        setError(null);
        const res = await fetch(`${BASE}/api/nodes`, { headers: authHeaders() });
        if (!res.ok) throw new Error(`nodes ${res.status}`);
        const data = (await res.json()) as NodesResponse;
        if (cancelled) return;
        setNodes(data.nodes);
        setSelectedNodeId((current) => current ?? data.nodes[0]?.id ?? null);
      } catch (err) {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : "Failed to load nodes.");
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    void loadNodes();
    return () => {
      cancelled = true;
    };
  }, []);

  const filteredNodes = useMemo(() => {
    return nodes.filter((node) => filter === "all" || node.status === filter);
  }, [filter, nodes]);

  const selectedNode =
    filteredNodes.find((node) => node.id === selectedNodeId) ?? filteredNodes[0] ?? null;

  const totals = useMemo(() => {
    return nodes.reduce(
      (acc, node) => {
        acc.slotsUsed += node.slotsUsed;
        acc.slotsTotal += node.slotsTotal;
        if (node.status === "online") acc.online += 1;
        if (node.status === "draining") acc.draining += 1;
        return acc;
      },
      { slotsUsed: 0, slotsTotal: 0, online: 0, draining: 0 },
    );
  }, [nodes]);

  return (
    <div className="view-scroll">
      <PageHeader
        title="Nodes"
        subtitle="Fleet — heartbeat, capacity, and runtime posture across operators"
        actions={<PlaceholderBadge epic="Epic L" />}
      />

      <div className="board-stats">
        <StatPill label="nodes" value={loading ? "—" : String(nodes.length)} />
        <StatPill
          label="slots free"
          value={loading ? "—" : String(Math.max(0, totals.slotsTotal - totals.slotsUsed))}
        />
        <StatPill label="online" value={loading ? "—" : String(totals.online)} />
        <StatPill label="draining" value={loading ? "—" : String(totals.draining)} />
      </div>

      <div className="nodes-toolbar" role="toolbar" aria-label="Node filters">
        <div className="nodes-filter-group">
          {FILTERS.map((item) => (
            <button
              key={item.key}
              className={`nodes-filter ${filter === item.key ? "active" : ""}`}
              onClick={() => setFilter(item.key)}
              type="button"
            >
              {item.label}
            </button>
          ))}
        </div>
        <div className="nodes-toolbar-note">Mock contract for /api/nodes; click a card to inspect queued work.</div>
      </div>

      {loading && <NodesSkeleton />}

      {!loading && error && (
        <EmptyState
          title="Nodes unavailable"
          message={`The mock contract failed to load: ${error}.`}
        />
      )}

      {!loading && !error && filteredNodes.length === 0 && (
        <EmptyState
          title="No nodes match this filter"
          message="Try a broader status filter or wait for the next heartbeat window."
        />
      )}

      {!loading && !error && filteredNodes.length > 0 && (
        <>
          <div className="node-grid">
            {filteredNodes.map((node) => {
              const sessions = NODE_SESSIONS[node.id] ?? [];
              return (
                <button
                  key={node.id}
                  className={`node-card node-card-button ${selectedNode?.id === node.id ? "selected" : ""}`}
                  onClick={() => setSelectedNodeId(node.id)}
                  type="button"
                >
                  <div className="node-card-head">
                    <span className="node-name">{node.id}</span>
                    <Badge kind={badgeKind(node.status)}>{node.status}</Badge>
                  </div>
                  <div className="node-card-body">
                    <div className="node-stat">
                      <span>slots</span>
                      <span>
                        {node.slotsUsed} / {node.slotsTotal}
                      </span>
                    </div>
                    <div className="node-slot-bar" aria-hidden="true">
                      <div className="node-slot-bar-fill" style={{ width: `${slotFill(node)}%` }} />
                    </div>
                    <div className="node-stat">
                      <span>heartbeat</span>
                      <span>{relativeTime(node.lastHeartbeat)}</span>
                    </div>
                    <div className="node-stat">
                      <span>runtime</span>
                      <span>{node.runtime}</span>
                    </div>
                    <div className="node-card-footer">
                      <span className="node-footer-label">sessions</span>
                      <span className="node-footer-value">{sessions.length}</span>
                    </div>
                  </div>
                </button>
              );
            })}
          </div>

          {selectedNode && (
            <section className="node-detail-card">
              <div className="node-detail-head">
                <div>
                  <div className="node-detail-kicker">selected node</div>
                  <h2 className="node-detail-title">{selectedNode.id}</h2>
                </div>
                <Badge kind={badgeKind(selectedNode.status)}>{selectedNode.status}</Badge>
              </div>

              <div className="node-detail-meta">
                <div className="node-detail-metric">
                  <span>runtime</span>
                  <strong>{selectedNode.runtime}</strong>
                </div>
                <div className="node-detail-metric">
                  <span>last heartbeat</span>
                  <strong>{relativeTime(selectedNode.lastHeartbeat)}</strong>
                </div>
                <div className="node-detail-metric">
                  <span>capacity</span>
                  <strong>
                    {selectedNode.slotsUsed} / {selectedNode.slotsTotal} slots busy
                  </strong>
                </div>
              </div>

              <div className="node-session-list">
                <div className="node-session-list-head">
                  <span>Running sessions</span>
                  <span>{(NODE_SESSIONS[selectedNode.id] ?? []).length}</span>
                </div>
                {(NODE_SESSIONS[selectedNode.id] ?? []).length > 0 ? (
                  <div className="node-session-items">
                    {(NODE_SESSIONS[selectedNode.id] ?? []).map((session) => (
                      <div key={session} className="node-session-item">
                        <span className="node-session-dot" />
                        <span>{session}</span>
                      </div>
                    ))}
                  </div>
                ) : (
                  <EmptyState
                    title="No running sessions"
                    message="This detail pane is a stub for Epic L. Backend drill-in lands when fleet orchestration is live."
                  />
                )}
              </div>
            </section>
          )}
        </>
      )}
    </div>
  );
}
