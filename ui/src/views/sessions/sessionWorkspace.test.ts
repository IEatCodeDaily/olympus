import { describe, expect, it } from "vitest";
import { findGroup, listGroups } from "../../workbench/model";
import {
  closeSessionPane,
  createSessionWorkspace,
  openSession,
  splitSession,
} from "./sessionWorkspace";

describe("session workspace policy", () => {
  it("uses one session per pane and replaces the active pane on ordinary open", () => {
    let state = createSessionWorkspace("session-a", "Alpha");
    state = openSession(state, "session-b", "Beta");

    expect(listGroups(state)).toHaveLength(1);
    expect(findGroup(state, state.activeGroupId)?.views[0]).toMatchObject({
      resourceKey: "session:session-b",
      title: "Beta",
    });
  });

  it("activates an already visible session instead of duplicating it", () => {
    let state = createSessionWorkspace("session-a", "Alpha");
    state = splitSession(state, "session-b", "Beta", "horizontal", "split-a", "group-b");
    const before = state.root;
    state = openSession(state, "session-a", "Alpha");

    expect(state.root).toBe(before);
    expect(state.activeGroupId).toBe("session-group-root");
    expect(listGroups(state).flatMap((group) => group.views)).toHaveLength(2);
  });

  it("opens a different session to the right or below without horizontal tab arrays", () => {
    let state = createSessionWorkspace("session-a", "Alpha");
    state = splitSession(state, "session-b", "Beta", "vertical", "split-a", "group-b");

    expect(state.root).toMatchObject({
      type: "split",
      axis: "vertical",
      first: { type: "group", views: [{ payload: { sessionId: "session-a" } }] },
      second: { type: "group", views: [{ payload: { sessionId: "session-b" } }] },
    });
    expect(listGroups(state).every((group) => group.views.length <= 1)).toBe(true);
  });

  it("closing a pane promotes its sibling without changing the Hall session", () => {
    let state = createSessionWorkspace("session-a", "Alpha");
    state = splitSession(state, "session-b", "Beta", "horizontal", "split-a", "group-b");
    state = closeSessionPane(state, "group-b");

    expect(state.root).toMatchObject({ type: "group", id: "session-group-root" });
    expect(findGroup(state, state.activeGroupId)?.views[0].payload.sessionId).toBe("session-a");
  });
});
