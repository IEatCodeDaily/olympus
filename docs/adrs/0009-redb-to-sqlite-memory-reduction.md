# ADR 0009 — redb → SQLite: Memory Reduction & Delta Fanout

Status: accepted · Date: 2026-07-10
Related: ADR 0002 (§2.4 — event-sourced projections), ADR 0008 (Hall/Envoy split)

## Context

Olympus Hall currently uses 1.4 GB of RAM on a 7.7 GB VPS. The host runs
Hermes Agent, Hermes Studio, Rust builds (Olympus S7), TypeScript language
servers, and kanban workers concurrently. Swap is exhausted (2/2 GB), load
average hits 24, and the system freezes under memory pressure.

Hermes Agent — written in Python — manages the same SQLite workload
(`state.db`, 1.7 GB, 1,938 sessions, 137K messages) in under 500 MB. A
Rust binary holding 3× the memory for a subset of that data is a design
problem, not a language problem.

### Root cause: unbounded in-memory materialized views

The current architecture (ADR 0002 §2.4):

```
redb event log (181 MB on disk)
  → in-memory ViewManager (SessionView + MessageView + CardView + ...)
    → tantivy search index (154 MB on disk, mmap'd)
      → axum REST + WebSocket
```

`ViewManager::replay()` loads every event from the redb log on boot and
builds in-memory projections that stay resident for the process lifetime:

- **SessionView** — `HashMap<String, SessionRow>` holding all 1,938 sessions.
- **MessageView** — `HashMap<String, VecDeque<MessageRow>>` with a 50-message
  sliding window per session. Each `MessageRow` holds decompressed content,
  tool\_calls, and reasoning as owned `String`s. With 1,938 sessions × 50
  messages × average ~8 KB of decompressed text, this alone is ~800 MB.
- **tantivy** — 50 MB writer heap + mmap'd index files.
- **redb** — mmap of the 181 MB log file (counts toward RSS under pressure).

### Data volume (measured)

| Metric | Value |
|--------|-------|
| Sessions | 1,938 |
| Active messages | 136,989 |
| Total content text | ~220 MB (raw) |
| Hermes `state.db` | 1.7 GB |
| Olympus `eventlog.redb` | 181 MB |
| Tantivy search index | 154 MB on disk |
| Process RSS | **1.4 GB** |

### Goal

Reduce Olympus Hall RSS to **< 100 MB** without losing functionality.
The event-sourcing model, REST API, WebSocket delta stream, and full-text
search must all continue to work.

## Decision

Replace redb + in-memory views + tantivy with a **single SQLite database**
in WAL mode. SQLite becomes both the event log and the read-side projection
store. No in-memory materialized views.

### New architecture

```
SQLite (WAL mode, ~/.olympus/olympus.db)
  ├── events table (append-only, same event-sourcing semantics)
  ├── sessions table (materialized projection, queryable)
  ├── messages table (materialized projection, queryable)
  ├── messages_fts (FTS5 virtual table — replaces tantivy)
  └── cards / setup / registry / projects / repos tables
        ↓
  axum REST handlers (SELECT on every request)
  WebSocket delta stream (broadcast::Sender<ServerFrame> — unchanged)
```

### Schema

