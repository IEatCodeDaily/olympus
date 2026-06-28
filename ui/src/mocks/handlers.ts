import { http, HttpResponse, delay, WebSocketHandler } from "msw";
import {
  SESSIONS,
  MESSAGES_BY_SESSION,
  MODELS_LIST,
  generateSearchHits,
} from "./fixtures";
import type {
  SessionListResponse,
  MessagesResponse,
  SearchResponse,
  ModelsResponse,
  HealthResponse,
  ServerFrame,
} from "../types";

// ── REST Handlers ─────────────────────────────────────

export const handlers = [
  // GET /api/sessions
  http.get("http://127.0.0.1:8787/api/sessions", async ({ request }) => {
    const url = new URL(request.url);
    const sourceParam = url.searchParams.get("source");
    const model = url.searchParams.get("model");
    const archived = url.searchParams.get("archived"); // null = default false (hide archived)
    const showArchived = archived === "true";
    const q = url.searchParams.get("q");
    const sort = (url.searchParams.get("sort") ?? "lastActivity") as "lastActivity" | "startedAt" | "messageCount";

    let filtered = [...SESSIONS];

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
  http.get<{ id: string }>("http://127.0.0.1:8787/api/sessions/:id", ({ params }) => {
    const sess = SESSIONS.find((s) => s.id === params.id);
    if (!sess) return new HttpResponse(null, { status: 404 });
    return HttpResponse.json(sess);
  }),

  // GET /api/sessions/:id/messages
  http.get<{ id: string }>(
    "http://127.0.0.1:8787/api/sessions/:id/messages",
    ({ params }) => {
      const msgs = MESSAGES_BY_SESSION[params.id] ?? [];
      return HttpResponse.json<MessagesResponse>({
        messages: msgs,
        nextCursor: null,
      });
    }
  ),

  // GET /api/search
  http.get("http://127.0.0.1:8787/api/search", async ({ request }) => {
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
];
