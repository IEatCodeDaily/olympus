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
  NodesResponse,
  EnrollResponse,
  HealthResponse,
  SetupResponse,
  SetupQueryParams,
  PutSetupBody,
  RegistryResponse,
  RegistryQueryParams,
  PutRegistryBody,
  VaultsResponse,
  NotesTreeResponse,
  NoteDocument,
  PutNoteBody,
  VaultSummary,
  NoteTreeEntry,
  CreateVaultBody,
  VaultDocumentsResponse,
} from "./types";
const BASE = import.meta.env.VITE_API_BASE as string;
const T = import.meta.env.VITE_API_TOKEN as string;

export function authHeaders(): Record<string, string> {
  return { Authorization: "Bearer " + T };
}

function jsonHeaders(): Record<string, string> {
  return { ...authHeaders(), "content-type": "application/json" };
}

async function expectJson<T>(res: Response, label: string): Promise<T> {
  if (!res.ok) {
    const body = await res.json().catch(() => null) as { message?: unknown } | null;
    const message = typeof body?.message === "string" ? body.message : `${label} ${res.status}`;
    throw new Error(message);
  }
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
  if (params?.pinned !== undefined) q.set("pinned", String(params.pinned));
  if (params?.managed !== undefined) q.set("managed", String(params.managed));
  if (params?.node) q.set("node", params.node);
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

export async function fetchModels(agentId?: string | null): Promise<ModelsResponse> {
  // Agent-scoped list (only models that agent's provider serves) when an id is
  // given; otherwise the full deduped list.
  const path = agentId
    ? `/api/agents/${encodeURIComponent(agentId)}/models`
    : `/api/models`;
  const res = await fetch(`${BASE}${path}`, { headers: authHeaders() });
  if (!res.ok) throw new Error(`models ${res.status}`);
  return res.json() as Promise<ModelsResponse>;
}

export async function fetchAgents(): Promise<AgentsResponse> {
  const res = await fetch(`${BASE}/api/agents`, { headers: authHeaders() });
  if (!res.ok) throw new Error(`agents ${res.status}`);
  return res.json() as Promise<AgentsResponse>;
}

/** Manually re-detect a node's agents (Fleet › Agents "detect" button).
 *  Local node re-probes in-process; remote nodes require their envoy. */
export async function refreshNodeAgents(nodeId: string): Promise<AgentsResponse> {
  const res = await fetch(
    `${BASE}/api/nodes/${encodeURIComponent(nodeId)}/agents/refresh`,
    { method: "POST", headers: authHeaders() },
  );
  if (!res.ok) throw new Error(`refresh agents ${res.status}`);
  return res.json() as Promise<AgentsResponse>;
}

export async function fetchNodes(): Promise<NodesResponse> {
  const res = await fetch(`${BASE}/api/nodes`, { headers: authHeaders() });
  if (!res.ok) throw new Error(`nodes ${res.status}`);
  return res.json() as Promise<NodesResponse>;
}

/** Mint an enroll token — returns the one-line envoy setup command. */
export async function mintEnroll(): Promise<EnrollResponse> {
  const res = await fetch(`${BASE}/api/enroll`, {
    method: "POST",
    headers: authHeaders(),
  });
  if (!res.ok) {
    const body = (await res.json().catch(() => null)) as { error?: string } | null;
    throw new Error(body?.error ?? `enroll ${res.status}`);
  }
  return res.json() as Promise<EnrollResponse>;
}

/** Mark a node draining (no new sessions routed to it). */
export async function drainNode(nodeId: string): Promise<void> {
  const res = await fetch(`${BASE}/api/nodes/${encodeURIComponent(nodeId)}/drain`, {
    method: "POST",
    headers: authHeaders(),
  });
  if (!res.ok) throw new Error(`drain ${res.status}`);
}

/** Remove a node from the fleet (deregisters + revokes its allowlist entry). */
export async function removeNode(nodeId: string): Promise<void> {
  const res = await fetch(`${BASE}/api/nodes/${encodeURIComponent(nodeId)}`, {
    method: "DELETE",
    headers: authHeaders(),
  });
  if (!res.ok) {
    const body = (await res.json().catch(() => null)) as { error?: string } | null;
    throw new Error(body?.error ?? `remove node ${res.status}`);
  }
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
  patch: {
    agent?: string;
    node?: string;
    model?: string;
    title?: string;
    archived?: boolean;
    pinned?: boolean;
  }
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
  model?: string,
  thinking?: string
): Promise<void> {
  const body: Record<string, unknown> = { text };
  if (model) body.model = model;
  if (thinking) body.thinking = thinking;
  const res = await fetch(`${BASE}/api/sessions/${sessionId}/messages`, {
    method: "POST",
    headers: jsonHeaders(),
    body: JSON.stringify(body),
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

/**
 * Steer (interrupt) a running turn without stopping it — injects guidance
 * into the in-flight LLM turn via the Hermes /steer command. Returns 202 on
 * accept; 409 when no turn is running (the caller should send a normal
 * message instead).
 */
export async function steerSession(
  sessionId: string,
  text: string,
): Promise<void> {
  const res = await fetch(`${BASE}/api/sessions/${sessionId}/steer`, {
    method: "POST",
    headers: jsonHeaders(),
    body: JSON.stringify({ text }),
  });
  if (res.status === 409) {
    throw new Error("not_running");
  }
  if (!res.ok) throw new Error(`steer failed (${res.status})`);
}

/** Answer a pending permission request. Pass optionId to allow/select, or
 *  omit it to cancel the request (ACP "cancelled" outcome). */
export async function respondPermission(
  sessionId: string,
  optionId: string | null,
): Promise<void> {
  const res = await fetch(`${BASE}/api/sessions/${sessionId}/permission`, {
    method: "POST",
    headers: jsonHeaders(),
    body: JSON.stringify({ optionId }),
  });
  if (!res.ok) throw new Error(`permission response failed (${res.status})`);
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

// ── Vaults (ADR 0004 — markdown knowledge base) ──────

export async function fetchVaults(): Promise<VaultsResponse> {
  const res = await fetch(`${BASE}/api/vaults`, { headers: authHeaders() });
  return expectJson(res, "vaults");
}

export async function createVault(body: CreateVaultBody): Promise<VaultSummary> {
  return postJson<VaultSummary, CreateVaultBody>(
    "/api/vaults",
    body,
    "create vault failed",
  );
}

export async function fetchVaultNotes(
  vaultId: string,
): Promise<NotesTreeResponse> {
  const res = await fetch(`${BASE}/api/vaults/${vaultId}/notes`, {
    headers: authHeaders(),
  });
  return expectJson(res, "vault notes");
}

export async function fetchVaultDocuments(
  vaultId: string,
): Promise<VaultDocumentsResponse> {
  const res = await fetch(`${BASE}/api/vaults/${vaultId}/documents`, {
    headers: authHeaders(),
  });
  return expectJson(res, "vault documents");
}

export async function fetchVaultNote(
  vaultId: string,
  path: string,
): Promise<NoteDocument> {
  const q = new URLSearchParams({ path });
  const res = await fetch(`${BASE}/api/vaults/${vaultId}/note?${q}`, {
    headers: authHeaders(),
  });
  return expectJson(res, "vault note");
}

export async function putVaultNote(
  vaultId: string,
  path: string,
  body: PutNoteBody,
): Promise<NoteDocument> {
  const q = new URLSearchParams({ path });
  const res = await fetch(`${BASE}/api/vaults/${vaultId}/note?${q}`, {
    method: "PUT",
    headers: jsonHeaders(),
    body: JSON.stringify(body),
  });
  return expectJson(res, "put vault note");
}

export async function deleteVaultNote(
  vaultId: string,
  path: string,
): Promise<void> {
  const q = new URLSearchParams({ path });
  const res = await fetch(`${BASE}/api/vaults/${vaultId}/note?${q}`, {
    method: "DELETE",
    headers: authHeaders(),
  });
  if (!res.ok) throw new Error(`delete vault note failed (${res.status})`);
}

// ── WebSocket (singleton, safe for mock mode) ──────────

type FrameListener = (frame: ServerFrame) => void;

let ws: WebSocket | null = null;
let connecting = false;
const listeners = new Set<FrameListener>();

function getWsUrl(): string {
  // BASE is empty in dev (API calls go through the vite proxy). When empty,
  // build the WS URL from the current page origin so the /ws request rides the
  // same proxy. `new URL("")` throws, which previously killed connectWs()
  // silently — so never feed an empty string to URL().
  const origin = BASE || window.location.origin;
  const u = new URL(origin);
  const proto = u.protocol === "https:" ? "wss" : "ws";
  // S8: send a stable display name for typing attribution. Falls back to
  // anon-<N> server-side when absent.
  const name = getDisplayName();
  const params = new URLSearchParams();
  if (T) params.set("token", T);
  if (name) params.set("name", name);
  const qs = params.toString();
  return `${proto}://${u.host}/ws${qs ? `?${qs}` : ""}`;
}

/** A stable display name for this browser (used for typing attribution, S8). */
export function getDisplayName(): string | null {
  try {
    let name = localStorage.getItem("olympus-display-name");
    if (!name) {
      // Derive a friendly default from the OS, else a stable random handle.
      const n = Math.floor(Math.random() * 9000 + 1000);
      name = `friend-${n}`;
      localStorage.setItem("olympus-display-name", name);
    }
    return name;
  } catch {
    return null;
  }
}

export function connectWs(): void {
  if (ws || connecting) return;

  // In mock mode the MockWebSocket is installed on window.WebSocket, so
  // `new WebSocket()` creates a mock instance that speaks ServerFrame. We
  // still need to connect so frames flow through onFrame listeners.
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
      // In mock mode the MockWebSocket is a singleton-like stand-in; don't
      // reconnect (the mock never closes during normal operation).
      const useMocks = import.meta.env.VITE_USE_MOCKS !== "false";
      if (!useMocks) setTimeout(() => connectWs(), 2000);
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

export type { Session, Message, SearchHit, ModelInfo, ServerFrame, ClientFrame, VaultSummary, NoteDocument, NoteTreeEntry };
