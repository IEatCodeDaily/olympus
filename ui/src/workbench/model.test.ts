import { describe, expect, it } from "vitest";
import {
  activateView,
  closeView,
  createWorkbench,
  findGroup,
  listGroups,
  moveView,
  normalizeWorkbench,
  openView,
  removeGroup,
  resizeSplit,
  splitGroup,
  type ViewRecord,
} from "./model";

interface TestPayload { value: string }
type TestView = ViewRecord<TestPayload>;

const view = (id: string, resourceKey = id): TestView => ({
  id,
  resourceKey,
  kind: "test",
  title: id,
  payload: { value: id },
});

describe("workbench model", () => {
  it("creates one group and opens views without inventing surface reuse policy", () => {
    let state = createWorkbench<TestPayload>("group-a");
    state = openView(state, "group-a", view("view-a"));
    state = openView(state, "group-a", view("view-b", "view-a"));

    expect(findGroup(state, "group-a")?.views.map((item) => item.id)).toEqual(["view-a", "view-b"]);
    expect(findGroup(state, "group-a")?.activeViewId).toBe("view-b");
  });

  it("nests right and down splits without cloning the active view", () => {
    let state = openView(createWorkbench<TestPayload>("group-a"), "group-a", view("view-a"));
    state = splitGroup(state, "group-a", "horizontal", "split-a", "group-b");
    state = splitGroup(state, "group-b", "vertical", "split-b", "group-c");

    expect(state.root).toMatchObject({
      type: "split",
      id: "split-a",
      axis: "horizontal",
      first: { type: "group", id: "group-a" },
      second: {
        type: "split",
        id: "split-b",
        axis: "vertical",
        first: { type: "group", id: "group-b", views: [] },
        second: { type: "group", id: "group-c", views: [] },
      },
    });
    expect(listGroups(state).flatMap((group) => group.views)).toHaveLength(1);
  });

  it("promotes a sibling when a group is removed", () => {
    let state = createWorkbench<TestPayload>("group-a");
    state = splitGroup(state, "group-a", "horizontal", "split-a", "group-b");
    state = openView(state, "group-b", view("view-b"));
    state = removeGroup(state, "group-a");

    expect(state.root).toMatchObject({ type: "group", id: "group-b" });
    expect(state.activeGroupId).toBe("group-b");
  });

  it("refuses to remove the only group", () => {
    const state = createWorkbench<TestPayload>("group-a");
    expect(removeGroup(state, "group-a")).toBe(state);
  });

  it("clamps split ratios and ignores unknown split IDs", () => {
    let state = splitGroup(createWorkbench<TestPayload>("group-a"), "group-a", "horizontal", "split-a", "group-b");
    state = resizeSplit(state, "split-a", 5);
    expect(state.root).toMatchObject({ ratio: 20 });
    state = resizeSplit(state, "split-a", 95);
    expect(state.root).toMatchObject({ ratio: 80 });
    expect(resizeSplit(state, "missing", 50)).toBe(state);
  });

  it("moves a view between groups and selects adjacent source content", () => {
    let state = createWorkbench<TestPayload>("group-a");
    state = openView(state, "group-a", view("view-a"));
    state = openView(state, "group-a", view("view-b"));
    state = splitGroup(state, "group-a", "horizontal", "split-a", "group-b");
    state = moveView(state, "group-a", "view-b", "group-b", 0);

    expect(findGroup(state, "group-a")?.activeViewId).toBe("view-a");
    expect(findGroup(state, "group-b")?.views.map((item) => item.id)).toEqual(["view-b"]);
    expect(state.activeGroupId).toBe("group-b");
  });

  it("closes the active view and selects its nearest successor", () => {
    let state = createWorkbench<TestPayload>("group-a");
    state = openView(state, "group-a", view("a"));
    state = openView(state, "group-a", view("b"));
    state = openView(state, "group-a", view("c"));
    state = activateView(state, "group-a", "b");
    state = closeView(state, "group-a", "b");

    expect(findGroup(state, "group-a")?.views.map((item) => item.id)).toEqual(["a", "c"]);
    expect(findGroup(state, "group-a")?.activeViewId).toBe("c");
  });

  it("returns the same state for invalid group and view operations", () => {
    const state = createWorkbench<TestPayload>("group-a");
    expect(openView(state, "missing", view("a"))).toBe(state);
    expect(activateView(state, "group-a", "missing")).toBe(state);
    expect(closeView(state, "group-a", "missing")).toBe(state);
    expect(splitGroup(state, "missing", "horizontal", "split-a", "group-b")).toBe(state);
    expect(moveView(state, "group-a", "missing", "group-a", 0)).toBe(state);
  });

  it("normalizes bounded ratios and invalid active selections", () => {
    const normalized = normalizeWorkbench<TestPayload>({
      version: 1,
      activeGroupId: "missing",
      root: {
        type: "split", id: "split-a", axis: "horizontal", ratio: 99,
        first: { type: "group", id: "group-a", views: [view("a")], activeViewId: "missing" },
        second: { type: "group", id: "group-b", views: [], activeViewId: "missing" },
      },
    }, "fallback");
    expect(normalized.root).toMatchObject({ ratio: 80, first: { activeViewId: "a" }, second: { activeViewId: null } });
    expect(normalized.activeGroupId).toBe("group-a");
  });

  it("fails closed on malformed or duplicate identity trees", () => {
    const malformed = normalizeWorkbench<TestPayload>({
      version: 1,
      activeGroupId: "group-a",
      root: {
        type: "split", id: "same", axis: "horizontal", ratio: 50,
        first: { type: "group", id: "same", views: [], activeViewId: null },
        second: { type: "group", id: "group-b", views: [], activeViewId: null },
      },
    }, "fallback");
    expect(malformed).toEqual(createWorkbench("fallback"));
    expect(normalizeWorkbench(null, "fallback")).toEqual(createWorkbench("fallback"));
  });
});
