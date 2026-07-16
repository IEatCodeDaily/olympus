import { useCallback, useEffect, useRef, useState } from "react";
import {
  DockviewReact,
  type DockviewApi,
  type DockviewDidDropEvent,
  type DockviewReadyEvent,
  type IDockviewPanelHeaderProps,
  type IDockviewPanelProps,
} from "dockview-react";
import "dockview-react/dist/styles/dockview.css";
import { loadWorkspaceState, saveWorkspaceState, getLocalUiState } from "../../../lib/uiState";
import { GraphPage } from "../pages/GraphPage";
import { NotePage } from "../pages/NotePage";
import { VaultTablePage } from "../pages/VaultTablePage";
import { noteTab, type WorkspaceTab } from "../vaultWorkspace";

type DockLayout = ReturnType<DockviewApi["toJSON"]>;

interface VaultPanelParams {
  vaultId: string;
  tab: WorkspaceTab;
  dirty: boolean;
  editorMode?: "rich" | "source";
  onOpenNote: (path: string, title?: string) => void;
  onDirtyChange: (panelId: string, dirty: boolean) => void;
  onEditorModeChange: (panelId: string, mode: "rich" | "source") => void;
}

let savedVaultLayouts = new Map<string, DockLayout | null>();

export function VaultWorkspace({
  vaultId,
  initialTab,
  onActivateTab,
  onCloseTab,
  onOpenNote,
  onDirtyResourceChange,
}: {
  vaultId: string;
  initialTab: WorkspaceTab | null;
  onActivateTab: (tab: WorkspaceTab) => void;
  onCloseTab: (tab: WorkspaceTab) => void;
  onOpenNote: (path: string, title?: string) => void;
  onDirtyResourceChange?: (resourceId: string, dirty: boolean) => void;
}) {
  const surface = `vault:${vaultId}`;
  const apiRef = useRef<DockviewApi | null>(null);

  const [dirtyPanels, setDirtyPanels] = useState<Set<string>>(() => new Set());
  const [editorModes, setEditorModes] = useState<Map<string, "rich" | "source">>(() => new Map());

  const persist = useCallback(() => {
    const api = apiRef.current;
    if (!api) return;
    const layout = api.toJSON();
    savedVaultLayouts.set(surface, layout);
    saveWorkspaceState(surface, layout);
  }, [surface]);

  const buildPanelParams = useCallback(
    (tab: WorkspaceTab): VaultPanelParams => ({
      vaultId,
      tab,
      dirty: dirtyPanels.has(tab.id),
      editorMode: editorModes.get(tab.id),
      onOpenNote,
      onDirtyChange: (panelId: string, dirty: boolean) => {
        onDirtyResourceChange?.(panelId, dirty);
        setDirtyPanels((current) => {
          const next = new Set(current);
          if (dirty) next.add(panelId);
          else next.delete(panelId);
          return next;
        });
      },
      onEditorModeChange: (panelId: string, mode: "rich" | "source") => {
        setEditorModes((current) => {
          const next = new Map(current);
          if (mode === "rich") next.delete(panelId);
          else next.set(panelId, mode);
          return next;
        });
      },
    }),
    [dirtyPanels, editorModes, onDirtyResourceChange, onOpenNote, vaultId],
  );

  const rehydratePanels = useCallback((api: DockviewApi) => {
    api.panels.forEach((panel) => {
      const tab = (panel.params as Partial<VaultPanelParams> | undefined)?.tab;
      if (tab) {
        panel.api.updateParameters(buildPanelParams(tab));
      }
    });
  }, [buildPanelParams]);

  const openPanel = useCallback(
    (tab: WorkspaceTab, drop?: DockviewDidDropEvent) => {
      const api = apiRef.current;
      if (!api) return;
      const existing = api.getPanel(tab.id);
      if (existing) {
        existing.api.setActive();
        return;
      }
      const panel = api.addPanel({
        id: tab.id,
        title: tab.title,
        component: "vault-panel",
        params: buildPanelParams(tab),
        ...(drop?.group ? {
          position: { referenceGroup: drop.group, direction: dropDirection(drop.position) },
        } : {}),
      });
      panel.api.setActive();
      persist();
    },
    [buildPanelParams, persist],
  );

  useEffect(() => {
    if (initialTab) openPanel(initialTab);
  }, [initialTab, openPanel]);

  useEffect(() => {
    apiRef.current?.panels.forEach((panel) => {
      const params = panel.params as VaultPanelParams | undefined;
      if (!params?.tab) return;
      const dirty = dirtyPanels.has(panel.id);
      panel.api.setTitle(`${params.tab.title}${dirty ? " *" : ""}`);
      panel.api.updateParameters(buildPanelParams(params.tab));
    });
  }, [buildPanelParams, dirtyPanels]);

  const handleReady = useCallback(
    (event: DockviewReadyEvent) => {
      apiRef.current = event.api;
      const local = savedVaultLayouts.get(surface) ?? getLocalUiState<DockLayout>(surface);
      if (local) {
        try {
          event.api.fromJSON(local);
          rehydratePanels(event.api);
        } catch {
          savedVaultLayouts.set(surface, null);
        }
      }
      void loadWorkspaceState<DockLayout>(surface).then((remote) => {
        if (!remote || apiRef.current !== event.api) return;
        try {
          event.api.fromJSON(remote);
          rehydratePanels(event.api);
          savedVaultLayouts.set(surface, remote);
          if (initialTab) openPanel(initialTab);
        } catch {
          // Keep the local/default dock layout if Hall has stale state.
        }
      });
      if (initialTab) openPanel(initialTab);

      const layoutDisposable = event.api.onDidLayoutChange(() => persist());
      const activeDisposable = event.api.onDidActivePanelChange(({ panel }) => {
        const tab = panel?.params ? (panel.params as VaultPanelParams).tab : null;
        if (tab) onActivateTab(tab);
      });
      const removeDisposable = event.api.onDidRemovePanel((panel) => {
        const tab = panel.params ? (panel.params as VaultPanelParams).tab : null;
        if (tab) onCloseTab(tab);
      });
      const dragOverDisposable = event.api.onUnhandledDragOver((dragEvent) => {
        if (hasDragType(dragEvent.nativeEvent, "application/x-olympus-vault-note")) dragEvent.accept();
      });
      const dropDisposable = event.api.onDidDrop((dropEvent) => {
        const payload = dragPayload(dropEvent.nativeEvent, "application/x-olympus-vault-note") as { path?: string; title?: string } | null;
        if (payload?.path) openPanel(noteTab(payload.path, payload.title), dropEvent);
      });
      return () => {
        layoutDisposable.dispose();
        activeDisposable.dispose();
        removeDisposable.dispose();
        dragOverDisposable.dispose();
        dropDisposable.dispose();
      };
    },
    [initialTab, onActivateTab, onCloseTab, openPanel, persist, rehydratePanels, surface],
  );

  return (
    <div className="vault-workspace-shell">
      <DockviewReact
        className="dockview-theme-abyss olympus-dockview vault-dockview"
        components={{ "vault-panel": VaultPanel }}
        defaultTabComponent={VaultTab}
        onReady={handleReady}
      />
    </div>
  );
}

