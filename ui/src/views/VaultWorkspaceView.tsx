import { useEffect, useRef, useState } from "react";
import { useNavigate, useRouterState } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { createVault, deleteVaultNote, putVaultNote } from "../api";
import { useVaults, useVaultNotes, qk } from "../hooks/queries";
import { parseRoute } from "../router";
import { useUIStore } from "../store";
import type { CreateVaultBody, NoteTreeEntry } from "../types";
import { CreateVaultDialog, NewNoteDialog } from "./vaults/components/VaultDialogs";
import { DeleteNoteDialog, RenameNoteDialog } from "./vaults/components/NoteActionDialogs";
import { VaultSidebar } from "./vaults/components/VaultSidebar";
import { VaultWorkspace } from "./vaults/components/VaultWorkspace";
import {
  activateWorkspaceGroup,
  activateWorkspaceTab,
  closeAllWorkspaceTabs,
  closeOtherWorkspaceTabs,
  closeWorkspaceGroup,
  closeWorkspaceResource,
  closeWorkspaceTab,
  closeWorkspaceTabsToRight,
  createInitialWorkspace,
  graphTab,
  moveWorkspaceTab,
  noteTab,
  openWorkspaceTab,
  openWorkspaceTabInPane,
  resizeWorkspaceSplit,
  splitWorkspaceGroup,
  tableTab,
  workspaceGroups,
  type VaultWorkspaceState,
  type WorkspaceTab,
} from "./vaults/vaultWorkspace";

const EMPTY_NOTES: NoteTreeEntry[] = [];

