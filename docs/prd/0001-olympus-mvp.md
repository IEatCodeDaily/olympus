# Olympus — Product Requirements Document (MVP)

- Version: 1.0 (MVP scope)
- Date: 2026-06-28
- Status: Draft
- Related: BRD (`docs/brd/0001-olympus-mvp.md`), ADR 0002, ADR 0003,
  Implementation plan (`docs/plans/2026-06-28-olympus-mvp.md`)

> This PRD defines the **features and user flows** for the Olympus MVP. It does
> not redefine architecture (ADR 0002 owns that) or task breakdown (the plan
> owns that). Where this PRD and the ADRs disagree, the ADRs win.

## 1. Product definition

Olympus MVP is a **sync-native chat interface for Hermes Agent** that unifies
every session from every channel into a single searchable, resumable store.
It replaces Hermes Studio as the operator's primary interface for interacting
with Hermes.

**One sentence:** Open Olympus → see every conversation you've ever had with
Hermes across every channel → pick any one → continue it.

## 2. User personas

**The Operator (rpw):** Power user. Runs Hermes across CLI, Telegram, Discord,
WebUI, cron, and subagents. Needs to search history, resume conversations mid-
thread, switch models, and never lose context when moving between devices or
channels. Tolerates complexity; intolerant of data loss or lag.

## 3. Functional requirements (by feature)

### F1: Session list (the home view)

**What:** A paginated, virtualized, filterable list of all sessions.

- **Columns/fields displayed:** title (or first-message preview), source channel
  (CLI/Telegram/Discord/WebUI/cron/subagent/api_server), model, message count,
  started_at (relative time), last-activity, token total, archived flag.
- **Filters:** by source (multi-select), by model, by date range, by archived
  (default: hide archived), by "has tool calls", free-text search.
- **Sort:** by last-activity (default desc), by started_at, by message count,
  by token count.
- **Actions:** open session, archive, delete (with confirm), export (JSON).
- **Reactivity:** new sessions appear live at the top as Hermes creates them;
  existing sessions update (message count, last-activity) in real time.

**Data source:** the `sessions` projection of the redb event log. Each session
row maps to a Hermes session record (see §F5 for the import mapping).

### F2: Chat view (the conversation interface)

**What:** A full streaming chat UI for reading and continuing a session.

- **Message rendering:** role-distinguishable (user/assistant/tool), markdown
  with code highlighting, tool-call cards (collapsible), reasoning blocks
  (collapsible), streaming text with smoothing.
