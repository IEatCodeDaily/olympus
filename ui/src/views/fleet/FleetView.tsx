// FleetView — fleet surface shell (View/Page architecture).
//
// Owns:
//   - Left sidebar: node list (name + status dot), "All nodes" item,
//     click → /fleet/$nodeId; "All nodes" → /fleet.
//   - Viewport: GridPage (default /fleet) or NodeDetailPage (/fleet/$nodeId).
//
// Data: live /api/nodes — 10s auto-refresh via useNodes().
// Design: .ol-* primitives only.

import { useNavigate } from "@tanstack/react-router";
import { Icon } from "../../components/Icon";
import { useNodes } from "../../hooks/queries";
import { useUIStore } from "../../store";
import type { NodeInfo } from "../../types";
import { statusDotClass } from "./helpers";
import { GridPage } from "./pages/GridPage";
import { NodeDetailPage } from "./pages/NodeDetailPage";

// ── Sidebar ─────────────────────────────────────────

function FleetSidebar({
  nodes,
  activeNodeId,
}: {
  nodes: NodeInfo[];
  activeNodeId: string | null;
}) {
  const navigate = useNavigate();

  return (
    <aside className="sidebar on">
      <div className="sbv on">
        <div className="sb-pad">
          <button
            type="button"
            className={`srow ${activeNodeId === null ? "on" : ""}`}
            onClick={() => void navigate({ to: "/fleet" })}
            style={{ width: "100%" }}
          >
            <span className="sic">
              <Icon name="server" size={13} />
            </span>
            <div className="info">
              <span className="title">All nodes</span>
            </div>
            <span className="ct">{nodes.length}</span>
          </button>
        </div>

        <div className="sec-head">
          <span className="lbl">NODES</span>
          <span className="sp" />
          <span className="ct">{nodes.length}</span>
        </div>

        <div className="sec-content">
          {nodes.length === 0 && (
            <div className="empty-state-msg" style={{ padding: "8px 0" }}>
              No nodes registered
            </div>
          )}
          {nodes.map((n) => (
            <button
              key={n.nodeId}
              type="button"
              data-node-id={n.nodeId}
              className={`srow ${activeNodeId === n.nodeId ? "on" : ""}`}
              onClick={() => {
                if (activeNodeId === n.nodeId) {
                  void navigate({ to: "/fleet" });
                } else {
                  void navigate({ to: "/fleet/$nodeId", params: { nodeId: n.nodeId } });
                }
              }}
              style={{ width: "100%" }}
            >
              <span className={statusDotClass(n.status)} />
              <div className="info">
                <span className="title">{n.nodeId}</span>
              </div>
              <span className="meta">
                <span>{n.lastHeartbeatAgoSecs}s</span>
              </span>
            </button>
          ))}
        </div>
      </div>
    </aside>
  );
}

// ── Main ─────────────────────────────────────────────

export function FleetView({ nodeId }: { nodeId: string | null }) {
  const { sidebarCollapsed } = useUIStore();
  const nodesQ = useNodes();
  const nodes: NodeInfo[] = nodesQ.data?.nodes ?? [];

  const activeNode = nodeId ? (nodes.find((n) => n.nodeId === nodeId) ?? null) : null;

  return (
    <>
      {/* View-owned left sidebar */}
      {!sidebarCollapsed && (
        <FleetSidebar nodes={nodes} activeNodeId={nodeId} />
      )}

      {/* Viewport */}
      <div className="viewport">
        <div className="view on" data-view="fleet" style={{ flexDirection: "column" }}>
          {activeNode ? (
            <NodeDetailPage node={activeNode} />
          ) : (
            <GridPage
              nodes={nodes}
              isLoading={nodesQ.isLoading}
              activeNodeId={nodeId}
            />
          )}
        </div>
      </div>
    </>
  );
}
