# Olympus MVP Implementation Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Build the Olympus MVP — a sync-native chat interface for Hermes that
unifies all sessions from all channels into one searchable, resumable store on
a Rust-native control plane.

**Architecture:** Rust single-binary control plane (redb event log + in-memory
views + delta broadcast over WSS + single-writer scheduler) + Hermes bridge
(**ACP over stdio** — `hermes acp`; see §ACP below) + React/Tauri UI. Single-node
MVP that **collapses Layer 1 + Layer 2 into one process but keeps an internal
envoy module boundary** (per the ADR 0002 §2.1 hard boundary — the host-effect
code lives behind an `envoy` module seam even when co-located, so multi-node is a
later deployment change, not a refactor). No multi-node, no workflows, no
cards/board UI in MVP. See ADR 0002 (full spec) and ADR 0003 (substrate).
BRD/PRD: `docs/brd/0001` + `docs/prd/0001`. Adversarial review:
`docs/reviews/2026-06-28-adversarial-review.md` (the fixes below address it).

**Tech stack:** Rust (redb, tantivy, zstd, tokio, axum, serde/postcard), React
+ Vite + Tauri (UI), TypeScript (UI only).

**Current repo state:** Phase 0 + Phase 1 DONE (commit `fe7580b`): Rust workspace
+ redb event log + zstd compression, 14 tests green. The legacy Convex/TS
scaffold is removed. Remaining phases build on `crates/control-plane`.

