/**
 * FleetView — the Fleet View component (owns sidebar + viewport layout).
 *
 * Architecture (matches SessionsView's View → Page pattern):
 *
 * The View OWNS:
 *   - left sidebar — FleetSidebar (node tree with agents under each node)
 *   - viewport layout — vp-head + vp-body
 *
 * Pages own viewport content ONLY:
 *   - NodeDetailPage — a selected node's details (status, transport, agents,
 *     sessions, Drain/Remove)
 *   - FleetOverviewPage — the default when no node is selected (fleet summary
 *     + Add-node affordance)
 *
 * Routes (URL-persistent):
 *   /fleet           → FleetOverviewPage (summary)
 *   /fleet/$nodeId   → NodeDetailPage
 *
 * Sidebar layout: an "Add node" button, then a NODES section where each node
 * is a row (status dot + name + agent count) with its agents nested beneath.
 *
 * Future: nodes may be sandbox/microVM hosts agents SSH into, not just envoy
 * hosts — the node model already carries `transport`; a `kind` field
 * (envoy | sandbox | remote) is the natural next extension.
 */
import React, { useState, useCallback } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { Icon } from "../components/Icon";
import { BrandIcon, agentBrand } from "../components/BrandIcons";
import { useUIStore } from "../store";
import { useResizable } from "../hooks/useResizable";
import { useNodes, useSessions } from "../hooks/queries";
import { refreshNodeAgents, mintEnroll, drainNode, removeNode } from "../api";
import { relativeTime } from "../lib/format";
import type { AgentInfo, EnrollResponse, NodeInfo, NodeStatus } from "../types";

// ── Helpers ────────────────────────────────────────

function statusDotColor(status: NodeStatus): string {
  if (status === "online") return "var(--green)";
  if (status === "draining") return "var(--amber)";
  return "var(--red)";
}

function statusTagClass(status: NodeStatus): string {
  if (status === "online") return "gtag ok";
  if (status === "draining") return "gtag warn";
  return "gtag err";
}

function transportLabel(node: NodeInfo): string {
  if (node.local) return "local";
  return node.transport;
}

function heartbeatLabel(epochSecAgo: number): string {
  if (epochSecAgo < 60) return `${epochSecAgo}s ago`;
  return relativeTime(Math.floor(Date.now() / 1000) - epochSecAgo);
}

function slotPct(used: number, total: number): number {
  if (total <= 0) return 0;
  return Math.max(0, Math.min(100, (used / total) * 100));
}

// ── Main View ──────────────────────────────────────

export default function FleetView({ nodeId }: { nodeId: string | null }) {
  const { sidebarCollapsed } = useUIStore();
  const sidebar = useResizable({
    axis: "x",
    min: 180,
    max: 380,
    initial: 240,
    direction: "right",
    persistKey: "olympus-fleet-sidebar-w",
  });

  return (
    <>
      {!sidebarCollapsed && (
        <FleetSidebar
          width={sidebar.size}
          activeNodeId={nodeId}
          onResizeStart={sidebar.onResizeStart}
        />
      )}
      <div className="viewport">
        {nodeId ? <NodeDetailPage nodeId={nodeId} /> : <FleetOverviewPage />}
      </div>
    </>
  );
}

// ── Sidebar ────────────────────────────────────────

