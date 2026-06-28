# Fork Spike: Session Fork on a Copied state.db

**Date:** 2026-06-28  
**Author:** Zephyr (adversarial review blocker #2)  
**Status:** VERIFIED on copy — gates the `create_fork` Hermes patch  
**Spike script:** /tmp/fork_spike.py (prototype only, not production code)

---

## 0. Goal

Prove the exact transaction required to create an ACP-resumable fork of an
arbitrary Hermes session (any source: cli, telegram, discord, etc.) so that:

1. `hermes acp` / `SessionManager._restore()` loads it without modification.
2. All Hermes app-level invariants are satisfied (no counter drift, no FTS gaps,
   no FK violations, no listability issues).
3. The live DB is never touched during development/testing.

---

## 1. Safety boundary

All work was done on a copy:

```
cp ~/.hermes/state.db     /tmp/fork-spike.db
cp ~/.hermes/state.db-shm /tmp/fork-spike.db-shm
cp ~/.hermes/state.db-wal /tmp/fork-spike.db-wal
```

Live DB snapshot at copy time: 1634 sessions, 108907 messages.  
After spike: copy has 1635 sessions, 108910 messages. Live DB untouched.

---

## 2. The ACP resume gate: source == "acp"

The single most important invariant is `sessions.source = 'acp'`.

`SessionManager._restore()` (acp_adapter/session.py:488-490) reads:

```python
if row.get("source") != "acp":
    return None
```

Any session with a different source is invisible to `session/resume`. This
means forking a telegram or cli session into an acp-resumable session requires
writing a **new row** with `source='acp'`, not modifying the original.

---

## 3. Complete invariant list

Every column/flag that Hermes checks at load or restore time. A naive INSERT
that skips any of these will produce an invisible, broken, or counter-drifted
fork.

| # | Column / field | Required value | Notes |
|---|---|---|---|
| 1 | `sessions.source` | `'acp'` | Hard gate in `_restore()`. Any other value → None returned. |
| 2 | `sessions.parent_session_id` | `<src_session_id>` | FK into sessions table. Must reference a real row. Enables branch visibility via `_BRANCH_CHILD_SQL`. |
| 3 | `sessions.model_config` | JSON with `"cwd"` key | `_restore()` extracts cwd from `json.loads(model_config).get("cwd", ".")`. Missing cwd → cwd defaults to "." (safe but wrong for editor sessions). |
| 4 | `model_config._branched_from` | `<src_session_id>` | Hermes's branch visibility check (`_BRANCH_CHILD_SQL`): `json_extract(model_config, '$._branched_from') IS NOT NULL`. Without this the fork shows as an ephemeral subagent child and is hidden from the session picker. |
| 5 | `sessions.cwd` | Same as `model_config.cwd` | Denormalized column used by `_cwd_prefix_clause()` and cwd-filtered session lists. Must match the cwd in model_config. |
| 6 | `sessions.message_count` | Exact count of active messages | NOT maintained by triggers. Must be set explicitly via `UPDATE sessions SET message_count = ? WHERE id = ?` after inserting messages. Counter drift causes broken pagination/display. |
| 7 | `sessions.tool_call_count` | Sum of `len(tool_calls)` for all active messages with non-null tool_calls | Same trigger caveat. Must be counted from the message rows and set explicitly. |
| 8 | `messages.active` | `1` for all copied messages | Default is 1 in schema. Must be explicit in INSERT (don't rely on DEFAULT when copying from another session — the source rows are all active=1 anyway, but be explicit). |
| 9 | `messages.compacted` | `0` for all copied messages | Must be explicit. A compacted=1 row is treated as "summarized away" by `get_messages_as_conversation`. |
| 10 | `messages.observed` | Copy from source, default 0 | Affects platform-specific recall flows. Safe to copy; safe to default 0. |
| 11 | FTS triggers | Automatic | The `messages_fts_insert` trigger fires on every `INSERT INTO messages` and adds the row to both `messages_fts` and `messages_fts_trigram`. No manual FTS update needed. |
| 12 | `sessions.title` | NULL on creation | The `idx_sessions_title_unique` unique index prevents two sessions with the same non-NULL title. Forked sessions should start with NULL title; Hermes sets it lazily. |
| 13 | `sessions.archived` | 0 (DEFAULT) | Fork should not start archived. |
| 14 | `sessions.model` | Copy from source | Used by `_restore()` to recreate the AIAgent with the correct model. |
| 15 | FK integrity | `PRAGMA foreign_keys = ON` | Must enable FK enforcement on the connection. The sessions table has `FOREIGN KEY (parent_session_id) REFERENCES sessions(id)`. |
| 16 | WAL write serialization | `BEGIN IMMEDIATE` | Hermes uses `BEGIN IMMEDIATE` + jitter retry (hermes_state.py:1008-1058). The fork transaction should do the same to avoid writer contention with a live Hermes process. |

### Columns safe to omit / leave NULL on fork

- `user_id` — copy from source if present, NULL is fine
- `system_prompt` — copy from source; NULL is fine for acp sessions
- `started_at` — set to current `time.time()` (the fork is a new session born now)
- `ended_at`, `end_reason` — NULL (session is open)
- `input_tokens`, `output_tokens`, `cache_*_tokens`, `reasoning_tokens` — 0 (will accumulate from future turns)
- `billing_*` — NULL (populated by future turns)
- `git_branch`, `git_repo_root` — NULL initially; Hermes or ACP client fills later
- `rewind_count` — 0 (DEFAULT)

---

## 4. Is source='acp' sufficient for resume?

Yes, with the following conditions all met:

1. `source = 'acp'` (hard gate).
2. `model_config` is valid JSON containing at minimum `{"cwd": "<path>"}`.
3. At least one message row exists with `active = 1` (otherwise `list_sessions`
   and `get_messages_as_conversation` return empty history and the session is
   invisible in the ACP session list).
4. `message_count` counter is correct (the ACP session list and resume both use
   this for display and sanity checks).

The `_restore()` path then:
1. Reads the session row.
2. Extracts `cwd` from `model_config`.
3. Calls `db.get_messages_as_conversation(session_id)` — filters `active=1`,
   ordered by `timestamp, id`.
4. Creates a fresh `AIAgent` with the stored model/provider/cwd.
5. Returns a `SessionState` with the recovered history.

No special flags, no extra tables, no `hermes acp --resume` CLI flag (that
does not exist — resume is an ACP JSON-RPC method `session/resume`, not a CLI
argument).

---

## 5. The exact required transaction (SQL)

```sql
-- Run with PRAGMA foreign_keys = ON and BEGIN IMMEDIATE

-- Step 1: Insert the fork session row
INSERT INTO sessions (
    id, source, user_id, model, model_config, system_prompt,
    parent_session_id, cwd, started_at,
    message_count, tool_call_count,
    input_tokens, output_tokens
) VALUES (
    '<new_uuid>',
    'acp',                              -- MUST be 'acp'
    <src.user_id or NULL>,
    <src.model>,
    '{"cwd":"<fork_cwd>","_branched_from":"<src_id>"}',
    <src.system_prompt or NULL>,
    '<src_session_id>',                 -- FK linkage + branch visibility
    '<fork_cwd>',                       -- denormalized cwd column
    <time.time()>,
    0,                                  -- set to 0 first; update below
    0,
    0,
    0
);

-- Step 2: Copy messages up to fork_point (active=1 rows only)
INSERT INTO messages (
    session_id, role, content, tool_call_id, tool_calls, tool_name,
    timestamp, token_count, finish_reason,
    reasoning, reasoning_content, reasoning_details,
    codex_reasoning_items, codex_message_items,
    platform_message_id, observed, active, compacted
)
SELECT
    '<new_uuid>',   -- new session id
    role, content, tool_call_id, tool_calls, tool_name,
    timestamp, token_count, finish_reason,
    reasoning, reasoning_content, reasoning_details,
    codex_reasoning_items, codex_message_items,
    platform_message_id, observed,
    1,   -- active = 1 (explicit)
    0    -- compacted = 0 (explicit)
FROM messages
WHERE session_id = '<src_session_id>'
  AND id <= <fork_point_msg_id>
  AND active = 1
ORDER BY id;
-- FTS triggers (messages_fts_insert, messages_fts_trigram_insert) fire
-- automatically on each INSERT — no manual FTS update needed.

-- Step 3: Update counters (NOT done by triggers — Hermes app-level only)
UPDATE sessions
SET
    message_count = (
        SELECT COUNT(*) FROM messages
        WHERE session_id = '<new_uuid>' AND active = 1
    ),
    tool_call_count = (
        SELECT COALESCE(SUM(
            CASE WHEN tool_calls IS NOT NULL THEN
                json_array_length(tool_calls)
            ELSE 0 END
        ), 0)
        FROM messages
        WHERE session_id = '<new_uuid>' AND active = 1
    )
WHERE id = '<new_uuid>';

COMMIT;
```

---

## 6. What a naive INSERT misses (would-be bugs)

1. **source != 'acp'**: session is permanently invisible to `session/resume`.
2. **No `_branched_from` in model_config**: fork looks like an ephemeral
   subagent child, hidden from the session picker by `_LISTABLE_CHILD_SQL`.
3. **message_count = 0 or wrong**: session shows as empty in the ACP session
   list; `list_sessions` filters out sessions with `message_count <= 0`.
4. **active = 0 on copied messages**: `get_messages_as_conversation` skips them
   (default filter is `active = 1`); history appears empty to the agent.
5. **Missing cwd in model_config**: `_restore()` falls back to `cwd = "."` —
   agent starts in the wrong directory; tool calls and file ops go to the
   wrong path.
6. **Skipping `BEGIN IMMEDIATE`**: concurrent Hermes write may interleave with
   the fork transaction, leaving inconsistent counter/message state.
7. **NULL title collisions**: if you copy the source session's non-NULL title,
   `idx_sessions_title_unique` raises `UNIQUE constraint failed`.
8. **Ignoring the -wal and -shm sidecars in a copy**: the WAL may contain
   unflushed pages; copying only state.db gives a stale or corrupt copy.
   Always copy all three files together.

---

## 7. Recommended `SessionDB.create_fork()` signature

```python
def create_fork(
    self,
    src_id: str,
    fork_point_msg_id: int | None = None,
    *,
    fork_type: str = "cross-channel",
    new_cwd: str | None = None,
    model: str | None = None,
) -> str:
    """Create an ACP-resumable fork of session *src_id*.

    Args:
        src_id:             Source session id (any source).
        fork_point_msg_id:  Copy messages with id <= this value (active=1 only).
                            None = copy all active messages.
        fork_type:          Metadata label stored in model_config._fork_type
                            (e.g. "cross-channel", "branch", "replay").
                            Hermes ignores unknown model_config keys.
        new_cwd:            Override cwd for the fork. Defaults to src cwd.
        model:              Override model for the fork. Defaults to src model.

    Returns:
        New session id (UUID string).

    Guarantees:
        - source = 'acp' (ACP resume gate)
        - parent_session_id = src_id (FK + branch visibility)
        - model_config._branched_from = src_id (session picker visibility)
        - model_config.cwd = fork_cwd (matches sessions.cwd column)
        - message_count / tool_call_count are exact (app-level counters)
        - All copied messages have active=1, compacted=0
        - FTS indexes are consistent (triggers fire on INSERT)
        - title = NULL (avoids unique constraint)
        - Transaction uses BEGIN IMMEDIATE + jitter retry (Hermes write protocol)
    """
```

Implementation location: `hermes_state.py` inside `SessionDB`, as a peer of
`create_session()` and `replace_messages()`. This is a Hermes-side helper —
Olympus calls it over ACP or via a new ACP method, never writes raw SQL to
state.db.

---

## 8. Verified results (from spike run)

Source session: `mqwqp1if9jcgtj` (source=cli, 6 messages, model=glm-5.2)  
Fork point: message id 118893 (after the first assistant reply — 3 messages)  
Fork id: `91d66fa0-51b8-47a9-a660-9105697e8c37`

All 13 invariant checks passed:

- source: acp
- model_config keys: cwd, _branched_from, _fork_type, _fork_point_msg_id
- parent_session_id: mqwqp1if9jcgtj
- message_count: 3 (matches db count)
- tool_call_count: 0 (correct)
- all_active: True
- fts_consistent: True (all 3 message IDs in messages_fts)
- fts_trigram_consistent: True (3 trigram hits for "hey")
- fk_ok: True (PRAGMA foreign_key_check passes)
- visible_in_acp_list: True
- restore_cwd: . (extracted correctly from model_config)
- history_length: 3 (messages load in timestamp,id order)
- title_is_null: True
- archived: False
- _restore() source guard: PASS — session would be loaded by ACP
- listable_kind: branch-child (not hidden as ephemeral subagent)

---

## 9. What this does NOT gate

- The new ACP method (`create_fork` over the wire) — that is the patch task.
- Mutation of the live DB — still requires human-confirmed backup/rollback.
- Partial-history fork via fork_type="replay" with ancestor chain traversal —
  this spike only tested single-session copy; `get_messages_as_conversation`
  with `include_ancestors=True` would need separate verification for
  compression-chained sessions.
- Concurrent write safety under live Hermes — the spike ran against an idle
  copy. Production needs `BEGIN IMMEDIATE` + Hermes jitter retry
  (`_execute_write()` protocol in hermes_state.py:1008-1058).
