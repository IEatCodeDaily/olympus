import type {
  Session,
  Message,
  ToolCall,
  ModelInfo,
  SearchHit,
} from "../types";

// ── Helpers ────────────────────────────────────────────

const SOURCES: Array<"cli" | "telegram" | "discord" | "webui" | "cron" | "subagent" | "api_server" | "acp"> = [
  "cli",
  "telegram",
  "discord",
  "webui",
  "cron",
  "subagent",
  "api_server",
  "acp",
];

const MODELS = [
  "glm-5v-turbo",
  "claude-sonnet-4-20250514",
  "gpt-5.2",
  "gemini-2.5-pro",
  "deepseek-r1-0528",
  "llama-4-maverick",
];

const TITLES = [
  null, // forces first-message preview
  "Debugging Hermes gateway timeout issue",
  "Grafana dashboard — Redpanda lag panel",
  "UCollect Box HiL firmware update plan",
  "Noovoleum capability map review",
  "Fork spike on state.db copy",
  "Olympus MVP architecture workshop",
  "Payment Go migration discussion",
  "ESP32-S3 low-power IoT design session",
  "Kanban board cleanup sprint",
  "Docker compose infra audit",
  "React frontend DX overhaul planning",
  "Tauri v2 desktop bootstrap research",
  "AWS SSO login flow debugging",
  "GitNexus code intelligence indexing",
  "MCP server integration testing",
  "Self-hosted service deployment guide",
  "Domain intel reconnaissance report",
  "ML paper writing — NeurIPS draft",
  "Axolotl fine-tuning experiment config",
];

const USER_MESSAGES = [
  "Can you check why the Grafana datasource is returning 401s?",
  "Run a full infrastructure audit across all accounts.",
  "I need to update the Traefik IngressRoute for monitoring.",
  "What's the current state of the UCollect backend migration?",
  "Help me debug this ESP32 firmware — it's not entering deep sleep.",
  "Draft a PR description for the Olympus control plane PR.",
  "Search for any sessions mentioning 'fork' in the last week.",
  "Set up a cron job to monitor the Redis cluster health.",
  "Can we move the payment module to Go? The Node version is painful.",
  "Review the latest changes to the kanban orchestrator skill.",
  "I want to fork this Telegram session into an ACP-managed one.",
  "Generate a capability map diagram for the engineering team.",
  "Check if the node-exporter is deployed in the EKS cluster.",
  "Write a test plan for the session fork feature.",
  "What models are currently available through the Hermes adapter?",
  "Deploy the updated dashboard to staging.",
  "Investigate the memory leak in the Bun runtime process.",
  "Create a new skill for the Grafana MCP observability workflow.",
  "Parse the latest CloudWatch metrics for the API gateway.",
  "Set up bytebase access so I can review the schema changes.",
];

const ASSISTANT_SNIPPETS = [
  "Looking at the Grafana configuration, the 401 is likely due to the org-scoped token. Let me check the datasource UID mapping...",
  "I've initiated the AWS infrastructure audit. Scanning all accounts now — this will take a few minutes.",
  "The Traefik IngressRoute needs to be in the `monitoring` namespace. Here's the updated YAML:",
  "The UCollect backend migration is at 67% completion. The remaining work involves the balance module and payout rail integration.",
  "Deep sleep issues on ESP32-S3 are usually caused by GPIO hold or UART peripheral still being active. Let me check your power management config.",
  "Here's a draft PR description for the Olympus control plane. I've focused on the redb event log and in-memory view projections.",
  "Found 23 sessions mentioning 'fork' in the last 7 days. Most are related to the recent spike on state.db copies.",
  "Setting up a Redis health monitor cron job now. It will alert if cluster nodes drop below quorum.",
  "Absolutely — the Go payment module already owns the balance logic with a 3-layer idempotency system. No Node bridge needed.",
  "The kanban orchestrator skill looks solid. One suggestion: add validation that assignee profiles exist before creating cards.",
  "Forking the Telegram session now. Creating a sub-session fork with full history up to message index 42.",
  "Generating the capability map. I'll use .drawio format with real brand icons embedded as the spec requires.",
  "Node-exporter is NOT deployed per the Grafana setup notes. That's why the Kubernetes dashboard shows N/A for node metrics.",
  "Test plan drafted. Covering: happy-path fork, cross-channel continuation, fork lineage integrity, and tombstone behavior.",
  "Current available models via Hermes adapter: glm-5v-turbo, claude-sonnet-4, gpt-5.2, gemini-2.5-pro, deepseek-r1, and several others.",
  "Dashboard deployed to staging. All panel queries are passing with the current data range.",
  "Memory profile shows steady growth correlated with session count. The in-memory view isn't evicting old entries. Need to implement LRU eviction.",
  "Created the Grafana MCP observability skill. It covers datasource health checks, dashboard querying, and alert rule inspection.",
  "CloudWatch API Gateway metrics show p50=45ms, p95=230ms. Error rate is 0.03%. No anomalies detected.",
  "Bytebase is accessible via ~/.hermes/scripts/bb.py. The 401 issue you saw earlier was because the token had expired — I've refreshed it.",
];

