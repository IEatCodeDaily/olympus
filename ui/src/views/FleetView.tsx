// FleetView — fleet operator surface (card N1).
//
// Grid of node .ol-card (nodeId, hostname, status dot, slots .ol-bar, version,
// local badge, last-heartbeat-ago). Click a node → drill-in panel listing that
// node's running sessions (filtered /api/sessions?node=<id>) + slot detail.
// "Add node" affordance opens a help popover (registration is UDS-side).
//
// Data: real /api/nodes (UDS node registry — auto-refreshes every 10s).
// Design: .ol-* primitives only. No raw hex.

import { useState } from "react";
import { Icon } from "../components/Icon";
import { useNodes, useSessions } from "../hooks/queries";
import { relativeTime } from "../lib/format";
import type { NodeInfo, NodeStatus, Session } from "../types";

// ── Helpers ────────────────────────────────────────

function statusDotClass(status: NodeStatus): string {
  if (status === "online") return "ol-dot ol-dot-live";
  if (status === "draining") return "ol-dot ol-dot-warn";
  return "ol-dot ol-dot-err";
}

function statusBadgeClass(status: NodeStatus): string {
  if (status === "online") return "ol-badge ol-badge-ok";
  if (status === "draining") return "ol-badge ol-badge-warn";
  return "ol-badge ol-badge-err";
}

function slotBarClass(pct: number): string {
  if (pct >= 90) return "ol-bar-fill err";
  if (pct >= 70) return "ol-bar-fill warn";
  return "ol-bar-fill";
}

function slotPct(used: number, total: number): number {
  if (total <= 0) return 0;
  return Math.max(0, Math.min(100, (used / total) * 100));
}

function heartbeatLabel(epochSecAgo: number): string {
  if (epochSecAgo < 60) return `${epochSecAgo}s ago`;
  return relativeTime(Math.floor(Date.now() / 1000) - epochSecAgo);
}

function sessionAge(ts: number): string {
  const diff = Date.now() / 1000 - ts;
  if (diff < 60) return "now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}

// ── Sub-components ─────────────────────────────────

function NodeCard({
  node,
  selected,
  onClick,
}: {
  node: NodeInfo;
  selected: boolean;
  onClick: () => void;
}) {
  const pct = slotPct(node.slotsUsed, node.slotsTotal);

  return (
    <button
      type="button"
      data-node-id={node.nodeId}
      className={[
        "ol-card ol-card-interactive",
        selected ? "ol-card-selected" : "",
      ]
        .filter(Boolean)
        .join(" ")}
      onClick={onClick}
      style={{ display: "flex", flexDirection: "column", gap: 10, textAlign: "left" }}
    >
      {/* Header: status dot + nodeId + local badge */}
      <div style={{ display: "flex", alignItems: "center", gap: 7, flexWrap: "wrap" }}>
        <span className={statusDotClass(node.status)} />
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: "var(--fs-12)",
            fontWeight: "var(--fw-semibold)",
            color: "var(--text)",
            flex: 1,
            minWidth: 0,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {node.nodeId}
        </span>
        {node.local && (
          <span className="ol-badge ol-badge-accent" style={{ flexShrink: 0 }}>
            LOCAL
          </span>
        )}
      </div>

      {/* Hostname */}
      <div
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: "var(--fs-11)",
          color: "var(--text-dim)",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {node.hostname}
      </div>

      {/* Slots bar */}
      <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            fontFamily: "var(--font-mono)",
            fontSize: "var(--fs-10)",
            color: "var(--text-faint)",
            textTransform: "uppercase",
            letterSpacing: "var(--tracking-caps)",
          }}
        >
          <span>SLOTS</span>
          <span>
            {node.slotsUsed} / {node.slotsTotal}
          </span>
        </div>
        <div className="ol-bar ol-bar-sm">
          <div className={slotBarClass(pct)} style={{ width: `${pct}%` }} />
        </div>
      </div>

      {/* Footer: version + heartbeat */}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          fontFamily: "var(--font-mono)",
          fontSize: "var(--fs-10)",
          color: "var(--text-faint)",
        }}
      >
        <span>{node.version}</span>
        <span>{heartbeatLabel(node.lastHeartbeatAgoSecs)}</span>
      </div>
    </button>
  );
}

