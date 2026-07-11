import { useRef, useState, type KeyboardEvent, type MouseEvent as ReactMouseEvent } from "react";
import { Icon } from "../../../components/Icon";
import { GraphPage } from "../pages/GraphPage";
import { NotePage } from "../pages/NotePage";
import { VaultTablePage } from "../pages/VaultTablePage";
import type { VaultWorkspaceLayout, VaultWorkspaceState, WorkspaceTab } from "../vaultWorkspace";

export function VaultWorkspace({
  vaultId,
  state,
  onActivatePane,
  onActivateTab,
  onCloseTab,
  onOpenNote,
  onLayout,
}: {
  vaultId: string;
  state: VaultWorkspaceState;
  onActivatePane: (paneId: string) => void;
  onActivateTab: (paneId: string, tab: WorkspaceTab) => void;
  onCloseTab: (paneId: string, tabId: string) => void;
  onOpenNote: (path: string, title?: string) => void;
  onLayout: (layout: VaultWorkspaceLayout) => void;
}) {
  const workspaceRef = useRef<HTMLDivElement>(null);
  const [columnSplit, setColumnSplit] = useState(50);
  const [rowSplit, setRowSplit] = useState(50);
  const [dirtyTabs, setDirtyTabs] = useState<Set<string>>(() => new Set());
  const hasColumnSplit = state.layout === "columns" || state.layout === "grid";
  const hasRowSplit = state.layout === "rows" || state.layout === "grid";

  const beginResize = (axis: "x" | "y") => (event: ReactMouseEvent<HTMLDivElement>) => {
    event.preventDefault();
    const onMove = (moveEvent: MouseEvent) => {
      const rect = workspaceRef.current?.getBoundingClientRect();
      if (!rect) return;
      const raw = axis === "x"
        ? ((moveEvent.clientX - rect.left) / rect.width) * 100
        : ((moveEvent.clientY - rect.top) / rect.height) * 100;
      const next = Math.max(20, Math.min(80, Math.round(raw)));
      if (axis === "x") setColumnSplit(next);
      else setRowSplit(next);
    };
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  };

  const resizeWithKeyboard = (axis: "x" | "y") => (event: KeyboardEvent<HTMLDivElement>) => {
    const backward = axis === "x" ? event.key === "ArrowLeft" : event.key === "ArrowUp";
    const forward = axis === "x" ? event.key === "ArrowRight" : event.key === "ArrowDown";
    if (!backward && !forward) return;
    event.preventDefault();
    const update = (current: number) => Math.max(20, Math.min(80, current + (forward ? 5 : -5)));
    if (axis === "x") setColumnSplit(update);
    else setRowSplit(update);
  };

  const workspaceStyle = {
    ...(hasColumnSplit ? { gridTemplateColumns: `calc(${columnSplit}% - 0.5px) calc(${100 - columnSplit}% - 0.5px)` } : {}),
    ...(hasRowSplit ? { gridTemplateRows: `calc(${rowSplit}% - 0.5px) calc(${100 - rowSplit}% - 0.5px)` } : {}),
  };

  return (
    <div className="vault-workspace-shell">
      <div ref={workspaceRef} className={`vault-workspace vault-layout-${state.layout}`} style={workspaceStyle}>
        {state.panes.map((pane) => {
          const activeTab = pane.tabs.find((tab) => tab.id === pane.activeTabId) ?? null;
          const activePane = pane.id === state.activePaneId;
          return (
            <section key={pane.id} className={`vault-pane ${activePane ? "active" : ""}`} onMouseDown={() => onActivatePane(pane.id)}>
              <div className="vault-pane-header">
                <div className="vault-tabs" role="tablist" aria-label="Open vault tabs">
                  {pane.tabs.map((tab) => {
                    const dirty = dirtyTabs.has(`${pane.id}:${tab.id}`);
                    return (
                    <div key={tab.id} className={`vault-tab ${tab.id === pane.activeTabId ? "on" : ""}`}>
                      <button type="button" role="tab" aria-selected={tab.id === pane.activeTabId} onClick={() => onActivateTab(pane.id, tab)}>
                        <Icon name={tab.kind === "note" ? "file" : tab.kind === "graph" ? "workflow" : "layout-grid"} size={12} />
                        <span>{tab.title}{dirty ? " *" : ""}</span>
                      </button>
                      <button type="button" className="vault-tab-close" aria-label={`Close ${tab.title}`} onClick={() => onCloseTab(pane.id, tab.id)}><Icon name="x" size={10} /></button>
                    </div>
                    );
                  })}
                </div>
                {activePane && (
                  <div className="vault-layout-actions" role="group" aria-label="Workspace layout">
                    <LayoutButton label="Single pane" layout="single" active={state.layout === "single"} onLayout={onLayout} icon="panel-right" />
                    <LayoutButton label="Split right" layout="columns" active={state.layout === "columns"} onLayout={onLayout} icon="panel-left" />
                    <LayoutButton label="Split down" layout="rows" active={state.layout === "rows"} onLayout={onLayout} icon="panel-bottom" />
                    <LayoutButton label="Grid" layout="grid" active={state.layout === "grid"} onLayout={onLayout} icon="layout-grid" />
                  </div>
                )}
              </div>
              <div className="vault-pane-content">
                {activeTab ? (
                  <TabContent
                    vaultId={vaultId}
                    tab={activeTab}
                    onOpenNote={onOpenNote}
                    onDirtyChange={(dirty) => {
                      const key = `${pane.id}:${activeTab.id}`;
                      setDirtyTabs((current) => {
                        const next = new Set(current);
                        if (dirty) next.add(key);
                        else next.delete(key);
                        return next;
                      });
                    }}
                  />
                ) : (
                  <div className="empty-state vault-pane-empty"><div className="empty-state-icon"><Icon name="plus" size={28} /></div><div className="empty-state-title">Empty pane</div><div className="empty-state-msg">Open a file or view from the sidebar.</div></div>
                )}
              </div>
            </section>
          );
        })}
        {hasColumnSplit && (
          <div
            className="vault-pane-resizer vault-pane-resizer-x"
            style={{ left: `${columnSplit}%` }}
            role="separator"
            aria-label="Resize editor columns"
            aria-orientation="vertical"
            aria-valuenow={columnSplit}
            tabIndex={0}
            onMouseDown={beginResize("x")}
            onKeyDown={resizeWithKeyboard("x")}
          />
        )}
        {hasRowSplit && (
          <div
            className="vault-pane-resizer vault-pane-resizer-y"
            style={{ top: `${rowSplit}%` }}
            role="separator"
            aria-label="Resize editor rows"
            aria-orientation="horizontal"
            aria-valuenow={rowSplit}
            tabIndex={0}
            onMouseDown={beginResize("y")}
            onKeyDown={resizeWithKeyboard("y")}
          />
        )}
      </div>
    </div>
  );
}

function TabContent({ vaultId, tab, onOpenNote, onDirtyChange }: { vaultId: string; tab: WorkspaceTab; onOpenNote: (path: string, title?: string) => void; onDirtyChange: (dirty: boolean) => void }) {
  if (tab.kind === "graph") return <GraphPage vaultId={vaultId} onOpenNote={onOpenNote} />;
  if (tab.kind === "table") return <VaultTablePage vaultId={vaultId} onOpenNote={onOpenNote} />;
  return <NotePage vaultId={vaultId} notePath={tab.path ?? null} onNavigateNote={onOpenNote} onDirtyChange={onDirtyChange} />;
}

function LayoutButton({ label, layout, active, onLayout, icon }: { label: string; layout: VaultWorkspaceLayout; active: boolean; onLayout: (layout: VaultWorkspaceLayout) => void; icon: "panel-right" | "panel-left" | "panel-bottom" | "layout-grid" }) {
  return <button type="button" className={active ? "on" : ""} aria-label={label} title={label} onClick={() => onLayout(layout)}><Icon name={icon} size={13} /></button>;
}
