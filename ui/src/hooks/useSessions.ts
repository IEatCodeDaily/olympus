import { useState, useEffect, useCallback, useRef } from "react";
import type { Session, SessionSort } from "../types";
import { fetchSessions } from "../api";
import { onFrame } from "../api";

export interface UseSessionsResult {
  sessions: Session[];
  loading: boolean;
  error: string | null;
  refetch: () => void;
}

export function useSessions(params: {
  source?: string[];
  archived?: boolean;
  q?: string;
  sort?: SessionSort;
}): UseSessionsResult {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const paramsRef = useRef(params);
  paramsRef.current = params;

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const res = await fetchSessions({
        source: paramsRef.current.source?.join(","),
        archived: paramsRef.current.archived,
        q: paramsRef.current.q,
        sort: paramsRef.current.sort,
      });
      setSessions(res.sessions);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  // Refetch when filters change
  useEffect(() => {
    load();
  }, [load, params.source, params.archived, params.q, params.sort]);

  // Merge WS deltas for live updates
  useEffect(() => {
    return onFrame((frame) => {
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
  }, []);

  return { sessions, loading, error, refetch: load };
}
