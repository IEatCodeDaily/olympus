import { create } from "zustand";

// ── Operator Cockpit store (ADR 0021) ────────────────────────────────────
//
// The cockpit is a SINGLE floating, tabbed, operator-only workspace that
// persists across every view (it is mounted once at the AppShell root, OUTSIDE
// the surface switch) and per user (geometry + tab manifest persisted to
// localStorage in Phase 0.C; moves to Hall in Phase 3.A).
//
// Layer A (this file, frontend-only): tab/window state lives here, at the top
// level, so navigating surfaces never unmounts it and toggling visibility never
// disposes a tab. Layer B (Phase 3) swaps the mock PTY for an Envoy-owned PTY
// over a dedicated operator WebSocket — the store shape is designed to absorb a
// real `terminalId`/`attemptEpoch` without changing the UI contract.
//
// Tabs are KIND-POLYMORPHIC: `kind` selects a renderer from the cockpit tab
// registry (cockpit/tabs.tsx). Built-in kinds: "terminal", "browser",
// "editor". Plugins register additional kinds at module load; a persisted tab
// whose kind has no registered renderer renders a fallback pane (never
// crashes, never silently dropped).
//
// HARD BOUNDARY: this is operator-only. No agent ever drives it.

export interface CockpitTab {
  id: string;
  /** Renderer key — see cockpit/tabs.tsx registry. */
  kind: string;
  title: string;
  /** Node this tab is pinned to at open (ADR 0021 §7 — never follows the
   *  active session). `"hall"` = the Hall host (terminal default). Kinds that
   *  are node-independent (browser) leave it null. */
  target: { nodeId: string } | null;
  /** Kind-specific persisted state (browser: { url }; editor: { path } …).
   *  Serialized to localStorage as-is — keep it small and JSON-safe. */
  state?: Record<string, unknown>;
}

interface Geometry {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface CockpitState {
  /** Window visible? Toggled from the top-right button. Hiding NEVER disposes
   *  tabs or their live sockets — it only detaches from layout. */
  open: boolean;
  /** Floating window geometry (persisted per user). */
  geometry: Geometry;
  /** Open tabs, in order. */
  tabs: CockpitTab[];
  /** Active tab id. */
  activeTabId: string | null;

  toggle: () => void;
  setOpen: (open: boolean) => void;
  setGeometry: (g: Partial<Geometry>) => void;
  addTab: (opts: {
    kind: string;
    node?: string;
    label?: string;
    state?: Record<string, unknown>;
  }) => void;
  /** Patch a tab's title/state in place (e.g. browser URL change). */
  updateTab: (id: string, patch: Partial<Pick<CockpitTab, "title" | "state">>) => void;
  closeTab: (id: string) => void;
  setActiveTab: (id: string) => void;
}

const LS_KEY = "olympus-cockpit-v1";

interface Persisted {
  geometry: Geometry;
  tabs: CockpitTab[];
  activeTabId: string | null;
}

const DEFAULT_GEOMETRY: Geometry = { x: 120, y: 96, w: 820, h: 520 };

function load(): Persisted {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw) {
      const p = JSON.parse(raw) as Partial<Persisted>;
      const tabs = (Array.isArray(p.tabs) ? p.tabs : []).map((t) => ({
        ...t,
        // Migration: tabs persisted before kinds existed are terminals.
        kind: (t as Partial<CockpitTab>).kind ?? "terminal",
      }));
      return {
        geometry: { ...DEFAULT_GEOMETRY, ...(p.geometry ?? {}) },
        tabs,
        activeTabId: p.activeTabId ?? null,
      };
    }
  } catch {
    // ignore malformed persistence
  }
  return { geometry: DEFAULT_GEOMETRY, tabs: [], activeTabId: null };
}

function persist(s: CockpitState) {
  try {
    const data: Persisted = {
      geometry: s.geometry,
      tabs: s.tabs,
      activeTabId: s.activeTabId,
    };
    localStorage.setItem(LS_KEY, JSON.stringify(data));
  } catch {
    // ignore quota / disabled storage
  }
}

let seq = 0;
function newId(): string {
  seq += 1;
  return `tab-${Date.now().toString(36)}-${seq}`;
}

function titleFor(kind: string, node: string | undefined, label: string, existing: CockpitTab[]): string {
  if (kind === "terminal") {
    const base = !node || node === "hall" ? "Hall" : label || node;
    const n = existing.filter((t) => t.kind === "terminal" && t.target?.nodeId === node).length + 1;
    return `${base} ${n}`;
  }
  const base = label || kind;
  const n = existing.filter((t) => t.kind === kind).length + 1;
  return n > 1 ? `${base} ${n}` : base;
}

const initial = load();

export const useCockpit = create<CockpitState>((set, get) => ({
  open: false,
  geometry: initial.geometry,
  tabs: initial.tabs,
  activeTabId: initial.activeTabId,

  toggle: () => {
    set((s) => ({ open: !s.open }));
  },
  setOpen: (open) => set({ open }),

  setGeometry: (g) => {
    set((s) => ({ geometry: { ...s.geometry, ...g } }));
    persist(get());
  },

  addTab: ({ kind, node, label, state }) => {
    set((s) => {
      const tab: CockpitTab = {
        id: newId(),
        kind,
        title: titleFor(kind, node, label ?? "", s.tabs),
        target: node ? { nodeId: node } : null,
        state,
      };
      return { tabs: [...s.tabs, tab], activeTabId: tab.id, open: true };
    });
    persist(get());
  },

  updateTab: (id, patch) => {
    set((s) => ({
      tabs: s.tabs.map((t) => (t.id === id ? { ...t, ...patch } : t)),
    }));
    persist(get());
  },

  closeTab: (id) => {
    set((s) => {
      const tabs = s.tabs.filter((t) => t.id !== id);
      const activeTabId =
        s.activeTabId === id ? (tabs[tabs.length - 1]?.id ?? null) : s.activeTabId;
      return { tabs, activeTabId };
    });
    persist(get());
  },

  setActiveTab: (id) => {
    set({ activeTabId: id });
    persist(get());
  },
}));
