import type { NoteIndexEntry, NoteTreeEntry } from "../../types";

export type VaultTabKind = "note" | "graph" | "table";

export interface WorkspaceTab {
  id: string;
  kind: VaultTabKind;
  title: string;
  path?: string;
}

export function deriveFrontmatterColumns(documents: Pick<NoteIndexEntry, "frontmatter">[]): string[] {
  return Array.from(new Set(documents.flatMap((document) => Object.keys(document.frontmatter))))
    .filter((column) => !["cid", "title", "path"].includes(column))
    .sort();
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
