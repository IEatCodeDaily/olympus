// VaultsView — the Vaults surface shell.
//
// Owns the left sidebar (vault picker + notes tree + views toggle), the
// viewport layout (tabbed note editor), and the right sidebar (vault agent).
// The viewport pages live under views/vaults/pages/*.

import type { ReactNode } from "react";
import { useNavigate, useRouterState } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { Icon } from "../components/Icon";
import { useVaults, useVaultNotes, qk } from "../hooks/queries";
import { createVault } from "../api";
import type { NoteTreeEntry } from "../types";
import { NotePage } from "./vaults/pages/NotePage";
import { TablesPage } from "./vaults/pages/TablesPage";
import { GraphPage } from "./vaults/pages/GraphPage";
import { parseRoute } from "../router";

export function VaultsView() {
  const { location } = useRouterState();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const { vaultId: routeVaultId, vaultPage } = parseRoute(location.pathname);
  const notePath = new URLSearchParams(location.search).get("note");

  const { data: vaultsData } = useVaults();
  const vaults = vaultsData?.vaults ?? [];
  const activeVaultId = routeVaultId ?? vaults[0]?.id ?? null;
  const { data: notesData } = useVaultNotes(activeVaultId);
  const notes = notesData?.notes ?? [];

  // Sync note URL to note page for /vaults/$vaultId (note editor)
  const activeNotePath = vaultPage === "note" ? notePath ?? firstNotePath(notes) : null;

  const handleSelectVault = (id: string) => {
    void navigate({ to: "/vaults/$vaultId", params: { vaultId: id } });
  };

  const handleSelectPage = (page: "note" | "tables" | "graph") => {
    if (!activeVaultId) return;
    const to =
      page === "tables"
        ? "/vaults/$vaultId/tables"
        : page === "graph"
          ? "/vaults/$vaultId/graph"
          : "/vaults/$vaultId";
    void navigate({
      to,
      params: { vaultId: activeVaultId },
      search: page === "note" && activeNotePath ? { note: activeNotePath } : undefined,
    });
  };

  const handleOpenNote = (path: string) => {
    if (!activeVaultId) return;
    void navigate({
      to: "/vaults/$vaultId",
      params: { vaultId: activeVaultId },
      search: { note: path },
    });
  };

  const handleNewVault = async () => {
    const name = window.prompt("Vault name", "New vault");
    if (!name) return;
    const vault = await createVault(name);
    await qc.invalidateQueries({ queryKey: qk.vaults() });
    void navigate({ to: "/vaults/$vaultId", params: { vaultId: vault.id } });
  };

  return (
    <div className="view on" data-view="vaults">
      <VaultSidebar
        vaults={vaults}
        activeVaultId={activeVaultId}
        notes={notes}
        activeNotePath={activeNotePath}
        onSelectVault={handleSelectVault}
        onSelectPage={handleSelectPage}
        onOpenNote={handleOpenNote}
        onNewVault={handleNewVault}
        activePage={vaultPage}
      />
      <div className="gv-wrap">
        {vaultPage === "note" ? (
          <NotePage
            vaultId={activeVaultId ?? ""}
            notePath={activeNotePath}
            onNavigateNote={handleOpenNote}
          />
        ) : vaultPage === "tables" ? (
          <TablesPage vaultId={activeVaultId ?? ""} />
        ) : (
          <GraphPage vaultId={activeVaultId ?? ""} />
        )}
        <VaultAgentPanel vaultId={activeVaultId} />
      </div>
    </div>
  );
}

