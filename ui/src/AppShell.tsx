// AppShell — the Olympus application frame.
//
// Layout (matches docs/design/concept/olympus-app-concept.html):
//   ┌─────────────────────────────────────────────────┐
//   │ TopBar (sidebar toggle · nav rail · search · org · profile) │
//   ├──────────┬──────────────┬───────────────────────┤
//   │ Left     │ Secondary    │ Viewport              │
//   │ rail     │ sidebar      │ (chat / fleet / etc.) │
//   │ (icons)  │ (per-surface)│                      │
//   │          │              │                      │
//   └──────────┴──────────────┴───────────────────────┘
//
// The left rail is a slim icon column with the 5 surfaces. Each surface
// provides its own secondary sidebar slot (session list, vault tree, etc.).
// Surfaces whose card hasn't merged render a .ol-* placeholder pane.

import { useCallback } from "react";
import { useRouterState, useNavigate } from "@tanstack/react-router";
import { Icon, type IconName } from "./components/Icon";
import { useUIStore } from "./store";
import { useSessions, useHealth, useNodes } from "./hooks/queries";
import { createSession } from "./api";
import { parseRoute, type SurfaceName } from "./router";
import { useTheme } from "./theme";
import type { Session } from "./types";
import ChatView from "./views/ChatView";
import FleetView from "./views/FleetView";
import { VaultsView, ProjectsView, SettingsView } from "./views/PlaceholderViews";

// ── Nav definition ─────────────────────────────────
// The 5 surfaces, in nav order. Matches the plan's table exactly.
const SURFACES: {
  surface: SurfaceName;
  label: string;
  icon: IconName;
  path: string;
}[] = [
  { surface: "sessions", label: "Sessions", icon: "message-square", path: "/" },
  { surface: "vaults", label: "Vaults", icon: "book", path: "/vaults" },
  { surface: "projects", label: "Projects", icon: "kanban", path: "/projects" },
  { surface: "fleet", label: "Fleet", icon: "server", path: "/fleet" },
  { surface: "settings", label: "Settings", icon: "gear", path: "/settings" },
];

// ── Helpers ────────────────────────────────────────