function dropDirection(position: "top" | "bottom" | "left" | "right" | "center"): "left" | "right" | "above" | "below" | "within" {
  return position === "top" ? "above" : position === "bottom" ? "below" : position === "center" ? "within" : position;
}

function hasDragType(event: globalThis.DragEvent | PointerEvent, type: string): boolean {
  return event instanceof globalThis.DragEvent && event.dataTransfer?.types.includes(type) === true;
}

function dragPayload(event: globalThis.DragEvent | PointerEvent, type: string): unknown | null {
  if (!(event instanceof globalThis.DragEvent)) return null;
  try {
    const value = event.dataTransfer?.getData(type);
    return value ? JSON.parse(value) : null;
  } catch {
    return null;
  }
}

function VaultTab({ api, params }: IDockviewPanelHeaderProps<VaultPanelParams>) {
  return (
    <div className="vault-dock-tab">
      <span>{api.title}</span>
      <button
        type="button"
        aria-label={`Close ${params.tab.title}`}
        onClick={(event) => {
          event.stopPropagation();
          if (!params.dirty || window.confirm("Close note with unsaved changes?")) api.close();
        }}
      >
        ×
      </button>
    </div>
  );
}

function VaultPanel({ params }: IDockviewPanelProps<VaultPanelParams>) {
  const { vaultId, tab, onOpenNote, onDirtyChange, onEditorModeChange } = params;
  const editorMode = params.editorMode;

  if (tab.kind === "graph") return <GraphPage vaultId={vaultId} onOpenNote={onOpenNote} />;
  if (tab.kind === "table") return <VaultTablePage vaultId={vaultId} onOpenNote={onOpenNote} />;
  return (
    <NotePage
      vaultId={vaultId}
      notePath={tab.path ?? null}
      onNavigateNote={onOpenNote}
      onDirtyChange={(dirty) => onDirtyChange(tab.id, dirty)}
      editorMode={editorMode}
      onEditorModeChange={(mode) => onEditorModeChange(tab.id, mode)}
    />
  );
}
