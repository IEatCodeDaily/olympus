import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { Session, ServerFrame } from "../types";
import { fetchSessions } from "../api";
import { connectWs, onFrame, sendFrame } from "../api";
import {
  sourceColor,
  sourceLabel,
  relativeTime,
  formatTokens,
  ModelPill,
  SourceDot,
  ALL_SOURCES,
} from "../components";

interface Props {
  selectedId: string | null;
  onSelect: (id: string) => void;
  onSearchClick: () => void;
}

const ROW_HEIGHT = 64;
const SOURCES_TO_SHOW = ALL_SOURCES;

export function SessionList({ selectedId, onSelect, onSearchClick }: Props) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [activeSources, setActiveSources] = useState<Set<string>>(new Set());
  const [showArchived, setShowArchived] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  // Fetch initial data
  const loadSessions = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const resp = await fetchSessions({
        source: activeSources.size > 0 ? Array.from(activeSources).join(",") : undefined,
        q: search || undefined,
        archived: showArchived ? undefined : false,
        sort: "lastActivity",
        limit: 200,
      });
      setSessions(resp.sessions);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load sessions");
    } finally {
      setLoading(false);
    }
  }, [activeSources, search, showArchived]);

  // Refetch when filters change (debounced for search)
  useEffect(() => {
    const timer = setTimeout(loadSessions, search ? 250 : 0);
    return () => clearTimeout(timer);
  }, [loadSessions]);

  // WebSocket: merge live session deltas
  useEffect(() => {
    connectWs();
    const unsub = onFrame((frame: ServerFrame) => {
      if (frame.kind === "session.added") {
        setSessions((prev) => {
          if (prev.some((s) => s.id === frame.session.id)) return prev;
          return [frame.session, ...prev];
        });
      } else if (frame.kind === "session.updated") {
        setSessions((prev) =>
          prev.map((s) =>
            s.id === frame.sessionId ? { ...s, ...frame.changes } : s
          )
        );
      } else if (frame.kind === "session.removed") {
        setSessions((prev) => prev.filter((s) => s.id !== frame.sessionId));
      }
    });
    return () => unsub();
  }, []);

  // Subscribe to session list updates (implicit — all session.* frames)
  useEffect(() => {
    // The session list gets all session.* frames automatically
    // No explicit subscribe needed per the contract
  }, []);

  const toggleSource = (src: string) => {
    setActiveSources((prev) => {
      const next = new Set(prev);
      if (next.has(src)) next.delete(src);
      else next.add(src);
      return next;
    });
  };

  // Virtualization
  const rowVirtualizer = useVirtualizer({
    count: sessions.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
  });

  const sortedSessions = useMemo(
    () => [...sessions].sort((a, b) => b.lastActivity - a.lastActivity),
    [sessions]
  );

  return (
    <div style={{
      display: "flex",
      flexDirection: "column",
      width: "100%",
      height: "100%",
      background: "var(--bg-1)",
      borderRight: "1px solid var(--border-subtle)",
    }}>
      {/* ── Header ── */}
      <div style={{
        padding: "10px 14px 8px",
        borderBottom: "1px solid var(--border-faint)",
        flexShrink: 0,
      }}>
        {/* Search bar */}
        <div
          onClick={onSearchClick}
          style={{
            display: "flex",
            alignItems: "center",
            gap: "8px",
            height: "32px",
            padding: "0 10px",
            borderRadius: "var(--radius-md)",
            background: "var(--bg-2)",
            border: "1px solid var(--border-faint)",
            cursor: "pointer",
            transition: `border-color var(--dur-fast) var(--ease-out)`,
          }}
        >
          <svg width="13" height="13" viewBox="0 0 16 16" fill="none" style={{ opacity: 0.5 }}>
            <circle cx="7" cy="7" r="5" stroke="currentColor" strokeWidth="1.3" />
            <path d="M11 11L14 14" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
          </svg>
          <span style={{
            fontSize: "12px",
            color: "var(--text-tertiary)",
            flex: 1,
          }}>
            Search messages…
          </span>
          <span className="mono" style={{
            fontSize: "10px",
            color: "var(--text-faint)",
            padding: "1px 5px",
            borderRadius: "3px",
            border: "1px solid var(--border-faint)",
            background: "var(--bg-1)",
          }}>
            ⌘K
          </span>
        </div>

        {/* Quick filter input */}
        <input
          type="text"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Filter sessions…"
          style={{
            width: "100%",
            height: "28px",
            marginTop: "6px",
            padding: "0 10px",
            fontSize: "12px",
            fontFamily: "var(--font-sans)",
            color: "var(--text-primary)",
            background: "var(--bg-2)",
            border: "1px solid var(--border-faint)",
            borderRadius: "var(--radius-md)",
            outline: "none",
          }}
          onFocus={(e) => e.currentTarget.style.borderColor = "var(--accent)"}
          onBlur={(e) => e.currentTarget.style.borderColor = "var(--border-faint)"}
        />

        {/* Source filters */}
        <div style={{
          display: "flex",
          flexWrap: "wrap",
          gap: "3px",
          marginTop: "8px",
        }}>
          {SOURCES_TO_SHOW.map((src) => {
            const active = activeSources.has(src);
            const color = sourceColor(src);
            return (
              <button
                key={src}
                onClick={() => toggleSource(src)}
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: "4px",
                  padding: "2px 7px",
                  fontSize: "10px",
                  fontFamily: "var(--font-mono)",
                  fontWeight: 500,
                  lineHeight: "18px",
                  borderRadius: "var(--radius-sm)",
                  border: `1px solid ${active ? color : "var(--border-faint)"}`,
                  color: active ? color : "var(--text-faint)",
                  background: active ? "var(--bg-2)" : "transparent",
                  cursor: "pointer",
                  transition: `all var(--dur-fast) var(--ease-out)`,
                }}
              >
                <SourceDot source={src} size={6} />
                {sourceLabel(src)}
              </button>
            );
          })}
        </div>

        {/* Count + archived toggle */}
        <div style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginTop: "6px",
          fontSize: "11px",
          color: "var(--text-tertiary)",
        }}>
          <span className="mono">
            {loading ? "…" : `${sessions.length} session${sessions.length !== 1 ? "s" : ""}`}
          </span>
          <button
            onClick={() => setShowArchived(!showArchived)}
            style={{
              fontSize: "10px",
              fontFamily: "var(--font-mono)",
              color: showArchived ? "var(--accent)" : "var(--text-faint)",
              background: "transparent",
              border: "none",
              cursor: "pointer",
              transition: `color var(--dur-fast) var(--ease-out)`,
            }}
          >
            {showArchived ? "• archived" : "archived"}
          </button>
        </div>
      </div>

      {/* ── List ── */}
      <div
        ref={scrollRef}
        style={{
          flex: 1,
          overflow: "auto",
          position: "relative",
        }}
      >
        {error && (
          <div style={{ padding: "20px 14px", fontSize: "12px", color: "var(--error)" }}>
            {error}
          </div>
        )}

        {!error && !loading && sessions.length === 0 && (
          <div style={{
            padding: "40px 20px",
            textAlign: "center",
            fontSize: "12px",
            color: "var(--text-faint)",
          }}>
            No sessions found
          </div>
        )}

        {loading && sessions.length === 0 && (
          <div style={{ padding: "20px 14px" }}>
            {[...Array(8)].map((_, i) => (
              <div key={i} style={{
                height: ROW_HEIGHT,
                borderBottom: "1px solid var(--border-faint)",
                display: "flex",
                alignItems: "center",
                gap: "10px",
                padding: "0 14px",
              }}>
                <div style={{ width: "8px", height: "8px", borderRadius: "50%", background: "var(--bg-3)" }} />
                <div style={{ flex: 1 }}>
                  <div style={{ height: "12px", width: "60%", background: "var(--bg-3)", borderRadius: "3px", marginBottom: "6px" }} />
                  <div style={{ height: "10px", width: "40%", background: "var(--bg-2)", borderRadius: "3px" }} />
                </div>
              </div>
            ))}
          </div>
        )}

        {sessions.length > 0 && (
          <div
            style={{
              height: `${rowVirtualizer.getTotalSize()}px`,
              width: "100%",
              position: "relative",
            }}
          >
            {rowVirtualizer.getVirtualItems().map((virtualRow) => {
              const session = sortedSessions[virtualRow.index];
              return (
                <div
                  key={session.id}
                  data-index={virtualRow.index}
                  ref={rowVirtualizer.measureElement}
                  onClick={() => onSelect(session.id)}
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    transform: `translateY(${virtualRow.start}px)`,
                    minHeight: ROW_HEIGHT,
                    padding: "10px 14px",
                    borderBottom: "1px solid var(--border-faint)",
                    cursor: "pointer",
                    background: selectedId === session.id
                      ? "var(--bg-3)"
                      : "transparent",
                    borderLeft: selectedId === session.id
                      ? "2px solid var(--accent)"
                      : "2px solid transparent",
                    transition: `background var(--dur-fast) var(--ease-out)`,
                  }}
                  onMouseEnter={(e) => {
                    if (selectedId !== session.id) e.currentTarget.style.background = "var(--bg-2)";
                  }}
                  onMouseLeave={(e) => {
                    if (selectedId !== session.id) e.currentTarget.style.background = "transparent";
                  }}
                >
                  <SessionRow session={session} />
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

/* ── Single session row ── */
function SessionRow({ session }: { session: Session }) {
  const title = session.title ?? "Untitled";
  const isManaged = session.managed;
  const isArchived = session.archived;

  return (
    <div style={{
      display: "flex",
      flexDirection: "column",
      gap: "4px",
    }}>
      {/* Top row: source dot + title + time */}
      <div style={{ display: "flex", alignItems: "center", gap: "7px" }}>
        <SourceDot source={session.source} />
        <span
          className="truncate"
          style={{
            flex: 1,
            fontSize: "12.5px",
            fontWeight: 500,
            color: isArchived ? "var(--text-tertiary)" : "var(--text-primary)",
            textDecoration: isArchived ? "line-through" : "none",
          }}
        >
          {title}
        </span>
        <span className="mono" style={{
          fontSize: "10px",
          color: "var(--text-faint)",
          flexShrink: 0,
        }}>
          {relativeTime(session.lastActivity)}
        </span>
      </div>

      {/* Bottom row: model + counts */}
      <div style={{
        display: "flex",
        alignItems: "center",
        gap: "6px",
        fontSize: "10.5px",
        color: "var(--text-tertiary)",
      }}>
        {session.model && <ModelPill model={session.model} />}
        <span className="mono" style={{ opacity: 0.7 }}>
          {session.messageCount} msg
        </span>
        {(session.inputTokens + session.outputTokens) > 0 && (
          <span className="mono" style={{ opacity: 0.5 }}>
            · {formatTokens(session.inputTokens + session.outputTokens)} tok
          </span>
        )}
        {isManaged && (
          <span className="mono" style={{
            color: "var(--accent)",
            fontSize: "9px",
            fontWeight: 500,
            textTransform: "uppercase",
            letterSpacing: "0.05em",
          }}>
            managed
          </span>
        )}
        {session.forkedFrom && (
          <span className="mono" style={{
            color: "var(--text-faint)",
            fontSize: "9px",
          }}>
            ↗ fork
          </span>
        )}
      </div>
    </div>
  );
}
