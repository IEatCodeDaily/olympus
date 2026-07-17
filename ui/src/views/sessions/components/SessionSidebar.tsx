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

import { useState, useCallback, useEffect, useRef } from "react";
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
  openSessionIds = new Set(),
  paneMarks = new Map(),
  onOpenSession,
  onResizeStart,
  onResizeKeyDown,
}: {
  width: number;
  activeSessionId: string | null;
  openSessionIds?: ReadonlySet<string>;
  paneMarks?: ReadonlyMap<string, string>;
  onOpenSession?: (id: string, split?: "right" | "below") => void;
  onResizeStart?: (e: React.MouseEvent) => void;
  onResizeKeyDown?: (e: React.KeyboardEvent) => void;
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
  const [pickerError, setPickerError] = useState<string | null>(null);
  const [metadataOpen, setMetadataOpen] = useState(false);
  const [metadataFields, setMetadataFields] = useState<ReadonlySet<SessionMetadataField>>(
    () => new Set(DEFAULT_SESSION_METADATA_FIELDS),
  );

  // Bug 10: open agent picker instead of creating immediately
  const handleNewSession = useCallback(() => {
    setPickerError(null);
    setPickerOpen(true);
  }, []);

  const handlePickAgent = useCallback(
    async (agentId: string, nodeId: string) => {
      setPickerError(null);
      try {
        const session = await createSession({ agent: agentId, node: nodeId });
        setPickerOpen(false);
        if (session?.id) {
          void navigate({
            to: "/sessions/$sessionId",
            params: { sessionId: session.id },
          });
        }
      } catch (error) {
        setPickerOpen(true);
        setPickerError(error instanceof Error ? error.message : `Could not create session on ${nodeId}`);
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
              openSessionIds={openSessionIds}
              paneMarks={paneMarks}
              metadataFields={metadataFields}
              onSelect={handleSelectSession}
              onOpenSession={onOpenSession}
            />
          )}
          <SessionSection
            label="RECENT"
            sessions={recent}
            activeSessionId={activeSessionId}
            openSessionIds={openSessionIds}
            paneMarks={paneMarks}
            metadataFields={metadataFields}
            onSelect={handleSelectSession}
            onOpenSession={onOpenSession}
          />
        </div>
      </aside>
      <div className="rz-x" role="separator" aria-label="Resize sessions sidebar" aria-orientation="vertical" aria-valuemin={160} aria-valuemax={400} aria-valuenow={width} tabIndex={0} onMouseDown={onResizeStart} onKeyDown={onResizeKeyDown} />

      {/* Bug 10: agent picker modal */}
      <AgentPicker
        open={pickerOpen}
        onSelect={handlePickAgent}
        onCancel={() => setPickerOpen(false)}
        error={pickerError}
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
  openSessionIds,
  paneMarks,
  metadataFields,
  onSelect,
  onOpenSession,
}: {
  label: string;
  sessions: Session[];
  activeSessionId: string | null;
  openSessionIds: ReadonlySet<string>;
  paneMarks: ReadonlyMap<string, string>;
  metadataFields: ReadonlySet<SessionMetadataField>;
  onSelect: (id: string) => void;
  onOpenSession?: (id: string, split?: "right" | "below") => void;
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
            open={openSessionIds.has(s.id)}
            paneMark={paneMarks.get(s.id)}
            metadataFields={metadataFields}
            onSelect={onSelect}
            onOpenSession={onOpenSession}
          />
        ))}
      </div>
    </>
  );
}

/** A clean session row: title left, time right, hover reveals pin/archive.
 *  Status icon (left): spinner when agent running, orange ! when waiting for
 *  input, nothing when idle. Right-click or ... button reveals context menu
 *  with Open / Open Right / Open Below. */
