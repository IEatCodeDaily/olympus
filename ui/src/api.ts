import type {
  Card,
  CardListResponse,
  Session,
  Message,
  SearchHit,
  ModelInfo,
  ServerFrame,
  ClientFrame,
  CreateCardBody,
  AssignCardBody,
  BlockCardBody,
  ReassignCardBody,
  SessionListParams,
  MessagesParams,
  SearchParams,
  SessionListResponse,
  MessagesResponse,
  SearchResponse,
  ModelsResponse,
  AgentsResponse,
  HealthResponse,
  SetupResponse,
  SetupQueryParams,
  PutSetupBody,
  RegistryResponse,
  RegistryQueryParams,
  PutRegistryBody,
} from "./types";

const BASE = import.meta.env.VITE_API_BASE as string;
const T = import.meta.env.VITE_API_TOKEN as string;

function authHeaders(): Record<string, string> {
  return { Authorization: "Bearer " + T };
}

function jsonHeaders(): Record<string, string> {
  return { ...authHeaders(), "content-type": "application/json" };
}

async function expectJson<T>(res: Response, label: string): Promise<T> {
  if (!res.ok) throw new Error(`${label} ${res.status}`);
  return res.json() as Promise<T>;
}

async function postJson<TResponse, TBody = unknown>(
  path: string,
  body?: TBody,
  label = "request"
): Promise<TResponse> {
  const res = await fetch(`${BASE}${path}`, {
    method: "POST",
    headers: jsonHeaders(),
    body: JSON.stringify(body ?? {}),
  });
  return expectJson<TResponse>(res, label);
}

// ── REST ───────────────────────────────────────────────

