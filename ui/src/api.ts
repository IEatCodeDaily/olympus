import type {
  Session,
  Message,
  SearchHit,
  ModelInfo,
  ServerFrame,
  ClientFrame,
  SessionListParams,
  MessagesParams,
  SearchParams,
  SessionListResponse,
  MessagesResponse,
  SearchResponse,
  ModelsResponse,
  HealthResponse,
} from "./types";

const BASE = import.meta.env.VITE_API_BASE as string;
const T = import.meta.env.VITE_API_TOKEN as string;

function authHeaders(): Record<string, string> {
  return { Authorization: `Bearer ${T}` };
}

// ── REST ───────────────────────────────────────────────

export async function fetchSessions(
  params?: SessionListParams
): Promise<SessionListResponse> {
  const q = new URLSearchParams();
  if (params?.source) q.set("source", params.source);
  if (params?.model) q.set("model", params.model);
  if (params?.archived !== undefined) q.set("archived", String(params.archived));
  if (params?.q) q.set("q", params.q);
  if (params?.sort) q.set("sort", params.sort);
  if (params?.cursor) q.set("cursor", params.cursor);
  if (params?.limit) q.set("limit", String(params.limit));

  const res = await fetch(`${BASE}/api/sessions?${q}`, { headers: authHeaders() });
  if (!res.ok) throw new Error(`sessions ${res.status}`);
  return res.json() as Promise<SessionListResponse>;
}

export async function fetchSession(id: string): Promise<Session> {
  const res = await fetch(`${BASE}/api/sessions/${id}`, { headers: authHeaders() });
  if (!res.ok) throw new Error(`session ${res.status}`);
  return res.json() as Promise<Session>;
}

export async function fetchMessages(
  sessionId: string,
  params?: MessagesParams
): Promise<MessagesResponse> {
  const q = new URLSearchParams();
  if (params?.cursor) q.set("cursor", params.cursor);
  if (params?.limit) q.set("limit", String(params.limit));

  const res = await fetch(
    `${BASE}/api/sessions/${sessionId}/messages?${q}`,
    { headers: authHeaders() }
  );
  if (!res.ok) throw new Error(`messages ${res.status}`);
  return res.json() as Promise<MessagesResponse>;
}

export async function searchSessions(
  params: SearchParams
): Promise<SearchResponse> {
  const q = new URLSearchParams({ q: params.q });
  if (params?.limit) q.set("limit", String(params.limit));
  if (params?.includeArchived !== undefined)
    q.set("includeArchived", String(params.includeArchived));

  const res = await fetch(`${BASE}/api/search?${q}`, { headers: authHeaders() });
  if (!res.ok) throw new Error(`search ${res.status}`);
  return res.json() as Promise<SearchResponse>;
}

export async function fetchModels(): Promise<ModelsResponse> {
  const res = await fetch(`${BASE}/api/models`, { headers: authHeaders() });
  if (!res.ok) throw new Error(`models ${res.status}`);
  return res.json() as Promise<ModelsResponse>;
}

export async function healthCheck(): Promise<HealthResponse> {
  const res = await fetch(`${BASE}/api/health`, { headers: authHeaders() });
  if (!res.ok) throw new Error(`health ${res.status}`);
  return res.json() as Promise<HealthResponse>;
}

// ── WebSocket (singleton, safe for mock mode) ──────────

type FrameListener = (frame: ServerFrame) => void;

let ws: WebSocket | null = null;
let connecting = false;
const listeners = new Set<FrameListener>();

function getWsUrl(): string {
  const proto = BASE.startsWith("https") ? "wss" : "ws";
  const u = new URL(BASE);
  return `${proto}://${u.host}/ws?token=${T}`;
}

export function connectWs(): void {
  // Singleton: if already connected or connecting, skip
  if (ws || connecting) return;

  // In mock mode, don't attempt real WS connection (no mock WS server)
  const useMocks = import.meta.env.VITE_USE_MOCKS !== "false";
  if (useMocks) return;

  connecting = true;
  try {
    ws = new WebSocket(getWsUrl());

    ws.onopen = () => {
      connecting = false;
    };

    ws.onmessage = (e) => {
      try {
        const frame = JSON.parse(e.data) as ServerFrame;
        for (const fn of listeners) fn(frame);
      } catch {
        // ignore malformed frames
      }
    };

    ws.onerror = () => {
      connecting = false;
    };

    ws.onclose = () => {
      connecting = false;
      ws = null;
      // auto-reconnect after 2s
      setTimeout(() => connectWs(), 2000);
    };
  } catch {
    connecting = false;
    ws = null;
  }
}

export function closeWs(): void {
  if (ws) {
    ws.onclose = null;
    ws.close();
    ws = null;
  }
  connecting = false;
}

export function onFrame(fn: FrameListener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function sendFrame(frame: ClientFrame): void {
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify(frame));
  }
}

// Re-export types for convenience
export type { Session, Message, SearchHit, ModelInfo, ServerFrame, ClientFrame };