function SessionRow({
  session,
  active,
  open,
  paneMark,
  metadataFields,
  onSelect,
  onOpenSession,
}: {
  session: Session;
  active: boolean;
  open: boolean;
  paneMark?: string;
  metadataFields: ReadonlySet<SessionMetadataField>;
  onSelect: (id: string) => void;
  onOpenSession?: (id: string, split?: "right" | "below") => void;
}) {
  const title = session.title || "Untitled";
  const time = timeAgo(session.lastActivity);
  const metadata = sessionMetadata(session, metadataFields);
  const update = useUpdateSession();

  const isRunning = session.liveness === "running" || session.liveness === "active";
  const needsInput = session.liveness === "input-required";
  const showIcon = isRunning || needsInput;

  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const rowRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") setMenu(null); };
    window.addEventListener("click", close);
    window.addEventListener("contextmenu", close, true);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("contextmenu", close, true);
      window.removeEventListener("keydown", onKey);
    };
  }, [menu]);

  const openMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY });
  }, []);

  const fireOpen = useCallback((split?: "right" | "below") => {
    setMenu(null);
    if (onOpenSession) onOpenSession(session.id, split);
    else onSelect(session.id);
  }, [onOpenSession, onSelect, session.id]);

  return (
    <div
      ref={rowRef}
      className={`srow ${active ? "on focused" : ""}`}
      data-session-id={session.id}
      data-managed={session.managed ? "true" : "false"}
      data-pinned={session.pinned ? "true" : "false"}
      data-open={open ? "true" : "false"}
      data-focused={active ? "true" : "false"}
      draggable
      onDragStart={(event) => {
        event.dataTransfer.effectAllowed = "copy";
        event.dataTransfer.setData(
          "application/x-olympus-session",
          JSON.stringify({ type: "session", sessionId: session.id, title }),
        );
      }}
      onClick={() => onSelect(session.id)}
      onContextMenu={openMenu}
    >
      {/* Instant hover card: node · agent · model (native title is too slow) */}
      <span className="srow-hovercard" role="tooltip">
        <span className="hc-row"><span className="hc-k">node</span><span className="hc-v">{session.node ?? "olympus"}</span></span>
        <span className="hc-row"><span className="hc-k">agent</span><span className="hc-v">{session.agent ?? "—"}</span></span>
        <span className="hc-row"><span className="hc-k">model</span><span className="hc-v">{session.model ?? "—"}</span></span>
      </span>
      {showIcon && (
        <span className="srow-icon">
          {isRunning ? (
            <span className="srow-spinner" />
          ) : (
            <span className="srow-dot needs-input" title="Waiting for your input" />
          )}
        </span>
      )}
      <span className="srow-copy">
        <span className="srow-title">{title}</span>
        {metadata.length > 0 && <span className="srow-meta">{metadata.join(" · ")}</span>}
      </span>
      {paneMark && <span className="srow-pane-mark">{paneMark}</span>}
      <span className="srow-time">{time}</span>
      {/* Hover actions */}
      <span className="srow-actions">
        <button
          type="button"
          className="srow-act"
          title="Open menu"
          onClick={openMenu}
        >
          <Icon name="ellipsis" size={11} />
        </button>
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
      {menu && (
        <div
          className="ctx-menu"
          role="menu"
          style={{ position: "fixed", left: menu.x, top: menu.y, zIndex: 1000 }}
          onClick={(e) => e.stopPropagation()}
        >
          <button type="button" className="ctx-item" role="menuitem" onClick={() => fireOpen()}>
            <Icon name="message-square" size={12} />
            Open
          </button>
          <button type="button" className="ctx-item" role="menuitem" onClick={() => fireOpen("right")}>
            <Icon name="panel-right" size={12} />
            Open Right
          </button>
          <button type="button" className="ctx-item" role="menuitem" onClick={() => fireOpen("below")}>
            <Icon name="panel-bottom" size={12} />
            Open Below
          </button>
          <div className="ctx-sep" />
          <button
            type="button"
            className="ctx-item"
            role="menuitem"
            onClick={(e) => { e.stopPropagation(); setMenu(null); update.mutate({ id: session.id, patch: { pinned: !session.pinned } }); }}
          >
            <Icon name="pin" size={12} />
            {session.pinned ? "Unpin" : "Pin"}
          </button>
          <button
            type="button"
            className="ctx-item"
            role="menuitem"
            onClick={(e) => { e.stopPropagation(); setMenu(null); update.mutate({ id: session.id, patch: { archived: true } }); }}
          >
            <Icon name="archive" size={12} />
            Archive
          </button>
        </div>
      )}
    </div>
  );
}
