import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { NoteTreeEntry, VaultSummary } from "../../../types";
import { VaultSidebar } from "./VaultSidebar";

const vaults: VaultSummary[] = [{ id: "v1", name: "Vault", noteCount: 1, updatedAt: 1, backend: null }];
const notes: NoteTreeEntry[] = [{ kind: "note", path: "deep/note.md", title: "Note", updatedAt: 1, children: [] }];

describe("VaultSidebar", () => {
  it("makes notes draggable into editor tab groups", () => {
    const data = new Map<string, string>();
    const dataTransfer = {
      effectAllowed: "all",
      setData: (type: string, value: string) => data.set(type, value),
    };
    render(
      <VaultSidebar
        vaults={[]}
        activeVaultId="vault-1"
        notes={[{ kind: "note", path: "docs/new.md", title: "New", updatedAt: 1, children: [] }]}
        activeNotePath={null}
        onSelectVault={vi.fn()}
        onCreateVault={vi.fn()}
        onCreateNote={vi.fn()}
        onOpenNote={vi.fn()}
        onOpenGraph={vi.fn()}
        onOpenTable={vi.fn()}
        onRenameNote={vi.fn()}
        onDeleteNote={vi.fn()}
      />,
    );

    const row = screen.getByRole("treeitem").querySelector(".vault-file-row") as HTMLElement;
    expect(row).toHaveAttribute("draggable", "true");
    fireEvent.dragStart(row, { dataTransfer });
    expect(JSON.parse(data.get("application/x-olympus-vault-note") ?? "{}")).toEqual({
      path: "docs/new.md",
      title: "New",
    });
  });

  it("bounds the file action menu to the viewport instead of inheriting bottom stretch", () => {
    vi.stubGlobal("innerHeight", 600);
    render(
      <VaultSidebar
        vaults={vaults}
        activeVaultId="v1"
        notes={notes}
        activeNotePath={null}
        onSelectVault={vi.fn()}
        onCreateVault={vi.fn()}
        onCreateNote={vi.fn()}
        onOpenNote={vi.fn()}
        onOpenGraph={vi.fn()}
        onOpenTable={vi.fn()}
        onRenameNote={vi.fn()}
        onDeleteNote={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByLabelText("Actions for Note"), { clientX: 48, clientY: 540 });

    expect(screen.getByRole("menu", { name: "File actions" })).toHaveStyle({
      top: "540px",
      bottom: "auto",
      maxHeight: "48px",
      overflowY: "auto",
    });
  });
});
