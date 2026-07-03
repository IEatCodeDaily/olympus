// NodeDetailPage — /fleet/$nodeId full-page detail view.
//
// Promoted from the old NodePanel aside:
//   - Node info header (status, badges, KV rows)
//   - Slot detail + bar
//   - Sessions running on this node (GET /api/sessions?node=<id>)
//   - Collapsible raw heartbeat JSON
//
// The back-chevron navigates to /fleet.

import { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { Icon } from "../../../components/Icon";
import { useSessions } from "../../../hooks/queries";
import type { NodeInfo, Session } from "../../../types";
import {
  statusDotClass,
  statusBadgeClass,
  slotBarClass,
  slotPct,
  heartbeatLabel,
  sessionAge,
} from "../helpers";

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

export function NodeDetailPage({ node }: { node: NodeInfo }) {
  const navigate = useNavigate();
  const pct = slotPct(node.slotsUsed, node.slotsTotal);
  const [heartbeatOpen, setHeartbeatOpen] = useState(false);

  const sessionsQ = useSessions({ node: node.nodeId, limit: 20 });
  const sessions: Session[] = sessionsQ.data?.sessions ?? [];

  const heartbeatRaw = JSON.stringify(
    {
      nodeId: node.nodeId,
      hostname: node.hostname,
      status: node.status,
      slotsUsed: node.slotsUsed,
      slotsTotal: node.slotsTotal,
      version: node.version,
      local: node.local,
      lastHeartbeatAgoSecs: node.lastHeartbeatAgoSecs,
    },
    null,
    2
  );

  return (
    <div
      style={{ display: "flex", flexDirection: "column", height: "100%", minHeight: 0 }}
      aria-label={`Node ${node.nodeId} detail`}
    >
      {/* Page header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "0 var(--panel-pad) var(--panel-pad)",
          flexShrink: 0,
        }}
      >
        <button
          type="button"
          className="ol-btn ol-btn-ghost ol-btn-sm"
          onClick={() => void navigate({ to: "/fleet" })}
          aria-label="Back to fleet"
          title="All nodes"
        >
          <Icon name="chevron-left" size={13} />
        </button>
        <span className={statusDotClass(node.status)} />
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: "var(--fs-14)",
            fontWeight: "var(--fw-semibold)",
            color: "var(--text)",
          }}
        >
          {node.nodeId}
        </span>
        {node.local && <span className="ol-badge ol-badge-accent">LOCAL</span>}
        <span className={statusBadgeClass(node.status)}>{node.status}</span>
      </div>

      {/* Scrollable body */}
      <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "0 var(--panel-pad)", display: "flex", flexDirection: "column", gap: 24 }}>

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

        {/* Running sessions */}
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

        {/* Raw heartbeat JSON — collapsible */}
        <div>
          <button
            type="button"
            style={{
              display: "flex",
              alignItems: "center",
              gap: 5,
              background: "none",
              border: "none",
              cursor: "pointer",
              fontFamily: "var(--font-mono)",
              fontSize: "var(--fs-10)",
              textTransform: "uppercase",
              letterSpacing: "var(--tracking-caps)",
              color: "var(--text-faint)",
              padding: 0,
              marginBottom: heartbeatOpen ? 8 : 0,
            }}
            onClick={() => setHeartbeatOpen((v) => !v)}
            aria-expanded={heartbeatOpen}
          >
            <Icon name={heartbeatOpen ? "chevron-down" : "chevron-right"} size={10} />
            RAW HEARTBEAT
          </button>
          {heartbeatOpen && (
            <pre
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: "var(--fs-10)",
                color: "var(--text-dim)",
                background: "var(--bg)",
                border: "var(--border-w) solid var(--border)",
                borderRadius: "var(--radius)",
                padding: "8px 10px",
                overflowX: "auto",
                margin: 0,
                whiteSpace: "pre-wrap",
              }}
            >
              {heartbeatRaw}
            </pre>
          )}
        </div>
      </div>
    </div>
  );
}
