import { http, HttpResponse, delay, WebSocketHandler } from "msw";
import { MockWebSocket } from "./ws-mock";
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
  VAULT_NOTES_MUTABLE,
  buildNoteDoc,
  buildVaultNoteTree,
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
  ToolCall,
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
    const pinnedParam = url.searchParams.get("pinned");
    if (pinnedParam === "true") filtered = filtered.filter((s) => s.pinned);
    if (pinnedParam === "false") filtered = filtered.filter((s) => !s.pinned);
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
      pinned: false,
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
        archived?: boolean;
        pinned?: boolean;
      };
      if (patch.agent !== undefined) sess.agent = patch.agent;
      if (patch.node !== undefined) sess.node = patch.node;
      if (patch.model !== undefined) sess.model = patch.model;
      if (patch.title !== undefined) sess.title = patch.title;
      if (patch.archived !== undefined) sess.archived = patch.archived;
      if (patch.pinned !== undefined) sess.pinned = patch.pinned;
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
      const userMsg = {
        messageId: msgs.length,
        sessionId: params.id,
        role: "user" as const,
        content: body.text ?? "",
        toolName: null,
        toolCalls: null,
        reasoning: null,
        timestamp: now,
        tokenCount: null,
        finishReason: null,
      };
      msgs.push(userMsg);
      sess.messageCount = msgs.length;
      sess.lastActivity = now;
      if (!sess.hermesId) sess.hermesId = `mock-${params.id}`;

      // Broadcast the user message.appended frame so live listeners see it.
      MockWebSocket.broadcast({ kind: "message.appended", sessionId: params.id, message: userMsg });

      // Simulate an assistant turn with tool calls after a short delay.
      // This drives the Output tab (tool activity) and Debug tab (raw frames).
      const assistantId = msgs.length;
      setTimeout(() => {
        const toolCalls = mockToolCallsForPrompt(body.text ?? "");
        const replyMsg = {
          messageId: assistantId,
          sessionId: params.id,
          role: "assistant" as const,
          content: "Done. I ran the necessary tools — see the Output panel for details.",
          toolName: null,
          toolCalls,
          reasoning: null,
          timestamp: Date.now() / 1000,
          tokenCount: 42,
          finishReason: null,
        };
        msgs.push(replyMsg);
        sess.messageCount = msgs.length;
        MockWebSocket.broadcast({
          kind: "message.appended",
          sessionId: params.id,
          message: replyMsg,
        });
        // Follow up with message.done so the transcript + query cache update.
        MockWebSocket.broadcast({
          kind: "message.done",
          sessionId: params.id,
          messageId: assistantId,
          finishReason: "stop",
        });
      }, 1500);

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
    const body = (await request.json()) as {
      name: string;
      backend: { kind: "github"; repository: string; branch: string; syncEngine: "jj-git" };
    };
    if (!body.name?.trim() || !body.backend?.repository?.includes("/")) {
      return HttpResponse.json(
        { message: "Name and GitHub owner/repository are required" },
        { status: 400 },
      );
    }
    const id = body.name.toLowerCase().replace(/\s+/g, "-").replace(/[^a-z0-9-]/g, "");
    const vault = {
      id,
      name: body.name,
      noteCount: 0,
      updatedAt: Math.floor(Date.now() / 1000),
      backend: body.backend,
    };
    VAULTS.push(vault);
    VAULT_NOTES_MUTABLE[id] = {};
    return HttpResponse.json(vault, { status: 201 });
  }),

  // GET /api/vaults/:id/notes
  http.get<{ id: string }>(
    "http://127.0.0.1:8787/api/vaults/:id/notes",
    ({ params }: { params: { id: string } }) => {
      const tree = buildVaultNoteTree(params.id);
      return HttpResponse.json({ notes: tree });
    },
  ),

  // GET /api/vaults/:id/documents
  http.get<{ id: string }>(
    "http://127.0.0.1:8787/api/vaults/:id/documents",
    ({ params }: { params: { id: string } }) => {
      const store = VAULT_NOTES_MUTABLE[params.id];
      if (!store) return new HttpResponse(null, { status: 404 });
      const documents = Object.keys(store).sort().map((path) => {
        const document = buildNoteDoc(params.id, path)!;
        return {
          path,
          title: document.title,
          updatedAt: Math.floor(Date.now() / 1000),
          frontmatter: document.frontmatter,
        };
      });
      return HttpResponse.json({ documents });
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
      const body = (await request.json()) as { markdown?: string; newPath?: string; createOnly?: boolean };
      const store = VAULT_NOTES_MUTABLE[params.id];
      if (!store) return new HttpResponse(null, { status: 404 });
      if (body.createOnly && store[path] !== undefined) {
        return HttpResponse.json({ message: "note already exists" }, { status: 409 });
      }
      if (body.markdown !== undefined) {
        const isNew = store[path] === undefined;
        store[path] = body.markdown;
        if (isNew) {
          const vault = VAULTS.find((candidate) => candidate.id === params.id);
          if (vault) vault.noteCount += 1;
        }
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
      const existed = store[path] !== undefined;
      delete store[path];
      if (existed) {
        const vault = VAULTS.find((candidate) => candidate.id === params.id);
        if (vault) vault.noteCount = Math.max(0, vault.noteCount - 1);
      }
      return new HttpResponse(null, { status: 204 });
    },
  ),

  // GET /api/vaults/:id/graph
  http.get<{ id: string }>(
    "http://127.0.0.1:8787/api/vaults/:id/graph",
    ({ params }: { params: { id: string } }) => {
      const store = VAULT_NOTES_MUTABLE[params.id];
      const nodes = store
        ? Object.entries(store).map(([path, md]) => {
            const titleMatch = md.match(/^# (.+)$/m);
            const title = titleMatch ? titleMatch[1] : path.replace(/\.md$/, "");
            return { id: title, title, path, cid: null, linkCount: (md.match(/\[\[/g) ?? []).length };
          })
        : [];
      const edges: Array<{ source: string; target: string }> = [];
      if (store) {
        for (const [, md] of Object.entries(store)) {
          const titleMatch = md.match(/^# (.+)$/m);
          const title = titleMatch ? titleMatch[1] : "";
          const linkMatches = md.matchAll(/\[\[([^\]]+)\]\]/g);
          for (const m of linkMatches) {
            const target = m[1]?.split("|")[0]?.split("#")[0]?.trim();
            if (target) edges.push({ source: title, target });
          }
        }
      }
      return HttpResponse.json({ nodes, edges });
    },
  ),

  // GET /api/vaults/:id/collections
  http.get<{ id: string }>(
    "http://127.0.0.1:8787/api/vaults/:id/collections",
    ({ params }: { params: { id: string } }) => {
      const store = VAULT_NOTES_MUTABLE[params.id];
      const collections = store
        ? Object.entries(store)
            .filter(([, md]) => md.includes("collection: true"))
            .map(([path, md]) => ({
              name: path,
              path,
              rowCount: 3,
            }))
        : [];
      return HttpResponse.json({ collections });
    },
  ),

  // GET /api/vaults/:id/collections/:path
  http.get<{ id: string; path: string }>(
    "http://127.0.0.1:8787/api/vaults/:id/collections/:path",
    ({ params }: { params: { id: string; path: string } }) => {
      // Mock: return a few sample rows
      return HttpResponse.json({
        columns: ["status", "priority", "tags"],
        rows: [
          { title: "Row 1", path: "row-1.md", status: "active", priority: "high", tags: "a,b" },
          { title: "Row 2", path: "row-2.md", status: "done", priority: "low", tags: "c" },
          { title: "Row 3", path: "row-3.md", status: "active", priority: "med", tags: "d,e" },
        ],
      });
    },
  ),
];

/**
 * Generate plausible tool calls for a mock assistant reply, based on the
 * prompt text. Used to drive the Output panel with realistic activity.
 */
function mockToolCallsForPrompt(prompt: string): ToolCall[] {
  const p = prompt.toLowerCase();
  const calls: ToolCall[] = [];

  if (p.includes("search") || p.includes("find") || p.includes("look")) {
    calls.push({
      name: "web_search",
      args: { query: prompt.slice(0, 60), limit: 5 },
      result: '[{"title":"Relevant result","url":"https://example.com/1"},{"title":"Another result","url":"https://example.com/2"}]',
    });
  }
  if (p.includes("file") || p.includes("read") || p.includes("code")) {
    calls.push({
      name: "read_file",
      args: { path: "/home/rpw/project/src/main.ts" },
      result: "import { app } from './app';\napp.listen(3000);",
    });
  }
  if (p.includes("run") || p.includes("execute") || p.includes("command")) {
    calls.push({
      name: "terminal",
      args: { command: "npm run build" },
      result: "> olympus@0.1.0 build\n> tsc && vite build\n✓ built in 2.3s",
    });
  }
  // Always include at least one call so the Output tab shows activity.
  if (calls.length === 0) {
    calls.push({
      name: "terminal",
      args: { command: "echo 'processing request'" },
      result: "processing request",
    });
  }
  return calls;
}
