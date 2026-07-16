export type SplitAxis = "horizontal" | "vertical";

export interface ViewRecord<TPayload = unknown> {
  id: string;
  resourceKey: string;
  kind: string;
  title: string;
  payload: TPayload;
}

export interface GroupNode<TPayload = unknown> {
  type: "group";
  id: string;
  views: ViewRecord<TPayload>[];
  activeViewId: string | null;
}

export interface SplitNode<TPayload = unknown> {
  type: "split";
  id: string;
  axis: SplitAxis;
  ratio: number;
  first: LayoutNode<TPayload>;
  second: LayoutNode<TPayload>;
}

export type LayoutNode<TPayload = unknown> = GroupNode<TPayload> | SplitNode<TPayload>;

export interface WorkbenchState<TPayload = unknown> {
  version: 1;
  root: LayoutNode<TPayload>;
  activeGroupId: string;
}

const MIN_RATIO = 20;
const MAX_RATIO = 80;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function emptyGroup<TPayload>(id: string): GroupNode<TPayload> {
  return { type: "group", id, views: [], activeViewId: null };
}

export function createWorkbench<TPayload>(groupId: string): WorkbenchState<TPayload> {
  return { version: 1, root: emptyGroup(groupId), activeGroupId: groupId };
}

export function normalizeWorkbench<TPayload>(value: unknown, fallbackGroupId: string): WorkbenchState<TPayload> {
  if (!isRecord(value) || value.version !== 1 || typeof value.activeGroupId !== "string") {
    return createWorkbench(fallbackGroupId);
  }
  const nodeIds = new Set<string>();
  const viewIds = new Set<string>();
  const normalizeNode = (candidate: unknown): LayoutNode<TPayload> | null => {
    if (!isRecord(candidate) || typeof candidate.id !== "string" || nodeIds.has(candidate.id)) return null;
    nodeIds.add(candidate.id);
    if (candidate.type === "group") {
      if (!Array.isArray(candidate.views)) return null;
      const views: ViewRecord<TPayload>[] = [];
      for (const rawView of candidate.views) {
        if (!isRecord(rawView)
          || typeof rawView.id !== "string"
          || typeof rawView.resourceKey !== "string"
          || typeof rawView.kind !== "string"
          || typeof rawView.title !== "string"
          || !("payload" in rawView)
          || viewIds.has(rawView.id)) return null;
        viewIds.add(rawView.id);
        views.push(rawView as unknown as ViewRecord<TPayload>);
      }
      const requested = typeof candidate.activeViewId === "string" ? candidate.activeViewId : null;
      const activeViewId = views.some((view) => view.id === requested) ? requested : views[0]?.id ?? null;
      return { type: "group", id: candidate.id, views, activeViewId };
    }
    if (candidate.type !== "split"
      || (candidate.axis !== "horizontal" && candidate.axis !== "vertical")
      || typeof candidate.ratio !== "number"
      || !Number.isFinite(candidate.ratio)) return null;
    const first = normalizeNode(candidate.first);
    const second = normalizeNode(candidate.second);
    if (!first || !second) return null;
    return {
      type: "split",
      id: candidate.id,
      axis: candidate.axis,
      ratio: Math.max(MIN_RATIO, Math.min(MAX_RATIO, Math.round(candidate.ratio))),
      first,
      second,
    };
  };
  const root = normalizeNode(value.root);
  if (!root) return createWorkbench(fallbackGroupId);
  const groups = collectGroups(root);
  if (groups.length === 0) return createWorkbench(fallbackGroupId);
  const activeGroupId = groups.some((group) => group.id === value.activeGroupId)
    ? value.activeGroupId
    : groups[0].id;
  return { version: 1, root, activeGroupId };
}

export function listGroups<TPayload>(state: WorkbenchState<TPayload>): GroupNode<TPayload>[] {
  const groups: GroupNode<TPayload>[] = [];
  visit(state.root, (node) => {
    if (node.type === "group") groups.push(node);
  });
  return groups;
}

export function findGroup<TPayload>(
  state: WorkbenchState<TPayload>,
  groupId: string,
): GroupNode<TPayload> | null {
  return listGroups(state).find((group) => group.id === groupId) ?? null;
}

