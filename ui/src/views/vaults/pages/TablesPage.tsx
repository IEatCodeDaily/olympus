// TablesPage — vault collections / structured data view.
//
// Scans for notes with `collection: true` in frontmatter (the collection
// definition). Each collection's rows are child notes in the same folder
// with structured frontmatter fields. Rendered as a sortable table.

import { useState, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { Icon } from "../../../components/Icon";
import { apiFetch } from "../../../api";

interface CollectionSummary {
  name: string;
  path: string;
  rowCount: number;
}

interface CollectionData {
  columns: string[];
  rows: Record<string, unknown>[];
}

export function TablesPage({ vaultId }: { vaultId: string }) {
  const [selectedCollection, setSelectedCollection] = useState<string | null>(null);

  const { data: collectionsData } = useQuery({
    queryKey: ["vaultCollections", vaultId],
    queryFn: async () => {
      const res = await apiFetch(
        `/api/vaults/${vaultId}/collections`,
      );
      if (!res.ok) return { collections: [] };
      return res.json() as Promise<{ collections: CollectionSummary[] }>;
    },
    enabled: !!vaultId,
    staleTime: 10_000,
  });

  const collections = collectionsData?.collections ?? [];

  const { data: rowsData, isLoading } = useQuery({
    queryKey: ["collectionRows", vaultId, selectedCollection],
    queryFn: async () => {
      if (!selectedCollection) return null;
      const res = await apiFetch(
        `/api/vaults/${vaultId}/collections/${encodeURIComponent(selectedCollection)}`,
      );
      if (!res.ok) throw new Error(`collection ${res.status}`);
      return res.json() as Promise<CollectionData>;
    },
    enabled: !!vaultId && !!selectedCollection,
    staleTime: 5_000,
  });

  const [sortCol, setSortCol] = useState<string | null>(null);
  const [sortDir, setSortDir] = useState<"asc" | "desc">("asc");

  const sortedRows = useMemo(() => {
    if (!rowsData?.rows || !sortCol) return rowsData?.rows ?? [];
    const sorted = [...rowsData.rows];
    sorted.sort((a, b) => {
      const av = String(a[sortCol] ?? "");
      const bv = String(b[sortCol] ?? "");
      const cmp = av.localeCompare(bv);
      return sortDir === "asc" ? cmp : -cmp;
    });
    return sorted;
  }, [rowsData, sortCol, sortDir]);

  if (!vaultId) {
    return (
      <div className="vault-content">
        <div className="empty-state">
          <div className="empty-state-icon"><Icon name="layout-grid" size={32} /></div>
          <div className="empty-state-title">No vault selected</div>
        </div>
      </div>
    );
  }

  if (collections.length === 0) {
    return (
      <div className="vault-content">
        <div className="empty-state">
          <div className="empty-state-icon"><Icon name="layout-grid" size={32} /></div>
          <div className="empty-state-title">No collections</div>
          <div className="empty-state-msg">
            Create a note with <code style={{ fontFamily: "var(--font-mono)", color: "var(--silver)" }}>collection: true</code> in frontmatter
            to define a data collection.
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="vault-content" style={{ overflow: "auto" }}>
      {/* Collection picker */}
      <div className="hist-filters" style={{ padding: "0 0 12px" }}>
        <select
          className="hist-select"
          value={selectedCollection ?? ""}
          onChange={(e) => setSelectedCollection(e.target.value || null)}
          title="Collection"
        >
          <option value="">Select a collection…</option>
          {collections.map((c) => (
            <option key={c.path} value={c.path}>
              {c.name} ({c.rowCount})
            </option>
          ))}
        </select>
      </div>

      {/* Data table */}
      {!selectedCollection ? (
        <div className="empty-state">
          <div className="empty-state-msg">Pick a collection to view its rows.</div>
        </div>
      ) : isLoading ? (
        <div className="empty-state">
          <span className="gk">Loading…</span>
        </div>
      ) : !rowsData || sortedRows.length === 0 ? (
        <div className="empty-state">
          <div className="empty-state-title">Empty collection</div>
          <div className="empty-state-msg">No rows found in this collection.</div>
        </div>
      ) : (
        <div className="hist-table-wrap">
          <table className="hist-table">
            <thead>
              <tr>
                {(rowsData.columns.length > 0
                  ? ["title", ...rowsData.columns]
                  : ["title", "path"]
                ).map((col) => (
                  <th
                    key={col}
                    onClick={() => {
                      if (sortCol === col) {
                        setSortDir(sortDir === "asc" ? "desc" : "asc");
                      } else {
                        setSortCol(col);
                        setSortDir("asc");
                      }
                    }}
                    style={{ cursor: "pointer" }}
                  >
                    {col.toUpperCase()}
                    {sortCol === col && (sortDir === "asc" ? " ▲" : " ▼")}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {sortedRows.map((row, i) => (
                <tr key={i} className="hist-row">
                  {(rowsData.columns.length > 0
                    ? ["title", ...rowsData.columns]
                    : ["title", "path"]
                  ).map((col) => (
                    <td key={col} className="mono">
                      {String(row[col] ?? "—")}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
