import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { findGroup } from "../../../workbench/model";
import { VaultWorkspace } from "./VaultWorkspace";
import { createInitialWorkspace, noteTab, openWorkspaceTab, splitWorkspaceGroup } from "../vaultWorkspace";

vi.mock("../pages/GraphPage", () => ({ GraphPage: () => <div>Graph</div> }));
vi.mock("../pages/NotePage", () => ({
  NotePage: ({ onDirtyChange }: { onDirtyChange: (dirty: boolean) => void }) => (
    <button type="button" onClick={() => onDirtyChange(true)}>Dirty note</button>
  ),
}));
vi.mock("../pages/VaultTablePage", () => ({ VaultTablePage: () => <div>Table</div> }));

function props(state: ReturnType<typeof createInitialWorkspace>) {
  return {
    vaultId: "vault-1",
    state,
    onActivateGroup: vi.fn(),
    onActivateTab: vi.fn(),
    onCloseTab: vi.fn(),
    onOpenNote: vi.fn(),
    onSplit: vi.fn(),
    onCloseGroup: vi.fn(),
    onResizeSplit: vi.fn(),
  };
}

describe("VaultWorkspace", () => {
  it("keeps split controls on the active editor-group tab row", () => {
    const state = createInitialWorkspace(noteTab("vault-1", "one.md", "One"));
    const { container } = render(<VaultWorkspace {...props(state)} />);

    expect(container.querySelector(".vault-workspace-toolbar")).not.toBeInTheDocument();
    const header = container.querySelector(".vault-pane.active .vault-pane-header") as HTMLElement;
    expect(within(header).getByRole("tablist")).toBeInTheDocument();
    expect(within(header).getByRole("group", { name: "Editor group layout" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Split right" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Split down" })).toBeInTheDocument();
  });

  it("supports the standard horizontal tab keyboard model", () => {
    let state = createInitialWorkspace(noteTab("vault-1", "one.md", "One"));
    state = openWorkspaceTab(state, noteTab("vault-1", "two.md", "Two"));
    const onActivateTab = vi.fn();
    render(<VaultWorkspace {...props(state)} onActivateTab={onActivateTab} />);
    const two = screen.getByRole("tab", { name: "Two" });
    two.focus();
    fireEvent.keyDown(two, { key: "ArrowLeft" });
    expect(onActivateTab).toHaveBeenCalledWith("vault-group-root", expect.objectContaining({ title: "One" }));
    expect(screen.getByRole("tab", { name: "One" })).toHaveFocus();
  });

  it("renders nested horizontal and vertical editor groups with accessible separators", () => {
    let state = createInitialWorkspace(noteTab("vault-1", "one.md", "One"));
    state = splitWorkspaceGroup(state, "vault-group-root", "horizontal", "split-a", "group-b");
    state = splitWorkspaceGroup(state, "group-b", "vertical", "split-b", "group-c");
    render(<VaultWorkspace {...props(state)} />);

    expect(screen.getByRole("separator", { name: "Resize Vault editor groups left and right" })).toBeInTheDocument();
    expect(screen.getByRole("separator", { name: "Resize Vault editor groups top and bottom" })).toBeInTheDocument();
    expect(screen.getAllByRole("tablist")).toHaveLength(3);
  });

  it("marks an unsaved note and prevents dragging it", () => {
    const state = createInitialWorkspace(noteTab("vault-1", "one.md", "One"));
    const onDirtyResourceChange = vi.fn();
    render(<VaultWorkspace {...props(state)} onDirtyResourceChange={onDirtyResourceChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Dirty note" }));

    expect(screen.getByRole("tab")).toHaveTextContent("One *");
    expect(screen.getByRole("tab").closest(".vault-tab")).toHaveAttribute("draggable", "false");
    expect(onDirtyResourceChange).toHaveBeenCalledWith("vault:vault-1:note:one.md", true);
  });

  it("fails closed when closing an editor group with a dirty note", () => {
    let state = createInitialWorkspace(noteTab("vault-1", "one.md", "One"));
    state = splitWorkspaceGroup(state, "vault-group-root", "horizontal", "split-a", "group-b");
    state = { ...state, activeGroupId: "vault-group-root" };
    const onCloseGroup = vi.fn();
    vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<VaultWorkspace {...props(state)} onCloseGroup={onCloseGroup} />);
    fireEvent.click(screen.getByRole("button", { name: "Dirty note" }));
    fireEvent.click(screen.getByRole("button", { name: "Close editor group" }));
    expect(onCloseGroup).not.toHaveBeenCalled();
  });

  it("reports a positional tab drop within a group", () => {
    let state = createInitialWorkspace(noteTab("vault-1", "one.md", "One"));
    state = openWorkspaceTab(state, noteTab("vault-1", "two.md", "Two"));
    const onMoveTab = vi.fn();
    const data = new Map<string, string>();
    const dataTransfer = {
      effectAllowed: "all", dropEffect: "move",
      setData: (type: string, value: string) => data.set(type, value),
      getData: (type: string) => data.get(type) ?? "",
    };
    render(<VaultWorkspace {...props(state)} onMoveTab={onMoveTab} />);

    const one = screen.getByRole("tab", { name: "One" }).closest(".vault-tab") as HTMLElement;
    const two = screen.getByRole("tab", { name: "Two" }).closest(".vault-tab") as HTMLElement;
    fireEvent.dragStart(one, { dataTransfer });
    fireEvent.drop(two, { dataTransfer, clientX: 1000 });

    const group = findGroup(state, "vault-group-root")!;
    expect(onMoveTab).toHaveBeenCalledWith(group.id, group.views[0].id, group.id, 2);
  });

  it("opens a sidebar note dropped on an editor group", () => {
    const state = createInitialWorkspace(noteTab("vault-1", "one.md", "One"));
    const onDropNote = vi.fn();
    const dataTransfer = {
      effectAllowed: "all", dropEffect: "copy", setData: vi.fn(),
      getData: (type: string) => type === "application/x-olympus-vault-note"
        ? JSON.stringify({ path: "docs/new.md", title: "New" }) : "",
    };
    render(<VaultWorkspace {...props(state)} onDropNote={onDropNote} />);
    fireEvent.drop(screen.getByRole("tablist"), { dataTransfer });
    expect(onDropNote).toHaveBeenCalledWith("vault-group-root", "docs/new.md", "New", 1);
  });

  it("offers VS Code-style tab management", () => {
    let state = createInitialWorkspace(noteTab("vault-1", "one.md", "One"));
    state = openWorkspaceTab(state, noteTab("vault-1", "two.md", "Two"));
    const onTabMenuAction = vi.fn();
    render(<VaultWorkspace {...props(state)} onTabMenuAction={onTabMenuAction} />);
    fireEvent.contextMenu(screen.getByRole("tab", { name: "One" }).closest(".vault-tab") as HTMLElement, { clientX: 2000, clientY: 2000 });
    fireEvent.click(screen.getByRole("menuitem", { name: "Close Others" }));
    expect(onTabMenuAction).toHaveBeenCalledWith("vault-group-root", findGroup(state, "vault-group-root")!.views[0].id, "closeOthers");
  });
});