function SessionRow({ session }: { session: Session }) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 9,
        padding: "6px 0",
        borderBottom: "var(--border-w) solid var(--border)",
      }}
    >
      <span
        className={session.liveness === "active" ? "ol-dot ol-dot-live" : "ol-dot"}
      />
      <span
        style={{
          flex: 1,
          fontSize: "var(--fs-12)",
          color: "var(--text)",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {session.title ?? "Untitled session"}
      </span>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: "var(--fs-10)",
          color: "var(--text-faint)",
          flexShrink: 0,
        }}
      >
        {sessionAge(session.lastActivity)}
      </span>
    </div>
  );
}

function NodePanel({
  node,
  onClose,
}: {
  node: NodeInfo;
  onClose: () => void;
}) {
  const pct = slotPct(node.slotsUsed, node.slotsTotal);

  const sessionsQ = useSessions({ node: node.nodeId, limit: 20 });
  const sessions: Session[] = sessionsQ.data?.sessions ?? [];

  return (
    <aside
      className="ol-card"
      style={{
        width: 300,
        flexShrink: 0,
        display: "flex",
        flexDirection: "column",
        gap: 16,
        alignSelf: "flex-start",
        position: "sticky",
        top: 0,
      }}
      aria-label={`Node ${node.nodeId} detail`}
    >
      {/* Panel header */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: "var(--fs-12)",
            fontWeight: "var(--fw-semibold)",
            color: "var(--text)",
          }}
        >
          {node.nodeId}
        </span>
        <button
          type="button"
          className="ol-btn ol-btn-ghost ol-btn-sm"
          onClick={onClose}
          aria-label="Close panel"
          title="Close"
        >
          <Icon name="x" size={12} />
        </button>
      </div>

      {/* Status + badges */}
      <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
        <span className={statusBadgeClass(node.status)}>{node.status}</span>
        {node.local && <span className="ol-badge ol-badge-accent">LOCAL</span>}
      </div>

      {/* KV rows */}
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {[
          ["HOST", node.hostname],
          ["VERSION", node.version],
          ["HEARTBEAT", heartbeatLabel(node.lastHeartbeatAgoSecs)],
        ].map(([k, v]) => (
          <div
            key={k}
            style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}
          >
            <span
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: "var(--fs-10)",
                textTransform: "uppercase",
                letterSpacing: "var(--tracking-caps)",
                color: "var(--text-faint)",
              }}
            >
              {k}
            </span>
            <span
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: "var(--fs-11)",
                color: "var(--text-dim)",
              }}
            >
              {v}
            </span>
          </div>
        ))}
      </div>

      {/* Slot detail */}
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <div style={{ display: "flex", justifyContent: "space-between" }}>
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: "var(--fs-10)",
              textTransform: "uppercase",
              letterSpacing: "var(--tracking-caps)",
              color: "var(--text-faint)",
            }}
          >
            SLOTS
          </span>
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: "var(--fs-11)",
              color: "var(--text-dim)",
            }}
          >
            {node.slotsUsed} / {node.slotsTotal} used
          </span>
        </div>
        <div className="ol-bar">
          <div className={slotBarClass(pct)} style={{ width: `${pct}%` }} />
        </div>
      </div>

      {/* Sessions on this node */}
      <div>
        <div
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: "var(--fs-10)",
            textTransform: "uppercase",
            letterSpacing: "var(--tracking-caps)",
            color: "var(--text-faint)",
            marginBottom: 8,
          }}
        >
          RUNNING SESSIONS
        </div>
        {sessionsQ.isLoading ? (
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            {[1, 2, 3].map((i) => (
              <div key={i} className="ol-skel" style={{ height: 14 }} />
            ))}
          </div>
        ) : sessions.length === 0 ? (
          <div
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: "var(--fs-11)",
              color: "var(--text-faint)",
            }}
          >
            No sessions on this node.
          </div>
        ) : (
          <div>
            {sessions.map((s) => (
              <SessionRow key={s.id} session={s} />
            ))}
          </div>
        )}
      </div>
    </aside>
  );
}

