import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useCallback } from "react";
import {
  fetchSessions,
  fetchSession,
  fetchMessages,
  fetchAgents,
  fetchAgentCatalog,
  fetchNodes,
  fetchModels,
  healthCheck,
  fetchCards,
  fetchVaults,
  fetchVaultNotes,
  fetchVaultNote,
  fetchVaultDocuments,
  updateSession,
  onFrame,
  connectWs,
} from "../api";
import type { ServerFrame } from "../types";

/** Query key factory — centralizes cache keys. */
export const qk = {
  sessions: (params?: Record<string, unknown>) => ["sessions", params] as const,
  session: (id: string) => ["session", id] as const,
  messages: (id: string) => ["messages", id] as const,
  agents: () => ["agents"] as const,
  agentCatalog: () => ["agents", "catalog"] as const,
  models: (agentId?: string | null) => ["models", agentId ?? "all"] as const,
  health: () => ["health"] as const,
  cards: (params?: Record<string, unknown>) => ["cards", params] as const,
  vaults: () => ["vaults"] as const,
  vaultNotes: (vaultId: string) => ["vaultNotes", vaultId] as const,
  vaultNote: (vaultId: string, path: string) =>
    ["vaultNote", vaultId, path] as const,
  vaultDocuments: (vaultId: string) => ["vaultDocuments", vaultId] as const,
};

/** Sessions list with auto-refetch. */
export function useSessions(params?: {
  managed?: boolean;
  archived?: boolean;
  sort?: string;
  limit?: number;
  node?: string;
}) {
  return useQuery({
    queryKey: qk.sessions(params),
    queryFn: () =>
      fetchSessions({
        managed: params?.managed,
        archived: params?.archived,
        sort: params?.sort as "lastActivity" | "startedAt" | "messageCount" | undefined,
        limit: params?.limit,
        node: params?.node,
      }),
    refetchInterval: 10_000,
    staleTime: 5_000,
  });
}

/** Single session by id. */
export function useSession(id: string | null) {
  return useQuery({
    queryKey: id ? qk.session(id) : ["session", "none"],
    queryFn: () => fetchSession(id!),
    enabled: !!id,
    staleTime: 5_000,
  });
}

/** Messages for a session. */
export function useMessages(sessionId: string | null) {
  return useQuery({
    queryKey: sessionId ? qk.messages(sessionId) : ["messages", "none"],
    queryFn: () => fetchMessages(sessionId!, { limit: 100 }),
    enabled: !!sessionId,
    staleTime: Infinity, // don't auto-refetch; WS drives live updates
  });
}

/** Agents list. */
export function useAgents() {
  return useQuery({ queryKey: qk.agents(), queryFn: fetchAgents, staleTime: 60_000 });
}

/** Per-node agent availability for new-session routing. */
export function useAgentCatalog() {
  return useQuery({
    queryKey: qk.agentCatalog(),
    queryFn: fetchAgentCatalog,
    refetchInterval: 10_000,
    staleTime: 5_000,
  });
}

/**
 * Mutate session metadata (pin, archive, title, agent/node/model rebind).
 * Invalidates the session lists + the single-session cache on success.
 */
export function useUpdateSession() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      patch,
    }: {
      id: string;
      patch: {
        agent?: string;
        node?: string;
        model?: string;
        title?: string;
        archived?: boolean;
        pinned?: boolean;
      };
    }) => updateSession(id, patch),
    onSuccess: (_data, { id }) => {
      void qc.invalidateQueries({ queryKey: ["sessions"] });
      void qc.invalidateQueries({ queryKey: qk.session(id) });
    },
  });
}

/** Fleet nodes — connected envoys + the local node. */
export function useNodes() {
  return useQuery({
    queryKey: ["nodes"],
    queryFn: fetchNodes,
    refetchInterval: 10_000,
    staleTime: 5_000,
  });
}

/** Models list, scoped to an agent's provider when `agentId` is given. */
export function useModels(agentId?: string | null) {
  return useQuery({
    queryKey: qk.models(agentId),
    queryFn: () => fetchModels(agentId ?? undefined),
    staleTime: 60_000,
  });
}

/** Health check. */
export function useHealth() {
  return useQuery({
    queryKey: qk.health(),
    queryFn: healthCheck,
    refetchInterval: 15_000,
    staleTime: 10_000,
  });
}

/** Cards list. */
export function useCards(params?: { boardId?: string; status?: string }) {
  return useQuery({
    queryKey: qk.cards(params),
    queryFn: () => fetchCards(params),
    staleTime: 10_000,
  });
}

/** All vaults. */
export function useVaults() {
  return useQuery({
    queryKey: qk.vaults(),
    queryFn: fetchVaults,
    staleTime: 30_000,
  });
}

/** Note tree for a vault. */
export function useVaultNotes(vaultId: string | null) {
  return useQuery({
    queryKey: vaultId ? qk.vaultNotes(vaultId) : ["vaultNotes", "none"],
    queryFn: () => fetchVaultNotes(vaultId!),
    enabled: !!vaultId,
    staleTime: 10_000,
  });
}

export function useVaultDocuments(vaultId: string | null) {
  return useQuery({
    queryKey: vaultId ? qk.vaultDocuments(vaultId) : ["vaultDocuments", "none"],
    queryFn: () => fetchVaultDocuments(vaultId!),
    enabled: !!vaultId,
    staleTime: 10_000,
  });
}

/** Single note document. */
export function useVaultNote(vaultId: string | null, path: string | null) {
  return useQuery({
    queryKey:
      vaultId && path ? qk.vaultNote(vaultId, path) : ["vaultNote", "none"],
    queryFn: () => fetchVaultNote(vaultId!, path!),
    enabled: !!vaultId && !!path,
    staleTime: 5_000,
  });
}

/**
 * WebSocket integration: connects once, listens for ServerFrame events,
 * and increments the relevant TanStack Query cache so the UI updates live
 * without a full refetch.
 *
 * Call this once at the app root.
 */
export function useLiveSync(organizationId: string) {
  const qc = useQueryClient();

  const handleFrame = useCallback(
    (frame: ServerFrame) => {
      switch (frame.kind) {
        case "session.added": {
          qc.invalidateQueries({ queryKey: ["sessions"] });
          break;
        }
        case "message.delta": {
          // Streaming delta — the full message arrives on done.
          break;
        }
        case "message.done": {
          qc.invalidateQueries({ queryKey: qk.messages(frame.sessionId) });
          qc.invalidateQueries({ queryKey: ["sessions"] });
          break;
        }
        case "message.appended": {
          qc.invalidateQueries({ queryKey: qk.messages(frame.sessionId) });
          qc.invalidateQueries({ queryKey: ["sessions"] });
          break;
        }
        case "session.updated": {
          qc.invalidateQueries({ queryKey: ["sessions"] });
          qc.invalidateQueries({ queryKey: qk.session(frame.sessionId) });
          break;
        }
        case "cards.changed": {
          qc.invalidateQueries({ queryKey: ["cards"] });
          break;
        }
        default:
          break;
      }
    },
    [qc],
  );

  useEffect(() => {
    connectWs();
    const unsub = onFrame(handleFrame);
    return unsub;
  }, [handleFrame, organizationId]);
}
