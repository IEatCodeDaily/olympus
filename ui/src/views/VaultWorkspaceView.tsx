import { useEffect, useMemo, useState } from "react";
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
  graphTab,
  noteTab,
  tableTab,
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
  const [createVaultOpen, setCreateVaultOpen] = useState(false);
  const [newNoteFolder, setNewNoteFolder] = useState<string | null>(null);
  const [renameEntry, setRenameEntry] = useState<NoteTreeEntry | null>(null);
  const [deleteEntry, setDeleteEntry] = useState<NoteTreeEntry | null>(null);
  const [busy, setBusy] = useState(false);
  const [mutationError, setMutationError] = useState<string | null>(null);

  const routeTarget = useMemo(
    () => targetFromRoute(route.vaultPage, routeNote, notes),
    [notes, route.vaultPage, routeNote],
  );
  const [focusedTab, setFocusedTab] = useState<WorkspaceTab | null>(routeTarget);
  const [dirtyResources, setDirtyResources] = useState<Set<string>>(() => new Set());
  const activeNotePath = focusedTab?.kind === "note" ? focusedTab.path ?? null : null;

  useEffect(() => {
    if (routeTarget) setFocusedTab(routeTarget);
  }, [routeTarget]);

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
    setFocusedTab(tab);
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
      setFocusedTab(null);
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
      queryClient.setQueryData(qk.vaultNote(activeVaultId, path), {
        path,
        title,
        markdown,
        frontmatter: { title },
        linkedNotes: [],
      });
      setNewNoteFolder(null);
      openTab(noteTab(path, title));
      const document = await putVaultNote(activeVaultId, path, { markdown, createOnly: true });
      queryClient.setQueryData(qk.vaultNote(activeVaultId, document.path), document);
      await invalidateVault(activeVaultId);
      openTab(noteTab(document.path, document.title));
    } catch (error) {
      setMutationError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const handleRename = async (newPath: string) => {
    if (!activeVaultId || !renameEntry) return;
    const resourceId = `note:${renameEntry.path}`;
    if (dirtyResources.has(resourceId) && !window.confirm("Discard unsaved changes and rename this note?")) return;
    setBusy(true);
    setMutationError(null);
    try {
      const document = await putVaultNote(activeVaultId, renameEntry.path, { newPath });
      await invalidateVault(activeVaultId);
      setDirtyResources((current) => { const next = new Set(current); next.delete(resourceId); return next; });
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
    const resourceId = `note:${deleteEntry.path}`;
    if (dirtyResources.has(resourceId) && !window.confirm("Discard unsaved changes and delete this note?")) return;
    setBusy(true);
    setMutationError(null);
    try {
      await deleteVaultNote(activeVaultId, deleteEntry.path);
      await invalidateVault(activeVaultId);
      setDirtyResources((current) => { const next = new Set(current); next.delete(resourceId); return next; });
      if (focusedTab?.id === `note:${deleteEntry.path}`) setFocusedTab(null);
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
          onSelectVault={(vaultId) => {
            if (dirtyResources.size > 0 && !window.confirm("Discard unsaved changes and switch vaults?")) return;
            setDirtyResources(new Set());
            setFocusedTab(null);
            void navigate({ to: "/vaults/$vaultId", params: { vaultId } });
          }}
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
            initialTab={routeTarget}
            onActivateTab={(tab) => { setFocusedTab(tab); navigateTab(tab); }}
            onCloseTab={(tab) => {
              setDirtyResources((current) => { const next = new Set(current); next.delete(tab.id); return next; });
              if (focusedTab?.id === tab.id) setFocusedTab(null);
            }}
            onOpenNote={(path, title) => openTab(noteTab(path, title))}
            onDirtyResourceChange={(resourceId, dirty) => setDirtyResources((current) => {
              const next = new Set(current);
              if (dirty) next.add(resourceId);
              else next.delete(resourceId);
              return next;
            })}
          />
        ) : (
          <div className="empty-state"><div className="empty-state-title">Create your first vault</div><div className="empty-state-msg">Connect a GitHub repository to begin.</div><button type="button" className="btn primary" onClick={() => setCreateVaultOpen(true)}>Create vault</button></div>
        )}
      </div>
      {createVaultOpen && <CreateVaultDialog busy={busy} error={mutationError} onClose={() => setCreateVaultOpen(false)} onCreate={handleCreateVault} />}
      {newNoteFolder !== null && <NewNoteDialog folder={newNoteFolder || null} notes={notes} busy={busy} error={mutationError} onClose={() => setNewNoteFolder(null)} onCreate={handleCreateNote} />}
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

function yamlString(value: string): string {
  return JSON.stringify(value);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "Vault operation failed";
}
