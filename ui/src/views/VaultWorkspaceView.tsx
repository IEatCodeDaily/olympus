import { useEffect, useState } from "react";
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
  activateWorkspaceTab,
  closeWorkspaceTab,
  createInitialWorkspace,
  graphTab,
  noteTab,
  openWorkspaceTab,
  setWorkspaceLayout,
  tableTab,
  type VaultWorkspaceLayout,
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
  const [createVaultOpen, setCreateVaultOpen] = useState(false);
  const [newNoteFolder, setNewNoteFolder] = useState<string | null>(null);
  const [renameEntry, setRenameEntry] = useState<NoteTreeEntry | null>(null);
  const [deleteEntry, setDeleteEntry] = useState<NoteTreeEntry | null>(null);
  const [busy, setBusy] = useState(false);
  const [mutationError, setMutationError] = useState<string | null>(null);

  useEffect(() => {
    const target = targetFromRoute(route.vaultPage, routeNote, notes);
    if (target) setWorkspace((current) => openWorkspaceTab(current, target));
  }, [notes, route.vaultPage, routeNote]);

  const activePane = workspace.panes.find((pane) => pane.id === workspace.activePaneId);
  const activeTab = activePane?.tabs.find((tab) => tab.id === activePane.activeTabId) ?? null;
  const activeNotePath = activeTab?.kind === "note" ? activeTab.path ?? null : null;

  const navigateTab = (tab: WorkspaceTab) => {
    if (!activeVaultId) return;
    if (tab.kind === "graph") {
      void navigate({ to: "/vaults/$vaultId/graph", params: { vaultId: activeVaultId } });
    } else if (tab.kind === "table") {
      void navigate({ to: "/vaults/$vaultId/tables", params: { vaultId: activeVaultId } });
    } else {
      void navigate({ to: "/vaults/$vaultId", params: { vaultId: activeVaultId }, search: { note: tab.path! } });
    }
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
      openTab(noteTab(document.path, document.title));
    } catch (error) {
      setMutationError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const handleRename = async (newPath: string) => {
    if (!activeVaultId || !renameEntry) return;
    setBusy(true);
    setMutationError(null);
    try {
      const document = await putVaultNote(activeVaultId, renameEntry.path, { newPath });
      await invalidateVault(activeVaultId);
      setRenameEntry(null);
      openTab(noteTab(document.path, document.title));
    } catch (error) {
      setMutationError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async () => {
    if (!activeVaultId || !deleteEntry) return;
    setBusy(true);
    setMutationError(null);
    try {
      await deleteVaultNote(activeVaultId, deleteEntry.path);
      await invalidateVault(activeVaultId);
      setWorkspace((current) => closeAllTarget(current, `note:${deleteEntry.path}`));
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
          onSelectVault={(vaultId) => { setWorkspace(createInitialWorkspace(null)); void navigate({ to: "/vaults/$vaultId", params: { vaultId } }); }}
          onCreateVault={() => { setMutationError(null); setCreateVaultOpen(true); }}
          onCreateNote={(folder) => { setMutationError(null); setNewNoteFolder(folder ?? ""); }}
          onOpenNote={(path, title) => openTab(noteTab(path, title))}
          onOpenGraph={() => openTab(graphTab)}
          onOpenTable={() => openTab(tableTab)}
          onRenameNote={(entry) => { setMutationError(null); setRenameEntry(entry); }}
          onDeleteNote={(entry) => { setMutationError(null); setDeleteEntry(entry); }}
        />
      )}
      <div className="viewport vault-viewport">
        {activeVaultId ? (
          <VaultWorkspace
            vaultId={activeVaultId}
            state={workspace}
            onActivatePane={(paneId) => setWorkspace((current) => ({ ...current, activePaneId: paneId }))}
            onActivateTab={(paneId, tab) => { setWorkspace((current) => activateWorkspaceTab(current, paneId, tab.id)); navigateTab(tab); }}
            onCloseTab={(paneId, tabId) => setWorkspace((current) => closeWorkspaceTab(current, paneId, tabId))}
            onOpenNote={(path, title) => openTab(noteTab(path, title))}
            onLayout={(layout: VaultWorkspaceLayout) => setWorkspace((current) => setWorkspaceLayout(current, layout))}
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

function targetFromRoute(page: "note" | "tables" | "graph", path: string | null, notes: NoteTreeEntry[]): WorkspaceTab | null {
  if (page === "graph") return graphTab;
  if (page === "tables") return tableTab;
  const notePath = path ?? firstNotePath(notes);
  return notePath ? noteTab(notePath, findNote(notes, notePath)?.title) : null;
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

function closeAllTarget(state: ReturnType<typeof createInitialWorkspace>, tabId: string) {
  return state.panes.reduce((current, pane) => closeWorkspaceTab(current, pane.id, tabId), state);
}

function yamlString(value: string): string {
  return JSON.stringify(value);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "Vault operation failed";
}
