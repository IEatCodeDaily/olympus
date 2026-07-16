import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { VaultSidebar } from "./VaultSidebar";

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
});