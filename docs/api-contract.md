# Olympus API Contract (MVP) — the seam between control plane and UI

> **Purpose:** lock the wire shape so the React UI can be built against a **mock**
> in parallel with the real Rust control-plane server (Phase 3). This contract is
> the source of truth for both sides. Derived from ADR 0002 §10.1 (tables) +
> §10.3.1 (delta streaming) + §3.5 (tenancy) + the PRD features. Changes here are
> breaking — update both sides together.
>
> Status: MVP. New chat, send, streaming, and fork-to-continue are wired; fields
> marked `(post-spike)` are not wired until those land.

## Transport & auth

- **REST** for queries/mutations; **WSS** (`/ws`) for the reactive delta stream.
- **Auth gate (mandatory, MVP):** localhost bind by default; per-install bearer
  token (`~/.olympus/token`, mode 0600). REST: `Authorization: Bearer <token>`.
  WS: `?token=<token>` query param (browsers can't set headers on WS upgrade).
  Strict `Origin`/`Host` checks on `/ws` + all `/api/*`. Unauth → `401`.
- Base URL (dev): `http://127.0.0.1:8787`. All paths below are under it.
- Every object carries tenancy fields (`orgId`, `ownerId`) per ADR §3.5; MVP
  values are `orgId:"personal"`, `ownerId:"rpw"`. UI shows them but does not gate
  on them yet (single-operator).

## Core types (TypeScript — shared contract; the UI imports these)

```ts
// A session as the UI consumes it (projection of the event log; ADR §10.1).
export interface Session {
  id: string;                 // Olympus session id
  hermesId: string;           // underlying Hermes session id
  orgId: string;              // "personal" in MVP
  ownerId: string;            // "rpw" in MVP
  contextId: string | null;   // null until contexts exist
  source: SessionSource;      // origin channel
  model: string | null;
  title: string | null;       // null → UI shows first-message preview
  startedAt: number;          // epoch seconds (float ok)
  lastActivity: number;       // epoch seconds; drives default sort
  messageCount: number;
  inputTokens: number;
  outputTokens: number;
  archived: boolean;
  // fork lineage (ADR §6.6) — null for non-forked sessions
  forkedFrom: string | null;  // source session id
  forkPoint: number | null;   // message index the fork branched at
  forkType: "sub" | "parallel" | null;
  // origin marker for forks: "forked from telegram", etc. (PRD Flow B)
  managed: boolean;           // true = Olympus-driven (steerable); false = observed/read-only
}

export type SessionSource =
  | "cli" | "telegram" | "discord" | "webui" | "cron" | "subagent" | "api_server" | "acp";

export interface Message {
  messageId: number;          // monotonic within session
  sessionId: string;
  role: "user" | "assistant" | "tool" | "system" | "session_meta";
  content: string | null;     // decompressed by the server
  toolName: string | null;
  toolCalls: ToolCall[] | null;
  reasoning: string | null;
  timestamp: number;          // epoch seconds
  tokenCount: number | null;
  finishReason: string | null;
}

export interface ToolCall {
  name: string;
  args: string;               // JSON string as stored
  result: string | null;      // null while running
}

export interface SearchHit {
  sessionId: string;
  messageId: number;
  source: SessionSource;
  snippet: string;            // highlighted excerpt (tantivy)
  score: number;
  timestamp: number;
}

export interface ModelInfo {
  provider: string;
  model: string;
  displayName: string;
}

export interface AgentInfo {
  id: string;                 // "default", Hermes profile name, "claude-code", "codex"
  provider: string | null;    // e.g. "anthropic", "claude-code", "openai-codex"
  model: string | null;       // configured model for Hermes; CLI version for CLI harnesses
  kind: "hermes" | "claude-code" | "codex";
  isDefault: boolean;         // true only for the implicit root Hermes profile
}

export interface VaultSummary {
  id: string;        // slug generated from the vault name
  name: string;
  noteCount: number;
  updatedAt: number; // epoch seconds
  backend: {
    kind: "github";
    repository: string; // canonical owner/repository, never credentials
    branch: string;
    syncEngine: "jj-git";
  } | null;          // null only for legacy/unconfigured vaults
}

export interface NoteTreeEntry {
  path: string;      // markdown path relative to vault root
  title: string;
  updatedAt: number; // epoch seconds
  kind: "folder" | "note";
  children: NoteTreeEntry[];
}

export interface NoteDocument {
  path: string;
  title: string;     // frontmatter title, H1, or file stem fallback
  markdown: string;  // full markdown including frontmatter
  frontmatter: Record<string, unknown>;
  linkedNotes: string[]; // wikilinks + simple markdown links, normalized for graphing
}
```

## REST endpoints (MVP)

### Queries (read-only — available in phase 1)

```
GET /api/sessions
  ?source=telegram,cli         # multi-select, comma-sep (optional)
  &model=glm-5.2               # optional
  &archived=false             # default false
  &q=<text>                   # optional free-text (server runs tantivy if set)
  &sort=lastActivity|startedAt|messageCount   # default lastActivity desc
  &cursor=<opaque>&limit=50   # pagination (virtualized list)
  → 200 { "sessions": Session[], "nextCursor": string | null, "total": number }

GET /api/sessions/:id
  → 200 Session   | 404

GET /api/sessions/:id/messages
  ?cursor=<opaque>&limit=50   # paginate; default = latest 50, scroll-back older
  → 200 { "messages": Message[], "nextCursor": string | null }

GET /api/search
  ?q=<text>&limit=50&includeArchived=false
  → 200 { "hits": SearchHit[] }   # grouped client-side by sessionId

GET /api/models
  → 200 { "models": ModelInfo[] }   # from Hermes config/CLI

GET /api/agents
  → 200 { "agents": AgentInfo[] }    # Hermes profiles + discovered local CLI harnesses

GET /api/health
  → 200 { "status":"ok", "importState": "idle"|"running"|"done",
          "snapshot": { "sessions": number, "messages": number } | null,
          "syncConnected": boolean, "hermesProfile": string }

GET /api/metrics                       # process + store stats (unauth, scrapeable)
  → 200 { "rssKb": number|null, "threads": number|null, "cpuTicks": number|null,
          "wsSubscribers": number, "snapshot": {...}, "syncConnected": boolean,
          "inFlight": number }

GET /api/events                        # tail-able event log (replication spine, ADR 0006)
  ?since=<seq>&limit=<n>               # since is an exclusive cursor; limit ≤ 5000
  → 200 { "events": [{ "seq": number, "event": {...} }], "next": number|null }
  # next is null at the head (caller is caught up)

GET /api/setup                         # declared agent setup (ADR 0006 §3)
  ?scope=org:<org> | ?scope=project:<org>/<project>   # one scope's raw declaration
  ?org=<org>&project=<project>         # OR: the merged EFFECTIVE setup (org + project)
  → 200 Setup    # an undeclared scope returns an empty Setup, not 404

GET /api/vaults                        # markdown-first knowledge vaults (ADR 0004)
  → 200 { "vaults": VaultSummary[] }

GET /api/vaults/:id/notes
  → 200 { "notes": NoteTreeEntry[] }  # recursive folder/note tree

GET /api/vaults/:id/documents
  → 200 { "documents": Array<{ path, title, updatedAt, frontmatter }> }
  # vault-wide derived index; does not duplicate Markdown bodies

GET /api/vaults/:id/note?path=<relative-markdown-path>
  → 200 NoteDocument | 404
```

Where `Setup` is:
```ts
interface Setup {
  scope: string;      // "org:<org>" | "project:<org>/<project>"
  skills: string[];   // active skill slugs (refs into the skill library)
  mcp: string[];      // active MCP server slugs
  plugins: string[];  // active plugin slugs (LSP, codegraph, services, installers)
  hooks: string[];    // active hook slugs
  declaredAt: number; // epoch seconds; 0 for an undeclared/empty scope
}
```

### Mutations

```
POST /api/sessions                     # start a new Olympus-managed chat
  body {}
  → 201 Session

POST /api/sessions/:id/fork            # cross-channel continuation
  body { forkType: "sub"|"parallel", forkPoint?: number }
  → 200 { "session": Session }         # the new managed fork; source untouched

POST /api/sessions/:id/messages        # drive a MANAGED session
  body { text: string, model?: string }
  → 202 { "accepted": true }           # response streams over /ws
  → 409 if session is not `managed` (observed sessions must be forked first)

PUT /api/setup                         # declare (set/replace) a scope's agent setup (ADR 0006 §3)
  body { scope: string, skills?: string[], mcp?: string[],
         plugins?: string[], hooks?: string[] }   # PUT = full replace of the scope
  → 200 Setup                          # the stored declaration
  → 400 if scope is not "org:<slug>" or "project:<org>/<project>"

POST /api/vaults                       # create a jj-colocated markdown vault
  body { name: string,
         backend: { kind: "github", repository: "owner/repository",
                    branch: "main", syncEngine: "jj-git" } }
  → 201 VaultSummary

PUT /api/vaults/:id/note?path=<relative-markdown-path>
  body { markdown?: string, newPath?: string, createOnly?: boolean }
  # write and/or rename; createOnly fails rather than overwriting an existing note
  → 200 NoteDocument
  → 400 if path escapes the vault root or a new note omits markdown
  → 409 if createOnly is true and the note already exists

DELETE /api/vaults/:id/note?path=<relative-markdown-path>
  → 204


POST /api/sessions/:id/cancel          # (post-spike) → ACP session/cancel
POST /api/sessions/:id/model           # (post-spike) body { model } → ACP session/set_model
POST /api/sessions/:id/steer           # (post-spike) body { text } → "/steer" prompt text
POST /api/sessions/:id/archive         # body { archived: bool }
```

## WSS delta stream (`/ws`) — reactivity (ADR §10.3.1)

Client connects `ws://127.0.0.1:8787/ws?token=…`, optionally subscribes to a
session's message stream. Server pushes JSON frames. **Envelope:**

```ts
export type ServerFrame =
  | { kind: "hello"; snapshot: { sessions: number; messages: number } }
  | { kind: "session.added"; session: Session }
  | { kind: "session.updated"; sessionId: string; changes: Partial<Session> }
  | { kind: "session.removed"; sessionId: string }     // tombstone (active=0 upstream)
  | { kind: "message.appended"; sessionId: string; message: Message }
  | { kind: "message.delta"; sessionId: string; messageId: number; textDelta: string } // streaming token
  | { kind: "message.done"; sessionId: string; messageId: number; finishReason: string | null }
  | { kind: "sync.status"; connected: boolean };

export type ClientFrame =
  | { kind: "subscribe"; sessionId: string }    // start receiving this session's message frames
  | { kind: "unsubscribe"; sessionId: string };
```

- The session-list view subscribes implicitly (gets all `session.*` frames).
- The chat view sends `subscribe {sessionId}` to receive `message.*` frames for
  the open session; `message.delta` streams tokens (UI applies smoothing).
- Ordering: frames for a given session arrive in order; `message.delta` always
  precedes its `message.done`.

## Mock contract (for parallel UI dev)

The UI ships an **MSW (Mock Service Worker)** layer implementing every endpoint +
a fake `/ws` that replays a scripted session stream, seeded from a fixtures file
(`ui/src/mocks/fixtures.ts`) shaped exactly like the types above. This lets the UI
be built, demoed, and tested with zero backend. When the real server lands, flip
one env flag (`VITE_USE_MOCKS=0`) — the types are identical, so no UI rewrite.

## Open items (resolve before freezing v1)

- Pagination cursor encoding (opaque base64 of `(sort_key, id)`) — server decides;
  UI treats it as opaque.
- Search hit grouping/snippet length — tune after tantivy lands.
- `message.delta` batching cadence (~100ms server-side per §10.3) — UI must not
  assume per-token frames.
