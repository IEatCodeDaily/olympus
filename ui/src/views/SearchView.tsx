import { useState, useMemo } from "react";
import { searchSessions } from "../api";
import { useSessions } from "../hooks/useSessions";
import { relativeTime, SOURCE_META } from "../lib/format";
import type { SearchHit } from "../types";

interface Props {
  onOpenSession: (id: string) => void;
}

interface GroupedHits {
  sessionId: string;
  source: string;
  sessionTitle: string;
  hits: SearchHit[];
  bestScore: number;
}

export default function SearchView({ onOpenSession }: Props) {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [searching, setSearching] = useState(false);
  const [searched, setSearched] = useState(false);
  const [includeArchived, setIncludeArchived] = useState(false);
  const sessionList = useSessions({ archived: true });
  const sessionMap = useMemo(() => {
    const m = new Map<string, { title: string | null; source: string }>();
    for (const s of sessionList.sessions) {
      m.set(s.id, { title: s.title, source: s.source });
    }
    return m;
  }, [sessionList.sessions]);

  const runSearch = async () => {
    if (!query.trim()) return;
    setSearching(true);
    setSearched(true);
    try {
      const res = await searchSessions({ q: query, includeArchived });
      setHits(res.hits);
    } catch {
      setHits([]);
    } finally {
      setSearching(false);
    }
  };

  // Group hits by session
  const grouped: GroupedHits[] = useMemo(() => {
    const bySession = new Map<string, SearchHit[]>();
    for (const hit of hits) {
      const arr = bySession.get(hit.sessionId) ?? [];
      arr.push(hit);
      bySession.set(hit.sessionId, arr);
    }
    const groups: GroupedHits[] = [];
    for (const [sessionId, sessionHits] of bySession) {
      const meta = sessionMap.get(sessionId);
      groups.push({
        sessionId,
        source: sessionHits[0].source,
        sessionTitle: meta?.title ?? "(unknown session)",
        hits: sessionHits.sort((a, b) => b.score - a.score),
        bestScore: Math.max(...sessionHits.map((h) => h.score)),
      });
    }
    return groups.sort((a, b) => b.bestScore - a.bestScore);
  }, [hits, sessionMap]);

  return (
    <div className="search-view">
      <div className="search-header">
        <div className="search-input-wrap">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="11" cy="11" r="8" />
            <path d="m21 21-4.35-4.35" />
          </svg>
          <input
            type="text"
            className="search-input"
            placeholder="Search across all sessions..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && runSearch()}
            autoFocus
          />
          {searching && <span className="search-spinner" />}
        </div>
        <label className="archive-toggle">
          <input
            type="checkbox"
            checked={includeArchived}
            onChange={(e) => setIncludeArchived(e.target.checked)}
          />
          <span>Include archived</span>
        </label>
      </div>

      <div className="search-stats">
        {!searched && <span>Search across all sessions and messages.</span>}
        {searched && !searching && (
          <span>{hits.length} match{hits.length !== 1 ? "es" : ""} in {grouped.length} session{grouped.length !== 1 ? "s" : ""}</span>
        )}
      </div>

      <div className="search-results">
        {grouped.map((group) => {
          const meta = SOURCE_META[group.source as keyof typeof SOURCE_META] ?? SOURCE_META.api_server;
          return (
            <div key={group.sessionId} className="search-group">
              <div className="search-group-header" onClick={() => onOpenSession(group.sessionId)}>
                <span className="group-source-dot" style={{ background: meta.color }} />
                <span className="group-title">{group.sessionTitle}</span>
                <span className="group-source-label" style={{ color: meta.color }}>{meta.label}</span>
                <span className="group-hit-count">{group.hits.length} match{group.hits.length !== 1 ? "es" : ""}</span>
                <svg className="group-open-icon" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M7 7h10v10M7 17 17 7" />
                </svg>
              </div>
              <div className="search-group-hits">
                {group.hits.map((hit) => (
                  <div
                    key={`${hit.sessionId}-${hit.messageId}`}
                    className="search-hit"
                    onClick={() => onOpenSession(hit.sessionId)}
                  >
                    <HighlightSnippet snippet={hit.snippet} query={query} />
                    <span className="hit-time">{relativeTime(hit.timestamp)}</span>
                  </div>
                ))}
              </div>
            </div>
          );
        })}
        {searched && hits.length === 0 && !searching && (
          <div className="search-empty">No results found.</div>
        )}
      </div>
    </div>
  );
}

function HighlightSnippet({ snippet, query }: { snippet: string; query: string }) {
  if (!query.trim()) return <span className="hit-snippet">{snippet}</span>;
  const terms = query.trim().split(/\s+/).filter((t) => t.length > 0);
  const escaped = terms.map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  const regex = new RegExp(`(${escaped.join("|")})`, "gi");
  const parts = snippet.split(regex);
  return (
    <span className="hit-snippet">
      {parts.map((part, i) =>
        regex.test(part) ? (
          <mark key={i} className="hit-highlight">{part}</mark>
        ) : (
          <span key={i}>{part}</span>
        )
      )}
    </span>
  );
}
