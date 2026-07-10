// NotePage — the vault markdown note editor (viewport content).
//
// Renders a single note with a view/edit toggle:
//   - View: richly rendered markdown (react-markdown + remark-gfm).
//   - Edit: Milkdown rich editor with an exact CodeMirror source fallback.
// On save, serializes to .md via PUT /api/vaults/:id/note (V-BE handles the
// jj snapshot commit). Shows LINKED NOTES footer with clickable wikilinks.

import { lazy, Suspense, useState, useEffect } from "react";
import { useNavigate } from "@tanstack/react-router";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Icon } from "../../../components/Icon";
import { useVaultNote } from "../../../hooks/queries";
import { putVaultNote, deleteVaultNote } from "../../../api";
import { useQueryClient } from "@tanstack/react-query";
import { qk } from "../../../hooks/queries";
import { collectVaultSuggestions, splitVaultMarkdown } from "../editor/vaultMarkdown";

const VaultMarkdownEditor = lazy(() =>
  import("../editor/VaultMarkdownEditor").then((module) => ({
    default: module.VaultMarkdownEditor,
  })),
);

interface NotePageProps {
  vaultId: string;
  notePath: string | null;
  /** Called when the user clicks a wikilink to navigate to another note. */
  onNavigateNote: (path: string) => void;
}

