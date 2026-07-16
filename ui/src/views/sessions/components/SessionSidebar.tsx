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
import {
  DEFAULT_SESSION_METADATA_FIELDS,
  SESSION_METADATA_FIELDS,
  sessionMetadata,
  toggleSessionMetadataField,
  type SessionMetadataField,
} from "./sessionSidebarPreferences";

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
  onResizeKeyDown,
  visibleSessionIds = new Set<string>(),
  sessionPaneLabels = new Map<string, string>(),
  onSelectSession,
  onOpenSessionRight,
  onOpenSessionBelow,
}: {
  width: number;
  activeSessionId: string | null;
  onResizeStart?: (e: React.MouseEvent) => void;
  onResizeKeyDown?: (e: React.KeyboardEvent) => void;
  visibleSessionIds?: ReadonlySet<string>;
  sessionPaneLabels?: ReadonlyMap<string, string>;
  onSelectSession?: (id: string) => void;
  onOpenSessionRight?: (id: string) => void;
  onOpenSessionBelow?: (id: string) => void;
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
  const recentIds = new Set(recent.map((session) => session.id));
  const openSessions = sessions.filter((session) =>
    visibleSessionIds.has(session.id) && !session.pinned && !recentIds.has(session.id),
  );

  const [pickerOpen, setPickerOpen] = useState(false);
  const [metadataOpen, setMetadataOpen] = useState(false);
  const [metadataFields, setMetadataFields] = useState<ReadonlySet<SessionMetadataField>>(
    () => new Set(DEFAULT_SESSION_METADATA_FIELDS),
  );

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
      if (onSelectSession) onSelectSession(id);
      else void navigate({ to: "/sessions/$sessionId", params: { sessionId: id } });
      closeIfPhone();
    },
    [navigate, closeIfPhone, onSelectSession],
  );

  return (
    <>
      <aside className="sidebar" style={{ width }}>
        <div className="sb-pad">
          <div className="session-sidebar-primary">
            <button type="button" className="newbtn" onClick={handleNewSession}>
              <Icon name="plus" size={14} />
              New session
            </button>
            <button type="button" className="icobtn" aria-label="Configure session row metadata" aria-expanded={metadataOpen} onClick={() => setMetadataOpen((open) => !open)}>
              <Icon name="settings-2" size={13} />
            </button>
          </div>
          {metadataOpen && (
            <div className="session-metadata-menu" role="group" aria-label="Session row metadata">
              {SESSION_METADATA_FIELDS.map((field) => (
                <label key={field}>
                  <input type="checkbox" checked={metadataFields.has(field)} onChange={() => setMetadataFields((current) => toggleSessionMetadataField(current, field))} />
                  <span>{field}</span>
                </label>
              ))}
            </div>
          )}
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
              visibleSessionIds={visibleSessionIds}
              sessionPaneLabels={sessionPaneLabels}
              metadataFields={metadataFields}
              onSelect={handleSelectSession}
              onOpenRight={onOpenSessionRight}
              onOpenBelow={onOpenSessionBelow}
            />
          )}
          {openSessions.length > 0 && (
            <SessionSection
              label="OPEN"
              sessions={openSessions}
              activeSessionId={activeSessionId}
              visibleSessionIds={visibleSessionIds}
              sessionPaneLabels={sessionPaneLabels}
              metadataFields={metadataFields}
              onSelect={handleSelectSession}
              onOpenRight={onOpenSessionRight}
              onOpenBelow={onOpenSessionBelow}
            />
          )}
          <SessionSection
            label="RECENT"
            sessions={recent}
            activeSessionId={activeSessionId}
            visibleSessionIds={visibleSessionIds}
            sessionPaneLabels={sessionPaneLabels}
            metadataFields={metadataFields}
            onSelect={handleSelectSession}
            onOpenRight={onOpenSessionRight}
            onOpenBelow={onOpenSessionBelow}
          />
        </div>
      </aside>
      <div className="rz-x" role="separator" aria-label="Resize sessions sidebar" aria-orientation="vertical" aria-valuemin={180} aria-valuemax={400} aria-valuenow={width} tabIndex={0} onMouseDown={onResizeStart} onKeyDown={onResizeKeyDown} />

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
  visibleSessionIds,
  sessionPaneLabels,
  metadataFields,
  onSelect,
  onOpenRight,
  onOpenBelow,
}: {
  label: string;
  sessions: Session[];
  activeSessionId: string | null;
  visibleSessionIds: ReadonlySet<string>;
  sessionPaneLabels: ReadonlyMap<string, string>;
  metadataFields: ReadonlySet<SessionMetadataField>;
  onSelect: (id: string) => void;
  onOpenRight?: (id: string) => void;
  onOpenBelow?: (id: string) => void;
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
            visible={visibleSessionIds.has(s.id)}
            paneLabel={sessionPaneLabels.get(s.id)}
            metadataFields={metadataFields}
            onSelect={onSelect}
            onOpenRight={onOpenRight}
            onOpenBelow={onOpenBelow}
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
  visible,
  paneLabel,
  metadataFields,
  onSelect,
  onOpenRight,
  onOpenBelow,
}: {
  session: Session;
  active: boolean;
  visible: boolean;
  paneLabel?: string;
  metadataFields: ReadonlySet<SessionMetadataField>;
  onSelect: (id: string) => void;
  onOpenRight?: (id: string) => void;
  onOpenBelow?: (id: string) => void;
}) {
  const title = session.title || "Untitled";
  const time = timeAgo(session.lastActivity);
  const update = useUpdateSession();

  const isRunning = session.liveness === "running" || session.liveness === "active";
  const needsInput = session.liveness === "input-required";
  const showIcon = isRunning || needsInput;
  const metadata = sessionMetadata(session, metadataFields);

  return (
    <div
      className={`srow ${active ? "on" : ""}${visible ? " visible" : ""}`}
      data-session-id={session.id}
      data-managed={session.managed ? "true" : "false"}
      data-pinned={session.pinned ? "true" : "false"}
    >
      <button
        type="button"
        className="srow-main"
        aria-current={active ? "page" : undefined}
        aria-describedby={visible ? `session-visible-${session.id}` : undefined}
        onClick={() => onSelect(session.id)}
      >
        {showIcon && (
          <span className="srow-icon" aria-label={isRunning ? "Running" : "Waiting for input"}>
            {isRunning ? <span className="srow-spinner" /> : <span className="srow-dot needs-input" />}
          </span>
        )}
        <span className="srow-copy">
          <span className="srow-title">{title}</span>
          {metadata.length > 0 && <span className="srow-meta">{metadata.join(" · ")}</span>}
        </span>
        <span className="srow-time">{time}</span>
        {visible && <span id={`session-visible-${session.id}`} className="srow-pane-mark">{active ? `ACTIVE · ${paneLabel}` : paneLabel}</span>}
      </button>
      {/* Hover actions */}
      <span className="srow-actions">
        {onOpenRight && (
          <button type="button" className="srow-act split-action" title="Open to right" aria-label={`Open ${title} to right`} onClick={() => onOpenRight(session.id)}>
            <Icon name="panel-right" size={11} />
          </button>
        )}
        {onOpenBelow && (
          <button type="button" className="srow-act split-action" title="Open below" aria-label={`Open ${title} below`} onClick={() => onOpenBelow(session.id)}>
            <Icon name="panel-bottom" size={11} />
          </button>
        )}
        <button
          type="button"
          className="srow-act manage-action"
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
          className="srow-act manage-action"
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
