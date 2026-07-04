/**
 * SessionSidebar — the View-owned left sidebar for Sessions.
 *
 * Sections:
 *  - PINNED  = sessions the USER pinned (session.pinned) — never derived from
 *    liveness. A running session gets a spinner icon, not a section move.
 *  - RECENT  = managed, non-pinned, non-archived sessions, capped at 5 most
 *    recent. Anything older lives in the History page ("View all" NavItem).
 *
 * Bug fixes:
 *  - Bug 5: Liveness dot shown ONLY when a turn is live; no dot for idle.
 *  - Bug 10: "New session" opens an agent picker (list from /api/agents).
 *  - Pin/archive hover actions actually PATCH the session now.
 */

import { useState, useCallback } from "react";
import { useNavigate, useRouterState } from "@tanstack/react-router";
import { Icon } from "../../../components/Icon";
import { useSessions, useUpdateSession } from "../../../hooks/queries";
import { useUIStore } from "../../../store";
import { createSession } from "../../../api";
import type { Session } from "../../../types";
import { timeAgo } from "../helpers";
import { AgentPicker } from "./AgentPicker";

/** Max rows in the RECENT section; the rest live in the History page. */
const RECENT_LIMIT = 5;

/** On phone-width screens the sidebar is a fixed overlay — close it after
 *  navigation so the destination is actually visible (drawer pattern). */
function isPhoneViewport(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(max-width: 820px)").matches
  );
}

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
  const { data: sessionData } = useSessions({ managed: true, archived: false });
  const sessions = sessionData?.sessions ?? [];
  const toggleSidebar = useUIStore((s) => s.toggleSidebar);

  // Close the overlay sidebar after navigating on phone screens.
  const closeIfPhone = useCallback(() => {
    if (isPhoneViewport()) toggleSidebar();
  }, [toggleSidebar]);

  // PINNED = user-pinned only. Liveness NEVER moves a session here.
  const pinned = sessions.filter((s) => s.pinned);
  // RECENT = managed, not pinned, capped at the most recent 5.
  const recent = sessions.filter((s) => !s.pinned).slice(0, RECENT_LIMIT);

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
      closeIfPhone();
    },
    [navigate, closeIfPhone],
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
          <NavItem label="Agents" icon="bot" path="/sessions/agents" />
          <NavItem label="Usage" icon="activity" path="/sessions/usage" />
          <NavItem label="History" icon="clock" path="/sessions/history" />
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
        {sessions.map((s) => (
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
 *  Status icon (left): spinner when agent running, orange ! when waiting for
 *  input, nothing when idle. */
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
  const update = useUpdateSession();

  // Tooltip: agent · node · model · summary
  const tooltip = [
    session.agent ? `agent: ${session.agent}` : null,
    session.node ? `node: ${session.node}` : null,
    session.model ? `model: ${session.model}` : null,
    `${session.messageCount} messages`,
  ]
    .filter(Boolean)
    .join(" · ");

  const isRunning = session.liveness === "running" || session.liveness === "active";
  const needsInput = session.liveness === "input-required";
  const showIcon = isRunning || needsInput;

  return (
    <div
      className={`srow ${active ? "on" : ""}`}
      data-session-id={session.id}
      data-managed={session.managed ? "true" : "false"}
      data-pinned={session.pinned ? "true" : "false"}
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
          title={session.pinned ? "Unpin" : "Pin"}
          onClick={(e) => {
            e.stopPropagation();
            update.mutate({ id: session.id, patch: { pinned: !session.pinned } });
          }}
        >
          <Icon name="pin" size={11} />
        </button>
        <button
          type="button"
          className="srow-act"
          title="Archive"
          onClick={(e) => {
            e.stopPropagation();
            update.mutate({ id: session.id, patch: { archived: true } });
          }}
        >
          <Icon name="archive" size={11} />
        </button>
      </span>
    </div>
  );
}