export function NotePage({ vaultId, notePath, onNavigateNote }: NotePageProps) {
  const qc = useQueryClient();
  const { data: note, isLoading, error } = useVaultNote(vaultId, notePath);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const navigate = useNavigate();

  // Sync local draft when the note changes or loads
  useEffect(() => {
    if (note) {
      setDraft(note.markdown);
      setDirty(false);
      setEditing(false);
    }
  }, [note?.path, vaultId]); // eslint-disable-line react-hooks/exhaustive-deps

  // No note selected
  if (!notePath) {
    return (
      <div className="vault-content">
        <div className="empty-state">
          <div className="empty-state-icon">
            <Icon name="file" size={32} />
          </div>
          <div className="empty-state-title">No note selected</div>
          <div className="empty-state-msg">
            Pick a note from the sidebar, or create a new one.
          </div>
        </div>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="vault-content">
        <div className="vault-note-pane">
          <div className="grow" style={{ maxWidth: 680, marginBottom: 12 }}>
            <span className="gk">Loading…</span>
          </div>
          <div
            style={{
              height: 200,
              background: "var(--elev)",
              border: "var(--border-w) solid var(--border)",
              borderRadius: "var(--radius-md)",
            }}
          />
        </div>
      </div>
    );
  }

  if (error || !note) {
    return (
      <div className="vault-content">
        <div className="empty-state">
          <div className="empty-state-icon">
            <Icon name="alert" size={32} />
          </div>
          <div className="empty-state-title">Note not found</div>
          <div className="empty-state-msg">{notePath}</div>
        </div>
      </div>
    );
  }

  const handleStartEdit = () => {
    setEditing(true);
    setDraft(note.markdown);
  };

  const handleCancel = () => {
    setEditing(false);
    setDraft(note.markdown);
    setDirty(false);
  };

  const handleSave = async () => {
    setSaving(true);
    setSaveError(null);
    try {
      await putVaultNote(vaultId, notePath, { markdown: draft });
      await qc.invalidateQueries({ queryKey: qk.vaultNote(vaultId, notePath) });
      await qc.invalidateQueries({ queryKey: qk.vaultNotes(vaultId) });
      setDirty(false);
      setEditing(false);
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : "Save failed");
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
      // ignore
    }
  };

  const fileName = notePath.split("/").pop() ?? notePath;
  const suggestions = collectVaultSuggestions(draft, note.linkedNotes);

  return (
    <div className="vault-content">
      <div className="vault-note-pane">
        {/* Edit bar */}
        <div className="grow" style={{ maxWidth: 680, marginBottom: 12 }}>
          <span className="gk" data-testid="vnotename">
            {fileName}
          </span>
          <span style={{ display: "flex", gap: 6, marginLeft: "auto" }}>
            {editing ? (
              <>
                <button
                  type="button"
                  className="btn pri"
                  onClick={handleSave}
                  disabled={saving || !dirty}
                  data-testid="vsave"
                >
                  {saving ? "Saving…" : "Save"}
                </button>
                <button
                  type="button"
                  className="btn"
                  onClick={handleCancel}
                  disabled={saving}
                  data-testid="vcancel"
                >
                  Cancel
                </button>
              </>
            ) : (
              <>
                <button type="button" className="btn" onClick={handleStartEdit} data-testid="vedit">
                  Edit
                </button>
                <button type="button" className="btn" onClick={handleDelete} data-testid="vdelete">
                  Delete
                </button>
              </>
            )}
          </span>
        </div>

        {saveError && (
          <div
            style={{
              maxWidth: 680,
              padding: "8px 12px",
              background: "var(--err-wash)",
              border: "var(--border-w) solid var(--err-line)",
              borderRadius: "var(--radius)",
              color: "var(--err)",
              fontSize: "var(--fs-12)",
            }}
          >
            {saveError}
          </div>
        )}

        {/* Editor or rendered markdown */}
        {editing ? (
          <Suspense fallback={<div className="vault-editor-loading">Loading editor…</div>}>
            <VaultMarkdownEditor
              key={`${vaultId}:${notePath}`}
              markdown={draft}
              suggestions={suggestions}
              onChange={(markdown) => {
                setDraft(markdown);
                setDirty(markdown !== note.markdown);
              }}
            />
          </Suspense>
        ) : (
          <div className="md" data-testid="mdbody" style={{ maxWidth: 680 }}>
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                // Intercept markdown links to handle internal note links
                a: ({ href, children }) => {
                  const isNoteLink =
                    href?.endsWith(".md") || href?.startsWith("#note:");
                  if (isNoteLink && href) {
                    const path = href.replace(/^#note:/, "");
                    return (
                      <a
                        className="vault-link-pill"
                        href={href}
                        onClick={(e) => {
                          e.preventDefault();
                          onNavigateNote(path);
                        }}
                      >
                        {children}
                      </a>
                    );
                  }
                  return <a href={href}>{children}</a>;
                },
              }}
            >
              {stripFrontmatter(note.markdown)}
            </ReactMarkdown>
          </div>
        )}

        {/* LINKED NOTES footer */}
        {note.linkedNotes.length > 0 && !editing && (
          <div className="vault-linked" style={{ maxWidth: 680 }}>
            <h2 className="md" style={{ marginBottom: "var(--space-3)" }}>
              Linked notes
            </h2>
            <p style={{ color: "var(--dim)", fontSize: "var(--fs-13)" }}>
              {note.linkedNotes.map((link, i) => (
                <span key={link}>
                  {i > 0 && <span style={{ color: "var(--faint)" }}> · </span>}
                  <a
                    className="vault-link-pill"
                    href={`?note=${encodeURIComponent(resolveLink(notePath, link))}`}
                    style={{ color: "var(--silver)", cursor: "pointer" }}
                    onClick={(e) => {
                      e.preventDefault();
                      onNavigateNote(resolveLink(notePath, link));
                    }}
                  >
                    {link.replace(/\.md$/, "")}
                  </a>
                </span>
              ))}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}

/** Strip YAML frontmatter (---\n...\n---) from markdown for rendering. */
function stripFrontmatter(md: string): string {
  return splitVaultMarkdown(md).body;
}

/**
 * Resolve a wikilink target relative to the current note's directory.
 * `[[event-log-design.md]]` from `redb/redb-compaction.md` → `redb/event-log-design.md`.
 * A bare filename resolves to the same directory; a path with `/` is used as-is.
 */
function resolveLink(currentPath: string, link: string): string {
  if (link.includes("/")) return link;
  const dir = currentPath.includes("/")
    ? currentPath.slice(0, currentPath.lastIndexOf("/") + 1)
    : "";
  return dir + link;
}
