import { create } from "zustand";

export type ViewName =
  | "sessions"
  | "vaults"
  | "projects"
  | "fleet"
  | "settings";

interface UIState {
  /** Active layout view (which pane is shown in the viewport). */
  view: ViewName;
  /** Active session id (for the chat view). */
  activeSessionId: string | null;
  /** Left sidebar collapsed. */
  sidebarCollapsed: boolean;
  /** Bottom panel collapsed. */
  bottomCollapsed: boolean;
  /** Right sidebar collapsed. */
  rightSidebarCollapsed: boolean;
  /** Bottom panel active tab. */
  bottomTab: "events" | "logs" | "raw";
  /** Right sidebar active tab. */
  rightTab: "info" | "artifacts";
  /** Command palette open. */
  paletteOpen: boolean;
  /** Sidebar width (px). */
  sidebarWidth: number;

  setView: (v: ViewName) => void;
  setActiveSession: (id: string | null) => void;
  toggleSidebar: () => void;
  toggleBottom: () => void;
  toggleRightSidebar: () => void;
  setBottomTab: (t: "events" | "logs" | "raw") => void;
  setRightTab: (t: "info" | "artifacts") => void;
  setPaletteOpen: (open: boolean) => void;
  setSidebarWidth: (w: number) => void;
}

/** True on phone-width screens where the sidebar renders as a fixed overlay. */
function startCollapsed(): boolean {
  try {
    return window.matchMedia("(max-width: 820px)").matches;
  } catch {
    return false; // jsdom / SSR
  }
}

export const useUIStore = create<UIState>((set) => ({
  view: "sessions",
  activeSessionId: null,
  sidebarCollapsed: startCollapsed(),
  bottomCollapsed: true,
  rightSidebarCollapsed: false,
  bottomTab: "events",
  rightTab: "info",
  paletteOpen: false,
  sidebarWidth: 220,

  setView: (view) => set({ view }),
  setActiveSession: (activeSessionId) => set({ activeSessionId }),
  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
  toggleBottom: () => set((s) => ({ bottomCollapsed: !s.bottomCollapsed })),
  toggleRightSidebar: () =>
    set((s) => ({ rightSidebarCollapsed: !s.rightSidebarCollapsed })),
  setBottomTab: (bottomTab) => set({ bottomTab }),
  setRightTab: (rightTab) => set({ rightTab }),
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
  setSidebarWidth: (sidebarWidth) =>
    set({ sidebarWidth: Math.max(160, Math.min(380, sidebarWidth)) }),
}));