const TOOL_CALLS: Array<{ name: string; args: string; result: string }> = [
  {
    name: "terminal",
    args: JSON.stringify({ command: "kubectl get pods -n monitoring" }),
    result: "NAME                             READY   STATUS    RESTARTS AGE\ngrafana-7d9f6c8b4-fqk2x       1/1     Running   0        3d\nprometheus-0                    2/2     Running   0        3d\nalertmanager-0                 2/2     Running   0        3d",
  },
  {
    name: "mcp_grafana_query_prometheus",
    args: JSON.stringify({ expr: 'up{job="prometheus"}', queryType: "instant", endTime: "now" }),
    result: '{"metric":{"__name__":"up","instance":"localhost:9090","job":"prometheus"},"value":[1719612345,"1"]}',
  },
  {
    name: "read_file",
    args: JSON.stringify({ path: "/home/rpw/olympus/docs/api-contract.md" }),
    result: "# Olympus API Contract (MVP)\n\n> **Purpose:** lock the wire shape...",
  },
  {
    name: "search_files",
    args: JSON.stringify({ pattern: "fork", target: "content", path: "." }),
    result: "docs/api-contract.md:45: fork lineage (ADR §6.6)\ndocs/plans/2026-06-28-olympus-mvp.md:120:Fork into acp-owned session",
  },
  {
    name: "web_search",
    args: JSON.stringify({ query: "ESP32-S3 deep sleep GPIO hold", limit: 5 }),
    result: '[{"title":"ESP-IDF Deep Sleep Guide","url":"https://docs.espressif.com/..."},{"title":"GPIO Hold in Light Sleep","url":"https://..."}]',
  },
  {
    name: "patch",
    args: JSON.stringify({ mode: "replace", path: "src/types.ts", old_string: "...", new_string: "..." }),
    result: "Applied patch to src/types.ts (+12 -3 lines)",
  },
  {
    name: "execute_code",
    args: JSON.stringify({ code: "from hermes_tools import web_search; print(web_search('test'))" }),
    result: "{'data': {'web': [{'url': '...', 'title': '...'}]}}",
  },
  {
    name: "memory",
    args: JSON.stringify({ action: "add", target: "memory", content: "User prefers Rust for new platform/backend code." }),
    result: "Memory saved (3,847/4,000 chars)",
  },
  {
    name: "kanban_create",
    args: JSON.stringify({ title: "Implement session fork UI", assignee: "tester" }),
    result: "{\"id\":\"t_a1b2c3d4\",\"status\":\"ready\"}",
  },
  {
    name: "delegate_task",
    args: JSON.stringify({ goal: "Audit all Grafana dashboards for broken queries", toolsets: ["terminal"] }),
    result: "Subagent dispatched (task-id: sg_001)",
  },
];

const REASONING_BLOCKS = [
  "The user is asking about a Grafana 401 error. Based on my memory, there are TWO orgs sharing datasource UIDs — org1 Noovoleum and org2 Noovoleum Dev. The 401 likely means the request is hitting org2's datasources with org1's token. I should check which org the dashboard belongs to and verify the service account token scope.",
  "This is a cross-channel fork request. Per ADR §6.6, we must NEVER modify the source session in place. The flow is: 1) Read source session messages up to forkPoint, 2) Create new Hermes session via ACP session/resume (only works for source==acp, otherwise fork first), 3) Inject <olympus fork/> marker, 4) Return the new managed session reference.",
  "The user wants to move payments to Go. From my notes, the Go backend (ucollect-backend-go) ALREADY owns balance with a 3-layer system: idempotency check → Redis distributed lock → MongoDB transaction. Building a Node.js bridge would violate the single-owner principle. I should confirm this direction and outline the migration steps.",
  "For the ESP32 deep sleep issue, common causes in order of likelihood: 1) GPIO hold not released before sleep, 2) UART peripheral still clocked (especially if USB-CDC is active), 3) RTC peripherals keeping power domain awake, 4) Touch pad interrupts preventing sleep entry. I should check the power management config and GPIO initialization order.",
];

// ── ID generator ───────────────────────────────────────

let seq = 1000;
function uid(prefix: string): string {
  return `${prefix}_${++seq}_${Math.random().toString(36).slice(2, 8)}`;
}

function epoch(daysAgo: number, hoursAgo = 0): number {
  return Math.floor(Date.now() / 1000) - daysAgo * 86400 - hoursAgo * 3600 + Math.floor(Math.random() * 3600);
}

// ── Message store (declared FIRST to avoid TDZ in the SESSIONS builder) ──

export const MESSAGES_BY_SESSION: Record<string, Message[]> = {};

// ── Build sessions ─────────────────────────────────────

