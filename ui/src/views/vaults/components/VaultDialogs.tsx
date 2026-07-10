import { useState, type FormEvent } from "react";
import { Icon } from "../../../components/Icon";
import type { CreateVaultBody } from "../../../types";

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
  busy,
  error,
  onClose,
  onCreate,
}: {
  folder: string | null;
  busy: boolean;
  error: string | null;
  onClose: () => void;
  onCreate: (path: string, title: string) => Promise<void>;
}) {
  const [title, setTitle] = useState("");
  const [path, setPath] = useState(folder ? `${folder}/` : "");

  const submit = (event: FormEvent) => {
    event.preventDefault();
    let target = path.trim().replace(/^\/+/, "");
    if (!target.endsWith(".md")) target += ".md";
    void onCreate(target, title.trim() || target.split("/").pop()!.replace(/\.md$/, ""));
  };

  return (
    <DialogShell title="New note" icon="file" onClose={onClose}>
      <form onSubmit={submit}>
        <div className="ol-dialog-body vault-form">
          <label><span>Title</span><input autoFocus value={title} onChange={(event) => setTitle(event.target.value)} placeholder="Untitled" /></label>
          <label><span>File path</span><input value={path} onChange={(event) => setPath(event.target.value)} placeholder="notes/untitled.md" required /></label>
          {error && <div className="vault-form-error" role="alert">{error}</div>}
        </div>
        <div className="ol-dialog-foot">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy || !path.trim()}>{busy ? "Creating…" : "Create note"}</button>
        </div>
      </form>
    </DialogShell>
  );
}
