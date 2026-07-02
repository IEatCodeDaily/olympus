import { useCallback } from "react";
import { useRouterState, useNavigate } from "@tanstack/react-router";
import { Icon, type IconName } from "./components/Icon";
import { useUIStore } from "./store";
import { useSessions, useHealth } from "./hooks/queries";
import { parseRoute } from "./router";
import type { Session } from "./types";
import ChatView from "./views/ChatView";
import FleetView from "./views/FleetView";

const LAYOUTS: { sec: string; label: string; icon: IconName; path: string }[] = [
  { sec: "sessions", label: "Sessions", icon: "message-square", path: "/" },
  { sec: "fleet", label: "Fleet", icon: "server", path: "/fleet" },
  { sec: "agents", label: "Agents", icon: "bot", path: "/agents" },
  { sec: "board", label: "Board", icon: "kanban", path: "/board" },
  { sec: "settings", label: "Settings", icon: "gear", path: "/settings" },
];

function timeAgo(ts: number): string {
  const diff = Date.now() / 1000 - ts;
  if (diff < 60) return "now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}

export function AppShell() {
  const { location } = useRouterState();
  const { view, sessionId } = parseRoute(location.pathname);

  return (
    <div className="app">
      <TopBar activeView={view} />
      <div className="body">
        <LeftSidebar activeView={view} activeSessionId={sessionId} />
        <div className="viewport">
          {view === "sessions" && sessionId ? (
            <ChatView sessionId={sessionId} />
          ) : view === "fleet" || view === "agents" ? (
            <FleetView />
          ) : view === "sessions" ? (
            <SessionListPane />
          ) : (
            <GenericPane title={view} />
          )}
        </div>
      </div>
    </div>
  );
}

function TopBar({ activeView }: { activeView: string }) {
  const navigate = useNavigate();
  const { toggleSidebar, setPaletteOpen } = useUIStore();
  const { data: health } = useHealth() as { data: { hermesProfile?: string } | undefined };

  return (
    <div className="topbar">
      <div className="tb-left">
        <button type="button" className="icobtn" onClick={toggleSidebar} title="Toggle sidebar">
          <Icon name="panel-left" />
        </button>
        <div className="divider" />
        <div className="layouts">
          {LAYOUTS.map((l) => (
            <button
              type="button"
              key={l.sec}
              className={`chip ${activeView === l.sec ? "on" : ""}`}
              onClick={() => void navigate({ to: l.path })}
            >
              <Icon name={l.icon} size={12} />
              {l.label}
            </button>
          ))}
        </div>
      </div>
      <div className="tb-center">
        <button type="button" className="tb-search" onClick={() => setPaletteOpen(true)}>
          <Icon name="search" size={13} />
          <span className="ph">Search…</span>
          <span className="sp" />
          <span className="kbd">⌘K</span>
        </button>
      </div>
      <div className="tb-right">
        <div className="org">
          <div className="mk" />
          <span className="nm">{health?.hermesProfile ?? "default"}</span>
        </div>
        <div className="profile">rp</div>
      </div>
    </div>
  );
}

function LeftSidebar({
  activeView,
  activeSessionId,
}: {
  activeView: string;
  activeSessionId: string | null;
}) {
  const navigate = useNavigate();
  const { sidebarCollapsed, sidebarWidth } = useUIStore();
  const { data: sessionData } = useSessions({ managed: true });
  const { data: historyData } = useSessions({ managed: false, limit: 20 });
  const sessions = sessionData?.sessions ?? [];
  const history = historyData?.sessions ?? [];

  const handleNewSession = useCallback(async () => {
    try {
      const res = await fetch("/api/sessions", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${import.meta.env.VITE_API_TOKEN}`,
        },
        body: JSON.stringify({}),
      });
      if (res.ok) {
        const data = await res.json();
        const id = data.session?.id;
        if (id) void navigate({ to: `/sessions/$sessionId`, params: { sessionId: id } });
      }
    } catch {
      // sessions list will refetch
    }
  }, [navigate]);

  const handleSelectSession = useCallback(
    (id: string) => {
      void navigate({ to: `/sessions/$sessionId`, params: { sessionId: id } });
    },
    [navigate],
  );

  if (sidebarCollapsed) return null;

  return (
    <>
      <div className="sidebar" style={{ width: sidebarWidth }}>
        <div className="sb-pad">
          <button type="button" className="newbtn" onClick={handleNewSession}>
            <Icon name="plus" size={14} />
            New Session
          </button>
        </div>
        <div className="sb-scroll">
          <SessionSection
            label="RECENT"
            sessions={sessions}
            activeSessionId={activeSessionId}
            onSelect={handleSelectSession}
          />
          {history.length > 0 && (
            <SessionSection
              label="HISTORY"
              sessions={history}
              activeSessionId={activeSessionId}
              onSelect={handleSelectSession}
            />
          )}
        </div>
      </div>
      <div className="rz-x" />
    </>
  );
}

function SessionSection({
  label,
  sessions,
  count: _count,
  activeSessionId,
  onSelect,
}: {
  label: string;
  sessions: Session[];
  count?: number;
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
            onClick={() => onSelect(s.id)}
          >
            <span className={`dot ${s.liveness === "active" ? "active" : "idle"}`} />
            <span className="info">
              <span className="title">{s.title || "Untitled"}</span>
            </span>
            <span className="meta">
              <span>{s.messageCount}</span>
              <span>{timeAgo(s.lastActivity)}</span>
            </span>
          </button>
        ))}
      </div>
    </>
  );
}

function SessionListPane() {
  return (
    <>
      <div className="gv-head">
        <span className="gv-title">Sessions</span>
      </div>
      <div className="gv-body">
        <div className="empty-state">
          <Icon name="message-square" size={32} />
          <div className="empty-state-title">Select a session</div>
          <div className="empty-state-msg">
            Choose a session from the sidebar or create a new one to start chatting.
          </div>
        </div>
      </div>
    </>
  );
}

function GenericPane({ title }: { title: string }) {
  return (
    <>
      <div className="gv-head">
        <span className="gv-title">{title.charAt(0).toUpperCase() + title.slice(1)}</span>
      </div>
      <div className="gv-body">
        <div className="empty-state">
          <div className="empty-state-title">{title} view</div>
          <div className="empty-state-msg">Loading…</div>
        </div>
      </div>
    </>
  );
}