- **Input:** composer with model selector (reads Hermes's available models),
  send-on-enter, shift-enter newline, attach-file (future).
- **Streaming:** tokens stream live from Hermes (via the bridge) into the UI;
  tool calls render as they execute; reasoning renders if present.
- **Scroll:** auto-follow new messages; "jump to latest" button when scrolled
  up; load-older on scroll-to-top (paginated from redb).
- **Actions on a message:** copy, re-run (future), branch (future).

### F3: Search (cross-session)

**What:** Full-text keyword search across all sessions and messages.

- **Scope:** all non-archived sessions by default; toggle to include archived.
- **Results:** matching messages grouped by session, with the session title,
  source, and a snippet of the match highlighted. Click → opens the session at
  that message.
- **Engine:** tantivy BM25 (§10A.3). Index built on import; updated live as new
  messages arrive.
- **Performance:** <500ms p95 across 108k+ messages.

### F4: Session resume = fork (cross-channel, never in-place)

**What:** Continuing any non-Olympus session **forks** it into a new
Olympus-managed session. Olympus never continues an external session in place
(that would diverge from the origin channel — the Olympus turns would never appear
in Telegram). See ADR §6.6.

- **Flow:** click a session → "Continue in Olympus" → Olympus forks it (copies
  history up to the fork point into a new Hermes session in state.db + injects an
  `<olympus fork=.../>` marker) → the chat view loads the forked session → type a
  message → the envoy drives the fork via ACP → response streams back.
- **Two fork types** (operator picks, default sub-session): **sub-session**
  (nested, branches the same context tree) or **parallel session** (independent,
  sibling under the same project). Both record `forked_from` + `fork_point`.
- **The source session is never modified.** The Telegram/Discord/CLI session's
  state.db rows are untouched. The fork is a new branch in the node graph (F9).
- **Zero Hermes change:** Hermes resumes the forked session from state.db
  (`SessionDB.resolve_resume_session_id`), oblivious it is a fork.
- **Steering caveat:** Olympus can steer/cancel/switch-model only on the *forked*
  (managed) session, not on the live external one (F6, ADR §6.6.3).

### F9: Session node-graph (fork lineage visualization)

**What:** A node-graph view of any line of work — every session is a node, every
fork an edge from `forked_from` at `fork_point`. Sub-sessions branch off their
parent; parallel sessions are siblings under a project node. Click any node to
open that session. (ADR §6.6.2.)

### F5: Import (one-time migration from Hermes state.db)

**What:** Import all existing sessions and messages from
`~/.hermes/state.db` into Olympus's redb event log.

- **Source:** `~/.hermes/state.db` — `sessions` + `messages` tables. **Counts
  are a read-only snapshot taken at import start (the live DB drifts; ~1,633
  sessions / ~108,584 active messages at last check), NOT hardcoded constants.**
  Schema documented in the implementation plan.
- **Mapping:** each Hermes session → an Olympus session event (with source
  channel, model, timestamps, token counts). Each Hermes message → an Olympus
  message event (with role, content, tool calls, reasoning, timestamp).
- **Idempotent:** re-running the import on the same DB skips already-imported
  sessions (keyed by Hermes session ID).
- **Verification:** Olympus count == the import-start snapshot count (recorded
  in the import report), spot-check
  10 sessions across each source channel, FTS search works post-import.
- **Indexing:** tantivy index built during import (every message indexed).

### F6: Hermes bridge (two lanes — observe via state.db, drive via ACP)

**What:** Olympus integrates with Hermes through two independent lanes, both
gateway-free. See ADR §6.7 (sync) and §19 (drive).

**Lane 1 — Observe (read-only, all external channels):**
- Source of truth is `~/.hermes/state.db` (WAL mode → concurrent reads while
  Hermes writes). NOT the JSONL files (incomplete: ~135 on disk vs the full
  archive in state.db).
- Live tail: `SELECT * FROM messages WHERE id > <last_seen_id>` polled ~1–2s
  (optionally inotify on `state.db-wal`). New rows → `MessageAppended` events →
  UI updates live. Covers Telegram, Discord, CLI, cron, api_server, subagent —
  every channel writes to the same state.db, so one mechanism covers all.

**Lane 2 — Drive (read/write, Olympus-managed sessions only):**
- The envoy spawns `hermes acp` (ACP — Agent Client Protocol, JSON-RPC over
  stdio) per managed session and holds the process handle + stdio channel.
- A uniform **command queue** in the envoy (`prompt`, `steer`, `cancel`, `stop`,
  `switchModel`, `slash`) maps to ACP methods: `session/prompt`, `steer` (mid-turn
  injection sent as `/steer` prompt TEXT, NOT an ACP method), `session/cancel`
  (interrupt — already present), with streaming `session/update` notifications.
- **ACP methods are real** for prompt/cancel/set_model/streaming (verified). Steer
  and slash are prompt-TEXT, not methods. `session/resume` works only for
  `source='acp'` rows — cross-channel continuation FORKS (F4). Forking needs a
  small Hermes patch (`SessionDB.create_fork`, see `hermes-patches/`), proven on
  a copied DB first — NOT a raw live-DB write.
- The same `AgentRuntime` interface will back `ClaudeCodeRuntime` (Agent SDK
  streaming mode) and `CodexRuntime` (app-server) later — all stdio control
  protocols. MVP ships `HermesAgentRuntime` (ACP) + a trivial second impl to
  prove the seam.

**Model switching:** read Hermes's model list (config/CLI); `switchModel` command
→ ACP. Per-session and per-message (F7).

**Tool calls:** stream from ACP `session/update` as structured events; the UI
renders collapsible cards. Olympus does NOT execute tools — Hermes does (§19).

### F7: Model switching

**What:** Select which model Hermes uses for a given session or message.

- **Model list:** read from Hermes's available models (provider + model ID +
  display name). Cached; refreshed on demand.
- **Per-session:** set the default model for a session; persists across resumes.
- **Per-message:** override the model for a single message (one-shot).
- **Display:** model pill/badge on each session in the list and in the chat
  header (parity with the recent Studio feature — "model pill in session list").

### F8: Settings (minimal)

**What:** Basic settings for the MVP.

- **Connection:** Hermes bridge endpoint (socket path or CLI path), auto-connect
  toggle.
- **Appearance:** theme (dark/light), density (comfortable/compact).
- **Data:** import trigger (re-run import), export all (JSON), clear cache
  (rebuild tantivy index).
- **MVP auth gate (mandatory, not deferred):** localhost bind by default,
  per-install token (mode-0600), strict Origin/Host checks on `/ws` + `/api/*`.
  state.db holds secrets — an unauthenticated local server is a privesc surface.
  (No multi-user RBAC UI yet; the `can()` seam is operator-only — ADR §3.5.2.)
  Olympus must also know WHICH Hermes profile/HERMES_HOME it operates on and
  display it.

## 4. Non-functional requirements (from BRD, refined)

