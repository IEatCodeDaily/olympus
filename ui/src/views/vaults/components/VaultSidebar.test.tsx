import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { VaultSidebar } from "./VaultSidebar";
import type { NoteTreeEntry, VaultSummary } from "../../../types";

const vaults: VaultSummary[] = [{ id: "vault-1", name: "Vault", noteCount: 1, updatedAt: 1, backend: null }];
const notes: NoteTreeEntry[] = [
  { kind: "note", path: "one.md", title: "One", updatedAt: 1, children: [] },
];

describe("VaultSidebar", () => {
  it("marks the active note as open and focused", () => {
    const { container } = render(
      <VaultSidebar
        vaults={vaults}
        activeVaultId="vault-1"
        notes={notes}
        activeNotePath="one.md"
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

    const row = container.querySelector(".vault-file-row");
    expect(row).toHaveAttribute("data-open", "true");
    expect(row).toHaveAttribute("data-focused", "true");
    expect(row).toHaveClass("focused");
  });

  it("writes a dockview vault-note drag payload", () => {
    render(
      <VaultSidebar
        vaults={vaults}
        activeVaultId="vault-1"
        notes={notes}
        activeNotePath="one.md"
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
    const data = new Map<string, string>();

    fireEvent.dragStart(screen.getByText("one.md").closest(".vault-file-row") as HTMLElement, {
      dataTransfer: {
        effectAllowed: "none",
        setData: (type: string, value: string) => data.set(type, value),
      },
    });

    expect(JSON.parse(data.get("application/x-olympus-vault-note") ?? "{}")).toMatchObject({
      type: "vault-note",
      path: "one.md",
      title: "One",
    });
  });
});
