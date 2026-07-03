// GridPage — /fleet default page: node card grid.
//
// Data: live /api/nodes (via parent FleetView).
// Clicking a card navigates to /fleet/$nodeId.

import React, { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { Icon } from "../../../components/Icon";
import type { NodeInfo } from "../../../types";
import {
  statusDotClass,
  slotBarClass,
  slotPct,
  heartbeatLabel,
} from "../helpers";

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

export function GridPage({
  nodes,
  isLoading,
  activeNodeId,
}: {
  nodes: NodeInfo[];
  isLoading: boolean;
  activeNodeId: string | null;
}) {
  const navigate = useNavigate();
  const [showAddNode, setShowAddNode] = useState(false);

  const handleNodeClick = (nodeId: string) => {
    if (activeNodeId === nodeId) {
      // clicking active node → back to grid
      void navigate({ to: "/fleet" });
    } else {
      void navigate({ to: "/fleet/$nodeId", params: { nodeId } });
    }
  };

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
      {isLoading && (
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
      {!isLoading && (
        <div style={{ padding: "0 var(--panel-pad)", flex: 1, minHeight: 0, overflow: "auto" }}>
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
                selected={activeNodeId === node.nodeId}
                onClick={() => handleNodeClick(node.nodeId)}
              />
            ))}
          </div>

          {nodes.length <= 1 && <FleetEmptyState />}
        </div>
      )}
    </div>
  );
}
