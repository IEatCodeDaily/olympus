import { useState } from "react";
import { Icon } from "../../../components/Icon";

function Shell({ title, children, onClose }: { title: string; children: React.ReactNode; onClose: () => void }) {
  return <div className="ol-overlay" role="dialog" aria-modal="true" aria-label={title} onClick={onClose}><div className="ol-dialog vault-dialog" onClick={(event) => event.stopPropagation()}><div className="ol-dialog-head"><div className="vault-dialog-title"><Icon name="file" size={18} /><span>{title}</span></div><button type="button" className="ibtn" aria-label="Close" onClick={onClose}><Icon name="x" size={14} /></button></div>{children}</div></div>;
}

export function RenameNoteDialog({ currentPath, busy, error, onClose, onRename }: { currentPath: string; busy: boolean; error: string | null; onClose: () => void; onRename: (path: string) => Promise<void> }) {
  const [path, setPath] = useState(currentPath);
  return <Shell title="Rename note" onClose={onClose}><form onSubmit={(event) => { event.preventDefault(); void onRename(path.trim()); }}><div className="ol-dialog-body vault-form"><label><span>File path</span><input autoFocus value={path} onChange={(event) => setPath(event.target.value)} required /></label>{error && <div className="vault-form-error" role="alert">{error}</div>}</div><div className="ol-dialog-foot"><button type="button" className="btn" onClick={onClose}>Cancel</button><button type="submit" className="btn primary" disabled={busy || !path.trim() || path.trim() === currentPath}>{busy ? "Renaming…" : "Rename"}</button></div></form></Shell>;
}

export function DeleteNoteDialog({ path, busy, error, onClose, onDelete }: { path: string | null; busy: boolean; error: string | null; onClose: () => void; onDelete: () => Promise<void> }) {
  if (!path) return null;
  return <Shell title="Delete note" onClose={onClose}><div className="ol-dialog-body">Delete <span className="mono">{path}</span>? jj history retains the previous version, but the file will disappear from the active vault.</div>{error && <div className="vault-form-error vault-dialog-error" role="alert">{error}</div>}<div className="ol-dialog-foot"><button type="button" className="btn" onClick={onClose}>Cancel</button><button type="button" className="btn danger" disabled={busy} onClick={() => void onDelete()}>{busy ? "Deleting…" : "Delete note"}</button></div></Shell>;
}
