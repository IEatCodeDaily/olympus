import type { NoteIndexEntry, NoteTreeEntry } from "../../types";

export type VaultTabKind = "note" | "graph" | "table";
export type VaultWorkspaceLayout = "single" | "columns" | "rows" | "grid";

export interface WorkspaceTab {
  id: string;
  kind: VaultTabKind;
  title: string;
  path?: string;
}

export interface WorkspacePane {
  id: string;
  tabs: WorkspaceTab[];
  activeTabId: string | null;
}

export interface VaultWorkspaceState {
  layout: VaultWorkspaceLayout;
  panes: WorkspacePane[];
  activePaneId: string;
}

const PANE_COUNTS: Record<VaultWorkspaceLayout, number> = {
  single: 1,
  columns: 2,
  rows: 2,
  grid: 4,
};

export function deriveFrontmatterColumns(documents: Pick<NoteIndexEntry, "frontmatter">[]): string[] {
  return Array.from(new Set(documents.flatMap((document) => Object.keys(document.frontmatter))))
    .filter((column) => !["cid", "title", "path"].includes(column))
    .sort();
}

function pane(index: number): WorkspacePane {
  return { id: `pane-${index + 1}`, tabs: [], activeTabId: null };
}

export function createInitialWorkspace(initial: WorkspaceTab | null): VaultWorkspaceState {
  const first = pane(0);
  if (initial) {
    first.tabs = [initial];
    first.activeTabId = initial.id;
  }
  return { layout: "single", panes: [first], activePaneId: first.id };
}

export function openWorkspaceTab(
  state: VaultWorkspaceState,
  tab: WorkspaceTab,
): VaultWorkspaceState {
  const existingPane = state.panes.find((candidate) =>
    candidate.tabs.some((candidateTab) => candidateTab.id === tab.id),
  );
  const targetPaneId = existingPane?.id ?? state.activePaneId;
  return {
    ...state,
    activePaneId: targetPaneId,
    panes: state.panes.map((candidate) => {
      if (candidate.id !== targetPaneId) return candidate;
      const exists = candidate.tabs.some((candidateTab) => candidateTab.id === tab.id);
      return {
        ...candidate,
        tabs: exists ? candidate.tabs : [...candidate.tabs, tab],
        activeTabId: tab.id,
      };
    }),
  };
}

export function activateWorkspaceTab(
  state: VaultWorkspaceState,
  paneId: string,
  tabId: string,
): VaultWorkspaceState {
  return {
    ...state,
    activePaneId: paneId,
    panes: state.panes.map((candidate) =>
      candidate.id === paneId && candidate.tabs.some((tab) => tab.id === tabId)
        ? { ...candidate, activeTabId: tabId }
        : candidate,
    ),
  };
}

export function closeWorkspaceTab(
  state: VaultWorkspaceState,
  paneId: string,
  tabId: string,
): VaultWorkspaceState {
  return {
    ...state,
    panes: state.panes.map((candidate) => {
      if (candidate.id !== paneId) return candidate;
      const index = candidate.tabs.findIndex((tab) => tab.id === tabId);
      if (index < 0) return candidate;
      const tabs = candidate.tabs.filter((tab) => tab.id !== tabId);
      const activeTabId =
        candidate.activeTabId === tabId
          ? tabs[Math.min(index, tabs.length - 1)]?.id ?? null
          : candidate.activeTabId;
      return { ...candidate, tabs, activeTabId };
    }),
  };
}

export function setWorkspaceLayout(
  state: VaultWorkspaceState,
  layout: VaultWorkspaceLayout,
): VaultWorkspaceState {
  const count = PANE_COUNTS[layout];
  if (count >= state.panes.length) {
    const panes = [...state.panes];
    const activePane = state.panes.find((candidate) => candidate.id === state.activePaneId);
    const activeTab = activePane?.tabs.find((tab) => tab.id === activePane.activeTabId) ?? null;
    while (panes.length < count) {
      const nextPane = pane(panes.length);
      if (activeTab) {
        nextPane.tabs = [activeTab];
        nextPane.activeTabId = activeTab.id;
      }
      panes.push(nextPane);
    }
    return { ...state, layout, panes };
  }

  const panes = state.panes.slice(0, count).map((candidate) => ({
    ...candidate,
    tabs: [...candidate.tabs],
  }));
  for (const removed of state.panes.slice(count)) {
    for (const tab of removed.tabs) {
      if (!panes[0].tabs.some((candidate) => candidate.id === tab.id)) panes[0].tabs.push(tab);
    }
  }
  if (!panes[0].activeTabId) panes[0].activeTabId = panes[0].tabs[0]?.id ?? null;
  const activePaneId = panes.some((candidate) => candidate.id === state.activePaneId)
    ? state.activePaneId
    : panes[0].id;
  return { ...state, layout, panes, activePaneId };
}

export function findFolderIndex(folder: NoteTreeEntry): NoteTreeEntry | null {
  if (folder.kind !== "folder") return null;
  return (
    folder.children.find(
      (entry) => entry.kind === "note" && entry.path === `${folder.path}/index.md`,
    ) ?? null
  );
}

export function noteTab(path: string, title?: string): WorkspaceTab {
  return { id: `note:${path}`, kind: "note", path, title: title ?? path };
}

export const graphTab: WorkspaceTab = { id: "view:graph", kind: "graph", title: "Graph View" };
export const tableTab: WorkspaceTab = { id: "view:table", kind: "table", title: "Table View" };
