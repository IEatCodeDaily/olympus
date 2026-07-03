import { createRouter, createRootRoute, createRoute } from "@tanstack/react-router";
import { AppShell } from "./AppShell";

// ── Route tree ─────────────────────────────────────
// URL is the single source of truth for:
//   /                         → sessions list (no active session)
//   /sessions                 → sessions list (no active session)
//   /sessions/$sessionId      → chat view for a specific session
//   /sessions/agents          → agents page (Page = NavItem in sidebar)
//   /sessions/usage           → usage page (Page = NavItem in sidebar)
//   /vaults                   → vaults list (vault picker)
//   /vaults/$vaultId          → vault detail (note editor)
//   /vaults/$vaultId/tables   → vault tables view
//   /vaults/$vaultId/graph    → vault graph view
//   /projects                 → projects / kanban board (placeholder until P1)
//   /projects/$boardId        → specific board (placeholder)
//   /fleet                    → fleet management
//   /settings                 → settings (placeholder until ST1)
//
// The active note within a vault is tracked via the ?note=<path> query param
// (note paths contain slashes, so they can't be a clean route segment).
//
// Everything else (sidebar collapse, panel toggles, right-tab) stays in
// Zustand — those are ephemeral UI preferences, not things you'd bookmark
// or share as a link.

const rootRoute = createRootRoute({
  component: AppShell,
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: () => null, // AppShell reads location for active session
});

const sessionsIndexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sessions",
  component: () => null,
});

const sessionRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sessions/$sessionId",
  component: () => null,
});

const vaultsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/vaults",
  component: () => null,
});

const vaultDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/vaults/$vaultId",
  component: () => null,
});

const vaultTablesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/vaults/$vaultId/tables",
  component: () => null,
});

const vaultGraphRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/vaults/$vaultId/graph",
  component: () => null,
});

const projectsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/projects",
  component: () => null,
});

const projectBoardRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/projects/$boardId",
  component: () => null,
});

const fleetRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/fleet",
  component: () => null,
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: () => null,
});

const routeTree = rootRoute.addChildren([
  indexRoute,
  sessionsIndexRoute,
  sessionRoute,
  vaultsRoute,
  vaultDetailRoute,
  vaultTablesRoute,
  vaultGraphRoute,
  projectsRoute,
  projectBoardRoute,
  fleetRoute,
  settingsRoute,
]);

export const router = createRouter({ routeTree });

// Required for TypeScript type safety with TanStack Router
declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

/** The five navigable surfaces (in nav order). */
export type SurfaceName = "sessions" | "vaults" | "projects" | "fleet" | "settings";

/** The Sessions-view pages (left-sidebar NavItems inside the View). */
export type SessionsPage = "chat" | "agents" | "usage";

/** Which sub-page is active within the Vaults surface. */
export type VaultPage = "note" | "tables" | "graph";

/** Extract the active surface + per-surface context from the current URL. */
export function parseRoute(pathname: string): {
  surface: SurfaceName;
  sessionId: string | null;
  page: SessionsPage | null;
  vaultId: string | null;
  vaultPage: VaultPage;
} {
  const base = { sessionId: null, page: null, vaultId: null, vaultPage: "note" as VaultPage };
  if (pathname === "/sessions" || pathname === "/") {
    return { surface: "sessions", ...base };
  }
  if (pathname === "/sessions/agents") {
    return { surface: "sessions", ...base, page: "agents" };
  }
  if (pathname === "/sessions/usage") {
    return { surface: "sessions", ...base, page: "usage" };
  }
  if (pathname.startsWith("/sessions/")) {
    const id = pathname.split("/sessions/")[1];
    return { surface: "sessions", ...base, sessionId: id || null, page: "chat" };
  }
  if (pathname.startsWith("/vaults/")) {
    const rest = pathname.slice("/vaults/".length);
    // /vaults/$vaultId/tables, /vaults/$vaultId/graph, /vaults/$vaultId
    const parts = rest.split("/");
    const vaultId = parts[0] || null;
    const sub = parts[1];
    const vaultPage: VaultPage =
      sub === "tables" ? "tables" : sub === "graph" ? "graph" : "note";
    return { surface: "vaults", ...base, vaultId, vaultPage };
  }
  if (pathname.startsWith("/vaults")) return { surface: "vaults", ...base };
  if (pathname.startsWith("/projects")) return { surface: "projects", ...base };
  if (pathname.startsWith("/fleet")) return { surface: "fleet", ...base };
  if (pathname.startsWith("/settings")) return { surface: "settings", ...base };
  return { surface: "sessions", ...base };
}
