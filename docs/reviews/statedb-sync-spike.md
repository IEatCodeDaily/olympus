# Spike: state.db mutation surface — sync design implications

**Date:** 2026-06-29  
**Task:** B1 (t_846374d0) — state.db live sync  
**Question:** What are the exact mutation cases a pure `id > last_seen` tail misses?

## state.db schema (live, observed)

### messages table (mutation-relevant columns)

```sql
id         INTEGER PRIMARY KEY AUTOINCREMENT  -- monotonic, NOT contiguous
session_id TEXT NOT NULL
active     INTEGER NOT NULL DEFAULT 1         -- 0 = tombstoned (rewind/undo)
compacted  INTEGER NOT NULL DEFAULT 0         -- 1 = compacted (summarized)
timestamp  REAL NOT NULL
```

### sessions table (mutation-relevant columns)

```sql
id             TEXT PRIMARY KEY
message_count  INTEGER DEFAULT 0   -- authoritative, trigger-maintained? NO (see below)
title          TEXT                 -- mutable (auto-title, rename)
model          TEXT                 -- mutable (model switch mid-session)
archived       INTEGER NOT NULL DEFAULT 0
rewind_count   INTEGER NOT NULL DEFAULT 0
```

## Observed mutation cases

### Case 1: New message insert (append-only path)

Hermes appends a row. `active=1, compacted=0`. `id` is monotonically increasing.

**Caught by:** `SELECT ... WHERE id > ?last_seen` ✓

### Case 2: Rewind / undo (active=0 tombstoning)

Hermes sets `active=0` on one or more rows (rewind_count increments on the
session). The rows are NOT deleted — they remain with `active=0`. The messages
table keeps them as tombstones for audit.

**Caught by:** Fast tail ✗ (the id already existed; no new row appears).  
**Requires:** Reconciliation sweep detecting count/signature mismatch.

### Case 3: Compaction (active=0, compacted=1)

Hermes compacts older messages: sets `active=0, compacted=1`. The compacted
summary replaces the detail. Same row mutation pattern as rewind but with
`compacted=1`.

**Caught by:** Fast tail ✗.  
**Requires:** Reconciliation sweep.

### Case 4: replace_messages (delete + reinsert)

ACP `replace_messages` deletes rows and reinserts new ones with NEW autoincrement
IDs. The old IDs vanish; new IDs appear. This is called after turns to reconcile
tool-call structure.

**Partially caught by:** Fast tail catches the NEW rows (id > last_seen).  
**Missed by:** Fast tail does not notice the old rows are gone.  
**Requires:** Reconciliation sweep detecting row_count / max(id) decrease.

### Case 5: Session metadata mutation (title, model, archived, message_count)

The `sessions` table is independently mutable. Title changes (auto-title after
first exchange), model switches, archive toggles, and message_count updates
happen outside the messages table.

**Caught by:** Neither tail nor message-level sweep.  
**Requires:** Session-meta reconciliation comparing `sessions` columns to the
Olympus view.

### Case 6: Session insert (new session row)

A new session appears in `sessions`. This is append-only for the sessions table
(id is TEXT PK, no autoincrement).

**Caught by:** Session-meta sweep detecting a new session_id.  
**Note:** Currently the MVP import creates sessions at boot; the sync must also
detect new sessions appearing live.

## Non-contiguous ID confirmation

**Live data:** max(id) = 121,675, total rows = 110,719.  
→ 10,956 IDs are missing (deleted via replace_messages).  
→ A pure `id > last_seen` tail works for inserts but cannot detect deletions.

## message_count authority

The `sessions.message_count` column is maintained by Hermes application code, NOT
by SQLite triggers. It can drift from the actual active message count. Olympus
should treat it as authoritative for display (matching what Hermes shows the user)
but use its own count for internal consistency checks.

## Design decision: two-layer sync

1. **Fast tail (1-2s poll):** `SELECT ... FROM messages WHERE id > ?last_seen AND active = 1 ORDER BY id` → MessageAppended events. Catches inserts with low latency.

2. **Reconciliation sweep (30-60s + on-demand):** Per-session signature `(max(id), active_row_count)` compared to Olympus view state. On mismatch, re-read the session's active rows and reconcile. Also sweep `sessions` for metadata changes (title/model/archived/message_count) and new sessions.

Signature choice: `(max(id), active_row_count)` per session is sufficient. A
checksum on content is overkill — `max(id)` catches replace_messages (new IDs
appear), `active_row_count` catches rewind/compaction (count drops). We skip a
content checksum; the sweep is cheap and runs frequently enough.

## What we will NOT build

- A Hermes changefeed table patch (candidate only, per spec — record if sweep
  proves too chatty).
- Content-level checksums (overkill for the observed mutation patterns).
- inotify on state.db-wal (polling at 1-2s is sufficient for MVP; inotify adds
  platform complexity).