**Import data:** `~/.hermes/state.db` (SQLite, WAL). **Counts are NOT hardcoded** —
they drift (a read-only snapshot at the start of this plan showed ~1,633 sessions
/ ~108,584 messages; the live DB changes constantly). **Acceptance = "counts equal
the read-only snapshot taken at import start," recorded in the import report** —
never fixed constants in prose (adversarial review blocker #6). Sources observed:
cli, telegram, cron, subagent, api_server, webui, discord.
Schema:
```sql
-- sessions: id, source, user_id, model, started_at, ended_at, message_count,
--   tool_call_count, input_tokens, output_tokens, title, archived, git_branch,
--   parent_session_id, model_config (holds _branched_from for forks), ...
-- messages: id (AUTOINCREMENT, NON-contiguous), session_id, role, content,
--   tool_call_id, tool_calls, tool_name, timestamp, token_count, finish_reason,
--   reasoning, reasoning_content,
--   active, observed, compacted  -- ⚠ MUTABLE state columns (see Phase 4 sync)
-- FTS triggers maintain messages_fts; session counters are updated only by
-- Hermes methods, NOT triggers.
```

> **§ACP — the real Hermes bridge contract (source-verified; replaces any
> "gateway socket / CLI subprocess" wording).** The drive lane is `hermes acp`
> (Agent Client Protocol, JSON-RPC over stdio). Real methods (verified in
> `acp_adapter/`):
> - `session/prompt` — send a prompt. **Slash commands (`/steer …`, `/model …`)
>   are sent as prompt TEXT**, intercepted inside prompt handling — there is NO
>   ACP `steer` or generic `slash` method.
> - `session/cancel` — interrupt the running turn.
> - `session/set_model` — switch model.
> - `session/resume` — resume, but **only restores rows whose `source == "acp"`**
>   (arbitrary external telegram/cli sessions are NOT directly ACP-resumable —
>   this is why cross-channel continuation must FORK into an acp-owned session).
> - `session/update` — streaming notifications (text / tool / thought).
> - There is NO `hermes acp --resume` CLI flag; resume is an ACP method call.
> Any §19 "uniform command queue" is an Olympus-side abstraction; the envoy maps
> it to these real methods. Do NOT call invented method names.

---

## Phase 0: Tear down Convex scaffold, create Rust workspace

### Task 0.1: Remove the Convex/TS scaffold

**Objective:** Clear the legacy scaffold so the Rust workspace starts clean.

**Files:**
- Remove: `convex/`, `apps/`, `packages/`, `bun.lock`, `tsconfig.json`,
  `package.json` (the old bun workspace root)
- Keep: `docs/`, `.gitignore`, `README.md`, `AGENTS.md`, `.env.local`

**Steps:**
1. `git rm -r convex/ apps/ packages/`
2. `git rm bun.lock tsconfig.json package.json`
3. `git commit -m "chore: remove legacy Convex/TS scaffold (superseded by ADR 0003)"`

### Task 0.2: Create the Rust workspace

**Objective:** Establish the Cargo workspace structure.

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/control-plane/Cargo.toml`
- Create: `crates/control-plane/src/main.rs`
- Create: `crates/control-plane/src/lib.rs`
- Create: `.gitignore` (add `/target/`)

**Workspace `Cargo.toml`:**
```toml
[workspace]
members = ["crates/control-plane"]
resolver = "2"

[workspace.package]
edition = "2021"
license = "MIT"

[workspace.dependencies]
redb = "2"
tantivy = "0.22"
zstd = "0.13"
tokio = { version = "1", features = ["full"] }
axum = { version = "0.8", features = ["ws"] }
serde = { version = "1", features = ["derive"] }
postcard = { version = "1", features = ["alloc"] }
tracing = "0.1"
tracing-subscriber = "0.3"
anyhow = "1"
rusqlite = "0.32"  # for reading Hermes state.db during import
```

**`crates/control-plane/Cargo.toml`:**
```toml
[package]
name = "olympus-control-plane"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
redb.workspace = true
tantivy.workspace = true
zstd.workspace = true
tokio.workspace = true
axum.workspace = true
serde.workspace = true
postcard.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
anyhow.workspace = true
rusqlite.workspace = true
```

**`crates/control-plane/src/main.rs`:**
```rust
fn main() {
    println!("olympus control plane — placeholder");
}
```

**Verify:**
```bash
cargo build
# Expected: compiles, prints "olympus control plane — placeholder" on `cargo run`
```

**Commit:** `feat: scaffold Rust workspace`

---

## Phase 1: Event log + redb (the source of truth)

### Task 1.1: Define the event types

**Objective:** Define the core event types that the log stores.

**Files:**
- Create: `crates/control-plane/src/event.rs`

**Events (v1 — MVP-scoped):**
```rust
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    /// A session was imported or created.
    SessionCreated {
        session_id: String,
        hermes_id: String,         // Hermes's session ID
        source: String,            // "cli"|"telegram"|"discord"|"webui"|"cron"|"subagent"|"api_server"
        model: Option<String>,
        title: Option<String>,
        started_at: f64,
        message_count: u64,
        input_tokens: u64,
        output_tokens: u64,
    },
    /// A message was appended to a session.
    MessageAppended {
        session_id: String,        // Olympus session ID
        hermes_session_id: String, // Hermes session ID
        message_id: u64,           // monotonic within session
        role: String,              // "user"|"assistant"|"tool"|"system"
        content: Option<String>,   // zstd-compressed in storage
        tool_name: Option<String>,
        tool_calls: Option<String>,
        reasoning: Option<String>,
        timestamp: f64,
        token_count: Option<u64>,
        finish_reason: Option<String>,
    },
    /// A session's metadata was updated (title, archived, model, etc).
    SessionUpdated {
        session_id: String,
        title: Option<String>,
        model: Option<String>,
        archived: Option<bool>,
        message_count: Option<u64>,
    },
}
```

### Task 1.2: Implement the redb log store

**Objective:** Append events to redb; read them back sequentially.

**Files:**
- Create: `crates/control-plane/src/log.rs`

**Design:**
- redb table: `"events"` with `u64` (monotonic sequence number) → `Vec<u8>` (postcard-serialized event).
- Another table: `"meta"` with `&str` → `Vec<u8>` for metadata like `next_seq`.
- API:
```rust
pub struct Log {
    db: redb::Database,
}