export async function fetchSessions(
  params?: SessionListParams
): Promise<SessionListResponse> {
  const q = new URLSearchParams();
  if (params?.source) q.set("source", params.source);
  if (params?.model) q.set("model", params.model);
  if (params?.archived !== undefined) q.set("archived", String(params.archived));
  if (params?.managed !== undefined) q.set("managed", String(params.managed));
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

export async function fetchAgents(): Promise<AgentsResponse> {
  const res = await fetch(`${BASE}/api/agents`, { headers: authHeaders() });
  if (!res.ok) throw new Error(`agents ${res.status}`);
  return res.json() as Promise<AgentsResponse>;
}

export async function healthCheck(): Promise<HealthResponse> {
  const res = await fetch(`${BASE}/api/health`, { headers: authHeaders() });
  if (!res.ok) throw new Error(`health ${res.status}`);
  return res.json() as Promise<HealthResponse>;
}

export async function fetchCards(params?: {
  boardId?: string;
  status?: string;
}): Promise<CardListResponse> {
  const q = new URLSearchParams();
  if (params?.boardId) q.set("boardId", params.boardId);
  if (params?.status) q.set("status", params.status);
  const suffix = q.size > 0 ? `?${q}` : "";
  const res = await fetch(`${BASE}/api/cards${suffix}`, { headers: authHeaders() });
  return expectJson<CardListResponse>(res, "cards");
}

export async function fetchCard(id: string): Promise<Card> {
  const res = await fetch(`${BASE}/api/cards/${id}`, { headers: authHeaders() });
  return expectJson<Card>(res, "card");
}

// ── Mutations ──────────────────────────────────────────

/**
 * Create a new Olympus-managed chat session OPTIMISTICALLY. Returns instantly
 * with a draft Session (source="olympus", managed=true, empty hermesId) — no
 * agent runtime is spawned until the first send. Optionally bind agent/node at
 * creation; otherwise assign them later via updateSession() before sending.
 */
export async function createSession(opts?: {
  agent?: string;
  node?: string;
}): Promise<Session> {
  const res = await fetch(`${BASE}/api/sessions`, {
    method: "POST",
    headers: jsonHeaders(),
    body: JSON.stringify(opts ?? {}),
  });
  if (!res.ok) throw new Error(`create session failed (${res.status})`);
  return res.json() as Promise<Session>;
}

/**
 * Bind/rebind agent, node, model, or title on an existing managed session.
 * Used in the optimistic-create flow: create instantly, then assign the
 * agent/model before the first send. Returns the updated Session.
 */
export async function updateSession(
  sessionId: string,
  patch: { agent?: string; node?: string; model?: string; title?: string }
): Promise<Session> {
  const res = await fetch(`${BASE}/api/sessions/${sessionId}`, {
    method: "PATCH",
    headers: jsonHeaders(),
    body: JSON.stringify(patch),
  });
  if (!res.ok) throw new Error(`update session failed (${res.status})`);
  return res.json() as Promise<Session>;
}

/** Fork an observed session into an Olympus-managed session and return it. */
export async function forkSession(sessionId: string): Promise<Session> {
  const res = await fetch(`${BASE}/api/sessions/${sessionId}/fork`, {
    method: "POST",
    headers: { ...authHeaders(), "content-type": "application/json" },
    body: JSON.stringify({ forkType: "sub" }),
  });
  if (!res.ok) throw new Error(`fork failed (${res.status})`);
  const body = (await res.json()) as { session: Session };
  return body.session;
}

/**
 * Send a message to a MANAGED (acp-source) session. The agent's response
 * streams back over the /ws delta channel; this POST just enqueues the prompt.
 * Returns 202 on accept; 409 if the session is observed (must be forked first).
 */
export async function sendMessage(
  sessionId: string,
  text: string,
  model?: string
): Promise<void> {
  const res = await fetch(`${BASE}/api/sessions/${sessionId}/messages`, {
    method: "POST",
    headers: jsonHeaders(),
    body: JSON.stringify({ text, model }),
  });
  if (res.status === 409) {
    throw new Error("This session is observed (read-only). Fork it to continue from Olympus.");
  }
  if (!res.ok) throw new Error(`send failed (${res.status})`);
}

export async function cancelSession(sessionId: string): Promise<void> {
  const res = await fetch(`${BASE}/api/sessions/${sessionId}/cancel`, {
    method: "POST",
    headers: jsonHeaders(),
  });
  if (!res.ok) throw new Error(`cancel failed (${res.status})`);
}

export async function createCard(body: CreateCardBody): Promise<Card> {
  return postJson<Card, CreateCardBody>("/api/cards", body, "create card failed");
}

export async function assignCard(id: string, body: AssignCardBody): Promise<Card> {
  return postJson<Card, AssignCardBody>(`/api/cards/${id}/assign`, body, "assign card failed");
}

export async function claimCard(id: string): Promise<Card> {
  return postJson<Card>(`/api/cards/${id}/claim`, undefined, "claim card failed");
}

export async function blockCard(id: string, body: BlockCardBody): Promise<Card> {
  return postJson<Card, BlockCardBody>(`/api/cards/${id}/block`, body, "block card failed");
}

export async function completeCard(id: string): Promise<Card> {
  return postJson<Card>(`/api/cards/${id}/complete`, undefined, "complete card failed");
}

export async function reassignCard(id: string, body: ReassignCardBody): Promise<Card> {
  return postJson<Card, ReassignCardBody>(`/api/cards/${id}/reassign`, body, "reassign card failed");
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
  if (ws || connecting) return;

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

// ── Setup & Registry (ADR 0006) ──────────────────────

export async function fetchSetup(
  params?: SetupQueryParams
): Promise<SetupResponse> {
  const q = new URLSearchParams();
  if (params?.scope) q.set("scope", params.scope);
  if (params?.effective) q.set("effective", "true");
  if (params?.org) q.set("org", params.org);
  if (params?.project) q.set("project", params.project);
  const qs = q.toString();
  const res = await fetch(`${BASE}/api/setup${qs ? `?${qs}` : ""}`, {
    headers: authHeaders(),
  });
  return expectJson(res, "setup");
}

export async function putSetup(body: PutSetupBody): Promise<SetupResponse> {
  const res = await fetch(`${BASE}/api/setup`, {
    method: "PUT",
    headers: jsonHeaders(),
    body: JSON.stringify(body),
  });
  return expectJson(res, "putSetup");
}

export async function fetchRegistry(
  params?: RegistryQueryParams
): Promise<RegistryResponse> {
  const q = new URLSearchParams();
  if (params?.kind) q.set("kind", params.kind);
  if (params?.slug) q.set("slug", params.slug);
  const qs = q.toString();
  const res = await fetch(`${BASE}/api/registry${qs ? `?${qs}` : ""}`, {
    headers: authHeaders(),
  });
  return expectJson(res, "registry");
}

export async function putRegistryEntry(
  body: PutRegistryBody
): Promise<RegistryResponse> {
  const res = await fetch(`${BASE}/api/registry`, {
    method: "PUT",
    headers: jsonHeaders(),
    body: JSON.stringify(body),
  });
  return expectJson(res, "putRegistry");
}

export async function handoverSession(
  id: string,
  toAgentKind: string,
  model?: string
): Promise<{ session: Session }> {
  return postJson(`/api/sessions/${id}/handover`, { toAgentKind, model }, "handover");
}

export async function fetchIrcPeers(): Promise<{ peers: string[] }> {
  const res = await fetch(`${BASE}/api/irc/peers`, { headers: authHeaders() });
  return expectJson(res, "ircPeers");
}

export async function sendIrcMessage(
  from: string,
  to: string,
  content: string
): Promise<{ ok: boolean }> {
  return postJson(`/api/irc/send`, { from, to, content }, "ircSend");
}

export type { Session, Message, SearchHit, ModelInfo, ServerFrame, ClientFrame };
