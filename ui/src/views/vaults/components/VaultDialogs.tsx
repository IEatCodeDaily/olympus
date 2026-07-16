import { useMemo, useState, type FormEvent } from "react";
import { Icon } from "../../../components/Icon";
import type { CreateVaultBody, NoteTreeEntry } from "../../../types";

interface DialogShellProps {
  title: string;
  icon: "book" | "file";
  children: React.ReactNode;
  onClose: () => void;
}

function DialogShell({ title, icon, children, onClose }: DialogShellProps) {
  return (
    <div className="ol-overlay" role="dialog" aria-modal="true" aria-label={title} onClick={onClose}>
      <div className="ol-dialog vault-dialog" onClick={(event) => event.stopPropagation()}>
        <div className="ol-dialog-head">
          <div className="vault-dialog-title"><Icon name={icon} size={18} /><span>{title}</span></div>
          <button type="button" className="ibtn" aria-label="Close" onClick={onClose}>
            <Icon name="x" size={14} />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

export function CreateVaultDialog({
  busy,
  error,
  onClose,
  onCreate,
}: {
  busy: boolean;
  error: string | null;
  onClose: () => void;
  onCreate: (body: CreateVaultBody) => Promise<void>;
}) {
  const [name, setName] = useState("");
  const [repository, setRepository] = useState("");
  const [branch, setBranch] = useState("main");

  const submit = (event: FormEvent) => {
    event.preventDefault();
    void onCreate({
      name: name.trim(),
      backend: {
        kind: "github",
        repository: repository.trim(),
        branch: branch.trim(),
        syncEngine: "jj-git",
      },
    });
  };

  return (
    <DialogShell title="Create vault" icon="book" onClose={onClose}>
      <form onSubmit={submit}>
        <div className="ol-dialog-body vault-form">
          <label><span>Vault name</span><input autoFocus value={name} onChange={(event) => setName(event.target.value)} placeholder="Engineering" required /></label>
          <label><span>Backend store</span><select value="github" disabled><option value="github">GitHub repository</option></select></label>
          <label><span>Repository</span><input value={repository} onChange={(event) => setRepository(event.target.value)} placeholder="owner/repository" pattern="[^/\s]+/[^/\s]+" required /></label>
          <label><span>Default branch</span><input value={branch} onChange={(event) => setBranch(event.target.value)} placeholder="main" required /></label>
          <div className="vault-form-note">Hall creates a local jj working copy and configures this existing GitHub repository as its durable remote. Credentials stay in Hall's Git environment.</div>
          {error && <div className="vault-form-error" role="alert">{error}</div>}
        </div>
        <div className="ol-dialog-foot">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy || !name.trim() || !repository.trim() || !branch.trim()}>{busy ? "Creating…" : "Create vault"}</button>
        </div>
      </form>
    </DialogShell>
  );
}

export function NewNoteDialog({
  folder,
  notes = [],
  busy,
  error,
  onClose,
  onCreate,
}: {
  folder: string | null;
  notes?: NoteTreeEntry[];
  busy: boolean;
  error: string | null;
  onClose: () => void;
  onCreate: (path: string, title: string) => Promise<void>;
}) {
  const folders = useMemo(() => collectFolders(notes), [notes]);
  const initialFolder = folder && (folder === "" || folders.some((entry) => entry.path === folder)) ? folder : "";
  const [title, setTitle] = useState("");
  const [destination, setDestination] = useState(initialFolder);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const noteTitle = title.trim() || "Untitled";
    const file = `${slugify(noteTitle)}.md`;
    void onCreate(destination ? `${destination}/${file}` : file, noteTitle);
  };

  const preview = destination ? `${destination}/${slugify(title || "Untitled")}.md` : `${slugify(title || "Untitled")}.md`;

  return (
    <DialogShell title="New note" icon="file" onClose={onClose}>
      <form onSubmit={submit}>
        <div className="ol-dialog-body vault-form">
          <label><span>Title</span><input autoFocus value={title} onChange={(event) => setTitle(event.target.value)} placeholder="Untitled" /></label>
          <div className="vault-folder-picker">
            <div className="vault-folder-picker-head"><span>Destination</span><code>{preview}</code></div>
            <div className="vault-folder-tree" role="tree" aria-label="Destination folder">
              <button type="button" role="treeitem" aria-pressed={destination === ""} className="vault-folder-option" onClick={() => setDestination("")}><Icon name="book" size={13} />Vault root</button>
              {folders.map((entry) => (
                <button key={entry.path} type="button" role="treeitem" aria-pressed={destination === entry.path} className="vault-folder-option" style={{ paddingLeft: 10 + entry.depth * 14 }} onClick={() => setDestination(entry.path)}><Icon name="folder" size={13} />{entry.title}</button>
              ))}
            </div>
          </div>
          {error && <div className="vault-form-error" role="alert">{error}</div>}
        </div>
        <div className="ol-dialog-foot">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy}>{busy ? "Saving…" : "Create note"}</button>
        </div>
      </form>
    </DialogShell>
  );
}

function collectFolders(notes: NoteTreeEntry[], depth = 0): Array<NoteTreeEntry & { depth: number }> {
  return notes.flatMap((entry) => entry.kind === "folder"
    ? [{ ...entry, depth }, ...collectFolders(entry.children, depth + 1)]
    : []);
}

function slugify(value: string): string {
  return value.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "untitled";
}
