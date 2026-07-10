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

import { useRouterState, useNavigate } from "@tanstack/react-router";
import { Icon, type IconName } from "./components/Icon";
import { useUIStore } from "./store";
import { useHealth } from "./hooks/queries";
import { parseRoute, type SurfaceName } from "./router";
import { useTheme } from "./theme";
import { SessionsView } from "./views/SessionsView";
import FleetView from "./views/FleetView";
import { VaultWorkspaceView } from "./views/VaultWorkspaceView";
import { ProjectsView } from "./views/ProjectsView";
import { SettingsView } from "./views/PlaceholderViews";

// ── Helpers ────────────────────────────────────────

// timeAgo moved to views/sessions/helpers.ts (View-owned)

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

// (timeAgo moved to views/sessions/helpers.ts)

// ── Main shell ─────────────────────────────────────

export function AppShell() {
  const { location } = useRouterState();
  const { surface, sessionId, page, nodeId } = parseRoute(location.pathname);
  const { sidebarCollapsed, sidebarWidth } = useUIStore();

  return (
    <div className="app">
      <TopBar activeSurface={surface} />
      <div className="body">
        {/* Sessions View owns its own sidebar + viewport layout */}
        {surface === "sessions" && (
          <SessionsView sessionId={sessionId} page={page} />
        )}

        {/* Fleet View owns its own sidebar + viewport layout */}
        {surface === "fleet" && (
          <FleetView nodeId={nodeId} />
        )}

        {/* Vaults View owns its own sidebar + viewport layout */}
        {surface === "vaults" && (
          <VaultWorkspaceView />
        )}

        {/* Other surfaces keep the shell-level sidebar + viewport split */}
        {!sidebarCollapsed && (surface === "projects" || surface === "settings") && (
          <SecondarySidebar width={sidebarWidth}>
            <PlaceholderSidebar surface={surface} />
          </SecondarySidebar>
        )}

        {/* Viewport for shell-managed surfaces (projects, settings) */}
        {surface === "projects" || surface === "settings" ? (
          <div className="viewport">
            {surface === "projects" ? (
              <ProjectsView />
            ) : surface === "settings" ? (
              <SettingsView />
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}

// ── TopBar ─────────────────────────────────────────

function TopBar({ activeSurface }: { activeSurface: SurfaceName }) {
  const navigate = useNavigate();
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
          <Icon name="mountain" size={14} />
        </button>
        <span className="divider" />
        {/* View selector — icon chips for each surface (concept: topbar .layouts) */}
        <div className="layouts" role="tablist" aria-label="Surfaces">
          {SURFACES.map((s) => (
            <button
              type="button"
              key={s.surface}
              className={`chip ${activeSurface === s.surface ? "on" : ""}`}
              onClick={() => void navigate({ to: s.path })}
              title={s.label}
              aria-label={s.label}
              aria-current={activeSurface === s.surface ? "page" : undefined}
            >
              <Icon name={s.icon} size={13} />
            </button>
          ))}
        </div>
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
          <Icon name={theme === "obsidian" ? "sun" : "moon"} size={14} />
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


// ── Session sidebar ────────────────────────────────
// MOVED to views/sessions/components/SessionSidebar.tsx
// (View-owned per the View/Page architecture)