impl Log {
    pub fn open(path: &Path) -> anyhow::Result<Self>;
    pub fn append(&self, event: &Event) -> anyhow::Result<u64>;  // returns seq
    pub fn read_from(&self, seq: u64, limit: usize) -> anyhow::Result<Vec<(u64, Event)>>;
    pub fn read_all(&self) -> anyhow::Result<Vec<(u64, Event)>>;  // for replay
}
```

**Verify:**
```rust
#[test]
fn append_and_read_event() {
    let log = Log::open(tempfile::NamedTempFile::new().unwrap().path()).unwrap();
    let seq = log.append(&Event::SessionCreated { ... }).unwrap();
    let events = log.read_all().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, seq);
}
```

**Commit:** `feat: redb event log`

### Task 1.3: zstd message compression

**Objective:** Compress message content before storing in the event log.

**Files:**
- Modify: `crates/control-plane/src/log.rs` (add compression to MessageAppended content)
- Create: `crates/control-plane/src/compress.rs`

**Design:**
- For MVP: use zstd level 3 without a trained dictionary (dictionary training is
  a post-MVP optimization). Add dictionary support as a TODO.
- Compress `content`, `tool_calls`, `reasoning` fields individually before
  postcard-serializing the event.

**Verify:**
```rust
#[test]
fn compressed_message_roundtrips() {
    let original = "Hello " .repeat(1000);
    let compressed = compress(&original).unwrap();
    assert!(compressed.len() < original.len() / 2);
    let decompressed = decompress(&compressed).unwrap();
    assert_eq!(decompressed, original);
}
```

**Commit:** `feat: zstd message compression`

---

## Phase 2: In-memory views + session/message projections

### Task 2.1: Session view (the session list data model)

**Objective:** Project events into an in-memory session list view.

**Files:**
- Create: `crates/control-plane/src/views/session.rs`

**Design:**
```rust
pub struct SessionRow {
    pub session_id: String,
    pub hermes_id: String,
    pub source: String,
    pub model: Option<String>,
    pub title: Option<String>,
    pub started_at: f64,
    pub message_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub archived: bool,
    pub last_activity: f64,
}

pub struct SessionView {
    sessions: HashMap<String, SessionRow>,
    by_started: BTreeMap<Reverse<OrderedFloat<f64>>, Vec<String>>, // started_at desc → session_ids
}

impl SessionView {
    pub fn apply(&mut self, event: &Event);  // update on event
    pub fn list(&self, filters: &Filters) -> Vec<&SessionRow>;
    pub fn get(&self, session_id: &str) -> Option<&SessionRow>;
}
```

### Task 2.2: Message view (per-session message cache)

**Objective:** Project message events into a per-session message cache (sliding window).

**Files:**
- Create: `crates/control-plane/src/views/message.rs`

**Design:**
```rust
pub struct MessageRow {
    pub message_id: u64,
    pub role: String,
    pub content: Option<String>,  // decompressed
    pub tool_name: Option<String>,
    pub timestamp: f64,
    pub token_count: Option<u64>,
}

pub struct MessageView {
    // session_id → (messages, total_count)
    // Hot: recent N messages in memory. Cold: read from log on demand.
    hot: HashMap<String, VecDeque<MessageRow>>,
    counts: HashMap<String, u64>,
    WINDOW_SIZE: usize,  // default 50
}

