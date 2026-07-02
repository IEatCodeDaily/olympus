import { createRouter, createRootRoute, createRoute } from "@tanstack/react-router";
import { AppShell } from "./AppShell";

// ── Route tree ─────────────────────────────────────
// URL is the single source of truth for:
//   /                         → sessions list (no active session)
//   /sessions/$sessionId      → chat view for a specific session
//   /vaults                   → vaults list (placeholder until V-UI)
//   /vaults/$vaultId          → vault detail (placeholder)
//   /vaults/$vaultId/$notePath→ note editor (placeholder)
//   /projects                 → projects / kanban board (placeholder until P1)
//   /projects/$boardId        → specific board (placeholder)
//   /fleet                    → fleet management
//   /settings                 → settings (placeholder until ST1)
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

const vaultNoteRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/vaults/$vaultId/$notePath",
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
  sessionRoute,
  vaultsRoute,
  vaultDetailRoute,
  vaultNoteRoute,
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

/** Extract the active surface + session from the current URL. */
export function parseRoute(pathname: string): {
  surface: SurfaceName;
  sessionId: string | null;
} {
  if (pathname.startsWith("/sessions/")) {
    const id = pathname.split("/sessions/")[1];
    return { surface: "sessions", sessionId: id || null };
  }
  if (pathname.startsWith("/vaults")) return { surface: "vaults", sessionId: null };
  if (pathname.startsWith("/projects")) return { surface: "projects", sessionId: null };
  if (pathname.startsWith("/fleet")) return { surface: "fleet", sessionId: null };
  if (pathname.startsWith("/settings")) return { surface: "settings", sessionId: null };
  return { surface: "sessions", sessionId: null };
}
