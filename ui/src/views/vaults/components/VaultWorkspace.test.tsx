import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { VaultWorkspace } from "./VaultWorkspace";
import { createInitialWorkspace, noteTab, setWorkspaceLayout } from "../vaultWorkspace";

vi.mock("../pages/GraphPage", () => ({ GraphPage: () => <div>Graph</div> }));
vi.mock("../pages/NotePage", () => ({
  NotePage: ({ onDirtyChange }: { onDirtyChange: (dirty: boolean) => void }) => (
    <button type="button" onClick={() => onDirtyChange(true)}>Dirty note</button>
  ),
}));
vi.mock("../pages/VaultTablePage", () => ({ VaultTablePage: () => <div>Table</div> }));

describe("VaultWorkspace", () => {
  it("puts layout controls on the active pane tab row instead of a second toolbar", () => {
    const state = setWorkspaceLayout(createInitialWorkspace(noteTab("one.md", "One")), "columns");
    const { container } = render(
      <VaultWorkspace vaultId="vault-1" state={state} onActivatePane={vi.fn()} onActivateTab={vi.fn()} onCloseTab={vi.fn()} onOpenNote={vi.fn()} onLayout={vi.fn()} />,
    );

    expect(container.querySelector(".vault-workspace-toolbar")).not.toBeInTheDocument();
    const header = container.querySelector(".vault-pane.active .vault-pane-header");
    expect(header).not.toBeNull();
    expect(within(header as HTMLElement).getByRole("tablist")).toBeInTheDocument();
    expect(within(header as HTMLElement).getByRole("group", { name: "Workspace layout" })).toBeInTheDocument();
    expect(screen.getAllByRole("group", { name: "Workspace layout" })).toHaveLength(1);
  });

  it("lets the operator resize split editor groups", () => {
    const state = setWorkspaceLayout(createInitialWorkspace(noteTab("one.md", "One")), "columns");
    const { container } = render(
      <VaultWorkspace vaultId="vault-1" state={state} onActivatePane={vi.fn()} onActivateTab={vi.fn()} onCloseTab={vi.fn()} onOpenNote={vi.fn()} onLayout={vi.fn()} />,
    );
    const workspace = container.querySelector(".vault-workspace") as HTMLElement;
    vi.spyOn(workspace, "getBoundingClientRect").mockReturnValue({
      left: 0, top: 0, width: 1000, height: 600, right: 1000, bottom: 600, x: 0, y: 0, toJSON: () => ({}),
    });

    fireEvent.mouseDown(screen.getByRole("separator", { name: "Resize editor columns" }), { clientX: 500 });
    fireEvent.mouseMove(document, { clientX: 650 });
    fireEvent.mouseUp(document);

    expect(workspace.style.gridTemplateColumns).toContain("65%");
  });

  it("marks an unsaved note with an asterisk in its tab title", () => {
    const state = createInitialWorkspace(noteTab("one.md", "One"));
    render(
      <VaultWorkspace vaultId="vault-1" state={state} onActivatePane={vi.fn()} onActivateTab={vi.fn()} onCloseTab={vi.fn()} onOpenNote={vi.fn()} onLayout={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Dirty note" }));

    expect(screen.getByRole("tab")).toHaveTextContent("One *");
    expect(screen.queryByText("Unsaved")).not.toBeInTheDocument();
  });
});