impl MessageView {
    pub fn apply(&mut self, event: &Event);
    pub fn recent(&self, session_id: &str, limit: usize) -> Vec<&MessageRow>;
    pub fn count(&self, session_id: &str) -> u64;
}
```

### Task 2.3: View manager (replays log on startup, applies events live)

**Objective:** On startup, replay the log to rebuild views; on new events, apply them.

**Files:**
- Create: `crates/control-plane/src/views/mod.rs`

**Verify:**
```rust
#[test]
fn replay_rebuilds_views() {
    let log = Log::open(...);
    log.append(&Event::SessionCreated { ... });
    log.append(&Event::MessageAppended { ... });
    let mut mgr = ViewManager::new();
    mgr.replay(&log).unwrap();
    assert_eq!(mgr.sessions.list(&Filters::default()).len(), 1);
    assert_eq!(mgr.messages.count("sess-1"), 1);
}
```

**Commit:** `feat: in-memory views with log replay`

---

## Phase 3: WSS server + delta broadcast (reactivity)

### Task 3.1: axum WSS server skeleton

**Objective:** Serve a WSS endpoint that clients connect to for reactive updates.

**Files:**
- Create: `crates/control-plane/src/server.rs`

**Endpoints (MVP):**
- `GET /ws` — WebSocket upgrade; subscribes to view deltas.
- `GET /api/sessions` — REST query: list sessions (paginated, filtered).
- `GET /api/sessions/:id/messages` — REST query: list messages (paginated).
- `POST /api/sessions/:id/messages` — REST mutation: send a message to Hermes.

**AUTH GATE (mandatory — adversarial review blocker #7; NOT deferred).** state.db
holds secrets, system prompts, tool outputs, and corporate context; an
unauthenticated local server that can read all history and drive Hermes is a
privilege-escalation surface. So even in single-user MVP:
- **Bind `127.0.0.1` by default**; remote bind is opt-in and **fails closed**.
- **Per-install random token**: generated on first run, stored mode-0600 under
  `~/.olympus/`; required on every `/api/*` and the `/ws` upgrade (query param for
  WS, header for REST — mirrors how Hermes's own dashboard gates `/api/pty`).
- **Strict Origin/Host checks** on the WS upgrade and mutations (reject
  cross-origin; a hostile local web page must not reach the port). No "localhost
  == trusted."
- This is the `can(user, action, resource)` seam (ADR §3.5.2) in its MVP form:
  one operator, one token; the call sites exist so real RBAC is a later flip.

**Verify:**
```bash
cargo run
# unauth request is rejected:
curl -sS http://localhost:8787/api/sessions ; echo " (expect 401)"
# with token:
curl -sS -H "authorization: Bearer $(cat ~/.olympus/token)" http://localhost:8787/api/sessions
# Expected: {"sessions": []} (empty until import)
```

### Task 3.2: Delta broadcast over WebSocket

**Objective:** When the view changes, push deltas to connected WS clients.

**Files:**
- Modify: `crates/control-plane/src/server.rs`
- Modify: `crates/control-plane/src/views/mod.rs` (emit deltas)

**Design:**
- ViewManager holds a `tokio::sync::broadcast::Sender<ViewDelta>`.
- On `apply(event)`, compute the delta and broadcast.
- WS handler receives the broadcast and sends JSON to the client.

```rust
#[derive(Serialize)]
pub enum ViewDelta {
    SessionAdded(SessionRow),
    SessionUpdated { session_id: String, changes: SessionChanges },
    MessageAppended { session_id: String, message: MessageRow },
}
```

**Verify:**
```bash
# Start olympus
cargo run &
# Connect a WS client (wscat or browser console)
wscat -c ws://localhost:8787/ws
# In another terminal, trigger an event (e.g. via REST POST)
# Expected: the WS client receives a {"SessionAdded": {...}} message
```

**Commit:** `feat: WSS server + delta broadcast`

---

## Phase 4: Hermes bridge (ACP-over-stdio + state.db sync)

### Task 4.1: ACP bridge — drive `hermes acp` over stdio

**Objective:** Define the `AgentRuntime` bridge and implement the Hermes/ACP impl.

**Files:**
- Create: `crates/control-plane/src/bridge/mod.rs`
- Create: `crates/control-plane/src/bridge/acp.rs` (JSON-RPC-over-stdio client)
- Create: `crates/control-plane/src/bridge/hermes.rs` (`HermesAgentRuntime`)

**Interface (Olympus-side uniform command queue → REAL native ACP methods):**
```rust
pub enum AgentCommand {
    Prompt { text: String, model: Option<String> },
    Steer { text: String },      // → session/prompt with text "/steer <text>" (NOT an ACP method)
    Cancel,                       // → session/cancel
    Stop,                         // → close the ACP child
    SwitchModel { model: String },// → session/set_model
    Slash { command: String },    // → session/prompt with text "/<command>" (NOT an ACP method)
}

pub enum AgentEvent {
    Text(String),
    ToolCall { name: String, args: String, result: Option<String> },
    Reasoning(String),
    Done { finish_reason: Option<String> },
    Error(String),
}

#[async_trait]
pub trait AgentRuntime: Send + Sync {
    async fn start(&self, session_id: &str) -> anyhow::Result<()>; // spawn `hermes acp`, open stdio, session/resume or session/new
    async fn send(&self, cmd: AgentCommand) -> anyhow::Result<()>; // → real ACP method (table below)
    fn events(&self) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send>>; // from `session/update`
    async fn stop(&self) -> anyhow::Result<()>;
}
```

**ACP mechanics (SOURCE-VERIFIED — see §ACP in the header + the adversarial
review). The earlier "steer is an ACP method / no fork change" claim was WRONG.**

| Olympus command | Real ACP wire call |
|---|---|
| `Prompt` | `session/prompt` (text) |
| `Steer` | `session/prompt` with text `"/steer <text>"` (slash intercepted in prompt handling; only lands while a turn is running) |
| `Slash` | `session/prompt` with text `"/<command>"` |
| `Cancel` | `session/cancel` (sets cancel_event + `agent.interrupt()`) |
| `SwitchModel` | `session/set_model` |
| start/resume | `session/resume` — **only for rows with `source=="acp"`**; for a non-acp external session, FORK first (Phase 5) into an acp-owned session, then resume the fork |
| streaming | `session/update` notifications |

- **`session/set_model` is a real method** (confirmed) — model switching does NOT
  need the slash path, though `/model` text also works.
- **Steer is best-effort and turn-scoped:** `/steer` only injects while a turn is
  actively running; otherwise it queues/no-ops per Hermes behavior. Olympus must
  not assume steer lands when the agent is idle.

**SPIKE 0 (MANDATORY — do BEFORE building the bridge; adversarial review blocker
#1).** Against a throwaway acp session: `session/new` → `session/prompt` "say
PONG" → assert streamed `session/update` PONG → `session/set_model` → a long
prompt then a concurrent `session/prompt "/steer …"` → assert it influences
output → `session/cancel`. Capture the exact JSON-RPC frames into
`docs/reviews/acp-wire-spike.md`. Only build `acp.rs` once the frames are proven.

**Verify:**
```bash
cargo test bridge::acp   # unit: frame encode/decode
# integration: spawn `hermes acp`, prompt "say PONG", assert streamed PONG;
#   prompt a long task, steer mid-turn, assert the steer text influences output.
```

**Commit:** `feat: ACP bridge (hermes acp over stdio)`

### Task 4.2: state.db sync — MUTABLE-source reconciliation (NOT append-only)

**Objective:** Keep Olympus in sync with `~/.hermes/state.db` across **inserts,
updates, and deletes**. (Adversarial review blocker #3: state.db is NOT
append-only — `messages.id` is non-contiguous, and Hermes mutates rows via
compaction (`active=0, compacted=1`), rewind/undo (`active=0`), and destructive
`replace_messages` (delete+reinsert, called by ACP after turns). A pure
`id > last_seen` tail misses every mutation and diverges.)

**Files:**
- Create: `crates/control-plane/src/sync.rs`

**Design — two layers:**
1. **Fast tail (latency):** read-only rusqlite (WAL → concurrent-safe);
   `SELECT * FROM messages WHERE id > ?last_seen ORDER BY id` on a ~1–2s poll
   (optional inotify on `state.db-wal`) → `MessageAppended` events. Catches new
   inserts quickly. **Honor `active`/`compacted`:** treat `active=0` as
   tombstoned (don't surface), `compacted=1` per Hermes search semantics.
2. **Reconciliation sweep (correctness):** periodically (e.g. every 30–60s, and
   on session open) compute a per-session signature — `(max(id), row_count,
   sum over active rows)` or a cheap checksum — and compare to Olympus's view.
   On mismatch, re-read that session's rows and reconcile (handle deletes,
   rewinds, compaction, title/counter/model changes via `SessionUpdated`).
3. Session metadata (`message_count`, `title`, `model`, `archived`) is read from
   `sessions` and reconciled too — counters there are authoritative (triggers do
   NOT maintain them).

**Acceptance / better path noted:** if reconciliation proves too chatty, the
clean fix is to ask Hermes (via a patch — see `hermes-patches/`) for an
append-only changefeed table of session mutations. Record that as a candidate
patch if the sweep is insufficient.

**Verify:**
```bash
# insert: hermes -z "test" in another terminal → appears in Olympus live.
# mutation: trigger a rewind/compaction in a Hermes session → Olympus reflects
#   the active=0 tombstone, not a stale duplicate.
```

**Commit:** `feat: state.db mutable-source sync (tail + reconciliation)`

---

## Phase 5: Import + fork (state.db migration + fork mechanics)

### Task 5.1: Import sessions from state.db

**Objective:** Bulk-import all sessions from `~/.hermes/state.db`.

**Files:**
- Create: `crates/control-plane/src/import.rs`

**Design (state.db is the complete archive — NOT JSONL which is pruned to ~135;
exclude `active=0` tombstoned messages; record the snapshot counts in an import
report per the §header acceptance rule, do NOT hardcode):**
```rust
pub fn import_sessions(state_db: &Path, log: &Log) -> anyhow::Result<ImportStats> {
    let conn = rusqlite::Connection::open_with_flags(
        state_db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;  // WAL-safe read-only
    let mut stmt = conn.prepare(
        "SELECT id, source, model, title, started_at, message_count,
                input_tokens, output_tokens, archived, parent_session_id
         FROM sessions ORDER BY started_at")?;
    // Each row → Event::SessionCreated (hermes_id = id; preserve parent_session_id
    // as fork lineage for the node graph).
}
```

### Task 5.2: Import messages from state.db

**Design:**
```rust
"SELECT session_id, role, content, tool_name, tool_calls, reasoning,
        timestamp, token_count, finish_reason
 FROM messages ORDER BY session_id, timestamp"
// zstd-compress content; batch in transactions of 1000; tantivy-index each.
```

### Task 5.3: Fork mechanics — via a Hermes patch, proven on a COPIED db first

**Objective:** Fork a session so `hermes acp` can resume the fork. (Adversarial
review blocker #2: raw-writing the LIVE state.db is a corruption risk. Hermes's
`append_message` updates session counters explicitly (no trigger does it); branch
lineage lives in `sessions.model_config._branched_from` (NOT a message marker);
ACP resume ignores non-`acp` rows; a mid-thread `<olympus>` system message gets
fed to the model but ignored by ACP UI replay. So the fork MUST replicate Hermes
invariants and must never be developed against the live DB.)

**Approach — a Hermes patch, not a raw writer:**
1. **GATE: fork-spike on a COPIED db.** `cp ~/.hermes/state.db /tmp/fork-spike.db`.
   Prototype the fork as a transaction that produces a resumable `source='acp'`
   session; `hermes acp` (pointed at the copy via `HERMES_HOME`) must
   `session/resume` it cleanly with correct history and NO invariant drift.
   Capture results in `docs/reviews/fork-spike.md`.
2. **Implement as a Hermes patch:** add `SessionDB.create_fork(src_id, fork_point,
   fork_type)` to Hermes that does the full transaction with Hermes invariants:
   `source='acp'`, `model_config._branched_from=src_id` + `parent_session_id`,
   correct `message_count`/`tool_call_count`, `cwd`/`model_config`, `active=1`
   rows, FTS, FK-valid lineage, timestamps. Save it via
   `hermes-patches/patchctl.sh save 001-sessiondb-create-fork hermes_state.py`,
   register in `manifest.toml`, commit to the Olympus repo.
3. **Olympus calls the patched helper** (via the envoy / `hermes` CLI), never raw
   SQL. Lineage of record is Olympus's event log (`forked_from`/`fork_point`/
   `fork_type`); if a Hermes-side marker is needed it goes in `model_config`
   (NOT a model-visible system message).
4. **Backup before any live write:** SQLite online-backup or operator-confirmed
   snapshot before the first live fork (adversarial review §6).

**Files:** `crates/control-plane/src/fork.rs` (calls the helper);
`hermes-patches/patches/001-sessiondb-create-fork.patch` (the Hermes change).

### Task 5.4: Import + fork verification

```bash
# Counts = the snapshot taken at import start (NOT hardcoded — §header rule).
#   Record snapshot in the import report; assert Olympus count == snapshot count.
SNAP_S=$(sqlite3 -readonly ~/.hermes/state.db "SELECT COUNT(*) FROM sessions;")
SNAP_M=$(sqlite3 -readonly ~/.hermes/state.db "SELECT COUNT(*) FROM messages WHERE active=1;")
curl -s localhost:8787/api/sessions | jq '.sessions | length'   # == $SNAP_S
# Fork (on the COPIED db first, then live with backup): fork a session, assert
#   `hermes acp` session/resume loads the copied history; assert the SOURCE
#   session rows are byte-for-byte unchanged.
```

**Commit:** `feat: import from state.db + session fork mechanics`

---

## Phase 6: tantivy search index

### Task 6.1: Build the search index from the event log

**Objective:** Index all message content for BM25 keyword search.

**Files:**
- Create: `crates/control-plane/src/search.rs`

**Design:**
- tantivy schema: `session_id` (stored), `message_id` (stored), `content` (text),
  `role` (text), `tool_name` (text), `timestamp` (fast field).
- On import: index every message. On new MessageAppended event: index live.
- Query: `content: <query>` → returns matching `(session_id, message_id)` pairs.

**Verify:**
```bash
curl 'http://localhost:8787/api/search?q=esp32'
# Expected: JSON array of matches with session_id, message snippet, highlight
```

**Commit:** `feat: tantivy full-text search`

---

## Phase 7: React UI

### Task 7.1: React app scaffold (Vite + WebSocket client)

**Objective:** Create the React app that talks to the control plane.

**Files:**
- Create: `ui/` directory (outside `crates/`, served by axum as static files)
- Create: `ui/package.json`, `ui/vite.config.ts`, `ui/src/main.tsx`, `ui/src/App.tsx`
- Create: `ui/src/api.ts` (WSS client + REST helpers)

### Task 7.2: Session list view

**Objective:** Virtualized, filterable session list.

**Files:**
- Create: `ui/src/views/SessionList.tsx`
- Create: `ui/src/components/SessionRow.tsx`
- Create: `ui/src/hooks/useSessions.ts` (REST + WS delta merge)

### Task 7.3: Chat view with streaming

**Objective:** Full chat UI with markdown, tool calls, streaming.

**Files:**
- Create: `ui/src/views/ChatView.tsx`
- Create: `ui/src/components/Message.tsx`
- Create: `ui/src/components/MessageInput.tsx`
- Create: `ui/src/hooks/useChat.ts` (message list + WS streaming)

### Task 7.4: Search view

**Objective:** Cross-session search with results grouped by session.

**Files:**
- Create: `ui/src/views/SearchView.tsx`

### Task 7.5: Model selector + settings

**Objective:** Model pill/selector per session + minimal settings.

**Files:**
- Create: `ui/src/components/ModelSelector.tsx`
- Create: `ui/src/views/Settings.tsx`

**Commit:** `feat: React UI (session list, chat, search, settings)`

---

## Phase 8: Integration + ship

### Task 8.1: End-to-end test — import + browse + search

**Objective:** Verify the full MVP flow works.

**Steps:**
1. `cargo run` (start control plane).
2. Import runs (or run manually).
3. Open `http://localhost:8787` in browser.
4. Session list shows all imported sessions (== the import-start snapshot count).
5. Filter by source = telegram → 285 sessions.
6. Search "esp32" → results from multiple channels.
7. Click a session → chat view → history loads.
8. Type a message → Hermes responds → streams.
9. Check memory: <200MB.

### Task 8.2: systemd service + crash recovery

**Objective:** Olympus survives crashes.

**Files:**
- Create: `deploy/olympus.service` (systemd unit)

### Task 8.3: Tauri wrapper (optional for MVP)

**Objective:** Wrap the React app in Tauri for desktop deployment.

**Files:**
- Create: `src-tauri/` (Tauri project)

**Commit:** `feat: MVP ship — integration, systemd, Tauri wrapper`

---

## Summary

| Phase | Tasks | What it delivers |
|---|---|---|
| 0 | 2 | Clean Rust workspace |
| 1 | 3 | Event log (redb) + compression |
| 2 | 3 | In-memory views (sessions, messages) |
| 3 | 2 | WSS server + delta broadcast |
| 4 | 2 | Hermes bridge (gateway + wiring) |
| 5 | 3 | Import from state.db (full snapshot) + fork (via Hermes patch) |
| 6 | 1 | tantivy search |
| 7 | 5 | React UI (list, chat, search, settings) |
| 8 | 3 | Integration, systemd, Tauri |

**Total: 24 tasks across 9 phases.** Each task is independently committable.
The riskiest is Phase 4/5 (ACP bridge + fork) — do the ACP wire-spike and the
fork-on-copied-DB spike FIRST (adversarial review blockers #1, #2). The read-only
track (import + view + search + sync) has no spike dependency and is built first.
