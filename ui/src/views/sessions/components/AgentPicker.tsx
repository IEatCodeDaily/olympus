import { useEffect, useMemo, useState } from "react";
import { Icon } from "../../../components/Icon";
import { BrandIcon, agentBrand } from "../../../components/BrandIcons";
import { useAgentCatalog, useSessions } from "../../../hooks/queries";
import type { AgentInfo, NodeInfo, Session } from "../../../types";

type PickRow = { node: NodeInfo; agent: AgentInfo };

function pairKey(agent: string, node: string): string {
  return `${node}\u0000${agent}`;
}

export function nodeAgentRows(nodes: NodeInfo[], query: string): PickRow[] {
  const q = query.trim().toLowerCase();
  return nodes.flatMap((node) =>
    (node.agents ?? [])
      .filter((agent) => {
        if (!q) return true;
        return [node.nodeId, node.hostname, agent.id, agent.provider, agent.model, agent.kind]
          .filter(Boolean)
          .some((value) => String(value).toLowerCase().includes(q));
      })
      .map((agent) => ({ node, agent })),
  );
}

export function deriveOftenSelectedPairs(
  sessions: Session[],
  nodes: NodeInfo[],
  limit = 6,
): PickRow[] {
  const available = new Map<string, PickRow>();
  for (const row of nodeAgentRows(nodes, "")) {
    if (row.node.status === "online") available.set(pairKey(row.agent.id, row.node.nodeId), row);
  }

  const counts = new Map<string, { count: number; last: number }>();
  for (const session of sessions) {
    if (!session.agent || !session.node) continue;
    const key = pairKey(session.agent, session.node);
    if (!available.has(key)) continue;
    const prev = counts.get(key) ?? { count: 0, last: 0 };
    counts.set(key, { count: prev.count + 1, last: Math.max(prev.last, session.lastActivity) });
  }

  return [...counts.entries()]
    .sort((a, b) => b[1].count - a[1].count || b[1].last - a[1].last)
    .slice(0, limit)
    .map(([key]) => available.get(key)!)
    .filter(Boolean);
}

