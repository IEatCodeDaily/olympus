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
  pinned: boolean;            // manual pin (sidebar PINNED section); user-set, never derived
  // fork lineage (ADR §6.6) — null for non-forked sessions
  forkedFrom: string | null;  // source session id
  forkPoint: number | null;   // message index the fork branched at
  forkType: "sub" | "parallel" | null;
  // origin marker for forks: "forked from telegram", etc. (PRD Flow B)
  managed: boolean;           // true = Olympus-driven (steerable); false = observed/read-only
  agent: string | null;       // Hermes profile bound to this session (assignable)
  node: string | null;        // node the runtime runs on ("local" for now)
  liveness?: "running" | "input-required" | "active" | "idle"; // managed: running/input-required from bridge; observed: active from recency; else idle
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
  /** Invocation id (provider-issued; may be absent). */
  id?: string | null;
  /** Tool/function name, e.g. "terminal", "patch", "web_search". */
  name: string;
  /** Parsed arguments (already an object; backend normalizes OpenAI's string form). */
  args: unknown;
  /** Display label when the provider gives one, else omitted. */
  label?: string | null;
  /** Lifecycle: "pending" (awaiting permission/queued) | "in_progress" | "completed" | "failed". */
  status?: string | null;
  /** Codepoint offset into the assistant text where this call fired — used to
   *  interleave the card chronologically inside the final message. */
  anchor?: number | null;
  /** Tool result, when known. null/absent while the call is in-flight. */
  result?: string | null;
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
  /** Model id, e.g. "claude-opus-4-8", "gpt-5.4". */
  id: string;
  /** Provider the model was seen under, e.g. "anthropic", "openai-codex". */
  provider: string | null;
}

/** A drivable agent with its configured provider + model/version. */
export interface AgentInfo {
  /** Agent id passed to POST /api/sessions { agent }. */
  id: string;
  provider: string | null;
  model: string | null;
  kind: "hermes" | "claude-code" | "codex";
  /** CLI harness auth readiness: true = credentials found, false = needs login, undefined = n/a (hermes profiles). */
  ready?: boolean;
  /** True for the implicit root profile the server runs as by default. */
  isDefault: boolean;
}

export interface AgentsResponse {
  agents: AgentInfo[];
}

export type NodeStatus = "online" | "draining" | "offline";

export interface NodeInfo {
  nodeId: string;
  hostname: string;
  status: NodeStatus;
  slotsUsed: number;
  slotsTotal: number;
  version: string;
  local: boolean;
  lastHeartbeatAgoSecs: number;
  // Agents this node's envoy discovered on its host (per-node, not global).
  // Optional: a remote node may not have reported yet.
  agents?: AgentInfo[];
}

export interface NodesResponse {
  nodes: NodeInfo[];
}

export type ServerFrame =
  | { kind: "hello"; snapshot: { sessions: number; messages: number } }
  | { kind: "session.added"; session: Session }
  | { kind: "session.updated"; sessionId: string; changes: Partial<Session> }
  | { kind: "session.removed"; sessionId: string }
  | { kind: "message.appended"; sessionId: string; message: Message }
  | { kind: "message.delta"; sessionId: string; messageId: number; textDelta: string }
  | { kind: "message.toolCall"; sessionId: string; messageId: number; toolCall: ToolCall }
  | { kind: "message.reasoning"; sessionId: string; messageId: number; textDelta: string }
  | { kind: "message.done"; sessionId: string; messageId: number; finishReason: string | null }
  | { kind: "session.log"; sessionId: string; level: "info" | "warn" | "error"; source: string; message: string; timestamp: number }
  | { kind: "sync.status"; connected: boolean }
  | { kind: "cards.changed" }
  | {
      kind: "permission.required";
      sessionId: string;
      toolCall: string;
      options: Array<{ optionId: string; name: string; kind: string }>;
    };

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

export interface UsageSummary {
  model: string;
  provider: string;
  tokensIn: number;
  tokensOut: number;
  estCost: number;
  subscriptionLimit: number;
  used: number;
}

