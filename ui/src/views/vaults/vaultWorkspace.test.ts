import { describe, expect, it } from "vitest";
import type { NoteTreeEntry } from "../../types";
import {
  createInitialWorkspace,
  deriveFrontmatterColumns,
  findFolderIndex,
  openWorkspaceTab,
  setWorkspaceLayout,
  type WorkspaceTab,
} from "./vaultWorkspace";

const note = (path: string): WorkspaceTab => ({
  id: `note:${path}`,
  kind: "note",
  title: path.split("/").pop() ?? path,
  path,
});

describe("vault workspace", () => {
  it("opens targets as tabs and reuses an existing target", () => {
    let workspace = createInitialWorkspace(note("index.md"));
    workspace = openWorkspaceTab(workspace, note("docs/design.md"));
    workspace = openWorkspaceTab(workspace, note("index.md"));

    expect(workspace.panes[0].tabs.map((tab) => tab.id)).toEqual([
      "note:index.md",
      "note:docs/design.md",
    ]);
    expect(workspace.panes[0].activeTabId).toBe("note:index.md");
  });

  it("keeps tabs in surviving panes when reducing the layout", () => {
    let workspace = createInitialWorkspace(note("index.md"));
    workspace = setWorkspaceLayout(workspace, "columns");
    workspace = { ...workspace, activePaneId: workspace.panes[1].id };
    workspace = openWorkspaceTab(workspace, note("second.md"));
    workspace = setWorkspaceLayout(workspace, "single");

    expect(workspace.panes).toHaveLength(1);
    expect(workspace.panes[0].tabs.map((tab) => tab.id)).toEqual([
      "note:index.md",
      "note:second.md",
    ]);
  });

  it("creates four panes for the grid layout", () => {
    const workspace = setWorkspaceLayout(createInitialWorkspace(null), "grid");
    expect(workspace.panes).toHaveLength(4);
  });

  it("derives unique frontmatter columns without duplicating built-in columns", () => {
    expect(deriveFrontmatterColumns([
      { frontmatter: { title: "One", status: "draft", cid: "one" } },
      { frontmatter: { status: "done", owner: "rpw", path: "ignored" } },
    ])).toEqual(["owner", "status"]);
  });
});

describe("vault file tree", () => {
  it("opens a folder's direct index note", () => {
    const folder: NoteTreeEntry = {
      kind: "folder",
      path: "docs",
      title: "docs",
      updatedAt: 1,
      children: [
        { kind: "note", path: "docs/index.md", title: "Docs", updatedAt: 1, children: [] },
        { kind: "note", path: "docs/design.md", title: "Design", updatedAt: 1, children: [] },
      ],
    };

    expect(findFolderIndex(folder)?.path).toBe("docs/index.md");
  });

  it("does not treat a nested index as the folder's own note", () => {
    const folder: NoteTreeEntry = {
      kind: "folder",
      path: "docs",
      title: "docs",
      updatedAt: 1,
      children: [
        {
          kind: "folder",
          path: "docs/nested",
          title: "nested",
          updatedAt: 1,
          children: [
            { kind: "note", path: "docs/nested/index.md", title: "Nested", updatedAt: 1, children: [] },
          ],
        },
      ],
    };

    expect(findFolderIndex(folder)).toBeNull();
  });
});
