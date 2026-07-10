import { useMemo, useState } from "react";
import { Icon } from "../../../components/Icon";
import { useVaultDocuments } from "../../../hooks/queries";
import type { NoteIndexEntry } from "../../../types";
import { deriveFrontmatterColumns } from "../vaultWorkspace";

const EMPTY_DOCUMENTS: NoteIndexEntry[] = [];

export function VaultTablePage({
  vaultId,
  onOpenNote,
}: {
  vaultId: string;
  onOpenNote: (path: string, title?: string) => void;
}) {
  const { data, isLoading, error } = useVaultDocuments(vaultId || null);
  const [sort, setSort] = useState("title");
  const [descending, setDescending] = useState(false);
  const documents = data?.documents ?? EMPTY_DOCUMENTS;
  const frontmatterColumns = useMemo(
    () => deriveFrontmatterColumns(documents),
    [documents],
  );
  const columns = ["title", "path", ...frontmatterColumns];
  const rows = useMemo(() => {
    const next = [...documents];
    next.sort((left, right) => {
      const leftValue = sort === "title" || sort === "path" ? left[sort] : left.frontmatter[sort];
      const rightValue = sort === "title" || sort === "path" ? right[sort] : right.frontmatter[sort];
      const comparison = displayValue(leftValue).localeCompare(displayValue(rightValue), undefined, { numeric: true });
      return descending ? -comparison : comparison;
    });
    return next;
  }, [descending, documents, sort]);

  if (!vaultId || isLoading || error || documents.length === 0) {
    return (
      <div className="vault-content vault-table-empty">
        <div className="empty-state">
          <div className="empty-state-icon"><Icon name="layout-grid" size={32} /></div>
          <div className="empty-state-title">{!vaultId ? "No vault selected" : isLoading ? "Loading notes…" : error ? "Could not load notes" : "No notes"}</div>
          {!isLoading && !error && vaultId && <div className="empty-state-msg">Create a note to populate the vault table.</div>}
        </div>
      </div>
    );
  }

  return (
    <div className="vault-content vault-table-view">
      <div className="vault-view-header">
        <div><span className="gk">Table View</span><strong>{documents.length} notes</strong></div>
        <span>Frontmatter columns are derived from canonical Markdown.</span>
      </div>
      <div className="vault-table-scroll">
        <table className="hist-table vault-document-table">
          <thead><tr>{columns.map((column) => <th key={column}><button type="button" onClick={() => { if (sort === column) setDescending((value) => !value); else { setSort(column); setDescending(false); } }}>{column}{sort === column ? descending ? " ↓" : " ↑" : ""}</button></th>)}</tr></thead>
          <tbody>{rows.map((document) => (
            <tr key={document.path} className="hist-row" onDoubleClick={() => onOpenNote(document.path, document.title)}>
              {columns.map((column) => (
                <td key={column}>
                  {column === "title" ? <button type="button" className="vault-table-note" onClick={() => onOpenNote(document.path, document.title)}><Icon name="file" size={12} />{document.title}</button> : displayValue(column === "path" ? document.path : document.frontmatter[column])}
                </td>
              ))}
            </tr>
          ))}</tbody>
        </table>
      </div>
    </div>
  );
}

function displayValue(value: unknown): string {
  if (value === null || value === undefined || value === "") return "—";
  if (Array.isArray(value)) return value.map(displayValue).join(", ");
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}
