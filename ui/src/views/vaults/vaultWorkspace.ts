import type { NoteIndexEntry, NoteTreeEntry } from "../../types";
import {
  activateView,
  closeView,
  createWorkbench,
  findGroup,
  listGroups,
  moveView,
  openView,
  removeGroup,
  resizeSplit,
  splitGroup,
  type SplitAxis,
  type ViewRecord,
  type WorkbenchState,
} from "../../workbench/model";

export type VaultTabKind = "note" | "graph" | "table";

export interface VaultTabPayload {
  path?: string;
}

export interface WorkspaceTab extends ViewRecord<VaultTabPayload> {
  kind: VaultTabKind;
  path?: string;
}

export type VaultWorkspaceState = WorkbenchState<VaultTabPayload>;

let viewSequence = 0;
function nextViewId(kind: VaultTabKind): string {
  viewSequence += 1;
  return `vault-view-${kind}-${viewSequence}`;
}

export function deriveFrontmatterColumns(documents: Pick<NoteIndexEntry, "frontmatter">[]): string[] {
  return Array.from(new Set(documents.flatMap((document) => Object.keys(document.frontmatter))))
    .filter((column) => !["cid", "title", "path"].includes(column))
    .sort();
}

export function createInitialWorkspace(initial: WorkspaceTab | null): VaultWorkspaceState {
  const state = createWorkbench<VaultTabPayload>("vault-group-root");
  return initial ? openView(state, state.activeGroupId, initial) : state;
}

export function workspaceGroups(state: VaultWorkspaceState) {
  return listGroups(state);
}

export function openWorkspaceTab(
  state: VaultWorkspaceState,
  tab: WorkspaceTab,
): VaultWorkspaceState {
  const existing = listGroups(state).find((group) =>
    group.views.some((candidate) => candidate.resourceKey === tab.resourceKey),
  );
  if (existing) {
    const view = existing.views.find((candidate) => candidate.resourceKey === tab.resourceKey)!;
    return activateView(state, existing.id, view.id);
  }
  return openView(state, state.activeGroupId, tab);
}

export function activateWorkspaceTab(
  state: VaultWorkspaceState,
  groupId: string,
  tabId: string,
): VaultWorkspaceState {
  return activateView(state, groupId, tabId);
}

export function activateWorkspaceGroup(
  state: VaultWorkspaceState,
  groupId: string,
): VaultWorkspaceState {
  return findGroup(state, groupId) ? { ...state, activeGroupId: groupId } : state;
}

export function closeWorkspaceTab(
  state: VaultWorkspaceState,
  groupId: string,
  tabId: string,
): VaultWorkspaceState {
  return closeView(state, groupId, tabId);
}

export function moveWorkspaceTab(
  state: VaultWorkspaceState,
  sourceGroupId: string,
  tabId: string,
  targetGroupId: string,
  targetIndex: number,
): VaultWorkspaceState {
  return moveView(state, sourceGroupId, tabId, targetGroupId, targetIndex);
}

export function openWorkspaceTabInPane(
  state: VaultWorkspaceState,
  groupId: string,
  tab: WorkspaceTab,
  targetIndex: number,
): VaultWorkspaceState {
  const existing = listGroups(state).find((group) =>
    group.views.some((candidate) => candidate.resourceKey === tab.resourceKey),
  );
  if (existing) {
    const view = existing.views.find((candidate) => candidate.resourceKey === tab.resourceKey)!;
    return activateView(state, existing.id, view.id);
  }
  return openView(state, groupId, tab, targetIndex);
}

export function closeOtherWorkspaceTabs(
  state: VaultWorkspaceState,
  groupId: string,
  tabId: string,
): VaultWorkspaceState {
  const group = findGroup(state, groupId);
  if (!group?.views.some((view) => view.id === tabId)) return state;
  return group.views
    .filter((view) => view.id !== tabId)
    .reduce((current, view) => closeView(current, groupId, view.id), state);
}

export function closeAllWorkspaceTabs(
  state: VaultWorkspaceState,
  groupId: string,
): VaultWorkspaceState {
  const group = findGroup(state, groupId);
  if (!group) return state;
  return group.views.reduce((current, view) => closeView(current, groupId, view.id), state);
}

export function closeWorkspaceTabsToRight(
  state: VaultWorkspaceState,
  groupId: string,
  tabId: string,
): VaultWorkspaceState {
  const group = findGroup(state, groupId);
  const index = group?.views.findIndex((view) => view.id === tabId) ?? -1;
  if (!group || index < 0) return state;
  return group.views.slice(index + 1)
    .reduce((current, view) => closeView(current, groupId, view.id), state);
}

export function splitWorkspaceGroup(
  state: VaultWorkspaceState,
  groupId: string,
  axis: SplitAxis,
  splitId: string,
  newGroupId: string,
): VaultWorkspaceState {
  return splitGroup(state, groupId, axis, splitId, newGroupId);
}

export function closeWorkspaceGroup(state: VaultWorkspaceState, groupId: string): VaultWorkspaceState {
  return removeGroup(state, groupId);
}

export function resizeWorkspaceSplit(state: VaultWorkspaceState, splitId: string, ratio: number): VaultWorkspaceState {
  return resizeSplit(state, splitId, ratio);
}

export function closeWorkspaceResource(state: VaultWorkspaceState, resourceKey: string): VaultWorkspaceState {
  return listGroups(state).reduce((current, group) => {
    const matches = group.views.filter((view) => view.resourceKey === resourceKey);
    return matches.reduce((next, view) => closeView(next, group.id, view.id), current);
  }, state);
}

export function findFolderIndex(folder: NoteTreeEntry): NoteTreeEntry | null {
  if (folder.kind !== "folder") return null;
  return folder.children.find(
    (entry) => entry.kind === "note" && entry.path === `${folder.path}/index.md`,
  ) ?? null;
}

export function noteTab(vaultId: string, path: string, title?: string): WorkspaceTab {
  return {
    id: nextViewId("note"),
    resourceKey: `vault:${vaultId}:note:${path}`,
    kind: "note",
    path,
    title: title ?? path,
    payload: { path },
  };
}

export function graphTab(vaultId: string): WorkspaceTab {
  return {
    id: nextViewId("graph"),
    resourceKey: `vault:${vaultId}:graph`,
    kind: "graph",
    title: "Graph View",
    payload: {},
  };
}

export function tableTab(vaultId: string): WorkspaceTab {
  return {
    id: nextViewId("table"),
    resourceKey: `vault:${vaultId}:table`,
    kind: "table",
    title: "Table View",
    payload: {},
  };
}