function makeMessages(count: number, baseTime: number): Message[] {
  const msgs: Message[] = [];
  for (let i = 0; i < count; i++) {
    const roleIdx = i % 4;
    const role: Message["role"] =
      roleIdx === 0 ? "user" : roleIdx === 1 ? "assistant" : roleIdx === 2 ? "tool" : "system";
    const mid = i + 1;

    let msg: Message = {
      messageId: mid,
      sessionId: "",
      role,
      content: null,
      toolName: null,
      toolCalls: null,
      reasoning: null,
      timestamp: baseTime + i * (30 + Math.floor(Math.random() * 120)),
      tokenCount: role === "assistant" ? 200 + Math.floor(Math.random() * 800) : role === "user" ? 20 + Math.floor(Math.random() * 100) : null,
      finishReason: role === "assistant" ? "stop" : null,
    };

    if (role === "user") {
      msg.content = USER_MESSAGES[i % USER_MESSAGES.length];
    } else if (role === "assistant") {
      msg.content = ASSISTANT_SNIPPETS[i % ASSISTANT_SNIPPETS.length];
      // Occasionally add reasoning
      if (i % 3 === 0) {
        msg.reasoning = REASONING_BLOCKS[i % REASONING_BLOCKS.length];
      }
      // Occasionally add tool calls
      if (i % 2 === 0) {
        const tc = TOOL_CALLS[i % TOOL_CALLS.length];
        msg.toolCalls = [{ name: tc.name, args: JSON.stringify(tc.args), result: tc.result }];
        msg.toolName = tc.name;
      }
    } else if (role === "tool") {
      const tc = TOOL_CALLS[(i + 1) % TOOL_CALLS.length];
      msg.content = tc.result;
      msg.toolName = tc.name;
      msg.toolCalls = [{ name: tc.name, args: tc.args, result: tc.result }];
    }

    msgs.push(msg);
  }
  return msgs;
}

export const SESSIONS: Session[] = Array.from({ length: 32 }, (_, i) => {
  const source = SOURCES[i % SOURCES.length];
  const model = MODELS[i % MODELS.length];
  const started = epoch(i % 15, i % 24);
  const msgCount = 2 + Math.floor(Math.random() * 18);
  const title = TITLES[i % TITLES.length];
  const msgs = makeMessages(msgCount, started);

  // Assign session IDs to messages
  const sid = uid("sess");
  for (const m of msgs) m.sessionId = sid;

  const inputTokens = msgs.reduce((s, m) => s + (m.tokenCount ?? 0), 0) * (Math.random() * 3 + 1);
  const outputTokens = msgs.reduce((s, m) => s + (m.role === "assistant" ? (m.tokenCount ?? 0) * (Math.random() * 5 + 2) : 0), 0);

  const session: Session = {
    id: sid,
    hermesId: uid("hermes"),
    orgId: "personal",
    ownerId: "rpw",
    contextId: null,
    source,
    model,
    title,
    startedAt: started,
    lastActivity: started + msgCount * (60 + Math.floor(Math.random() * 300)),
    messageCount: msgCount,
    inputTokens: Math.floor(inputTokens),
    outputTokens: Math.floor(outputTokens),
    archived: i > 28, // last few archived
    forkedFrom: i === 7 ? uid("sess") : null,
    forkPoint: i === 7 ? 12 : null,
    forkType: i === 7 ? "sub" : null,
    managed: source === "acp" || (i % 8 === 0 && i < 16),
  };

  // Store messages keyed by session
  MESSAGES_BY_SESSION[sid] = msgs;
  return session;
});

// ── Models ─────────────────────────────────────────────

export const MODELS_LIST: ModelInfo[] = [
  { provider: "zai", model: "glm-5v-turbo", displayName: "GLM-5V Turbo" },
  { provider: "anthropic", model: "claude-sonnet-4-20250514", displayName: "Claude Sonnet 4" },
  { provider: "openai", model: "gpt-5.2", displayName: "GPT-5.2" },
  { provider: "google", model: "gemini-2.5-pro", displayName: "Gemini 2.5 Pro" },
  { provider: "deepseek", model: "deepseek-r1-0528", displayName: "DeepSeek R1" },
  { provider: "meta", model: "llama-4-maverick", displayName: "Llama 4 Maverick" },
  { provider: "zai", model: "glm-4.6", displayName: "GLM 4.6" },
  { provider: "mistral", model: "mistral-large", displayName: "Mistral Large" },
];

// ── Search fixture helper ──────────────────────────────

export function generateSearchHits(query: string): SearchHit[] {
  const hits: SearchHit[] = [];
  const lowerQ = query.toLowerCase();
  let idx = 0;
  for (const sess of SESSIONS.slice(0, 18)) {
    const msgs = MESSAGES_BY_SESSION[sess.id] ?? [];
    for (const msg of msgs) {
      if (!msg.content) continue;
      if (msg.content.toLowerCase().includes(lowerQ) || lowerQ.length < 3) {
        const start = Math.max(0, msg.content.toLowerCase().indexOf(lowerQ) - 40);
        const end = Math.min(msg.content.length, start + 120 + (lowerQ.length > 0 ? lowerQ.length : 0));
        const snippet = (start > 0 ? "…" : "") + msg.content.slice(start, end) + (end < msg.content.length ? "…" : "");
        hits.push({
          sessionId: sess.id,
          messageId: msg.messageId,
          source: sess.source,
          snippet,
          score: 0.72 + Math.random() * 0.27,
          timestamp: msg.timestamp,
        });
        if (++idx >= 25) return hits;
      }
    }
  }
  return hits;
}
