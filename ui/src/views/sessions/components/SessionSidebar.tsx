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

  // PINNED = managed + active liveness
  const pinned = sessions.filter((s) => s.liveness === "active");
  // RECENT = managed + not active
  const recent = sessions.filter((s) => s.liveness !== "active");
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
          <button
            type="button"
            key={s.id}
            className={`srow ${activeSessionId === s.id ? "on" : ""}`}
            data-session-id={s.id}
            data-managed={s.managed ? "true" : "false"}
            onClick={() => onSelect(s.id)}
          >
            {/* Bug 5: dot ONLY when liveness === "active" */}
            {s.liveness === "active" ? (
              <span className="dot active" />
            ) : (
              <span className="dot-spacer" />
            )}
            <span className="info">
              <span className="title">{s.title || "Untitled"}</span>
            </span>
            <span className="meta">
              {s.model && (
                <span
                  className="modelpill"
                  style={{
                    fontSize: 9,
                    padding: "1px 4px",
                    background: "none",
                    border: "none",
                    color: "var(--faint)",
                    fontFamily: "var(--font-mono)",
                    maxWidth: 64,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {s.model.split("-").slice(0, 2).join("-")}
                </span>
              )}
              <span>{s.messageCount}</span>
              <span>{timeAgo(s.lastActivity)}</span>
            </span>
          </button>
        ))}
      </div>
    </>
  );
}