function FleetSidebar({
  width,
  activeNodeId,
  onResizeStart,
}: {
  width: number;
  activeNodeId: string | null;
  onResizeStart?: (e: React.MouseEvent) => void;
}) {
  const navigate = useNavigate();
  const { data: nodeData } = useNodes();
  const nodes = nodeData?.nodes ?? [];
  const toggleSidebar = useUIStore((s) => s.toggleSidebar);
  const [addOpen, setAddOpen] = useState(false);

  const closeIfPhone = useCallback(() => {
    if (
      typeof window !== "undefined" &&
      typeof window.matchMedia === "function" &&
      window.matchMedia("(max-width: 820px)").matches
    ) {
      toggleSidebar();
    }
  }, [toggleSidebar]);

  const handleSelectNode = useCallback(
    (id: string) => {
      void navigate({ to: "/fleet/$nodeId", params: { nodeId: id } });
      closeIfPhone();
    },
    [navigate, closeIfPhone],
  );

  return (
    <>
      <aside className="sidebar" style={{ width }}>
        <div className="sb-pad">
          <button type="button" className="newbtn" onClick={() => setAddOpen(true)}>
            <Icon name="plus" size={14} />
            Add node
          </button>
        </div>
        <div className="sb-scroll">
          {nodes.length > 0 ? (
            <>
              <div className="sec-head">
                <span className="lbl">NODES</span>
                <span className="sp" />
                <span className="ct">{nodes.length}</span>
              </div>
              {nodes.map((node) => (
                <NodeTreeItem
                  key={node.nodeId}
                  node={node}
                  active={activeNodeId === node.nodeId}
                  onSelect={() => handleSelectNode(node.nodeId)}
                />
              ))}
            </>
          ) : (
            <div
              className="mono"
              style={{ fontSize: 11, color: "var(--faint)", padding: "12px 16px" }}
            >
              No nodes registered.
            </div>
          )}
        </div>
      </aside>
      <div className="rz-x" onMouseDown={onResizeStart} />
      {addOpen && <AddNodeModal onClose={() => setAddOpen(false)} />}
    </>
  );
}

/** A node row with its agents nested underneath as expandable children. */
function NodeTreeItem({
  node,
  active,
  onSelect,
}: {
  node: NodeInfo;
  active: boolean;
  onSelect: () => void;
}) {
  const agents = node.agents ?? [];
  const [expanded, setExpanded] = useState(true);

  return (
    <div>
      <div
        className={`srow${active ? " on" : ""}`}
        onClick={onSelect}
        title={`${node.hostname} · ${transportLabel(node)}`}
      >
        <span
          style={{
            width: 7,
            height: 7,
            borderRadius: 999,
            background: statusDotColor(node.status),
            flexShrink: 0,
          }}
        />
        <span className="srow-title">{node.nodeId}</span>
        <span className="gk" style={{ fontSize: 9, flexShrink: 0 }}>
          {agents.length > 0 ? `${agents.length}` : ""}
        </span>
        {agents.length > 0 && (
          <button
            type="button"
            className="icobtn"
            style={{ padding: 0, width: 16, height: 16 }}
            title={expanded ? "Collapse" : "Expand"}
            onClick={(e) => {
              e.stopPropagation();
              setExpanded((v) => !v);
            }}
          >
            <Icon name={expanded ? "chevron-down" : "chevron-right"} size={10} />
          </button>
        )}
      </div>
      {expanded &&
        agents.map((a) => (
          <div
            key={a.id}
            className="srow"
            style={{ paddingLeft: 28, paddingTop: 3, paddingBottom: 3 }}
            title={`${a.provider ?? a.kind} · ${a.model ?? "—"}`}
          >
            <BrandIcon name={agentBrand(a.kind, a.provider)} size={12} />
            <span className="srow-title" style={{ fontSize: 11 }}>
              {a.id}
            </span>
            <span className="srow-time">{a.model ?? "—"}</span>
          </div>
        ))}
    </div>
  );
}

// ── Viewport Pages ─────────────────────────────────

