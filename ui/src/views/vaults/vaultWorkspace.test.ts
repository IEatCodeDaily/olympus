import { describe, expect, it } from "vitest";
import type { NoteTreeEntry } from "../../types";
import {
  deriveFrontmatterColumns,
  findFolderIndex,
} from "./vaultWorkspace";

describe("vault workspace", () => {
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