```sql
-- Event log (append-only, sole source of truth — same semantics as redb)
CREATE TABLE events (
  seq        INTEGER PRIMARY KEY AUTOINCREMENT,
  event_type TEXT NOT NULL,
  payload    BLOB NOT NULL,           -- JSON-serialized, zstd-compressed
  created_at REAL NOT NULL
);

-- Materialized session projection (was: in-memory SessionView)
CREATE TABLE sessions (
  session_id        TEXT PRIMARY KEY,
  hermes_id         TEXT,
  source            TEXT,
  model             TEXT,
  title             TEXT,
  started_at        REAL NOT NULL,
  message_count     INTEGER DEFAULT 0,
  input_tokens      INTEGER DEFAULT 0,
  output_tokens     INTEGER DEFAULT 0,
  archived          INTEGER DEFAULT 0,
  pinned            INTEGER DEFAULT 0,
  last_activity     REAL,
  agent             TEXT,
  node              TEXT,
  parent_session_id TEXT,
  card_id           TEXT,
  project_id        TEXT
);
CREATE INDEX idx_sessions_started ON sessions(started_at DESC);
CREATE INDEX idx_sessions_source  ON sessions(source);
CREATE INDEX idx_sessions_archived ON sessions(archived);
CREATE INDEX idx_sessions_pinned  ON sessions(pinned);

-- Messages (was: in-memory MessageView with 50-msg sliding window)
-- No eviction needed — full history lives on disk, paged on demand.
CREATE TABLE messages (
  session_id    TEXT NOT NULL,
  message_id    INTEGER NOT NULL,
  role          TEXT,
  content       TEXT,                 -- plain TEXT (SQLite handles compression)
  tool_name     TEXT,
  tool_calls    TEXT,
  reasoning     TEXT,
  timestamp     REAL,
  token_count   INTEGER,
  finish_reason TEXT,
  PRIMARY KEY (session_id, message_id)
) WITHOUT ROWID;
CREATE INDEX idx_messages_ts ON messages(session_id, timestamp);

-- Full-text search (replaces tantivy)
CREATE VIRTUAL TABLE messages_fts USING fts5(
  session_id  UNINDEXED,
  message_id  UNINDEXED,
  content,
  role        UNINDEXED,
  tool_name   UNINDEXED,
  timestamp   UNINDEXED,
  tokenize = 'porter unicode61'
);

-- Triggers to keep FTS in sync with messages table
CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts(session_id, message_id, content, role, tool_name, timestamp)
  VALUES (new.session_id, new.message_id, new.content, new.role, new.tool_name, new.timestamp);
END;
CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
  DELETE FROM messages_fts WHERE session_id = old.session_id AND message_id = old.message_id;
END;
```

Remaining projections (cards, setup, registry, projects, repos) follow the
same pattern: one table per view, indexed for the query patterns the REST
handlers use.

### Delta fanout: the broadcast channel stays

The `tokio::sync::broadcast::Sender<ServerFrame>` in `AppState` is the
fanout mechanism — and it doesn't change. The key insight is that
ServerFrames already carry their data inline:

```rust
MessageAppended { session_id, message: MessageDto { role, content, ... } }
SessionUpdated { session_id, changes: serde_json::Value }
```

The in-memory views were never the source of truth for broadcasts. The
write path was:

```
receive event → append to redb → apply to in-memory view → broadcast ServerFrame
```

The new write path:

```
receive event → INSERT/UPDATE in SQLite (one transaction) → broadcast ServerFrame
```

All ServerFrame data comes from the event itself, not from a view lookup.
The broadcast channel (capacity 1024) stays as-is; the WebSocket handler
in `server/ws.rs` stays as-is. The only thing that changes is what backs
the read path.

### Read path: direct SQLite queries

REST handlers currently do `state.views.read().await.sessions.list(&filters)`.
New pattern:

```rust
async fn list_sessions(State(state): State<AppState>, Query(q): Query<...>) -> impl IntoResponse {
    let conn = state.db.conn();
    let rows = sessions::list(&conn, &q.source, q.archived, q.pinned, q.limit)?;
    Json(rows.into_iter().map(SessionDto::from).collect::<Vec<_>>())
}
```

SQLite in WAL mode supports unlimited concurrent readers alongside a single
writer. The page cache (default 2 MB, tunable) plus the OS page cache handles
read buffering. Indexed lookups on `(session_id, message_id)` or
`(started_at DESC)` are sub-millisecond on NVMe.

### Search: FTS5 replaces tantivy

```sql
SELECT session_id, message_id,
       snippet(messages_fts, 2, '<mark>', '</mark>', '...', 32) as snippet,
       rank
FROM messages_fts
WHERE messages_fts MATCH ?
ORDER BY rank
LIMIT ?;
```

FTS5 provides BM25 ranking (the `rank` column), snippet generation, and
 porter + unicode61 tokenization. This covers the same query surface as the
 current tantivy integration (`SearchIndex::search`) without a separate
 index format, process, or mmap region.

Trade-off: FTS5 lacks tantivy's custom analyzers and multi-field boost
weighting. For keyword search over message content — the only use case —
this is not a meaningful loss.

### Write path: single transaction per event

A `Store` struct wraps a `rusqlite::Connection` (or a `r2d2` pool) and
exposes the same `append(event)` interface as the current `Log`:

```rust
impl Store {
    pub fn append(&self, event: &Event) -> Result<()> {
        let tx = self.conn.transaction()?;
        // 1. Append to event log
        tx.execute("INSERT INTO events (event_type, payload, created_at) VALUES (?, ?, ?)", ...)?;
        // 2. Apply projection (the match on Event variant, same logic as ViewManager::apply)
        self.apply_projection(&tx, event)?;
        // 3. Commit (FTS triggers fire here)
        tx.commit()?;
        // 4. Broadcast (caller does this, or Store takes a broadcast::Sender)
        Ok(())
    }
}
```