function AddNodePopover({ onClose }: { onClose: () => void }) {
  return (
    <div
      className="ol-card"
      style={{ maxWidth: 360 }}
      role="dialog"
      aria-label="Add node"
    >
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", marginBottom: 12 }}>
        <span style={{ fontSize: "var(--fs-13)", fontWeight: "var(--fw-semibold)", color: "var(--text)" }}>
          Adding a fleet node
        </span>
        <button
          type="button"
          className="ol-btn ol-btn-ghost ol-btn-sm"
          onClick={onClose}
          aria-label="Close"
        >
          <Icon name="x" size={12} />
        </button>
      </div>
      <p style={{ fontSize: "var(--fs-12)", color: "var(--text-dim)", lineHeight: "var(--lh-relaxed)", marginBottom: 10 }}>
        Node registration is UDS-side. On the remote machine, run the Olympus
        envoy pointed at this control plane&apos;s socket:
      </p>
      <pre
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: "var(--fs-11)",
          color: "var(--text)",
          background: "var(--bg)",
          border: "var(--border-w) solid var(--border)",
          borderRadius: "var(--radius)",
          padding: "8px 10px",
          overflowX: "auto",
          whiteSpace: "pre-wrap",
        }}
      >
        {"olympus-envoy --control ~/.olympus/control.sock"}
      </pre>
      <p style={{ fontSize: "var(--fs-11)", color: "var(--text-faint)", marginTop: 8 }}>
        Once connected it auto-registers and appears in this grid.
      </p>
    </div>
  );
}

function FleetEmptyState() {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 10,
        padding: "48px 0",
        color: "var(--text-faint)",
      }}
      data-testid="fleet-empty"
    >
      <Icon name="server" size={28} />
      <div style={{ fontSize: "var(--fs-13)", color: "var(--text-dim)" }}>
        Single-node fleet
      </div>
      <div style={{ fontSize: "var(--fs-12)", color: "var(--text-faint)", maxWidth: 320, textAlign: "center" }}>
        No other nodes registered. Additional envoys appear here once they connect via UDS.
      </div>
    </div>
  );
}

// ── Main ───────────────────────────────────────────

export default function FleetView() {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [showAddNode, setShowAddNode] = useState(false);

  const nodesQ = useNodes();

  const nodes: NodeInfo[] = nodesQ.data?.nodes ?? [];

  const selectedNode = selectedId ? nodes.find((n) => n.nodeId === selectedId) ?? null : null;

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", minHeight: 0 }} data-testid="fleet-view">
      {/* View header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "0 var(--panel-pad) var(--panel-pad)",
          flexShrink: 0,
        }}
      >
        <span style={{ fontSize: "var(--fs-16)", fontWeight: "var(--fw-semibold)", color: "var(--text)" }}>
          Fleet
        </span>
        <span style={{ fontSize: "var(--fs-12)", color: "var(--text-faint)" }}>
          · nodes
        </span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 6, position: "relative" }}>
          <button
            type="button"
            className="ol-btn ol-btn-secondary ol-btn-sm"
            onClick={() => setShowAddNode((v) => !v)}
            aria-expanded={showAddNode}
            aria-label="Add node"
          >
            <Icon name="plus" size={12} />
            Add node
          </button>
          {showAddNode && (
            <div style={{ position: "absolute", top: "calc(100% + 8px)", right: 0, zIndex: 20 }}>
              <AddNodePopover onClose={() => setShowAddNode(false)} />
            </div>
          )}
        </div>
      </div>

      {/* Loading skeletons */}
      {nodesQ.isLoading && (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fill, minmax(230px, 1fr))",
            gap: 12,
            padding: "0 var(--panel-pad)",
          }}
        >
          {[1, 2, 3].map((i) => (
            <div key={i} className="ol-card" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              <div className="ol-skel" style={{ height: 14, width: "60%" }} />
              <div className="ol-skel" style={{ height: 11, width: "80%" }} />
              <div className="ol-bar ol-bar-sm" />
              <div className="ol-skel" style={{ height: 10, width: "50%" }} />
            </div>
          ))}
        </div>
      )}

      {/* Content */}
      {!nodesQ.isLoading && (
        <div style={{ display: "flex", gap: 16, padding: "0 var(--panel-pad)", flex: 1, minHeight: 0, overflow: "auto" }}>
          {/* Node grid */}
          <div style={{ flex: 1, minWidth: 0 }}>
            <div
              data-testid="fleet-grid"
              style={{
                display: "grid",
                gridTemplateColumns: "repeat(auto-fill, minmax(230px, 1fr))",
                gap: 12,
                alignContent: "start",
              }}
            >
              {nodes.map((node) => (
                <NodeCard
                  key={node.nodeId}
                  node={node}
                  selected={selectedNode?.nodeId === node.nodeId}
                  onClick={() =>
                    setSelectedId((cur) => (cur === node.nodeId ? null : node.nodeId))
                  }
                />
              ))}
            </div>

            {nodes.length <= 1 && <FleetEmptyState />}
          </div>

          {/* Drill-in panel */}
          {selectedNode && (
            <NodePanel node={selectedNode} onClose={() => setSelectedId(null)} />
          )}
        </div>
      )}
    </div>
  );
}