export function openView<TPayload>(
  state: WorkbenchState<TPayload>,
  groupId: string,
  view: ViewRecord<TPayload>,
  targetIndex?: number,
): WorkbenchState<TPayload> {
  if (!findGroup(state, groupId) || hasViewId(state, view.id)) return state;
  const root = updateGroup(state.root, groupId, (group) => {
    const index = targetIndex === undefined
      ? group.views.length
      : Math.max(0, Math.min(targetIndex, group.views.length));
    const views = [...group.views];
    views.splice(index, 0, view);
    return { ...group, views, activeViewId: view.id };
  });
  return { ...state, root, activeGroupId: groupId };
}

export function activateView<TPayload>(
  state: WorkbenchState<TPayload>,
  groupId: string,
  viewId: string,
): WorkbenchState<TPayload> {
  const group = findGroup(state, groupId);
  if (!group?.views.some((view) => view.id === viewId)) return state;
  if (group.activeViewId === viewId && state.activeGroupId === groupId) return state;
  if (group.activeViewId === viewId) return { ...state, activeGroupId: groupId };
  return {
    ...state,
    root: updateGroup(state.root, groupId, (candidate) => ({ ...candidate, activeViewId: viewId })),
    activeGroupId: groupId,
  };
}

export function closeView<TPayload>(
  state: WorkbenchState<TPayload>,
  groupId: string,
  viewId: string,
): WorkbenchState<TPayload> {
  const group = findGroup(state, groupId);
  const index = group?.views.findIndex((view) => view.id === viewId) ?? -1;
  if (!group || index < 0) return state;
  const root = updateGroup(state.root, groupId, (candidate) => {
    const views = candidate.views.filter((view) => view.id !== viewId);
    const activeViewId = candidate.activeViewId === viewId
      ? views[Math.min(index, views.length - 1)]?.id ?? null
      : candidate.activeViewId;
    return { ...candidate, views, activeViewId };
  });
  return { ...state, root };
}

export function moveView<TPayload>(
  state: WorkbenchState<TPayload>,
  sourceGroupId: string,
  viewId: string,
  targetGroupId: string,
  targetIndex: number,
): WorkbenchState<TPayload> {
  const source = findGroup(state, sourceGroupId);
  const target = findGroup(state, targetGroupId);
  const sourceIndex = source?.views.findIndex((view) => view.id === viewId) ?? -1;
  const moved = sourceIndex >= 0 ? source?.views[sourceIndex] : undefined;
  if (!source || !target || !moved) return state;

  if (sourceGroupId === targetGroupId) {
    const without = source.views.filter((view) => view.id !== viewId);
    let insertionIndex = targetIndex;
    if (sourceIndex < insertionIndex) insertionIndex -= 1;
    insertionIndex = Math.max(0, Math.min(insertionIndex, without.length));
    const views = [...without];
    views.splice(insertionIndex, 0, moved);
    return {
      ...state,
      root: updateGroup(state.root, sourceGroupId, (group) => ({ ...group, views, activeViewId: viewId })),
      activeGroupId: sourceGroupId,
    };
  }

  let root = updateGroup(state.root, sourceGroupId, (group) => {
    const views = group.views.filter((view) => view.id !== viewId);
    const activeViewId = group.activeViewId === viewId
      ? views[Math.min(sourceIndex, views.length - 1)]?.id ?? null
      : group.activeViewId;
    return { ...group, views, activeViewId };
  });
  root = updateGroup(root, targetGroupId, (group) => {
    const views = [...group.views];
    views.splice(Math.max(0, Math.min(targetIndex, views.length)), 0, moved);
    return { ...group, views, activeViewId: viewId };
  });
  return { ...state, root, activeGroupId: targetGroupId };
}

export function splitGroup<TPayload>(
  state: WorkbenchState<TPayload>,
  groupId: string,
  axis: SplitAxis,
  splitId: string,
  newGroupId: string,
): WorkbenchState<TPayload> {
  if (!findGroup(state, groupId) || hasNodeId(state.root, splitId) || hasNodeId(state.root, newGroupId)) {
    return state;
  }
  const replacement = (group: GroupNode<TPayload>): SplitNode<TPayload> => ({
    type: "split",
    id: splitId,
    axis,
    ratio: 50,
    first: group,
    second: emptyGroup(newGroupId),
  });
  return {
    ...state,
    root: replaceGroup(state.root, groupId, replacement),
    activeGroupId: newGroupId,
  };
}

