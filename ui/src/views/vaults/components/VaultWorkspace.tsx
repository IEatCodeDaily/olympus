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
  return (
    <div className="vault-workspace-shell">
      <div className="vault-workspace-toolbar">
        <span className="gk">Layout</span>
        <div className="vault-layout-actions" role="group" aria-label="Workspace layout">
          <LayoutButton label="Single pane" layout="single" active={state.layout === "single"} onLayout={onLayout} icon="panel-right" />
          <LayoutButton label="Two columns" layout="columns" active={state.layout === "columns"} onLayout={onLayout} icon="panel-left" />
          <LayoutButton label="Two rows" layout="rows" active={state.layout === "rows"} onLayout={onLayout} icon="panel-bottom" />
          <LayoutButton label="Grid" layout="grid" active={state.layout === "grid"} onLayout={onLayout} icon="layout-grid" />
        </div>
      </div>
      <div className={`vault-workspace vault-layout-${state.layout}`}>
        {state.panes.map((pane) => {
          const activeTab = pane.tabs.find((tab) => tab.id === pane.activeTabId) ?? null;
          return (
            <section key={pane.id} className={`vault-pane ${pane.id === state.activePaneId ? "active" : ""}`} onMouseDown={() => onActivatePane(pane.id)}>
              <div className="vault-tabs" role="tablist" aria-label="Open vault tabs">
                {pane.tabs.map((tab) => (
                  <div key={tab.id} className={`vault-tab ${tab.id === pane.activeTabId ? "on" : ""}`}>
                    <button type="button" role="tab" aria-selected={tab.id === pane.activeTabId} onClick={() => onActivateTab(pane.id, tab)}>
                      <Icon name={tab.kind === "note" ? "file" : tab.kind === "graph" ? "workflow" : "layout-grid"} size={12} />
                      <span>{tab.title}</span>
                    </button>
                    <button type="button" className="vault-tab-close" aria-label={`Close ${tab.title}`} onClick={() => onCloseTab(pane.id, tab.id)}><Icon name="x" size={10} /></button>
                  </div>
                ))}
              </div>
              <div className="vault-pane-content">
                {activeTab ? (
                  <TabContent vaultId={vaultId} tab={activeTab} onOpenNote={onOpenNote} />
                ) : (
                  <div className="empty-state vault-pane-empty"><div className="empty-state-icon"><Icon name="plus" size={28} /></div><div className="empty-state-title">Empty pane</div><div className="empty-state-msg">Open a file or view from the sidebar.</div></div>
                )}
              </div>
            </section>
          );
        })}
      </div>
    </div>
  );
}

function TabContent({ vaultId, tab, onOpenNote }: { vaultId: string; tab: WorkspaceTab; onOpenNote: (path: string, title?: string) => void }) {
  if (tab.kind === "graph") return <GraphPage vaultId={vaultId} onOpenNote={onOpenNote} />;
  if (tab.kind === "table") return <VaultTablePage vaultId={vaultId} onOpenNote={onOpenNote} />;
  return <NotePage vaultId={vaultId} notePath={tab.path ?? null} onNavigateNote={onOpenNote} />;
}

function LayoutButton({ label, layout, active, onLayout, icon }: { label: string; layout: VaultWorkspaceLayout; active: boolean; onLayout: (layout: VaultWorkspaceLayout) => void; icon: "panel-right" | "panel-left" | "panel-bottom" | "layout-grid" }) {
  return <button type="button" className={active ? "on" : ""} aria-label={label} title={label} onClick={() => onLayout(layout)}><Icon name={icon} size={13} /></button>;
}