/** Default page — fleet summary + the Add-node affordance. */
function FleetOverviewPage() {
  const { data: nodeData } = useNodes();
  const nodes = nodeData?.nodes ?? [];
  const online = nodes.filter((n) => n.status === "online").length;
  const draining = nodes.filter((n) => n.status === "draining").length;
  const offline = nodes.filter((n) => n.status === "offline").length;
  const totalAgents = nodes.reduce((sum, n) => sum + (n.agents?.length ?? 0), 0);
  const navigate = useNavigate();

  return (
    <div className="view on" data-view="fleet" style={{ flexDirection: "column" }}>
      <div className="gv-head">
        <span className="gv-title">Fleet overview</span>
        <span className="gv-sub">
          · {nodes.length} node{nodes.length === 1 ? "" : "s"}
        </span>
      </div>
      <div className="gv-body">
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fill, minmax(180px, 1fr))",
            gap: 16,
            marginBottom: 24,
          }}
        >
          <StatCard label="Nodes" value={nodes.length} />
          <StatCard label="Online" value={online} tone="ok" />
          {draining > 0 && <StatCard label="Draining" value={draining} tone="warn" />}
          {offline > 0 && <StatCard label="Offline" value={offline} tone="err" />}
          <StatCard label="Agents" value={totalAgents} />
        </div>

        <div className="gk" style={{ marginBottom: 8 }}>
          Nodes
        </div>
        {nodes.length > 0 ? (
          <div
            className="ggrid"
            style={{ gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))" }}
          >
            {nodes.map((node) => (
              <div
                key={node.nodeId}
                role="button"
                tabIndex={0}
                className="gcard click"
                onClick={() =>
                  void navigate({ to: "/fleet/$nodeId", params: { nodeId: node.nodeId } })
                }
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    void navigate({
                      to: "/fleet/$nodeId",
                      params: { nodeId: node.nodeId },
                    });
                  }
                }}
              >
                <div className="grow" style={{ marginBottom: 8 }}>
                  <span
                    className="gtitle"
                    style={{ fontSize: 13, fontFamily: "var(--mono)" }}
                  >
                    {node.nodeId}
                  </span>
                  <span style={{ display: "inline-flex", gap: 4 }}>
                    <span className="gtag">{transportLabel(node)}</span>
                    <span className={statusTagClass(node.status)}>{node.status}</span>
                  </span>
                </div>
                <div
                  className="grow"
                  style={{ fontSize: 12, color: "var(--dim)", marginBottom: 6 }}
                >
                  <span>agents</span>
                  <span>{node.agents?.length ?? 0}</span>
                </div>
                <div className="grow" style={{ fontSize: 11, color: "var(--faint)" }}>
                  <span>heartbeat</span>
                  <span>{heartbeatLabel(node.lastHeartbeatAgoSecs)}</span>
                </div>
              </div>
            ))}
          </div>
        ) : (
          <div className="empty-state">
            <Icon name="server" size={28} />
            <div className="empty-state-title">No nodes registered</div>
            <div className="empty-state-msg">
              Click "Add node" in the sidebar to get a one-line setup command
              for a remote envoy.
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function StatCard({
  label,
  value,
  tone,
}: {
  label: string;
  value: number;
  tone?: "ok" | "warn" | "err";
}) {
  const color =
    tone === "ok"
      ? "var(--green)"
      : tone === "warn"
        ? "var(--amber)"
        : tone === "err"
          ? "var(--red)"
          : "var(--text)";
  return (
    <div className="gcard" style={{ textAlign: "center" }}>
      <div
        style={{ fontSize: 24, fontWeight: 600, fontFamily: "var(--mono)", color }}
      >
        {value}
      </div>
      <div className="gk" style={{ marginTop: 4 }}>
        {label}
      </div>
    </div>
  );
}

/** Detail page — a selected node's status, agents, sessions, actions. */
function NodeDetailPage({ nodeId }: { nodeId: string }) {
  const navigate = useNavigate();
  const { data: nodeData } = useNodes();
  const queryClient = useQueryClient();
  const [detecting, setDetecting] = useState(false);
  const [detectError, setDetectError] = useState<string | null>(null);
  const [actionBusy, setActionBusy] = useState(false);

  const node = nodeData?.nodes.find((n) => n.nodeId === nodeId) ?? null;

  if (!node) {
    return (
      <div className="view on" data-view="fleet" style={{ flexDirection: "column" }}>
        <div className="gv-head">
          <button
            type="button"
            className="icobtn"
            onClick={() => void navigate({ to: "/fleet" })}
            title="Back to fleet"
          >
            <Icon name="chevron-left" size={14} />
          </button>
          <span className="gv-title">{nodeId}</span>
        </div>
        <div className="gv-body">
          <div className="empty-state">
            <Icon name="server" size={28} />
            <div className="empty-state-title">Node not found</div>
            <div className="empty-state-msg">
              This node may have been removed or gone offline.
            </div>
          </div>
        </div>
      </div>
    );
  }

  const agents = node.agents ?? [];
  const pct = slotPct(node.slotsUsed, node.slotsTotal);

  const invalidate = async () => {
    await queryClient.invalidateQueries({ queryKey: ["nodes"] });
    await queryClient.invalidateQueries({ queryKey: ["agents"] });
  };

  const handleDetect = async () => {
    setDetecting(true);
    setDetectError(null);
    try {
      await refreshNodeAgents(node.nodeId);
      await invalidate();
    } catch (e) {
      // Surface the honest error (e.g. node's envoy disconnected, probe
      // timeout) instead of silently doing nothing.
      setDetectError(e instanceof Error ? e.message : String(e));
    } finally {
      setDetecting(false);
    }
  };

  const handleDrain = async () => {
    setActionBusy(true);
    try {
      await drainNode(node.nodeId);
      await invalidate();
    } catch {
      // node may have vanished
    } finally {
      setActionBusy(false);
    }
  };

  const handleRemove = async () => {
    if (
      !window.confirm(
        `Remove node "${node.nodeId}" from the fleet? Its allowlist entry is revoked.`,
      )
    )
      return;
    setActionBusy(true);
    try {
      await removeNode(node.nodeId);
      void navigate({ to: "/fleet" });
      await invalidate();
    } catch {
      // keep page open on failure
    } finally {
      setActionBusy(false);
    }
  };

  return (
    <div className="view on" data-view="fleet" style={{ flexDirection: "column" }}>
      <div className="gv-head">
        <button
          type="button"
          className="icobtn"
          onClick={() => void navigate({ to: "/fleet" })}
          title="Back to fleet"
        >
          <Icon name="chevron-left" size={14} />
        </button>
        <span className="gv-title" style={{ fontFamily: "var(--mono)" }}>
          {node.nodeId}
        </span>
        <span className="gv-sub">· {transportLabel(node)}</span>
        <div className="gv-actions">
          <button
            type="button"
            className="btn"
            title="Re-detect agents on this node"
            disabled={detecting}
            onClick={handleDetect}
          >
            <Icon name="activity" size={12} />
            {detecting ? "Detecting…" : "Detect agents"}
          </button>
          <button
            type="button"
            className="btn"
            disabled={node.local || actionBusy || node.status === "draining"}
            title={
              node.local
                ? "Local node cannot be drained"
                : "Stop routing new sessions to this node"
            }
            onClick={handleDrain}
          >
            Drain
          </button>
          <button
            type="button"
            className="btn"
            disabled={node.local || actionBusy}
            title={
              node.local
                ? "Local node cannot be removed"
                : "Deregister + revoke allowlist entry"
            }
            onClick={handleRemove}
          >
            <Icon name="trash" size={12} />
            Remove
          </button>
        </div>
      </div>
      <div className="gv-body">
        {detectError && (
          <div className="fleet-detect-err" role="alert">
            <Icon name="alert" size={13} />
            <span>Detect failed: {detectError}</span>
          </div>
        )}

        {/* Node header — status / transport / slots / heartbeat at a glance. */}
        <section className="fleet-section">
          <div className="fleet-node-header">
            <div className="gcard">
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
            </div>
            <div className="gcard">
              <div className="kv">
                <span className="k">SLOTS</span>
                <span className="v">
                  {node.slotsUsed} / {node.slotsTotal}
                </span>
              </div>
              <div className="gbar">
                <i style={{ width: `${pct}%` }} />
              </div>
              <div className="kv">
                <span className="k">HEARTBEAT</span>
                <span className="v">{heartbeatLabel(node.lastHeartbeatAgoSecs)}</span>
              </div>
              <div className="kv">
                <span className="k">VERSION</span>
                <span className="v fleet-version">{node.version}</span>
              </div>
            </div>
            {node.irohNodeId && (
              <div className="gcard">
                <div className="kv">
                  <span className="k">IROH ID</span>
                </div>
                <span className="v mono fleet-iroh-id">{node.irohNodeId}</span>
              </div>
            )}
          </div>
        </section>

        {/* Agents table — the harnesses this node's envoy discovered. */}
        <section className="fleet-section">
          <div className="fleet-section-head">
            <span className="gk">Agents</span>
            <span className="fleet-count">{agents.length}</span>
          </div>
          {agents.length > 0 ? (
            <div className="fleet-table-wrap">
              <table className="hist-table fleet-agents">
                <thead>
                  <tr>
                    <th className="col-kind">Kind</th>
                    <th>ID</th>
                    <th>Provider / Model</th>
                    <th className="col-avail">Availability</th>
                  </tr>
                </thead>
                <tbody>
                  {agents.map((a) => (
                    <AgentRow key={a.id} agent={a} />
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <div className="fleet-empty">
              No agents detected — click “Detect agents” to re-scan.
            </div>
          )}
        </section>

        {/* Sessions pinned to this node. */}
        <section className="fleet-section">
          <NodeSessions nodeId={node.nodeId} />
        </section>
      </div>
    </div>
  );
}

/** Human-readable availability for an agent's `ready` state. */
function availability(agent: AgentInfo): { label: string; tone: string } {
  if (agent.ready === undefined) return { label: "ready", tone: "ok" };
  if (agent.ready) return { label: "ready", tone: "ok" };
  return { label: "needs login", tone: "warn" };
}

function AgentRow({ agent }: { agent: AgentInfo }) {
  const avail = availability(agent);
  const providerModel = [agent.provider, agent.model].filter(Boolean).join(" · ") || "—";
  return (
    <tr>
      <td className="col-kind">
        <BrandIcon
          name={agentBrand(agent.kind, agent.provider)}
          size={15}
          title={agent.kind}
        />
      </td>
      <td className="mono">
        {agent.id}
        {agent.isDefault && <span className="fleet-default-tag">default</span>}
      </td>
      <td className="mono fleet-provider" title={agent.version ?? undefined}>
        {providerModel}
      </td>
      <td className="col-avail">
        <span className={`gtag ${avail.tone}`}>{avail.label}</span>
      </td>
    </tr>
  );
}

/** Live sessions pinned to a node (inline in detail page). */
function NodeSessions({ nodeId }: { nodeId: string }) {
  const sessionsQ = useSessions({ node: nodeId, limit: 10 });
  const sessions = sessionsQ.data?.sessions ?? [];

  return (
    <>
      <div className="gk" style={{ marginBottom: 8 }}>
        Sessions on node ({sessions.length})
      </div>
      {sessions.length > 0 ? (
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
          {sessions.map((s) => (
            <span key={s.id} title={s.id}>
              {s.title ?? s.id} · {s.liveness}
            </span>
          ))}
        </div>
      ) : (
        <div className="mono" style={{ fontSize: 11, color: "var(--faint)" }}>
          No live sessions pinned to this node.
        </div>
      )}
    </>
  );
}

// ── Add-node modal ─────────────────────────────────

function AddNodeModal({ onClose }: { onClose: () => void }) {
  const [enroll, setEnroll] = useState<EnrollResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  React.useEffect(() => {
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