function VaultSidebar({
  vaults,
  activeVaultId,
  notes,
  activeNotePath,
  onSelectVault,
  onSelectPage,
  onOpenNote,
  onNewVault,
  activePage,
}: {
  vaults: { id: string; name: string; noteCount: number; updatedAt: number }[];
  activeVaultId: string | null;
  notes: NoteTreeEntry[];
  activeNotePath: string | null;
  onSelectVault: (id: string) => void;
  onSelectPage: (page: "note" | "tables" | "graph") => void;
  onOpenNote: (path: string) => void;
  onNewVault: () => void | Promise<void>;
  activePage: "note" | "tables" | "graph";
}) {
  return (
    <aside className="sidebar on">
      <div className="sbv on">
        <div className="sb-pad">
          <button type="button" className="newbtn" onClick={() => void onNewVault()}>
            <Icon name="plus" size={14} />
            New vault
          </button>
        </div>

        <div className="vsel" title="Switch vault">
          <Icon name="book" size={14} />
          <span className="vn">{vaults.find((v) => v.id === activeVaultId)?.name ?? "No vault"}</span>
          <span className="vc">{vaults.find((v) => v.id === activeVaultId)?.noteCount ?? 0}</span>
          <Icon name="chevron-down" size={12} />
        </div>

        <div className="sec-head">
          <span className="lbl">VAULTS</span>
          <span className="sp" />
          <span className="ct">{vaults.length}</span>
        </div>
        <div className="sec-content">
          {vaults.map((vault) => (
            <button
              key={vault.id}
              type="button"
              className={`srow ${vault.id === activeVaultId ? "on" : ""}`}
              onClick={() => onSelectVault(vault.id)}
            >
              <span className="dot" style={{ background: "var(--silver)" }} />
              <div className="info">
                <span className="title">{vault.name}</span>
                <div className="meta">
                  <span>{vault.noteCount}</span>
                </div>
              </div>
            </button>
          ))}
        </div>

        <div className="sec-head">
          <span className="lbl">NOTES</span>
          <span className="sp" />
          <span className="ct">{vaults.find((v) => v.id === activeVaultId)?.noteCount ?? notes.length}</span>
        </div>

        <div className="sec-content">
          {notes.map((entry) => renderEntry(entry, activeNotePath, onOpenNote))}
        </div>

        <div className="sec-head">
          <span className="lbl">VIEWS</span>
        </div>
        <div className="sec-content">
          <button
            type="button"
            className={`navitem ${activePage === "graph" ? "on" : ""}`}
            onClick={() => onSelectPage("graph")}
          >
            <Icon name="workflow" size={14} />
            <span>Graph</span>
          </button>
          <button
            type="button"
            className={`navitem ${activePage === "tables" ? "on" : ""}`}
            onClick={() => onSelectPage("tables")}
          >
            <Icon name="layout-grid" size={14} />
            <span>Tables</span>
          </button>
        </div>
      </div>
    </aside>
  );
}

function renderEntry(
  entry: NoteTreeEntry,
  activeNotePath: string | null,
  onOpenNote: (path: string) => void,
  depth = 0,
): ReactNode {
  const isFolder = entry.kind === "folder";
  return (
    <div key={entry.path} style={{ paddingLeft: depth * 14 }}>
      <button
        type="button"
        className={`srow ${!isFolder && activeNotePath === entry.path ? "on" : ""}`}
        onClick={() => {
          if (isFolder) return;
          onOpenNote(entry.path);
        }}
        style={{ width: "100%" }}
      >
        <span className="sic" style={{ color: "var(--faint)" }}>
          <Icon name={isFolder ? "folder" : "file"} size={12} />
        </span>
        <div className="info">
          <span className="title">{entry.title}</span>
          <div className="meta">
            <span>{timeAgo(entry.updatedAt)}</span>
          </div>
        </div>
      </button>
      {isFolder && entry.children.length > 0 && (
        <div style={{ display: "flex", flexDirection: "column", gap: 1 }}>
          {entry.children.map((child) => renderEntry(child, activeNotePath, onOpenNote, depth + 1))}
        </div>
      )}
    </div>
  );
}

function VaultAgentPanel({ vaultId }: { vaultId: string | null }) {
  return (
    <aside className="rsidebar" style={{ width: 250 }}>
      <div className="rs-sec">
        <div className="gk" style={{ marginBottom: 2 }}>
          vault agent
        </div>
        <div className="kv">
          <span className="k">AGENT</span>
          <span className="v">claude-code</span>
        </div>
        <div className="kv">
          <span className="k">MODEL</span>
          <span className="v">sonnet-4.5</span>
        </div>
        <div className="kv">
          <span className="k">SCOPE</span>
          <span className="v">{vaultId ?? "—"}</span>
        </div>
      </div>
      <div className="rs-sec">
        <div className="gk">recent activity</div>
        <div className="mono" style={{ fontSize: 10.5, color: "var(--dim)", display: "flex", flexDirection: "column", gap: 5 }}>
          <span>14:20 · updated redb-compaction.md</span>
          <span>13:44 · linked acp-wire-spike.md</span>
          <span>Yesterday · created zstd-tuning.md</span>
        </div>
      </div>
      <div className="rs-sec">
        <div className="gk">related notes</div>
        <div className="art">
          <Icon name="file" size={14} />
          <span className="nm">event-log-design.md</span>
        </div>
        <div className="art">
          <Icon name="file" size={14} />
          <span className="nm">zstd-tuning.md</span>
        </div>
      </div>
      <div style={{ marginTop: "auto", padding: 10 }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "8px 10px",
            background: "var(--elev)",
            border: "var(--border-w) solid var(--border-strong)",
            borderRadius: "var(--radius)",
            color: "var(--faint)",
            fontSize: 12,
          }}
        >
          <Icon name="sparkles" size={14} />
          <span>Ask about this vault…</span>
        </div>
      </div>
    </aside>
  );
}

function firstNotePath(notes: NoteTreeEntry[]): string | null {
  for (const n of notes) {
    if (n.kind === "note") return n.path;
    const child = firstNotePath(n.children);
    if (child) return child;
  }
  return null;
}

function timeAgo(ts: number): string {
  const diff = Date.now() / 1000 - ts;
  if (diff < 60) return "now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}
