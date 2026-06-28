# Olympus MVP Implementation Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Build the Olympus MVP — a sync-native chat interface for Hermes that
unifies all sessions from all channels into one searchable, resumable store on
a Rust-native control plane.

**Architecture:** Rust single-binary control plane (redb event log + in-memory
views + delta broadcast over WSS + single-writer scheduler) + Hermes bridge
(gateway socket or CLI subprocess) + React/Tauri UI. Single-node MVP; no envoy,
no multi-node, no workflows, no cards/board. See ADR 0002 (full spec) and ADR
0003 (substrate). BRD/PRD: `docs/brd/0001` + `docs/prd/0001`.

**Tech stack:** Rust (redb, tantivy, zstd, tokio, axum, serde/postcard), React
+ Vite + Tauri (UI), TypeScript (UI only).

**Current repo state:** Legacy Convex/TS scaffold from ADR 0001. No Cargo.toml,
no Rust workspace. The `convex/`, `apps/`, `packages/` dirs are the old TS
scaffold. This plan replaces them with a Rust workspace.

**Import data:** `~/.hermes/state.db` — 1,626 sessions, 108,169 messages, 1.4GB.
Schema:
```sql
-- sessions table: id, source, user_id, model, started_at, ended_at, message_count,
--   tool_call_count, input_tokens, output_tokens, title, archived, git_branch, ...
-- messages table: id, session_id, role, content, tool_call_id, tool_calls,
--   tool_name, timestamp, token_count, finish_reason, reasoning, reasoning_content, ...
```

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

**Verify:**
```bash
cargo run
# In another terminal:
curl http://localhost:8787/api/sessions
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

**Interface (uniform command queue → native ACP):**
```rust
pub enum AgentCommand {
    Prompt { text: String, model: Option<String> },
    Steer { text: String },      // ACP `steer` — mid-turn injection
    Cancel,                       // ACP `session/cancel`
    Stop,
    SwitchModel { model: String },
    Slash { command: String },
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
    async fn start(&self, session_id: &str) -> anyhow::Result<()>; // spawn `hermes acp`, open stdio
    async fn send(&self, cmd: AgentCommand) -> anyhow::Result<()>; // → ACP method over stdin
    fn events(&self) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send>>; // from `session/update`
    async fn stop(&self) -> anyhow::Result<()>;
}
```

**ACP mechanics (verified against Hermes source):**
- Spawn `hermes acp` as a child; it speaks **ACP (Agent Client Protocol),
  JSON-RPC over stdio**.
- `Prompt` → `session/prompt`; `Steer` → `steer` (Hermes `acp_adapter/server.py`
  line ~456, *"Inject guidance into the currently running agent turn"*); `Cancel`
  → `session/cancel` (sets cancel_event + `agent.interrupt()`); streaming via
  `session/update` notifications.
- **No Hermes fork change needed** — these methods already exist.

**SPIKE (do first):** start `hermes acp`, send `session/prompt`, confirm streamed
`session/update`, then send `steer` mid-turn and confirm it lands. This is the
one integration to validate before building on it.

**Verify:**
```bash
cargo test bridge::acp   # unit: frame encode/decode
# integration: spawn `hermes acp`, prompt "say PONG", assert streamed PONG;
#   prompt a long task, steer mid-turn, assert the steer text influences output.
```

**Commit:** `feat: ACP bridge (hermes acp over stdio)`

### Task 4.2: state.db live sync (observe all external channels)

**Objective:** Tail `~/.hermes/state.db` for new sessions/messages from any channel.

**Files:**
- Create: `crates/control-plane/src/sync.rs`

**Design:**
- Open `~/.hermes/state.db` read-only via rusqlite (WAL mode → concurrent-safe).
- Track `last_seen_message_id` in Olympus's redb meta.
- Poll loop (~1–2s, optional inotify on `state.db-wal`):
  `SELECT * FROM messages WHERE id > ?1 ORDER BY id` → append `MessageAppended`
  events; `SELECT * FROM sessions WHERE started_at > ?1` → `SessionCreated`.
- One mechanism covers Telegram, Discord, CLI, cron, api_server, subagent.

**Verify:**
```bash
# Start a CLI session in another terminal: hermes -z "test message"
# Assert: Olympus's session list gains the new session live; the message appears.
```

**Commit:** `feat: state.db live sync`

---

## Phase 5: Import + fork (state.db migration + fork mechanics)

### Task 5.1: Import sessions from state.db

**Objective:** Bulk-import all sessions from `~/.hermes/state.db`.

**Files:**
- Create: `crates/control-plane/src/import.rs`

**Design (state.db is the complete archive — 1,629 sessions; NOT JSONL which is
pruned to ~135):**
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

### Task 5.3: Fork mechanics (prepare a forked session in state.db)

**Objective:** Implement session forking — write a forked session into state.db so
`hermes acp` can resume it.

**Files:**
- Create: `crates/control-plane/src/fork.rs`

**Design (ADR §6.6.1):**
```rust
pub fn fork_session(
    state_db: &Path, src_session_id: &str, fork_point: u64,
    fork_type: ForkType,  // SubSession | Parallel
) -> anyhow::Result<String> {  // returns new session_id
    // 1. open state.db (WAL, read-write for the fork-prep write)
    // 2. create new session row; copy src messages [0..fork_point] into it
    // 3. inject system message:
    //    <olympus fork="true" from_agent="..." from_session="<src>"
    //             fork_point="<n>" olympus_session="<new>"/>
    // 4. record forked_from + fork_point + fork_type in Olympus's event log
    // 5. (envoy then runs `hermes acp` --resume <new>)
}
```

### Task 5.4: Import + fork verification

```bash
sqlite3 ~/.hermes/state.db "SELECT COUNT(*) FROM sessions;"   # 1629
sqlite3 ~/.hermes/state.db "SELECT COUNT(*) FROM messages;"   # 108169
curl localhost:8787/api/sessions | jq '.sessions | length'    # 1629
# Fork: fork a session, assert `hermes acp --resume <fork>` loads the copied
#   history + the marker; assert the source session row is unchanged.
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
4. Session list shows 1,626 sessions.
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
| 5 | 3 | Import from state.db (1,626 sessions) |
| 6 | 1 | tantivy search |
| 7 | 5 | React UI (list, chat, search, settings) |
| 8 | 3 | Integration, systemd, Tauri |

**Total: 24 tasks across 9 phases.** Each task is independently committable.
The riskiest is Phase 4 (Hermes bridge) — spike the gateway protocol first.
