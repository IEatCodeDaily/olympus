import {
  activateView,
  closeView,
  createWorkbench,
  findGroup,
  listGroups,
  openView,
  removeGroup,
  splitGroup,
  type SplitAxis,
  type ViewRecord,
  type WorkbenchState,
} from "../../workbench/model";

export interface SessionViewPayload {
  sessionId: string;
}

export type SessionWorkspace = WorkbenchState<SessionViewPayload>;
export type SessionView = ViewRecord<SessionViewPayload>;

const ROOT_GROUP_ID = "session-group-root";

function sessionView(sessionId: string, title: string): SessionView {
  return {
    id: `session-view:${sessionId}`,
    resourceKey: `session:${sessionId}`,
    kind: "session",
    title,
    payload: { sessionId },
  };
}

export function createSessionWorkspace(sessionId?: string | null, title = "Untitled"): SessionWorkspace {
  const state = createWorkbench<SessionViewPayload>(ROOT_GROUP_ID);
  return sessionId ? openView(state, ROOT_GROUP_ID, sessionView(sessionId, title)) : state;
}

export function openSession(
  state: SessionWorkspace,
  sessionId: string,
  title: string,
): SessionWorkspace {
  const existing = listGroups(state).find((group) =>
    group.views.some((view) => view.resourceKey === `session:${sessionId}`),
  );
  if (existing) {
    const view = existing.views.find((candidate) => candidate.resourceKey === `session:${sessionId}`)!;
    return activateView(state, existing.id, view.id);
  }

  const target = findGroup(state, state.activeGroupId);
  if (!target) return state;
  let next = state;
  for (const view of target.views) next = closeView(next, target.id, view.id);
  return openView(next, target.id, sessionView(sessionId, title));
}

export function splitSession(
  state: SessionWorkspace,
  sessionId: string,
  title: string,
  axis: SplitAxis,
  splitId: string,
  groupId: string,
): SessionWorkspace {
  const existing = listGroups(state).find((group) =>
    group.views.some((view) => view.resourceKey === `session:${sessionId}`),
  );
  if (existing) {
    return activateView(state, existing.id, existing.views[0].id);
  }
  const split = splitGroup(state, state.activeGroupId, axis, splitId, groupId);
  return openView(split, groupId, sessionView(sessionId, title));
}

export function closeSessionPane(state: SessionWorkspace, groupId: string): SessionWorkspace {
  const groups = listGroups(state);
  if (groups.length === 1) {
    const group = groups[0];
    return group.views.reduce((current, view) => closeView(current, group.id, view.id), state);
  }
  return removeGroup(state, groupId);
}
