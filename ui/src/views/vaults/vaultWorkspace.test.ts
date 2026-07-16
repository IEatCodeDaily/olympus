import { describe, expect, it } from "vitest";
import type { NoteTreeEntry } from "../../types";
import { findGroup, listGroups } from "../../workbench/model";
import {
  activateWorkspaceTab,
  closeOtherWorkspaceTabs,
  closeWorkspaceTabsToRight,
  createInitialWorkspace,
  deriveFrontmatterColumns,
  findFolderIndex,
  moveWorkspaceTab,
  noteTab,
  openWorkspaceTab,
  splitWorkspaceGroup,
} from "./vaultWorkspace";

const note = (path: string) => noteTab("vault-a", path, path.split("/").pop());

describe("vault workspace", () => {
  it("opens targets as tabs and reuses an existing vault-scoped resource", () => {
    let workspace = createInitialWorkspace(note("index.md"));
    workspace = openWorkspaceTab(workspace, note("docs/design.md"));
    workspace = openWorkspaceTab(workspace, note("index.md"));

    const group = findGroup(workspace, "vault-group-root")!;
    expect(group.views.map((tab) => tab.resourceKey)).toEqual([
      "vault:vault-a:note:index.md",
      "vault:vault-a:note:docs/design.md",
    ]);
    expect(group.activeViewId).toBe(group.views[0].id);
  });

  it("creates empty nested right and down groups instead of cloning writable notes", () => {
    let workspace = createInitialWorkspace(note("index.md"));
    workspace = splitWorkspaceGroup(workspace, "vault-group-root", "horizontal", "split-a", "group-b");
    workspace = splitWorkspaceGroup(workspace, "group-b", "vertical", "split-b", "group-c");

    expect(workspace.root).toMatchObject({
      type: "split",
      axis: "horizontal",
      first: { type: "group", views: [{ resourceKey: "vault:vault-a:note:index.md" }] },
      second: {
        type: "split",
        axis: "vertical",
        first: { type: "group", views: [] },
        second: { type: "group", views: [] },
      },
    });
  });

  it("activates an already open note across groups rather than duplicating it", () => {
    let workspace = createInitialWorkspace(note("one.md"));
    workspace = splitWorkspaceGroup(workspace, "vault-group-root", "horizontal", "split-a", "group-b");
    workspace = openWorkspaceTab(workspace, note("one.md"));

    expect(listGroups(workspace).flatMap((group) => group.views)).toHaveLength(1);
    expect(workspace.activeGroupId).toBe("vault-group-root");
  });

  it("reorders and moves tabs between editor groups", () => {
    let workspace = createInitialWorkspace(note("one.md"));
    workspace = openWorkspaceTab(workspace, note("two.md"));
    workspace = openWorkspaceTab(workspace, note("three.md"));
    const root = findGroup(workspace, "vault-group-root")!;
    const three = root.views.find((tab) => tab.payload.path === "three.md")!;
    workspace = moveWorkspaceTab(workspace, root.id, three.id, root.id, 1);
    expect(findGroup(workspace, root.id)?.views.map((tab) => tab.payload.path)).toEqual(["one.md", "three.md", "two.md"]);

    workspace = splitWorkspaceGroup(workspace, root.id, "horizontal", "split-a", "group-b");
    const two = findGroup(workspace, root.id)!.views.find((tab) => tab.payload.path === "two.md")!;
    workspace = moveWorkspaceTab(workspace, root.id, two.id, "group-b", 0);
    expect(findGroup(workspace, "group-b")?.views.map((tab) => tab.payload.path)).toEqual(["two.md"]);
  });

  it("supports close others and close to the right", () => {
    let workspace = createInitialWorkspace(note("one.md"));
    workspace = openWorkspaceTab(workspace, note("two.md"));
    workspace = openWorkspaceTab(workspace, note("three.md"));
    const group = findGroup(workspace, "vault-group-root")!;
    const two = group.views.find((tab) => tab.payload.path === "two.md")!;

    expect(findGroup(closeWorkspaceTabsToRight(workspace, group.id, two.id), group.id)?.views.map((tab) => tab.payload.path))
      .toEqual(["one.md", "two.md"]);
    expect(findGroup(closeOtherWorkspaceTabs(workspace, group.id, two.id), group.id)?.views.map((tab) => tab.payload.path))
      .toEqual(["two.md"]);
  });

  it("activates a concrete tab instance", () => {
    let workspace = createInitialWorkspace(note("one.md"));
    workspace = openWorkspaceTab(workspace, note("two.md"));
    const group = findGroup(workspace, "vault-group-root")!;
    workspace = activateWorkspaceTab(workspace, group.id, group.views[0].id);
    expect(findGroup(workspace, group.id)?.activeViewId).toBe(group.views[0].id);
  });

  it("derives unique frontmatter columns without built-ins", () => {
    expect(deriveFrontmatterColumns([
      { frontmatter: { title: "One", status: "draft", cid: "one" } },
      { frontmatter: { status: "done", owner: "rpw", path: "ignored" } },
    ])).toEqual(["owner", "status"]);
  });
});

describe("vault file tree", () => {
  it("opens only a folder's direct index note", () => {
    const folder: NoteTreeEntry = {
      kind: "folder", path: "docs", title: "docs", updatedAt: 1,
      children: [
        { kind: "note", path: "docs/index.md", title: "Docs", updatedAt: 1, children: [] },
        { kind: "note", path: "docs/design.md", title: "Design", updatedAt: 1, children: [] },
      ],
    };
    expect(findFolderIndex(folder)?.path).toBe("docs/index.md");

    const nestedOnly: NoteTreeEntry = {
      kind: "folder", path: "docs", title: "docs", updatedAt: 1,
      children: [{
        kind: "folder", path: "docs/nested", title: "nested", updatedAt: 1,
        children: [{ kind: "note", path: "docs/nested/index.md", title: "Nested", updatedAt: 1, children: [] }],
      }],
    };
    expect(findFolderIndex(nestedOnly)).toBeNull();
  });
});