`apply_projection` is the same `match event { SessionCreated => INSERT, SessionUpdated => UPDATE, MessageAppended => INSERT, ... }`
logic that `ViewManager::apply` already does — it just targets SQL instead
of HashMaps.

### Memory budget

| Component | Before | After |
|-----------|--------|-------|
| SessionView (HashMap) | ~20 MB | 0 (SQLite) |
| MessageView (decompressed windows) | ~800 MB | 0 (SQLite) |
| CardView, SetupView, etc. | ~10 MB | 0 (SQLite) |
| Tantivy writer heap + mmap | ~100 MB | 0 (FTS5 on disk) |
| redb mmap | ~180 MB | 0 (SQLite) |
| Tokio broadcast | ~1 MB | ~1 MB |
| Axum / router / misc | ~20 MB | ~20 MB |
| SQLite page cache | — | ~2–10 MB |
| **Total RSS** | **~1.4 GB** | **< 50 MB** |

SQLite's page cache defaults to 2 MB. Even under load with aggressive
querying and WAL growth, RSS should stay well under 100 MB.

## Migration path

1. Add `rusqlite` dependency alongside `redb` (not replacing yet).
2. Implement `SqliteStore` with the same interface as `Log` +
   `ViewManager`.
3. Write a one-shot migrator: read all events from `eventlog.redb` →
   write to `olympus.db` → verify row counts match.
4. Switch `AppState` to hold `Arc<SqliteStore>` instead of
   `Arc<RwLock<ViewManager>>`.
5. Update REST handlers to query SQLite instead of reading views.
6. Replace `SearchIndex` (tantivy) with FTS5 queries.
7. Remove `redb`, `tantivy` dependencies and the `log.rs`, `search.rs`,
   `views/` modules.
8. Delete `eventlog.redb` and `search-index/` after confirming the
   SQLite migration is stable.

The event-sourcing invariant is preserved throughout: the `events` table
is the sole source of truth, and all projection tables are deterministic
functions of it. They can be rebuilt from `events` at any time.

## Consequences

### Legacy log removal addendum

The legacy redb log implementation and dependency have been deleted. Existing
`~/.olympus/eventlog.redb` files are inert: Hall warns once at boot, ignores the
file without reading it, and operators may remove it manually.

### Self-describing event payload addendum

The sole SQLite `events.payload` codec in steady state is JSON compressed with
zstd (`json+zstd-v1`). There is no long-lived dual-codec mode: the only
exception is Hall's marker-guarded, transactional boot rewrite from historical
postcard payloads to JSON+zstd. That rewrite verifies the event count and each
event's semantic equality, changes only the payload encoding, and does not alter
event content, sequence ordering, append-only history semantics, REST DTOs, or
WebSocket/proto wire-frame formats. Once it records
`meta.event_payload_codec = json+zstd-v1`, normal operation reads and writes only
JSON+zstd event payloads.

Future `Event` schema evolution is additive. New persisted fields must either be
optional by construction or use `#[serde(default)]` when older JSON records may
omit them. Positional enum or field reshaping is no longer an accepted
persistence-evolution strategy for the SQLite event log.

The rationale is settled rather than reopened here: JSON keeps event payloads
inspectable with the `sqlite3` CLI for operations and debugging, while zstd keeps
the on-disk size bounded enough for the control-plane workload.

- **RSS drops from ~1.4 GB to < 50 MB.** 15× reduction.
- **Startup is faster.** No log replay into in-memory views — SQLite
  reads are lazy, first query pays the I/O, not the whole boot.
- **Crash safety improves.** SQLite WAL gives durable commits with proper
  crash recovery. redb was durable too, but we now get ACID across the
  event + projection in one transaction.
- **Read latency increases slightly.** In-memory HashMap lookups (~100 ns)
  become SQLite indexed reads (~10–50 µs on NVMe). For a UI serving a
  single operator, this is imperceptible.
- **Search features narrow.** FTS5 covers keyword search + snippets + BM25
  but drops tantivy's custom analyzers. Acceptable for the use case.
- **The `events` table can be vacuumed / archived** independently of the
  projections if the log grows large. Not needed for MVP but available.
- **Backup becomes simpler.** One `.db` file (plus `-wal` and `-shm`)
  instead of redb + tantivy directories. `VACUUM INTO` for snapshots.
