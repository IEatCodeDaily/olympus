import { useState, useEffect, useRef } from "react";
import type { SearchHit, Session } from "../types";
import { searchSessions, fetchSession } from "../api";
import { sourceColor, sourceLabel, relativeTime, Highlight, SourceDot, Spinner } from "../components";

interface Props {
  onOpenSession: (sessionId: string) => void;
  onClose: () => void;
}

interface GroupedHits {
  sessionId: string;
  source: Session["source"];
  sessionTitle: string | null;
  hits: SearchHit[];
}

export function SearchView({ onOpenSession, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [includeArchived, setIncludeArchived] = useState(false);
  const [hasSearched, setHasSearched] = useState(false);
  const [sessionCache, setSessionCache] = useState<Map<string, Session>>(new Map());
  const inputRef = useRef<HTMLInputElement>(null);

  // Focus on mount
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // Debounced search
  useEffect(() => {
    if (!query.trim()) {
      setHits([]);
      setHasSearched(false);
      return;
    }

    const timer = setTimeout(async () => {
      setLoading(true);
      setError(null);
      try {
        const resp = await searchSessions({
          q: query,
          limit: 50,
          includeArchived,
        });
        setHits(resp.hits);
        setHasSearched(true);
      } catch (e) {
        setError(e instanceof Error ? e.message : "Search failed");
      } finally {
        setLoading(false);
      }
    }, 250);

    return () => clearTimeout(timer);
  }, [query, includeArchived]);

  // Group hits by session
  const grouped: GroupedHits[] = [];
  const groupMap = new Map<string, GroupedHits>();
  for (const hit of hits) {
    if (groupMap.has(hit.sessionId)) {
      groupMap.get(hit.sessionId)!.hits.push(hit);
    } else {
      const g: GroupedHits = {
        sessionId: hit.sessionId,
        source: hit.source,
        sessionTitle: null,
        hits: [hit],
      };
      groupMap.set(hit.sessionId, g);
      grouped.push(g);
    }
  }

  // Fetch session titles for groups (lazy, deduped)
  useEffect(() => {
    for (const g of grouped) {
      if (g.sessionTitle === null && !sessionCache.has(g.sessionId)) {
        fetchSession(g.sessionId)
          .then((sess) => {
            g.sessionTitle = sess.title ?? "Untitled";
            setSessionCache((prev) => new Map(prev).set(g.sessionId, sess));
          })
          .catch(() => {
            g.sessionTitle = "Untitled";
          });
      } else if (sessionCache.has(g.sessionId)) {
        g.sessionTitle = sessionCache.get(g.sessionId)?.title ?? "Untitled";
      }
    }
  }, [grouped, sessionCache]);

  return (
    <div style={{
      display: "flex",
      flexDirection: "column",
      height: "100%",
      background: "var(--bg-0)",
    }}>
      {/* ── Search header ── */}
      <div style={{
        display: "flex",
        alignItems: "center",
        gap: "10px",
        height: "var(--header-h)",
        padding: "0 20px",
        borderBottom: "1px solid var(--border-faint)",
        background: "var(--bg-1)",
        flexShrink: 0,
      }}>
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" style={{ opacity: 0.5 }}>
          <circle cx="7" cy="7" r="5" stroke="currentColor" strokeWidth="1.3" />
          <path d="M11 11L14 14" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
        </svg>
        <input
          ref={inputRef}
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search across all conversations…"
          style={{
            flex: 1,
            height: "32px",
            fontSize: "13px",
            fontFamily: "var(--font-sans)",
            color: "var(--text-primary)",
            background: "transparent",
            border: "none",
            outline: "none",
          }}
        />
        {loading && <Spinner size={14} />}
        <button
          onClick={() => setIncludeArchived(!includeArchived)}
          className="mono"
          style={{
            fontSize: "10px",
            color: includeArchived ? "var(--accent)" : "var(--text-faint)",
            background: "transparent",
            border: "1px solid var(--border-faint)",
            padding: "3px 8px",
            borderRadius: "var(--radius-sm)",
            cursor: "pointer",
          }}
        >
          {includeArchived ? "• archived" : "archived"}
        </button>
        <button
          onClick={onClose}
          className="mono"
          style={{
            fontSize: "10px",
            color: "var(--text-faint)",
            background: "transparent",
            border: "none",
            cursor: "pointer",
          }}
        >
          esc
        </button>
      </div>

      {/* ── Results ── */}
      <div style={{ flex: 1, overflow: "auto", padding: "16px 0" }}>
        {error && (
          <div style={{ padding: "20px 24px", color: "var(--error)", fontSize: "13px" }}>
            {error}
          </div>
        )}

        {!loading && !error && hasSearched && grouped.length === 0 && (
          <div style={{
            textAlign: "center",
            padding: "80px 20px",
            color: "var(--text-faint)",
          }}>
            <div style={{ fontSize: "13px", marginBottom: "6px", color: "var(--text-tertiary)" }}>
              No results for "{query}"
            </div>
            <div style={{ fontSize: "11px" }}>
              Try different keywords or include archived sessions.
            </div>
          </div>
        )}

        {!loading && !error && !hasSearched && (
          <div style={{
            textAlign: "center",
            padding: "80px 20px",
            color: "var(--text-faint)",
            fontSize: "13px",
          }}>
            Type to search across all conversations
          </div>
        )}

        {grouped.length > 0 && (
          <div style={{ maxWidth: "760px", margin: "0 auto", padding: "0 24px" }}>
            {/* Result count */}
            <div className="mono" style={{
              fontSize: "10.5px",
              color: "var(--text-faint)",
              marginBottom: "16px",
              paddingBottom: "12px",
              borderBottom: "1px solid var(--border-faint)",
            }}>
              {hits.length} match{hits.length !== 1 ? "es" : ""} in {grouped.length} session{grouped.length !== 1 ? "s" : ""}
            </div>

            {/* Grouped results */}
            {grouped.map((group) => {
              const title = group.sessionTitle ?? "…";
              return (
                <div key={group.sessionId} style={{ marginBottom: "24px" }}>
                  {/* Session header */}
                  <button
                    onClick={() => onOpenSession(group.sessionId)}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: "8px",
                      width: "100%",
                      padding: "6px 0",
                      background: "transparent",
                      border: "none",
                      cursor: "pointer",
                      textAlign: "left",
                      marginBottom: "4px",
                    }}
                  >
                    <SourceDot source={group.source} />
                    <span style={{
                      fontSize: "12.5px",
                      fontWeight: 500,
                      color: "var(--text-primary)",
                      flex: 1,
                    }} className="truncate">
                      {title}
                    </span>
                    <span className="mono" style={{
                      fontSize: "10px",
                      color: "var(--text-faint)",
                    }}>
                      {sourceLabel(group.source)}
                    </span>
                    <span className="mono" style={{
                      fontSize: "10px",
                      color: "var(--text-faint)",
                      padding: "1px 5px",
                      borderRadius: "3px",
                      background: "var(--bg-2)",
                    }}>
                      {group.hits.length}
                    </span>
                  </button>

                  {/* Individual hits */}
                  {group.hits.slice(0, 5).map((hit, i) => (
                    <button
                      key={`${hit.messageId}-${i}`}
                      onClick={() => onOpenSession(group.sessionId)}
                      style={{
                        display: "block",
                        width: "100%",
                        padding: "6px 0 6px 18px",
                        background: "transparent",
                        border: "none",
                        cursor: "pointer",
                        textAlign: "left",
                        borderLeft: "1px solid var(--border-faint)",
                        marginLeft: "3px",
                      }}
                    >
                      <div style={{
                        fontSize: "12px",
                        lineHeight: "1.55",
                        color: "var(--text-secondary)",
                        marginBottom: "3px",
                      }}>
                        <Highlight text={hit.snippet} query={query} />
                      </div>
                      <div className="mono" style={{
                        fontSize: "9.5px",
                        color: "var(--text-faint)",
                      }}>
                        msg #{hit.messageId} · {relativeTime(hit.timestamp)} · score {hit.score.toFixed(2)}
                      </div>
                    </button>
                  ))}
                  {group.hits.length > 5 && (
                    <div className="mono" style={{
                      fontSize: "10px",
                      color: "var(--text-faint)",
                      padding: "4px 0 4px 18px",
                      marginLeft: "3px",
                    }}>
                      +{group.hits.length - 5} more
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