export function AgentPicker({
  open,
  onSelect,
  onCancel,
  nodesOverride,
  sessionsOverride,
}: {
  open: boolean;
  onSelect: (agentId: string, nodeId: string) => void;
  onCancel: () => void;
  nodesOverride?: NodeInfo[];
  sessionsOverride?: Session[];
}) {
  const { data: catalog, isLoading } = useAgentCatalog();
  const { data: sessionData } = useSessions({ managed: true, archived: false, limit: 50 });
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);

  const nodes = nodesOverride ?? catalog?.nodes ?? [];
  const sessions = sessionsOverride ?? sessionData?.sessions ?? [];
  const rows = useMemo(() => nodeAgentRows(nodes, query), [nodes, query]);
  const selectable = rows.filter((row) => row.node.status === "online");
  const often = useMemo(
    () => deriveOftenSelectedPairs(sessions, nodes).filter((row) => nodeAgentRows([row.node], query).some((r) => r.agent.id === row.agent.id)),
    [sessions, nodes, query],
  );

  useEffect(() => setActive(0), [query, open]);
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setActive((i) => Math.min(i + 1, Math.max(0, selectable.length - 1)));
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        setActive((i) => Math.max(0, i - 1));
      }
      if (event.key === "Enter" && selectable[active]) {
        event.preventDefault();
        const row = selectable[active];
        onSelect(row.agent.id, row.node.nodeId);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, onCancel, onSelect, open, selectable]);

  if (!open) return null;

  const grouped = nodes.map((node) => ({
    node,
    rows: rows.filter((row) => row.node.nodeId === node.nodeId),
  }));

  return (
    <div className="ol-overlay" role="dialog" aria-modal="true" aria-label="Start new session" onClick={onCancel}>
      <div className="ol-dialog" style={{ maxWidth: 560, width: "92vw" }} onClick={(e) => e.stopPropagation()}>
        <div className="ol-dialog-head">
          <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
            <Icon name="plus" size={18} />
            <div><div className="ol-dialog-title">New session</div></div>
          </div>
          <button type="button" className="icobtn" onClick={onCancel} title="Close" aria-label="Close">
            <Icon name="x" size={14} />
          </button>
        </div>
        <div className="ol-dialog-body" style={{ padding: 0 }}>
          <div style={{ padding: "10px 12px", borderBottom: "1px solid var(--line)" }}>
            <input
              autoFocus
              className="ol-input"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search agents or nodes…"
              aria-label="Search agents"
              style={{ width: "100%" }}
            />
          </div>
          {isLoading && !nodesOverride && <div style={{ padding: 12, color: "var(--dim)", fontSize: 12 }}>Loading agents…</div>}
          {!isLoading && rows.length === 0 && <div style={{ padding: 12, color: "var(--dim)", fontSize: 12 }}>No matching agents.</div>}
          <div style={{ maxHeight: 440, overflowY: "auto" }}>
            {often.length > 0 && (
              <section aria-label="Often selected">
                <div className="gk" style={{ padding: "10px 12px 4px" }}>often selected</div>
                {often.map((row) => <AgentButton key={`often:${row.node.nodeId}:${row.agent.id}`} row={row} onSelect={onSelect} />)}
              </section>
            )}
            {grouped.map(({ node, rows: nodeRows }) => {
              if (nodeRows.length === 0) return null;
              const online = node.status === "online";
              return (
                <section key={node.nodeId} aria-label={`${node.hostname} ${node.status}`}>
                  <div className="gk" style={{ padding: "12px 12px 4px", display: "flex", gap: 8, alignItems: "center" }}>
                    <span>{node.hostname || node.nodeId}</span>
                    <span className="tag" style={{ color: online ? "var(--success)" : "var(--dim)" }}>{node.status}</span>
                  </div>
                  {online ? nodeRows.map((row) => (
                    <AgentButton key={`${row.node.nodeId}:${row.agent.id}`} row={row} onSelect={onSelect} />
                  )) : (
                    <div style={{ padding: "4px 12px 12px", color: "var(--dim)", fontSize: 12 }}>
                      {nodeRows.length} agent{nodeRows.length === 1 ? "" : "s"} hidden while this node is {node.status}.
                    </div>
                  )}
                </section>
              );
            })}
          </div>
        </div>
        <div className="ol-dialog-foot"><button type="button" className="btn" onClick={onCancel}>Cancel</button></div>
      </div>
    </div>
  );
}

function AgentButton({ row, onSelect }: { row: PickRow; onSelect: (agentId: string, nodeId: string) => void }) {
  const label = `${row.agent.id} on ${row.node.nodeId}`;
  return (
    <button
      type="button"
      className="ol-menu-item"
      aria-label={label}
      style={{ width: "100%", border: "none", background: "none", cursor: "pointer", textAlign: "left" }}
      onClick={() => onSelect(row.agent.id, row.node.nodeId)}
    >
      <BrandIcon name={agentBrand(row.agent.kind, row.agent.provider)} size={15} />
      <span style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>
          {row.agent.id}
          {row.agent.isDefault && <span className="tag" style={{ marginLeft: 6, fontSize: 9, color: "var(--silver)", background: "var(--silver-wash)" }}>default</span>}
          {row.agent.ready === false && <span className="tag" style={{ marginLeft: 6, fontSize: 9, color: "var(--warn)" }}>needs login</span>}
        </div>
        <div style={{ fontSize: 10, color: "var(--faint)", fontFamily: "var(--font-mono)" }}>
          {row.agent.provider ?? "—"}{row.agent.model ? ` · ${row.agent.model}` : ""}<span style={{ color: "var(--silver)", marginLeft: 4 }}>· {row.node.nodeId}</span>
        </div>
      </span>
    </button>
  );
}
