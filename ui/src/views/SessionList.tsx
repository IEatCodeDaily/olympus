import { useState, useEffect } from "react";
import { useSessions } from "../hooks/useSessions";
import { connectWs, createSession } from "../api";
import type { Session, SessionSort } from "../types";
import { relativeTime, formatTokens, SOURCE_META, ALL_SOURCES } from "../lib/format";

interface Props {
  selectedId: string | null;
  onOpenSession: (id: string) => void;
}

const SORT_OPTIONS: { value: SessionSort; label: string }[] = [
  { value: "lastActivity", label: "Last Activity" },
  { value: "startedAt", label: "Started" },
  { value: "messageCount", label: "Messages" },
];

export default function SessionList({ selectedId, onOpenSession }: Props) {
  const [selectedSources, setSelectedSources] = useState<Set<string>>(new Set());
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SessionSort>("lastActivity");
  const [showArchived, setShowArchived] = useState(false);
  const searchDebounced = useDebounce(search, 250);

  useEffect(() => {
    connectWs();
  }, []);

  const sourceParam = selectedSources.size > 0 ? Array.from(selectedSources) : undefined;

  const { sessions, loading, error } = useSessions({
    source: sourceParam,
    archived: showArchived ? true : false,
    q: searchDebounced || undefined,
    sort,
  });

  const toggleSource = (src: string) => {
    setSelectedSources((prev) => {
      const next = new Set(prev);
      if (next.has(src)) next.delete(src);
      else next.add(src);
      return next;
    });
  };

  const [creating, setCreating] = useState(false);
  const [createErr, setCreateErr] = useState<string | null>(null);

  const handleNewChat = async () => {
    if (creating) return;
    setCreating(true);
    setCreateErr(null);
    try {
      const session = await createSession();
      onOpenSession(session.id);
    } catch (e) {
      setCreateErr(String(e));
    } finally {
      setCreating(false);
    }
  };

  return (
    <div className="session-list">
      {/* Toolbar */}
      <div className="toolbar">
        <div className="toolbar-row">
          <button
            className="new-chat-btn"
            onClick={handleNewChat}
            disabled={creating}
            title="Start a new Olympus-managed chat"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M12 5v14M5 12h14" />
            </svg>
            {creating ? "Starting…" : "New Chat"}
          </button>
          <div className="search-box">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="11" cy="11" r="8" />
              <path d="m21 21-4.35-4.35" />
            </svg>
            <input
              type="text"
              placeholder="Search sessions..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
          <div className="toolbar-controls">
            <select value={sort} onChange={(e) => setSort(e.target.value as SessionSort)} className="sort-select">
              {SORT_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>{o.label}</option>
              ))}
            </select>
            <label className="archive-toggle">
              <input
                type="checkbox"
                checked={showArchived}
                onChange={(e) => setShowArchived(e.target.checked)}
              />
              <span>Archived</span>
            </label>
          </div>
        </div>
        {/* Source filter pills */}
        <div className="source-filters">
          {ALL_SOURCES.map((src) => {
            const meta = SOURCE_META[src];
            const active = selectedSources.has(src);
            return (
              <button
                key={src}
                className={`source-pill ${active ? "active" : ""}`}
                style={active ? { borderColor: meta.color, color: meta.color, background: meta.glow } : {}}
                onClick={() => toggleSource(src)}
              >
                {meta.label}
              </button>
            );
          })}
          {selectedSources.size > 0 && (
            <button className="source-pill clear" onClick={() => setSelectedSources(new Set())}>
              Clear
            </button>
          )}
        </div>
      </div>

      {/* List — plain render (no virtualizer). ~29 mock sessions is trivial for the DOM. */}
      <div className="list-scroll">
        {createErr && <div className="list-error">{createErr}</div>}
        {loading && sessions.length === 0 && <SessionListSkeleton />}
        {error && <div className="list-error">{error}</div>}
        {!loading && sessions.length === 0 && !error && (
          <div className="list-empty">No sessions match your filters.</div>
        )}
        {sessions.map((session) => (
          <div
            key={session.id}
            className={`session-row ${selectedId === session.id ? "selected" : ""}`}
            data-session-id={session.id}
            onClick={() => onOpenSession(session.id)}
          >
            <SessionRowContent session={session} />
          </div>
        ))}
      </div>
      {/* Footer summary */}
      <div className="list-footer">
        {sessions.length} session{sessions.length !== 1 ? "s" : ""}
      </div>
    </div>
  );
}

function SessionRowContent({ session }: { session: Session }) {
  const meta = SOURCE_META[session.source];
  const title = session.title ?? "(no title)";

  return (
    <>
      <div className="row-source" style={{ background: meta.color }} title={meta.label} />
      <div className="row-body">
        <div className="row-line1">
          <span className="row-title">{title}</span>
          {session.managed && <span className="row-managed" title="Olympus-managed">M</span>}
          {session.forkedFrom && <span className="row-fork" title={`Forked from ${session.forkedFrom}`}>fork</span>}
        </div>
        <div className="row-line2">
          <span className="row-source-label" style={{ color: meta.color }}>{meta.label}</span>
          {session.model && <span className="row-model">{session.model}</span>}
          <span className="row-msgs">{session.messageCount} msg</span>
          <span className="row-tokens">{formatTokens(session.inputTokens + session.outputTokens)} tok</span>
        </div>
      </div>
      <div className="row-time">{relativeTime(session.lastActivity)}</div>
    </>
  );
}

function SessionListSkeleton() {
  return (
    <div className="session-skeleton" aria-label="Loading sessions">
      {Array.from({ length: 8 }).map((_, i) => (
        <div key={i} className="skel-row">
          <div className="skel-rail" />
          <div className="skel-row-body">
            <div className="skel-line" style={{ width: `${55 + ((i * 7) % 35)}%` }} />
            <div className="skel-line skel-line-sm" style={{ width: `${35 + ((i * 5) % 25)}%` }} />
          </div>
        </div>
      ))}
    </div>
  );
}

function useDebounce(value: string, delay: number): string {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setDebounced(value), delay);
    return () => clearTimeout(t);
  }, [value, delay]);
  return debounced;
}