export type UsageRange = "24h" | "7d" | "30d";

export interface UsageResponse {
  range: UsageRange;
  generatedAt: number;
  summaries: UsageSummary[];
}

export interface HealthResponse {
  status: "ok";
  importState: "idle" | "running" | "done";
  snapshot: { sessions: number; messages: number } | null;
  syncConnected: boolean;
  hermesProfile: string;
}

export type CardStatus = "todo" | "assigned" | "claimed" | "blocked" | "done";

export interface Card {
  id: string;
  boardId: string;
  title: string;
  status: CardStatus;
  assignedId: string | null;
  assignedKind: string | null;
  currentSessionId: string | null;
  currentBookmark: string | null;
  blockedBy: string[];
  priority: number;
  createdAt: number;
  statusChangedAt: number;
}

export interface CardListResponse {
  cards: Card[];
  total: number;
}

export interface CreateCardBody {
  boardId: string;
  title: string;
}

export interface AssignCardBody {
  assignedId: string;
  assignedKind: string;
  sessionId: string;
  attemptBookmark: string;
}

export interface BlockCardBody {
  blockedBy: string[];
}

export interface ReassignCardBody extends AssignCardBody {
  previousSessionId: string;
}

// Workflows (U5, mock-first — backend Epic H)
export interface Workflow {
  id: string;
  name: string;
  description: string;
  stepCount: number;
}

export type WorkflowRunStatus = "running" | "done" | "failed";
export type WorkflowStepStatus = "pending" | "running" | "done" | "failed";

export interface WorkflowRunStep {
  id: string;
  label: string;
  status: WorkflowStepStatus;
}

export interface WorkflowRun {
  id: string;
  workflowId: string;
  status: WorkflowRunStatus;
  startedAt: number;
  steps: WorkflowRunStep[];
}

export interface WorkflowsResponse {
  workflows: Workflow[];
  runs: WorkflowRun[];
}

export type SessionSort = "lastActivity" | "startedAt" | "messageCount";

export interface SessionListParams {
  source?: string;        // comma-separated SessionSource values
  model?: string;
  archived?: boolean;
  pinned?: boolean;
  /** true → Olympus-managed sessions only; false → imported history only. */
  managed?: boolean;
  /** Filter to sessions running on a specific node by nodeId. */
  node?: string;
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

// ---- Setup declaration (ADR 0006 §3) ----

export interface SetupScope {
  scope: string;
  skills: string[];
  mcp: string[];
  plugins: string[];
  hooks: string[];
  declaredAt: number;
}

export interface SetupResponse {
  scopes: SetupScope[];
}

export interface SetupQueryParams {
  scope?: string;
  effective?: boolean;
  org?: string;
  project?: string;
}

export interface PutSetupBody {
  scope: string;
  skills: string[];
  mcp: string[];
  plugins: string[];
  hooks: string[];
}

// ---- Registry (ADR 0006 §9.4) ----

export interface RegistryEntry {
  kind: string;
  slug: string;
  definition: string;
  registeredAt: number;
}

export interface RegistryResponse {
  entries: RegistryEntry[];
}

export interface RegistryQueryParams {
  kind?: string;
  slug?: string;
}

export interface PutRegistryBody {
  kind: string;
  slug: string;
  definition: string;
}

// ---- Vaults (ADR 0004 — markdown-first knowledge base) ----

export interface VaultSummary {
  id: string;
  name: string;
  noteCount: number;
  updatedAt: number;
}

export interface VaultsResponse {
  vaults: VaultSummary[];
}

export type NoteTreeEntryKind = "folder" | "note";

export interface NoteTreeEntry {
  path: string;
  title: string;
  updatedAt: number;
  kind: NoteTreeEntryKind;
  children: NoteTreeEntry[];
}

export interface NotesTreeResponse {
  notes: NoteTreeEntry[];
}

export interface NoteDocument {
  path: string;
  title: string;
  markdown: string;
  frontmatter: Record<string, unknown>;
  linkedNotes: string[];
}

export interface PutNoteBody {
  markdown?: string;
  newPath?: string;
}
