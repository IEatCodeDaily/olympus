import { createRouter, createRootRoute, createRoute } from "@tanstack/react-router";
import { AppShell } from "./AppShell";

// ── Route tree ─────────────────────────────────────
// URL is the single source of truth for:
//   /              → sessions list (no active session)
//   /sessions/:id  → chat view for a specific session
//   /fleet         → fleet management
//   /agents        → agent management
//   /board         → kanban board
//   /settings      → settings
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
  component: () => null, // AppShell reads search params for active session
});

const sessionRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sessions/$sessionId",
  component: () => null,
});

const fleetRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/fleet",
  component: () => null,
});

const agentsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/agents",
  component: () => null,
});

const boardRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/board",
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
  fleetRoute,
  agentsRoute,
  boardRoute,
  settingsRoute,
]);

export const router = createRouter({ routeTree });

// Required for TypeScript type safety with TanStack Router
declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

/** Extract the active view + session from the current URL. */
export function parseRoute(pathname: string): {
  view: string;
  sessionId: string | null;
} {
  if (pathname.startsWith("/sessions/")) {
    const id = pathname.split("/sessions/")[1];
    return { view: "sessions", sessionId: id || null };
  }
  if (pathname.startsWith("/fleet")) return { view: "fleet", sessionId: null };
  if (pathname.startsWith("/agents")) return { view: "agents", sessionId: null };
  if (pathname.startsWith("/board")) return { view: "board", sessionId: null };
  if (pathname.startsWith("/settings")) return { view: "settings", sessionId: null };
  return { view: "sessions", sessionId: null };
}
