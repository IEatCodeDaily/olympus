import { http, HttpResponse, delay, WebSocketHandler } from "msw";
import {
  SESSIONS,
  MESSAGES_BY_SESSION,
  MODELS_LIST,
  AGENTS_LIST,
  USAGE_BY_RANGE,
  WORKFLOWS,
  WORKFLOW_RUNS,
  generateSearchHits,
  NODES,
  VAULTS,
  VAULT_NOTES,
  VAULT_NOTES_MUTABLE,
  buildNoteDoc,
} from "./fixtures";
import type {
  SessionListResponse,
  MessagesResponse,
  SearchResponse,
  ModelsResponse,
  AgentsResponse,
  HealthResponse,
  WorkflowsResponse,
  ServerFrame,
  UsageRange,
  UsageResponse,
  NodesResponse,
} from "../types";

// ── REST Handlers ─────────────────────────────────────

export const handlers = [
  // GET /api/sessions
  http.get("http://127.0.0.1:8787/api/sessions", async ({ request }: { request: Request }) => {
    const url = new URL(request.url);
    const sourceParam = url.searchParams.get("source");
    const model = url.searchParams.get("model");
    const archived = url.searchParams.get("archived"); // null = default false (hide archived)
    const showArchived = archived === "true";
    const q = url.searchParams.get("q");
    const sort = (url.searchParams.get("sort") ?? "lastActivity") as "lastActivity" | "startedAt" | "messageCount";

    let filtered = [...SESSIONS];

    const managedParam = url.searchParams.get("managed");
    if (managedParam === "true") filtered = filtered.filter((s) => s.managed);
    if (managedParam === "false") filtered = filtered.filter((s) => !s.managed);

    const nodeParam = url.searchParams.get("node");
    if (nodeParam) filtered = filtered.filter((s) => s.node === nodeParam);

    if (sourceParam) {
      const sources = sourceParam.split(",");
      filtered = filtered.filter((s) => sources.includes(s.source));
    }
    if (model) filtered = filtered.filter((s) => s.model === model);
    if (!showArchived) filtered = filtered.filter((s) => !s.archived);
    if (q) {
      const ql = q.toLowerCase();
      filtered = filtered.filter(
        (s) =>
          (s.title ?? "").toLowerCase().includes(ql) ||
          s.model?.toLowerCase().includes(ql)
      );
    }

    filtered.sort((a, b) => b[sort] - a[sort]);

    return HttpResponse.json<SessionListResponse>({
      sessions: filtered.slice(0, 50),
      nextCursor: null,
      total: filtered.length,
    });
  }),

  // GET /api/sessions/:id
  http.get<{ id: string }>("http://127.0.0.1:8787/api/sessions/:id", ({ params }: { params: { id: string } }) => {
    const sess = SESSIONS.find((s) => s.id === params.id);
    if (!sess) return new HttpResponse(null, { status: 404 });
    return HttpResponse.json(sess);
  }),

  // POST /api/sessions — optimistic create (no runtime spawned). Mirrors the
  // backend: instant draft with source=olympus, managed=true, empty hermesId,
  // optional agent/node binding from the body.
  http.post("http://127.0.0.1:8787/api/sessions", async ({ request }) => {
    const body = (await request.json().catch(() => ({}))) as {
      agent?: string;
      node?: string;
    };
    const now = Date.now() / 1000;
    const id = `oly-draft-${Math.floor(now * 1000)}`;
    const draft = {
      id,
      hermesId: "",
      orgId: "personal",
      ownerId: "rpw",
      contextId: null,
      source: "olympus" as const,
      model: null,
      title: null,
      startedAt: now,
      lastActivity: now,
      messageCount: 0,
      inputTokens: 0,
      outputTokens: 0,
      archived: false,
      forkedFrom: null,
      forkPoint: null,
      forkType: null,
      managed: true,
      agent: body.agent ?? null,
      node: body.node ?? null,
      liveness: "active" as const,
    };
    SESSIONS.unshift(draft);
    MESSAGES_BY_SESSION[id] = [];
    return HttpResponse.json(draft, { status: 201 });
  }),

  // PATCH /api/sessions/:id — bind/rebind agent, node, model, title.
  http.patch<{ id: string }>(
    "http://127.0.0.1:8787/api/sessions/:id",
    async ({ params, request }) => {
      const sess = SESSIONS.find((s) => s.id === params.id);
      if (!sess) return new HttpResponse(null, { status: 404 });
      const patch = (await request.json().catch(() => ({}))) as {
        agent?: string;
        node?: string;
        model?: string;
        title?: string;
      };
      if (patch.agent !== undefined) sess.agent = patch.agent;
      if (patch.node !== undefined) sess.node = patch.node;
      if (patch.model !== undefined) sess.model = patch.model;
      if (patch.title !== undefined) sess.title = patch.title;
      return HttpResponse.json(sess);
    }
  ),

  // POST /api/sessions/:id/messages — accept the prompt (202). Observed
  // sessions are read-only (409); managed sessions lazily "spawn" and echo a
  // reply over the next tick (mock).
  http.post<{ id: string }>(
    "http://127.0.0.1:8787/api/sessions/:id/messages",
    async ({ params, request }) => {
      const sess = SESSIONS.find((s) => s.id === params.id);
      if (!sess) return new HttpResponse(null, { status: 404 });
      if (!sess.managed) {
        return HttpResponse.json(
          {
            error: "observed",
            message:
              "This session is observed (read-only). Fork it into an Olympus-managed session to continue.",
          },
          { status: 409 }
        );
      }
      const body = (await request.json().catch(() => ({}))) as { text?: string };
      const now = Date.now() / 1000;
      const msgs = MESSAGES_BY_SESSION[params.id] ?? (MESSAGES_BY_SESSION[params.id] = []);
      msgs.push({
        messageId: msgs.length,
        sessionId: params.id,
        role: "user",
        content: body.text ?? "",
        toolName: null,
        toolCalls: null,
        reasoning: null,
        timestamp: now,
        tokenCount: null,
        finishReason: null,
      });
      sess.messageCount = msgs.length;
      sess.lastActivity = now;
      if (!sess.hermesId) sess.hermesId = `mock-${params.id}`;
      return HttpResponse.json({ accepted: true }, { status: 202 });
    }
  ),

  // POST /api/sessions/:id/fork
  http.post<{ id: string }>("http://127.0.0.1:8787/api/sessions/:id/fork", ({ params }) => {
    const source = SESSIONS.find((session) => session.id === params.id);
    if (!source) return new HttpResponse(null, { status: 404 });
    const id = `${source.id}-fork`;
    const now = Date.now() / 1000;
    const forked = {
      ...source,
      id,
      hermesId: `${source.hermesId}-fork`,
      source: "olympus" as const,
      forkedFrom: source.id,
      forkPoint: source.messageCount,
      forkType: "sub" as const,
      managed: true,
      archived: false,
      startedAt: now,
      lastActivity: now,
      liveness: "active" as const,
    };
    MESSAGES_BY_SESSION[id] = (MESSAGES_BY_SESSION[source.id] ?? []).map((message) => ({
      ...message,
      sessionId: id,
    }));
    SESSIONS.unshift(forked);
    return HttpResponse.json({ session: forked });
  }),

  // GET /api/sessions/:id/messages
  http.get<{ id: string }>(
    "http://127.0.0.1:8787/api/sessions/:id/messages",
    ({ params }: { params: { id: string } }) => {
      const msgs = MESSAGES_BY_SESSION[params.id] ?? [];
      return HttpResponse.json<MessagesResponse>({
        messages: msgs,
        nextCursor: null,
      });
    }
  ),

  // GET /api/search
  http.get("http://127.0.0.1:8787/api/search", async ({ request }: { request: Request }) => {
    const q = new URL(request.url).searchParams.get("q") ?? "";
    await delay(200 + Math.random() * 300); // simulate tantivy latency
    return HttpResponse.json<SearchResponse>({
      hits: generateSearchHits(q),
    });
  }),

  // GET /api/models
  http.get("http://127.0.0.1:8787/api/models", () => {
    return HttpResponse.json<ModelsResponse>({ models: MODELS_LIST });
  }),

  // GET /api/agents
  http.get("http://127.0.0.1:8787/api/agents", () => {
    return HttpResponse.json<AgentsResponse>({ agents: AGENTS_LIST });
  }),

  // GET /api/usage
  http.get("http://127.0.0.1:8787/api/usage", async ({ request }) => {
    const range = (new URL(request.url).searchParams.get("range") ?? "24h") as UsageRange;
    await delay(120 + Math.random() * 160);
    return HttpResponse.json<UsageResponse>(USAGE_BY_RANGE[range] ?? USAGE_BY_RANGE["24h"]);
  }),

  // GET /api/nodes
  http.get("http://127.0.0.1:8787/api/nodes", async () => {
    await delay(250 + Math.random() * 250);
    return HttpResponse.json<NodesResponse>({ nodes: NODES });
  }),

  // GET /api/health
  http.get("http://127.0.0.1:8787/api/health", () => {
    return HttpResponse.json<HealthResponse>({
      status: "ok",
      importState: "done",
      snapshot: { sessions: SESSIONS.length, messages: Object.values(MESSAGES_BY_SESSION).flat().length },
      syncConnected: true,
      hermesProfile: "tester",
    });
  }),

  // GET /api/workflows
  http.get("http://127.0.0.1:8787/api/workflows", async () => {
    await delay(220 + Math.random() * 220);
    return HttpResponse.json<WorkflowsResponse>({
      workflows: WORKFLOWS,
      runs: WORKFLOW_RUNS,
    });
  }),

  // ── Vaults ──────────────────────────────────────────

  // GET /api/vaults
  http.get("http://127.0.0.1:8787/api/vaults", async () => {
    await delay(80 + Math.random() * 60);
    return HttpResponse.json({ vaults: VAULTS });
  }),

  // POST /api/vaults
  http.post("http://127.0.0.1:8787/api/vaults", async ({ request }) => {
    const body = (await request.json()) as { name: string };
    const id = body.name.toLowerCase().replace(/\s+/g, "-").replace(/[^a-z0-9-]/g, "");
    const vault = { id, name: body.name, noteCount: 0, updatedAt: Math.floor(Date.now() / 1000) };
    VAULTS.push(vault);
    VAULT_NOTES_MUTABLE[id] = {};
    return HttpResponse.json(vault, { status: 201 });
  }),

  // GET /api/vaults/:id/notes
  http.get<{ id: string }>(
    "http://127.0.0.1:8787/api/vaults/:id/notes",
    ({ params }: { params: { id: string } }) => {
      const tree = VAULT_NOTES[params.id] ?? [];
      return HttpResponse.json({ notes: tree });
    },
  ),

  // GET /api/vaults/:id/note?path=...
  http.get<{ id: string }>(
    "http://127.0.0.1:8787/api/vaults/:id/note",
    ({ params, request }: { params: { id: string }; request: Request }) => {
      const path = new URL(request.url).searchParams.get("path") ?? "";
      const doc = buildNoteDoc(params.id, path);
      if (!doc) return new HttpResponse(null, { status: 404 });
      return HttpResponse.json(doc);
    },
  ),

  // PUT /api/vaults/:id/note?path=...
  http.put<{ id: string }>(
    "http://127.0.0.1:8787/api/vaults/:id/note",
    async ({ params, request }: { params: { id: string }; request: Request }) => {
      const path = new URL(request.url).searchParams.get("path") ?? "";
      const body = (await request.json()) as { markdown?: string; newPath?: string };
      const store = VAULT_NOTES_MUTABLE[params.id];
      if (!store) return new HttpResponse(null, { status: 404 });
      if (body.markdown !== undefined) {
        store[path] = body.markdown;
      }
      // Rename support
      const targetPath = body.newPath ?? path;
      if (body.newPath && body.newPath !== path) {
        store[body.newPath] = store[path] ?? body.markdown ?? "";
        delete store[path];
      }
      const doc = buildNoteDoc(params.id, targetPath);
      if (!doc) return new HttpResponse(null, { status: 404 });
      return HttpResponse.json(doc);
    },
  ),

  // DELETE /api/vaults/:id/note?path=...
  http.delete<{ id: string }>(
    "http://127.0.0.1:8787/api/vaults/:id/note",
    ({ params, request }: { params: { id: string }; request: Request }) => {
      const path = new URL(request.url).searchParams.get("path") ?? "";
      const store = VAULT_NOTES_MUTABLE[params.id];
      if (!store) return new HttpResponse(null, { status: 404 });
      delete store[path];
      return new HttpResponse(null, { status: 204 });
    },
  ),
];
