/**
 * SessionSidebar — the View-owned left sidebar for Sessions.
 *
 * Moved out of AppShell per the View/Page architecture: the View owns the
 * left sidebar (session list + NavItems).
 *
 * Bug fixes:
 *  - Bug 5: Liveness dot shown ONLY when liveness === "active"; no dot for idle.
 *  - Bug 10: "New session" opens an agent picker (list from /api/agents);
 *    selecting an agent creates the managed session bound to that profile.
 */

import { useState, useCallback } from "react";
import { useNavigate, useRouterState } from "@tanstack/react-router";
import { Icon } from "../../../components/Icon";
import { useSessions } from "../../../hooks/queries";
import { createSession } from "../../../api";
import type { Session } from "../../../types";
import { timeAgo } from "../helpers";
import { AgentPicker } from "./AgentPicker";

export function SessionSidebar({
  width,
  activeSessionId,
  onResizeStart,
}: {
  width: number;
  activeSessionId: string | null;
  onResizeStart?: (e: React.MouseEvent) => void;
}) {
  const navigate = useNavigate();
  const { data: sessionData } = useSessions({ managed: true });
  const { data: historyData } = useSessions({ managed: false, limit: 20 });
  const sessions = sessionData?.sessions ?? [];
  const history = historyData?.sessions ?? [];

  // PINNED = a turn is live (running) or recently active
  const isLive = (s: Session) => s.liveness === "running" || s.liveness === "active";
  const pinned = sessions.filter(isLive);
  // RECENT = managed + not live
  const recent = sessions.filter((s) => !isLive(s));
  // OBSERVED = not managed (imported from hermes)
  const observed = history;

  const [pickerOpen, setPickerOpen] = useState(false);

  // Bug 10: open agent picker instead of creating immediately
  const handleNewSession = useCallback(() => {
    setPickerOpen(true);
  }, []);

  const handlePickAgent = useCallback(
    async (agentId: string) => {
      setPickerOpen(false);
      try {
        const session = await createSession({ agent: agentId });
        if (session?.id) {
          void navigate({
            to: "/sessions/$sessionId",
            params: { sessionId: session.id },
          });
        }
      } catch {
        // sessions list will refetch
      }
    },
    [navigate],
  );

  const handleSelectSession = useCallback(
    (id: string) => {
      void navigate({ to: "/sessions/$sessionId", params: { sessionId: id } });
    },
    [navigate],
  );

  return (
    <>
      <aside className="sidebar" style={{ width }}>
        <div className="sb-pad">
          <button type="button" className="newbtn" onClick={handleNewSession}>
            <Icon name="plus" size={14} />
            New session
          </button>
          {/* NavItems — Pages inside the Sessions View */}
          <NavItem
            label="Agents"
            icon="bot"
            path="/sessions/agents"
          />
          <NavItem
            label="Usage"
            icon="activity"
            path="/sessions/usage"
          />
        </div>
        <div className="sb-scroll">
          {pinned.length > 0 && (
            <SessionSection
              label="PINNED"
              sessions={pinned}
              activeSessionId={activeSessionId}
              onSelect={handleSelectSession}
            />
          )}
          <SessionSection
            label="RECENT"
            sessions={recent}
            activeSessionId={activeSessionId}
            onSelect={handleSelectSession}
          />
          {observed.length > 0 && (
            <SessionSection
              label="OBSERVED"
              sessions={observed}
              activeSessionId={activeSessionId}
              onSelect={handleSelectSession}
            />
          )}
        </div>
      </aside>
      <div className="rz-x" onMouseDown={onResizeStart} />

      {/* Bug 10: agent picker modal */}
      <AgentPicker
        open={pickerOpen}
        onSelect={handlePickAgent}
        onCancel={() => setPickerOpen(false)}
      />
    </>
  );
}

function NavItem({
  label,
  icon,
  path,
}: {
  label: string;
  icon: import("../../../components/Icon").IconName;
  path: string;
}) {
  const navigate = useNavigate();
  const pathname = useRouterState().location.pathname;
  const isActive = pathname === path;

  return (
    <button
      type="button"
      className={`navitem${isActive ? " on" : ""}`}
      onClick={() => void navigate({ to: path })}
      title={label}
    >
      <Icon name={icon} size={14} />
      <span>{label}</span>
    </button>
  );
}

function SessionSection({
  label,
  sessions,
  activeSessionId,
  onSelect,
}: {
  label: string;
  sessions: Session[];
  activeSessionId: string | null;
  onSelect: (id: string) => void;
}) {
  if (sessions.length === 0) return null;
  return (
    <>
      <div className="sec-head">
        <span className="lbl">{label}</span>
        <span className="sp" />
        <span className="ct">{sessions.length}</span>
      </div>
      <div className="sec-content">
        {sessions.slice(0, 50).map((s) => (
          <SessionRow
            key={s.id}
            session={s}
            active={activeSessionId === s.id}
            onSelect={onSelect}
          />
        ))}
      </div>
    </>
  );
}

/** A clean session row: title left, time right, hover reveals pin/archive.
 *  Status icon (left): spinner when agent running, green dot when completed,
 *  orange ! when waiting for input, nothing when idle/opened. */
function SessionRow({
  session,
  active,
  onSelect,
}: {
  session: Session;
  active: boolean;
  onSelect: (id: string) => void;
}) {
  const title = session.title || "Untitled";
  const time = timeAgo(session.lastActivity);

  // Tooltip: agent · node · model · summary
  const tooltip = [
    session.agent ? `agent: ${session.agent}` : null,
    session.node ? `node: ${session.node}` : null,
    session.model ? `model: ${session.model}` : null,
    `${session.messageCount} messages`,
  ]
    .filter(Boolean)
    .join(" · ");

  // Status icon logic:
  //   running = a turn is in-flight (managed) → spinner
  //   active  = recent activity (observed) → spinner
  //   input-required = agent blocked on a permission decision → orange dot
  //   idle    = no icon
  const isRunning = session.liveness === "running" || session.liveness === "active";
  const needsInput = session.liveness === "input-required";
  const showIcon = (isRunning || needsInput) && !active;

  return (
    <div
      className={`srow ${active ? "on" : ""}`}
      data-session-id={session.id}
      data-managed={session.managed ? "true" : "false"}
      title={tooltip}
      onClick={() => onSelect(session.id)}
    >
      {showIcon && (
        <span className="srow-icon">
          {isRunning ? (
            <span className="srow-spinner" />
          ) : (
            <span className="srow-dot needs-input" title="Waiting for your input" />
          )}
        </span>
      )}
      <span className="srow-title">{title}</span>
      <span className="srow-time">{time}</span>
      {/* Hover actions */}
      <span className="srow-actions">
        <button
          type="button"
          className="srow-act"
          title="Pin"
          onClick={(e) => e.stopPropagation()}
        >
          <Icon name="pin" size={11} />
        </button>
        <button
          type="button"
          className="srow-act"
          title="Archive"
          onClick={(e) => e.stopPropagation()}
        >
          <Icon name="archive" size={11} />
        </button>
      </span>
    </div>
  );
}
