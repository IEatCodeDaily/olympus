import { useEffect, useState, useCallback, type DragEvent } from "react";
import { Icon } from "../../../components/Icon";
import { SplitLayout } from "../../../workbench/SplitLayout";
import type { GroupNode, SplitAxis } from "../../../workbench/model";
import { GraphPage } from "../pages/GraphPage";
import { NotePage } from "../pages/NotePage";
import { VaultTablePage } from "../pages/VaultTablePage";
import { workspaceGroups, type VaultTabPayload, type VaultWorkspaceState, type WorkspaceTab } from "../vaultWorkspace";
import { VAULT_NOTE_DRAG_TYPE, VAULT_TAB_DRAG_TYPE, readDragData, type VaultNoteDragData, type VaultTabDragData } from "../vaultDrag";

export type VaultTabMenuAction = "close" | "closeOthers" | "closeRight" | "closeAll";

interface TabMenuState {
  groupId: string;
  tabId: string;
  x: number;
  y: number;
}

function clampTabMenuPosition(x: number, y: number): Pick<TabMenuState, "x" | "y"> {
  return {
    x: Math.max(8, Math.min(x, window.innerWidth - 188)),
    y: Math.max(8, Math.min(y, window.innerHeight - 158)),
  };
}

export function VaultWorkspace({
  vaultId,
  state,
  onActivateGroup,
  onActivateTab,
  onCloseTab,
  onMoveTab,
  onDropNote,
  onTabMenuAction,
  onOpenNote,
  onSplit,
  onCloseGroup,
  onResizeSplit,
  onDirtyResourceChange,
}: {
  vaultId: string;
  state: VaultWorkspaceState;
  onActivateGroup: (groupId: string) => void;
  onActivateTab: (groupId: string, tab: WorkspaceTab) => void;
  onCloseTab: (groupId: string, tabId: string) => void;
  onMoveTab?: (sourceGroupId: string, tabId: string, targetGroupId: string, targetIndex: number) => void;
  onDropNote?: (groupId: string, path: string, title: string, targetIndex: number) => void;
  onTabMenuAction?: (groupId: string, tabId: string, action: VaultTabMenuAction) => void;
  onOpenNote: (path: string, title?: string) => void;
  onSplit: (groupId: string, axis: SplitAxis) => void;
  onCloseGroup: (groupId: string) => void;
  onResizeSplit: (splitId: string, ratio: number) => void;
  onDirtyResourceChange?: (resourceKey: string, dirty: boolean) => void;
}) {
  const [dirtyTabs, setDirtyTabs] = useState<Set<string>>(() => new Set());
  const [dropTarget, setDropTarget] = useState<{ groupId: string; index: number } | null>(null);
  const [tabMenu, setTabMenu] = useState<TabMenuState | null>(null);
  const [tabModes, setTabModes] = useState<Map<string, "rich" | "source">>(() => new Map());

  const handleEditorModeChange = useCallback((tabId: string, mode: "rich" | "source") => {
    setTabModes((current) => {
      const next = new Map(current);
      if (mode === "rich") next.delete(tabId);
      else next.set(tabId, mode);
      return next;
    });
  }, []);

  useEffect(() => {
    if (!tabMenu) return;
    const close = () => setTabMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("blur", close);
    };
  }, [tabMenu]);

  const closeTab = (groupId: string, tabId: string) => {
    if (dirtyTabs.has(tabId) && !window.confirm("Discard unsaved changes and close this tab?")) return;
    const tab = workspaceGroups(state).find((group) => group.id === groupId)?.views.find((view) => view.id === tabId);
    if (tab && dirtyTabs.has(tabId)) onDirtyResourceChange?.(tab.resourceKey, false);
    onCloseTab(groupId, tabId);
  };

  const closeGroup = (groupId: string) => {
    const group = workspaceGroups(state).find((candidate) => candidate.id === groupId);
    const discardsDraft = group?.views.some((tab) => dirtyTabs.has(tab.id)) ?? false;
    if (discardsDraft && !window.confirm("Discard unsaved changes and close this editor group?")) return;
    group?.views.forEach((tab) => { if (dirtyTabs.has(tab.id)) onDirtyResourceChange?.(tab.resourceKey, false); });
    onCloseGroup(groupId);
  };

  const runTabMenuAction = (groupId: string, tabId: string, action: VaultTabMenuAction) => {
    if (action === "close") {
      closeTab(groupId, tabId);
      return;
    }
    const group = workspaceGroups(state).find((candidate) => candidate.id === groupId);
    const tabIndex = group?.views.findIndex((tab) => tab.id === tabId) ?? -1;
    const closingTabs = group?.views.filter((tab, index) => {
      if (action === "closeOthers") return tab.id !== tabId;
      if (action === "closeRight") return index > tabIndex;
      return true;
    }) ?? [];
    if (closingTabs.some((tab) => dirtyTabs.has(tab.id)) && !window.confirm("Discard unsaved changes in the tabs being closed?")) return;
    closingTabs.forEach((tab) => { if (dirtyTabs.has(tab.id)) onDirtyResourceChange?.(tab.resourceKey, false); });
    onTabMenuAction?.(groupId, tabId, action);
  };

  const dropAt = (groupId: string, index: number) => (event: DragEvent<HTMLElement>) => {
    event.preventDefault();
    event.stopPropagation();
    const tab = readDragData<VaultTabDragData>(event.dataTransfer, VAULT_TAB_DRAG_TYPE);
    if (tab?.paneId && tab.tabId) {
      onMoveTab?.(tab.paneId, tab.tabId, groupId, index);
    } else {
      const note = readDragData<VaultNoteDragData>(event.dataTransfer, VAULT_NOTE_DRAG_TYPE);
      if (note?.path && note.title) onDropNote?.(groupId, note.path, note.title, index);
    }
    setDropTarget(null);
  };

  const dragOverAt = (groupId: string, index: number) => (event: DragEvent<HTMLElement>) => {
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = event.dataTransfer.getData(VAULT_TAB_DRAG_TYPE) ? "move" : "copy";
    setDropTarget({ groupId, index });
  };

  const renderGroup = (group: GroupNode<VaultTabPayload>) => {
    const tabs = group.views as WorkspaceTab[];
    const activeGroup = group.id === state.activeGroupId;
    return (
      <section
        className={`vault-pane ${activeGroup ? "active" : ""}`}
        data-group-id={group.id}
        onMouseDown={() => onActivateGroup(group.id)}
      >
        <div className="vault-pane-header">
          <div
            className={`vault-tabs ${dropTarget?.groupId === group.id && dropTarget.index === tabs.length ? "drop-end" : ""}`}
            role="tablist"
            aria-label="Open vault tabs"
            onDragOver={dragOverAt(group.id, tabs.length)}
            onDrop={dropAt(group.id, tabs.length)}
          >
            {tabs.map((tab, tabIndex) => {
              const dirty = dirtyTabs.has(tab.id);
              const selected = tab.id === group.activeViewId;
              const tabDomId = `vault-tab-${tab.id}`;
              const panelDomId = `vault-panel-${tab.id}`;
              return (
                <div
                  key={tab.id}
                  className={`vault-tab ${selected ? "on" : ""} ${dropTarget?.groupId === group.id && dropTarget.index === tabIndex ? "drop-before" : ""} ${dropTarget?.groupId === group.id && dropTarget.index === tabIndex + 1 ? "drop-after" : ""}`}
                  draggable={!dirty}
                  title={dirty ? "Save before moving this tab" : tab.path}
                  onDragStart={(event) => {
                    event.dataTransfer.effectAllowed = "move";
                    event.dataTransfer.setData(VAULT_TAB_DRAG_TYPE, JSON.stringify({ paneId: group.id, tabId: tab.id }));
                  }}
                  onDragOver={(event) => {
                    const rect = event.currentTarget.getBoundingClientRect();
                    dragOverAt(group.id, event.clientX < rect.left + rect.width / 2 ? tabIndex : tabIndex + 1)(event);
                  }}
                  onDrop={(event) => {
                    const rect = event.currentTarget.getBoundingClientRect();
                    dropAt(group.id, event.clientX < rect.left + rect.width / 2 ? tabIndex : tabIndex + 1)(event);
                  }}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    setTabMenu({ groupId: group.id, tabId: tab.id, ...clampTabMenuPosition(event.clientX, event.clientY) });
                  }}
                  onAuxClick={(event) => { if (event.button === 1) closeTab(group.id, tab.id); }}
                >
                  <button
                    id={tabDomId}
                    type="button"
                    role="tab"
                    aria-selected={selected}
                    aria-controls={panelDomId}
                    tabIndex={selected ? 0 : -1}
                    onClick={() => onActivateTab(group.id, tab)}
                    onKeyDown={(event) => {
                      let nextIndex: number | null = null;
                      if (event.key === "ArrowLeft") nextIndex = (tabIndex - 1 + tabs.length) % tabs.length;
                      else if (event.key === "ArrowRight") nextIndex = (tabIndex + 1) % tabs.length;
                      else if (event.key === "Home") nextIndex = 0;
                      else if (event.key === "End") nextIndex = tabs.length - 1;
                      if (nextIndex === null) return;
                      event.preventDefault();
                      const nextTab = tabs[nextIndex];
                      onActivateTab(group.id, nextTab);
                      document.getElementById(`vault-tab-${nextTab.id}`)?.focus();
                    }}
                  >
                    <Icon name={tab.kind === "note" ? "file" : tab.kind === "graph" ? "workflow" : "layout-grid"} size={12} />
                    <span>{tab.title}{dirty ? " *" : ""}</span>
                  </button>
                  <button type="button" className="vault-tab-close" aria-label={`Close ${tab.title}`} onClick={() => closeTab(group.id, tab.id)}><Icon name="x" size={10} /></button>
                </div>
              );
            })}
          </div>
          {activeGroup && (
            <div className="vault-layout-actions" role="group" aria-label="Editor group layout">
              <button type="button" aria-label="Split right" title="Split right" onClick={() => onSplit(group.id, "horizontal")}><Icon name="panel-left" size={13} /></button>
              <button type="button" aria-label="Split down" title="Split down" onClick={() => onSplit(group.id, "vertical")}><Icon name="panel-bottom" size={13} /></button>
              {workspaceGroups(state).length > 1 && (
                <button type="button" aria-label="Close editor group" title="Close editor group" onClick={() => closeGroup(group.id)}><Icon name="x" size={13} /></button>
              )}
            </div>
          )}
        </div>
        <div className="vault-pane-content">
          {tabs.length > 0 ? tabs.map((tab) => {
            const selected = tab.id === group.activeViewId;
            return (
              <div
                key={tab.id}
                id={`vault-panel-${tab.id}`}
                className={`vault-tab-panel ${selected ? "active" : ""}`}
                role="tabpanel"
                aria-labelledby={`vault-tab-${tab.id}`}
                hidden={!selected}
              >
                <TabContent
                  vaultId={vaultId}
                  tab={tab}
                  onOpenNote={onOpenNote}
                  onDirtyChange={(dirty) => setDirtyTabs((current) => {
                    const next = new Set(current);
                    if (dirty) next.add(tab.id);
                    else next.delete(tab.id);
                    onDirtyResourceChange?.(tab.resourceKey, dirty);
                    return next;
                  })}
                  editorMode={tabModes.get(tab.id)}
                  onEditorModeChange={(mode) => handleEditorModeChange(tab.id, mode)}
                />
              </div>
            );
          }) : (
            <div className="empty-state vault-pane-empty"><div className="empty-state-icon"><Icon name="plus" size={28} /></div><div className="empty-state-title">Empty group</div><div className="empty-state-msg">Open a file or view from the sidebar.</div></div>
          )}
        </div>
      </section>
    );
  };

  return (
    <div className="vault-workspace-shell">
      <SplitLayout root={state.root} surfaceLabel="Vault editor groups" renderGroup={renderGroup} onResize={onResizeSplit} />
      {tabMenu && (
        <div className="menu on vault-tab-menu" role="menu" style={{ left: tabMenu.x, top: tabMenu.y }} onClick={(event) => event.stopPropagation()}>
          {([ ["close", "Close"], ["closeOthers", "Close Others"], ["closeRight", "Close to the Right"], ["closeAll", "Close All"] ] as const).map(([action, label]) => (
            <button key={action} type="button" className="mi" role="menuitem" onClick={() => {
              runTabMenuAction(tabMenu.groupId, tabMenu.tabId, action);
              setTabMenu(null);
            }}>{label}</button>
          ))}
        </div>
      )}
    </div>
  );
}

function TabContent({ vaultId, tab, onOpenNote, onDirtyChange, editorMode, onEditorModeChange }: { vaultId: string; tab: WorkspaceTab; onOpenNote: (path: string, title?: string) => void; onDirtyChange: (dirty: boolean) => void; editorMode?: "rich" | "source"; onEditorModeChange?: (mode: "rich" | "source") => void }) {
  if (tab.kind === "graph") return <GraphPage vaultId={vaultId} onOpenNote={onOpenNote} />;
  if (tab.kind === "table") return <VaultTablePage vaultId={vaultId} onOpenNote={onOpenNote} />;
  return <NotePage vaultId={vaultId} notePath={tab.path ?? tab.payload.path ?? null} onNavigateNote={onOpenNote} onDirtyChange={onDirtyChange} editorMode={editorMode} onEditorModeChange={onEditorModeChange} />;
}
