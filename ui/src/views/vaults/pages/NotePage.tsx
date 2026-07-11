// NotePage — an always-editable, full-pane Vault note surface.

import { lazy, Suspense, useEffect, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { Icon } from "../../../components/Icon";
import { deleteVaultNote, putVaultNote } from "../../../api";
import { qk, useVaultNote } from "../../../hooks/queries";
import { collectVaultSuggestions } from "../editor/vaultMarkdown";

const VaultMarkdownEditor = lazy(() =>
  import("../editor/VaultMarkdownEditor").then((module) => ({
    default: module.VaultMarkdownEditor,
  })),
);

interface NotePageProps {
  vaultId: string;
  notePath: string | null;
  onNavigateNote: (path: string) => void;
  onDirtyChange: (dirty: boolean) => void;
}

export function NotePage({ vaultId, notePath, onDirtyChange }: NotePageProps) {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const { data: note, isLoading, error } = useVaultNote(vaultId, notePath);
  const [draft, setDraft] = useState("");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    if (!note) return;
    setDraft(note.markdown);
    setDirty(false);
    onDirtyChange(false);
    setSaveError(null);
  }, [note?.path, vaultId]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!notePath) {
    return (
      <div className="vault-content">
        <div className="empty-state">
          <div className="empty-state-icon"><Icon name="file" size={32} /></div>
          <div className="empty-state-title">No note selected</div>
          <div className="empty-state-msg">Pick a note from the sidebar, or create a new one.</div>
        </div>
      </div>
    );
  }

  if (isLoading) {
    return <div className="vault-content vault-note-surface"><div className="vault-editor-loading">Loading note…</div></div>;
  }

  if (error || !note) {
    return (
      <div className="vault-content">
        <div className="empty-state">
          <div className="empty-state-icon"><Icon name="alert" size={32} /></div>
          <div className="empty-state-title">Note not found</div>
          <div className="empty-state-msg">{notePath}</div>
        </div>
      </div>
    );
  }

  const handleSave = async () => {
    setSaving(true);
    setSaveError(null);
    try {
      await putVaultNote(vaultId, notePath, { markdown: draft });
      await qc.invalidateQueries({ queryKey: qk.vaultNote(vaultId, notePath) });
      await qc.invalidateQueries({ queryKey: qk.vaultNotes(vaultId) });
      setDirty(false);
      onDirtyChange(false);
    } catch (saveFailure) {
      setSaveError(saveFailure instanceof Error ? saveFailure.message : "Save failed");
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    try {
      await deleteVaultNote(vaultId, notePath);
      await qc.invalidateQueries({ queryKey: qk.vaultNotes(vaultId) });
      void navigate({ to: "/vaults/$vaultId", params: { vaultId } });
    } catch {
      // The surrounding Vault query state remains authoritative on failure.
    }
  };

  return (
    <div className="vault-content vault-note-surface">
      <Suspense fallback={<div className="vault-editor-loading">Loading editor…</div>}>
        <VaultMarkdownEditor
          key={`${vaultId}:${notePath}`}
          markdown={draft}
          suggestions={collectVaultSuggestions(draft, note.linkedNotes)}
          dirty={dirty}
          saving={saving}
          saveError={saveError}
          onSave={handleSave}
          onDelete={handleDelete}
          onChange={(markdown) => {
            const nextDirty = markdown !== note.markdown;
            setDraft(markdown);
            setDirty(nextDirty);
            onDirtyChange(nextDirty);
          }}
        />
      </Suspense>
    </div>
  );
}
