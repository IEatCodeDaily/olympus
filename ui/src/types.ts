// Core shared types — VERBATIM from docs/api-contract.md
// DO NOT diverge from this. If the contract changes, update both here and the contract doc.

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
  | "cli" | "telegram" | "discord" | "webui" | "cron" | "subagent" | "api_server" | "acp" | "olympus";

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

export type ServerFrame =
  | { kind: "hello"; snapshot: { sessions: number; messages: number } }
  | { kind: "session.added"; session: Session }
  | { kind: "session.updated"; sessionId: string; changes: Partial<Session> }
  | { kind: "session.removed"; sessionId: string }
  | { kind: "message.appended"; sessionId: string; message: Message }
  | { kind: "message.delta"; sessionId: string; messageId: number; textDelta: string }
  | { kind: "message.done"; sessionId: string; messageId: number; finishReason: string | null }
  | { kind: "sync.status"; connected: boolean };

export type ClientFrame =
  | { kind: "subscribe"; sessionId: string }
  | { kind: "unsubscribe"; sessionId: string };

// API response shapes
export interface SessionListResponse {
  sessions: Session[];
  nextCursor: string | null;
  total: number;
}

export interface MessagesResponse {
  messages: Message[];
  nextCursor: string | null;
}

export interface SearchResponse {
  hits: SearchHit[];
}

export interface ModelsResponse {
  models: ModelInfo[];
}

export interface HealthResponse {
  status: "ok";
  importState: "idle" | "running" | "done";
  snapshot: { sessions: number; messages: number } | null;
  syncConnected: boolean;
  hermesProfile: string;
}

export type SessionSort = "lastActivity" | "startedAt" | "messageCount";

export interface SessionListParams {
  source?: string;        // comma-separated SessionSource values
  model?: string;
  archived?: boolean;
  q?: string;
  sort?: SessionSort;
  cursor?: string;
  limit?: number;
}

export interface MessagesParams {
  cursor?: string;
  limit?: number;
}

export interface SearchParams {
  q: string;
  limit?: number;
  includeArchived?: boolean;
}