export function VaultWorkspaceView() {
  const { location } = useRouterState();
  const { sidebarCollapsed } = useUIStore();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const route = parseRoute(location.pathname);
  const routeNote = new URLSearchParams(location.search).get("note");
  const { data: vaultsData } = useVaults();
  const vaults = vaultsData?.vaults ?? [];
  const activeVaultId = route.vaultId ?? vaults[0]?.id ?? null;
  const { data: notesData } = useVaultNotes(activeVaultId);
  const notes = notesData?.notes ?? EMPTY_NOTES;
  const [workspace, setWorkspace] = useState(() => createInitialWorkspace(null));
  const [workspaceVaultId, setWorkspaceVaultId] = useState<string | null>(activeVaultId);
  const [dirtyResources, setDirtyResources] = useState<Set<string>>(() => new Set());
  const [createVaultOpen, setCreateVaultOpen] = useState(false);
  const [newNoteFolder, setNewNoteFolder] = useState<string | null>(null);
  const [renameEntry, setRenameEntry] = useState<NoteTreeEntry | null>(null);
  const [deleteEntry, setDeleteEntry] = useState<NoteTreeEntry | null>(null);
  const [busy, setBusy] = useState(false);
  const [mutationError, setMutationError] = useState<string | null>(null);
  const splitSequence = useRef(0);

  useEffect(() => {
    if (!activeVaultId || activeVaultId === workspaceVaultId) return;
    if (workspaceVaultId && dirtyResources.size > 0 && !window.confirm("Discard unsaved changes and switch vaults?")) {
      void navigate({ to: "/vaults/$vaultId", params: { vaultId: workspaceVaultId } });
      return;
    }
    setWorkspace(createInitialWorkspace(null));
    setDirtyResources(new Set());
    setWorkspaceVaultId(activeVaultId);
  }, [activeVaultId, dirtyResources.size, navigate, workspaceVaultId]);

  useEffect(() => {
    if (activeVaultId !== workspaceVaultId) return;
    const target = activeVaultId
      ? targetFromRoute(activeVaultId, route.vaultPage, routeNote, notes)
      : null;
    if (target) setWorkspace((current) => openWorkspaceTab(current, target));
  }, [activeVaultId, notes, route.vaultPage, routeNote, workspaceVaultId]);

  const activeGroup = workspaceGroups(workspace).find((group) => group.id === workspace.activeGroupId);
  const activeTab = activeGroup?.views.find((tab) => tab.id === activeGroup.activeViewId) as WorkspaceTab | undefined;
  const activeNotePath = activeTab?.kind === "note" ? activeTab.path ?? activeTab.payload.path ?? null : null;

  const navigateTab = (tab: WorkspaceTab) => {
    if (!activeVaultId) return;
    if (tab.kind === "graph") void navigate({ to: "/vaults/$vaultId/graph", params: { vaultId: activeVaultId } });
    else if (tab.kind === "table") void navigate({ to: "/vaults/$vaultId/tables", params: { vaultId: activeVaultId } });
    else void navigate({ to: "/vaults/$vaultId", params: { vaultId: activeVaultId }, search: { note: tab.path ?? tab.payload.path! } });
  };

  const navigateWorkspace = (next: VaultWorkspaceState) => {
    const group = workspaceGroups(next).find((candidate) => candidate.id === next.activeGroupId);
    const tab = group?.views.find((candidate) => candidate.id === group.activeViewId) as WorkspaceTab | undefined;
    if (tab) navigateTab(tab);
    else if (activeVaultId) void navigate({ to: "/vaults/$vaultId", params: { vaultId: activeVaultId } });
  };

  const openTab = (tab: WorkspaceTab) => {
    setWorkspace((current) => openWorkspaceTab(current, tab));
    navigateTab(tab);
  };

  const invalidateVault = async (vaultId: string) => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: qk.vaults() }),
      queryClient.invalidateQueries({ queryKey: qk.vaultNotes(vaultId) }),
      queryClient.invalidateQueries({ queryKey: qk.vaultDocuments(vaultId) }),
      queryClient.invalidateQueries({ queryKey: ["vaultGraph", vaultId] }),
    ]);
  };

  const handleCreateVault = async (body: CreateVaultBody) => {
    setBusy(true);
    setMutationError(null);
    try {
      const vault = await createVault(body);
      await queryClient.invalidateQueries({ queryKey: qk.vaults() });
      setCreateVaultOpen(false);
      setWorkspace(createInitialWorkspace(null));
      void navigate({ to: "/vaults/$vaultId", params: { vaultId: vault.id } });
    } catch (error) {
      setMutationError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const handleCreateNote = async (path: string, title: string) => {
    if (!activeVaultId) return;
    setBusy(true);
    setMutationError(null);
    try {
      const markdown = `---\ntitle: ${yamlString(title)}\n---\n\n# ${title}\n`;
      const document = await putVaultNote(activeVaultId, path, { markdown, createOnly: true });
      await invalidateVault(activeVaultId);
      setNewNoteFolder(null);
      openTab(noteTab(activeVaultId, document.path, document.title));
    } catch (error) {
      setMutationError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const handleRename = async (newPath: string) => {
    if (!activeVaultId || !renameEntry) return;
    const oldResource = `vault:${activeVaultId}:note:${renameEntry.path}`;
    if (dirtyResources.has(oldResource) && !window.confirm("Discard unsaved changes and rename this note?")) return;
    setWorkspace((current) => closeWorkspaceResource(current, oldResource));
    setDirtyResources((current) => { const next = new Set(current); next.delete(oldResource); return next; });
    setBusy(true);
    setMutationError(null);
    try {
      const document = await putVaultNote(activeVaultId, renameEntry.path, { newPath });
      await invalidateVault(activeVaultId);
      setRenameEntry(null);
      setWorkspace((current) => openWorkspaceTab(current, noteTab(activeVaultId, document.path, document.title)));
      navigateTab(noteTab(activeVaultId, document.path, document.title));
    } catch (error) {
      setMutationError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async () => {
    if (!activeVaultId || !deleteEntry) return;
    const resourceKey = `vault:${activeVaultId}:note:${deleteEntry.path}`;
    if (dirtyResources.has(resourceKey) && !window.confirm("Discard unsaved changes and delete this note?")) return;
    const workspaceWithoutDeleted = closeWorkspaceResource(workspace, resourceKey);
    setWorkspace(workspaceWithoutDeleted);
    setDirtyResources((current) => { const updated = new Set(current); updated.delete(resourceKey); return updated; });
    setBusy(true);
    setMutationError(null);
    try {
      await deleteVaultNote(activeVaultId, deleteEntry.path);
      await invalidateVault(activeVaultId);
      navigateWorkspace(workspaceWithoutDeleted);
      setDeleteEntry(null);
    } catch (error) {
      setMutationError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      {!sidebarCollapsed && (
        <VaultSidebar
          vaults={vaults}
          activeVaultId={activeVaultId}
          notes={notes}
          activeNotePath={activeNotePath}
          onSelectVault={(vaultId) => { void navigate({ to: "/vaults/$vaultId", params: { vaultId } }); }}
          onCreateVault={() => { setMutationError(null); setCreateVaultOpen(true); }}
          onCreateNote={(folder) => { setMutationError(null); setNewNoteFolder(folder ?? ""); }}
          onOpenNote={(path, title) => { if (activeVaultId) openTab(noteTab(activeVaultId, path, title)); }}
          onOpenGraph={() => { if (activeVaultId) openTab(graphTab(activeVaultId)); }}
          onOpenTable={() => { if (activeVaultId) openTab(tableTab(activeVaultId)); }}
          onRenameNote={(entry) => { setMutationError(null); setRenameEntry(entry); }}
          onDeleteNote={(entry) => { setMutationError(null); setDeleteEntry(entry); }}
        />
      )}
      <div className="viewport vault-viewport">
        {activeVaultId && activeVaultId === workspaceVaultId ? (
          <VaultWorkspace
            vaultId={activeVaultId}
            state={workspace}
            onActivateGroup={(groupId) => setWorkspace((current) => activateWorkspaceGroup(current, groupId))}
            onActivateTab={(groupId, tab) => { setWorkspace((current) => activateWorkspaceTab(current, groupId, tab.id)); navigateTab(tab); }}
            onCloseTab={(groupId, tabId) => {
              const next = closeWorkspaceTab(workspace, groupId, tabId);
              setWorkspace(next);
              navigateWorkspace(next);
            }}
            onMoveTab={(sourceGroupId, tabId, targetGroupId, targetIndex) => setWorkspace((current) => moveWorkspaceTab(current, sourceGroupId, tabId, targetGroupId, targetIndex))}
            onDropNote={(groupId, path, title, targetIndex) => {
              const tab = noteTab(activeVaultId, path, title);
              setWorkspace((current) => openWorkspaceTabInPane(current, groupId, tab, targetIndex));
              navigateTab(tab);
            }}
            onTabMenuAction={(groupId, tabId, action) => {
              const next = action === "closeOthers"
                ? closeOtherWorkspaceTabs(workspace, groupId, tabId)
                : action === "closeRight"
                  ? closeWorkspaceTabsToRight(workspace, groupId, tabId)
                  : action === "closeAll"
                    ? closeAllWorkspaceTabs(workspace, groupId)
                    : closeWorkspaceTab(workspace, groupId, tabId);
              setWorkspace(next);
              navigateWorkspace(next);
            }}
            onOpenNote={(path, title) => openTab(noteTab(activeVaultId, path, title))}
            onSplit={(groupId, axis) => {
              splitSequence.current += 1;
              setWorkspace((current) => splitWorkspaceGroup(current, groupId, axis, `vault-split-${splitSequence.current}`, `vault-group-${splitSequence.current}`));
            }}
            onCloseGroup={(groupId) => {
              const next = closeWorkspaceGroup(workspace, groupId);
              setWorkspace(next);
              navigateWorkspace(next);
            }}
            onResizeSplit={(splitId, ratio) => setWorkspace((current) => resizeWorkspaceSplit(current, splitId, ratio))}
            onDirtyResourceChange={(resourceKey, dirty) => setDirtyResources((current) => {
              const next = new Set(current);
              if (dirty) next.add(resourceKey);
              else next.delete(resourceKey);
              return next;
            })}
          />
        ) : (
          <div className="empty-state"><div className="empty-state-title">Create your first vault</div><div className="empty-state-msg">Connect a GitHub repository to begin.</div><button type="button" className="btn primary" onClick={() => setCreateVaultOpen(true)}>Create vault</button></div>
        )}
      </div>
      {createVaultOpen && <CreateVaultDialog busy={busy} error={mutationError} onClose={() => setCreateVaultOpen(false)} onCreate={handleCreateVault} />}
      {newNoteFolder !== null && <NewNoteDialog folder={newNoteFolder || null} busy={busy} error={mutationError} onClose={() => setNewNoteFolder(null)} onCreate={handleCreateNote} />}
      {renameEntry && <RenameNoteDialog key={renameEntry.path} currentPath={renameEntry.path} busy={busy} error={mutationError} onClose={() => setRenameEntry(null)} onRename={handleRename} />}
      <DeleteNoteDialog path={deleteEntry?.path ?? null} busy={busy} error={mutationError} onClose={() => setDeleteEntry(null)} onDelete={handleDelete} />
    </>
  );
}

function targetFromRoute(vaultId: string, page: "note" | "tables" | "graph", path: string | null, notes: NoteTreeEntry[]): WorkspaceTab | null {
  if (page === "graph") return graphTab(vaultId);
  if (page === "tables") return tableTab(vaultId);
  const notePath = path ?? firstNotePath(notes);
  return notePath ? noteTab(vaultId, notePath, findNote(notes, notePath)?.title) : null;
}

function firstNotePath(notes: NoteTreeEntry[]): string | null {
  for (const entry of notes) {
    if (entry.kind === "note") return entry.path;
    const child = firstNotePath(entry.children);
    if (child) return child;
  }
  return null;
}

function findNote(notes: NoteTreeEntry[], path: string): NoteTreeEntry | null {
  for (const entry of notes) {
    if (entry.path === path) return entry;
    const child = findNote(entry.children, path);
    if (child) return child;
  }
  return null;
}

function yamlString(value: string): string {
  return JSON.stringify(value);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "Vault operation failed";
}