function timeAgo(ts: number): string {
  const diff = Date.now() / 1000 - ts;
  if (diff < 60) return "now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}

// ── Main shell ─────────────────────────────────────

export function AppShell() {
  const { location } = useRouterState();
  const { surface, sessionId } = parseRoute(location.pathname);
  const { sidebarCollapsed, sidebarWidth } = useUIStore();

  return (
    <div className="app">
      <TopBar activeSurface={surface} />
      <div className="body">
        {/* Left icon rail */}
        <Rail activeSurface={surface} />

        {/* Secondary sidebar — per-surface content */}
        {!sidebarCollapsed && surface === "sessions" && (
          <>
            <SessionSidebar
              width={sidebarWidth}
              activeSessionId={sessionId}
            />
          </>
        )}
        {!sidebarCollapsed && surface === "fleet" && (
          <SecondarySidebar width={sidebarWidth}>
            <FleetSidebar />
          </SecondarySidebar>
        )}
        {!sidebarCollapsed && (surface === "vaults" || surface === "projects" || surface === "settings") && (
          <SecondarySidebar width={sidebarWidth}>
            <PlaceholderSidebar surface={surface} />
          </SecondarySidebar>
        )}

        {/* Viewport — the active surface's main content */}
        <div className="viewport">
          {surface === "sessions" && sessionId ? (
            <ChatView sessionId={sessionId} />
          ) : surface === "sessions" ? (
            <SessionListPane />
          ) : surface === "fleet" ? (
            <FleetView />
          ) : surface === "vaults" ? (
            <VaultsView />
          ) : surface === "projects" ? (
            <ProjectsView />
          ) : surface === "settings" ? (
            <SettingsView />
          ) : (
            <SessionListPane />
          )}
        </div>
      </div>
    </div>
  );
}

// ── TopBar ─────────────────────────────────────────

function TopBar({ activeSurface }: { activeSurface: SurfaceName }) {
  const { toggleSidebar } = useUIStore();
  const { theme, toggleTheme } = useTheme();

  return (
    <div className="topbar">
      <div className="tb-left">
        <button
          type="button"
          className="icobtn"
          onClick={toggleSidebar}
          title="Toggle sidebar"
          aria-label="Toggle sidebar"
        >
          <Icon name="panel-left" size={14} />
        </button>
        <span className="divider" />
        <span className="tb-title">{SURFACES.find((s) => s.surface === activeSurface)?.label ?? "Olympus"}</span>
      </div>

      <div className="tb-center">
        <SearchPill />
      </div>

      <div className="tb-right">
        {/* Theme toggle */}
        <button
          type="button"
          className="icobtn"
          onClick={toggleTheme}
          title={theme === "obsidian" ? "Switch to light" : "Switch to dark"}
          aria-label="Toggle theme"
        >
          <Icon name={theme === "obsidian" ? "globe" : "sparkles"} size={14} />
        </button>
        <OrgChip />
        <div className="profile" title="rpw">rp</div>
      </div>
    </div>
  );
}

function SearchPill() {
  const { setPaletteOpen } = useUIStore();
  return (
    <button
      type="button"
      className="tb-search"
      onClick={() => setPaletteOpen(true)}
      title="Search (⌘K)"
    >
      <Icon name="search" size={13} />
      <span className="ph">Search…</span>
      <span className="sp" />
      <span className="kbd">⌘K</span>
    </button>
  );
}

function OrgChip() {
  const { data: health } = useHealth() as { data: { hermesProfile?: string } | undefined };
  return (
    <div className="org" title="Hermes profile">
      <span className="mk" />
      <span className="nm">{health?.hermesProfile ?? "default"}</span>
    </div>
  );
}

// ── Left icon rail ─────────────────────────────────

function Rail({ activeSurface }: { activeSurface: SurfaceName }) {
  const navigate = useNavigate();
  return (
    <nav className="rail" aria-label="Navigation">
      {SURFACES.map((s) => (
        <button
          type="button"
          key={s.surface}
          className={`rail-btn ${activeSurface === s.surface ? "on" : ""}`}
          onClick={() => void navigate({ to: s.path })}
          title={s.label}
          aria-label={s.label}
          aria-current={activeSurface === s.surface ? "page" : undefined}
        >
          <Icon name={s.icon} size={16} />
        </button>
      ))}
    </nav>
  );
}

// ── Secondary sidebar wrappers ─────────────────────

function SecondarySidebar({
  width,
  children,
}: {
  width: number;
  children: React.ReactNode;
}) {
  return (
    <>
      <aside className="sidebar" style={{ width }}>
        {children}
      </aside>
      <div className="rz-x" />
    </>
  );
}

function PlaceholderSidebar({ surface }: { surface: SurfaceName }) {
  const label = SURFACES.find((s) => s.surface === surface)?.label ?? surface;
  return (
    <div className="sb-scroll">
      <div className="sec-head">
        <span className="lbl">{label.toUpperCase()}</span>
      </div>
      <div className="sec-content">
        <div
          className="empty-state"
          style={{ minHeight: 120, padding: "16px 8px" }}
        >
          <div className="empty-state-msg">Coming soon</div>
        </div>
      </div>
    </div>
  );
}

function FleetSidebar() {
  const { data: nodesData } = useNodes();
  const nodes = nodesData?.nodes ?? [];

  return (
    <div className="sb-scroll">
      <div className="sb-pad">
        <button type="button" className="newbtn" title="Add node (UDS registration)">
          <Icon name="plus" size={14} />
          Add node
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
          <div key={n.nodeId} className="srow">
            <span
              className="dot"
              style={{
                background:
                  n.status === "online"
                    ? "var(--green)"
                    : n.status === "draining"
                      ? "var(--amber)"
                      : "var(--red)",
              }}
            />
            <div className="info">
              <span className="title">{n.nodeId}</span>
            </div>
            <span className="meta">
              <span>{n.lastHeartbeatAgoSecs}s</span>
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

// ── Session sidebar ────────────────────────────────

function SessionSidebar({
  width,
  activeSessionId,
}: {
  width: number;
  activeSessionId: string | null;
}) {
  const navigate = useNavigate();
  const { data: sessionData } = useSessions({ managed: true });
  const { data: historyData } = useSessions({ managed: false, limit: 20 });
  const sessions = sessionData?.sessions ?? [];
  const history = historyData?.sessions ?? [];

  const handleNewSession = useCallback(async () => {
    try {
      const session = await createSession();
      if (session?.id) void navigate({ to: `/sessions/$sessionId`, params: { sessionId: session.id } });
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

  return (
    <SecondarySidebar width={width}>
      <div className="sb-pad">
        <button type="button" className="newbtn" onClick={handleNewSession}>
          <Icon name="plus" size={14} />
          New session
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
            label="OBSERVED"
            sessions={history}
            activeSessionId={activeSessionId}
            onSelect={handleSelectSession}
          />
        )}
      </div>
    </SecondarySidebar>
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
            <span
              className={`dot ${s.liveness === "active" ? "active" : "idle"}`}
            />
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

// ── Session list empty pane ────────────────────────

function SessionListPane() {
  return (
    <>
      <div className="gv-head">
        <span className="gv-title">Sessions</span>
      </div>
      <div className="gv-body">
        <div className="empty-state">
          <div className="empty-state-icon">
            <Icon name="message-square" size={32} />
          </div>
          <div className="empty-state-title">Select a session</div>
          <div className="empty-state-msg">
            Choose a session from the sidebar or create a new one to start chatting.
          </div>
        </div>
      </div>
    </>
  );
}
