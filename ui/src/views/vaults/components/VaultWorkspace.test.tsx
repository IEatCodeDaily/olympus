import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { VaultWorkspace } from "./VaultWorkspace";
import { noteTab } from "../vaultWorkspace";

vi.mock("../../../lib/uiState", () => ({
  getLocalUiState: () => null,
  loadWorkspaceState: async () => null,
  saveWorkspaceState: vi.fn(),
}));

type Panel = {
  id: string;
  title: string;
  component: string;
  params: Record<string, unknown>;
  api: {
    setActive: () => void;
    setTitle: (title: string) => void;
    updateParameters: (params: Record<string, unknown>) => void;
    close: () => void;
  };
};

let panels: Panel[] = [];
let activeListener: (event: { panel: Panel | null }) => void = () => {};
let removeListener: (panel: Panel) => void = () => {};

vi.mock("dockview-react", () => ({
  DockviewReact: ({ components, defaultTabComponent: Tab, onReady }: { components: Record<string, (props: { params: Record<string, unknown>; api: Panel["api"] }) => JSX.Element>; defaultTabComponent: (props: { params: Record<string, unknown>; api: Panel["api"] }) => JSX.Element; onReady: (event: { api: unknown }) => void }) => {
    const api = {
      panels,
      toJSON: () => ({ panels: panels.map((panel) => panel.id) }),
      fromJSON: vi.fn(),
      getPanel: (id: string) => panels.find((panel) => panel.id === id) ?? null,
      addPanel: (options: { id: string; title: string; component: string; params: Record<string, unknown> }) => {
        const panel: Panel = {
          ...options,
          api: {
            setActive: () => activeListener({ panel }),
            setTitle: (title) => { panel.title = title; },
            updateParameters: (params) => { panel.params = params; },
            close: () => {
              panels = panels.filter((candidate) => candidate.id !== panel.id);
              removeListener(panel);
            },
          },
        };
        panels.push(panel);
        return panel;
      },
      onDidLayoutChange: () => ({ dispose: vi.fn() }),
      onDidActivePanelChange: (listener: typeof activeListener) => { activeListener = listener; return { dispose: vi.fn() }; },
      onDidRemovePanel: (listener: typeof removeListener) => { removeListener = listener; return { dispose: vi.fn() }; },
      onUnhandledDragOver: () => ({ dispose: vi.fn() }),
      onDidDrop: () => ({ dispose: vi.fn() }),
    };
    onReady({ api });
    return (
      <div data-testid="dockview">
        {panels.map((panel) => {
          const Component = components[panel.component];
          return (
            <section key={panel.id} data-testid={panel.id}>
              <div role="tab"><Tab params={panel.params} api={panel.api} /></div>
              <Component params={panel.params} api={panel.api} />
            </section>
          );
        })}
      </div>
    );
  },
}));

vi.mock("../pages/GraphPage", () => ({ GraphPage: () => <div>Graph</div> }));
vi.mock("../pages/NotePage", () => ({
  NotePage: ({ onDirtyChange }: { onDirtyChange: (dirty: boolean) => void }) => (
    <button type="button" onClick={() => onDirtyChange(true)}>Dirty note</button>
  ),
}));
vi.mock("../pages/VaultTablePage", () => ({ VaultTablePage: () => <div>Table</div> }));

describe("VaultWorkspace", () => {
  beforeEach(() => {
    panels = [];
    activeListener = () => {};
    removeListener = () => {};

    vi.restoreAllMocks();
  });

  it("opens one dockview panel per target and focuses the existing panel", () => {
    const onActivateTab = vi.fn();
    const { rerender } = render(
      <VaultWorkspace vaultId="vault-1" initialTab={noteTab("one.md", "One")} onActivateTab={onActivateTab} onCloseTab={vi.fn()} onOpenNote={vi.fn()} />,
    );
    rerender(
      <VaultWorkspace vaultId="vault-1" initialTab={noteTab("one.md", "One")} onActivateTab={onActivateTab} onCloseTab={vi.fn()} onOpenNote={vi.fn()} />,
    );

    expect(panels.map((panel) => panel.id)).toEqual(["note:one.md"]);
    expect(onActivateTab).toHaveBeenCalledWith(noteTab("one.md", "One"));
  });

  it("marks an unsaved note and blocks close when the operator cancels", () => {
    vi.spyOn(window, "confirm").mockReturnValue(false);
    const onCloseTab = vi.fn();
    const { rerender } = render(
      <VaultWorkspace vaultId="vault-1" initialTab={noteTab("one.md", "One")} onActivateTab={vi.fn()} onCloseTab={onCloseTab} onOpenNote={vi.fn()} />,
    );
    rerender(
      <VaultWorkspace vaultId="vault-1" initialTab={noteTab("one.md", "One")} onActivateTab={vi.fn()} onCloseTab={onCloseTab} onOpenNote={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Dirty note" }));
    expect(panels[0].title).toBe("One *");

    fireEvent.click(screen.getByRole("button", { name: "Close One" }));
    expect(onCloseTab).not.toHaveBeenCalled();
    expect(panels).toHaveLength(1);
  });
});