export function removeGroup<TPayload>(
  state: WorkbenchState<TPayload>,
  groupId: string,
): WorkbenchState<TPayload> {
  if (state.root.type === "group" || !findGroup(state, groupId)) return state;
  const root = removeGroupNode(state.root, groupId);
  if (!root) return state;
  const groups = collectGroups(root);
  const activeGroupId = groups.some((group) => group.id === state.activeGroupId)
    ? state.activeGroupId
    : groups[0].id;
  return { ...state, root, activeGroupId };
}

export function resizeSplit<TPayload>(
  state: WorkbenchState<TPayload>,
  splitId: string,
  ratio: number,
): WorkbenchState<TPayload> {
  if (!hasSplitId(state.root, splitId)) return state;
  return {
    ...state,
    root: updateSplit(state.root, splitId, (split) => ({
      ...split,
      ratio: Math.max(MIN_RATIO, Math.min(MAX_RATIO, Math.round(ratio))),
    })),
  };
}

function visit<TPayload>(node: LayoutNode<TPayload>, callback: (node: LayoutNode<TPayload>) => void): void {
  callback(node);
  if (node.type === "split") {
    visit(node.first, callback);
    visit(node.second, callback);
  }
}

function collectGroups<TPayload>(root: LayoutNode<TPayload>): GroupNode<TPayload>[] {
  const groups: GroupNode<TPayload>[] = [];
  visit(root, (node) => { if (node.type === "group") groups.push(node); });
  return groups;
}

function hasViewId<TPayload>(state: WorkbenchState<TPayload>, viewId: string): boolean {
  return listGroups(state).some((group) => group.views.some((view) => view.id === viewId));
}

function hasNodeId<TPayload>(root: LayoutNode<TPayload>, nodeId: string): boolean {
  let found = false;
  visit(root, (node) => { if (node.id === nodeId) found = true; });
  return found;
}

function hasSplitId<TPayload>(root: LayoutNode<TPayload>, splitId: string): boolean {
  let found = false;
  visit(root, (node) => { if (node.type === "split" && node.id === splitId) found = true; });
  return found;
}

function updateGroup<TPayload>(
  node: LayoutNode<TPayload>,
  groupId: string,
  update: (group: GroupNode<TPayload>) => GroupNode<TPayload>,
): LayoutNode<TPayload> {
  if (node.type === "group") return node.id === groupId ? update(node) : node;
  const first = updateGroup(node.first, groupId, update);
  const second = updateGroup(node.second, groupId, update);
  return first === node.first && second === node.second ? node : { ...node, first, second };
}

function replaceGroup<TPayload>(
  node: LayoutNode<TPayload>,
  groupId: string,
  replacement: (group: GroupNode<TPayload>) => LayoutNode<TPayload>,
): LayoutNode<TPayload> {
  if (node.type === "group") return node.id === groupId ? replacement(node) : node;
  const first = replaceGroup(node.first, groupId, replacement);
  const second = replaceGroup(node.second, groupId, replacement);
  return first === node.first && second === node.second ? node : { ...node, first, second };
}

function removeGroupNode<TPayload>(
  node: LayoutNode<TPayload>,
  groupId: string,
): LayoutNode<TPayload> | null {
  if (node.type === "group") return node.id === groupId ? null : node;
  const first = removeGroupNode(node.first, groupId);
  const second = removeGroupNode(node.second, groupId);
  if (!first) return second;
  if (!second) return first;
  return first === node.first && second === node.second ? node : { ...node, first, second };
}

function updateSplit<TPayload>(
  node: LayoutNode<TPayload>,
  splitId: string,
  update: (split: SplitNode<TPayload>) => SplitNode<TPayload>,
): LayoutNode<TPayload> {
  if (node.type === "group") return node;
  if (node.id === splitId) return update(node);
  const first = updateSplit(node.first, splitId, update);
  const second = updateSplit(node.second, splitId, update);
  return first === node.first && second === node.second ? node : { ...node, first, second };
}