| NFR | Target | How |
|---|---|---|
| Import time | <10 min for the full snapshot (~1,633 sessions / ~108k messages) | Bulk insert into redb; zstd compress; tantivy index in batches |
| Search p95 | <500ms across 108k messages | tantivy BM25; index on SSD |
| Session list | <200ms, virtualized | redb range scan + in-memory view; react-window virtualization |
| Streaming | <100ms token-to-UI | WSS delta broadcast; no per-token redb write (batch 100ms) |
| Olympus memory | <200MB | zstd compression; sliding window per session; cold sessions evicted |
| Crash recovery | restart-on-crash via systemd; log survives | redb is durable; views rebuild from log on restart |

## 5. User flows (primary paths)

### Flow A: First run (import + connect)

1. Operator starts Olympus (`olympus` binary or `cargo run`).
2. Olympus detects `~/.hermes/state.db`, snapshots the counts, and prompts:
   "Import N sessions?" (N from the live snapshot, not a constant).
3. Operator confirms → import runs (progress bar) → tantivy index builds.
4. Olympus detects Hermes gateway (or prompts for CLI path).
5. Session list loads → operator sees all sessions.

### Flow B: Continue (fork) a Telegram session

1. Operator opens Olympus → session list.
2. Filters by source = Telegram.
3. Finds the session by title or search.
4. Clicks it → chat view loads with full history, **read-only** (it's an
   externally-owned session; Olympus observes, does not drive it).
5. Clicks **"Continue in Olympus"** → Olympus **forks** it (Hermes `create_fork`
   helper → a new `source='acp'` session seeded with history up to the fork
   point; the Telegram session's rows are untouched).
6. Types a message in the **fork** → the envoy drives it via `hermes acp` → Hermes
   responds → streams back.
7. The fork's displayed origin is **"forked from telegram"** with a node-graph
   edge to the source (§F4/F9); the original Telegram session is unchanged and
   independent.

### Flow C: Search across all channels

1. Operator types "esp32 firmware" in the search bar.
2. Results show matching messages from CLI, Telegram, WebUI sessions, grouped
   by session with snippets.
3. Operator clicks a result → jumps to that session at that message.

### Flow D: Watch a CLI session live

1. Operator starts a Hermes CLI session in a terminal (`hermes`).
2. Olympus session list updates live — the new session appears at the top.
3. Operator clicks it in Olympus → sees the CLI conversation streaming in
   real time, side-by-side with the terminal.

## 6. Analytics / observability (minimal for MVP)

- Olympus process RSS (self-monitored).
- redb log size.
- tantivy index size.
- Bridge connection status (connected/disconnected/reconnecting).
- Message throughput (events/sec) — surfaced in a debug panel.

## 7. Constraints

- **Single-node MVP.** No multi-node, no envoy-on-remote-host, no iroh transport.
  Olympus and Hermes run on the same host (rpw's WSL/Talos).
- **Hermes Studio goes read-only or is shut down** during MVP operation to avoid
  dual-writer conflicts on `state.db`. (Olympus is the new writer; Studio is the
  legacy reader.)
- **Host-direct sandbox only** — no bwrap/Docker in MVP (§12.1).
- **React + Tauri** for the UI (per the UI-stack decision).
- **redb + tantivy + zstd** for storage/search/compression (§10A).

## 8. Dependencies

| Dependency | Version | License | Purpose |
|---|---|---|---|
| redb | latest stable | MIT | embedded ACID KV (event log) |
| tantivy | latest stable | MIT | full-text search index |
| zstd | latest stable | BSD | message compression (trained dict) |
| tokio | latest stable | MIT | async runtime |
| axum | latest stable | MIT | HTTP server + WSS |
| serde / postcard | latest stable | MIT | serialization |
| React + Vite | latest stable | MIT | UI framework |
| xterm.js | latest stable | MIT | terminal (not in MVP, but UI deps chosen for it) |
| Tauri | v2 | MIT/Apache | desktop wrapper (optional for MVP; web-first) |

## 9. Acceptance criteria (MVP ship gate)

- [ ] Session count == the import-start read-only snapshot (recorded in report).
- [ ] Active-message count == the import-start snapshot (recorded in report).
- [ ] Tantivy FTS returns results across all sources in <500ms.
- [ ] Session list loads in <200ms, virtualized, with live updates.
- [ ] Chat view renders markdown, tool calls, reasoning, streaming.
- [ ] Cross-channel resume works (Telegram session resumed from Olympus).
- [ ] Hermes bridge streams responses live (<100ms token-to-UI).
- [ ] Model switching works (per-session and per-message).
- [ ] Olympus process memory <200MB with full dataset loaded.
- [ ] Crash recovery: kill Olympus → restart → state intact, views rebuilt.
