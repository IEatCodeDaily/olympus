# Hermes ACP session/resume semantics — empirical findings

**Ticket:** olympus wayfinder research #2 · **Date:** 2026-07-09
**Agent under test:** `hermes acp` (Hermes Agent v0.18.0 (2026.7.1) · upstream aabfedca), `hermes acp --check` → OK.
**Method:** Python `subprocess.Popen` probes speaking newline-delimited JSON-RPC 2.0 over stdio. Probe scripts: `/tmp/acp_probe.py`, `/tmp/exp1_kill_resume.py`, `/tmp/exp23_double_state.py`, `/tmp/exp4_cost.py`. All output below is captured from real runs.

## Summary

| # | Question | Verdict |
|---|----------|---------|
| 1 | Resume after SIGTERM mid-turn | **Works, no corruption.** But the interrupted turn's partial assistant output is **LOST** — only the user message was persisted (`Restored … from DB (1 messages)`). |
| 2 | Double-attach (resume while old child alive) | **No lock, no error, no breakage.** Both children can prompt the same session concurrently-ish. Safe but the two children hold independent in-memory copies — divergence risk, not a crash risk. Old child does NOT need to be killed first for resume to succeed. |
| 3 | State locality after resume | **Model = persisted** in Hermes store (survives kill+resume). **Mode = child-local** (reverts to `default` after kill+resume; new envoy must re-apply `session/set_mode`). |
| 4 | Resume cost | **~2.0–2.5 s flat**, dominated by fixed overhead, not history size: 6-msg session 2.10–2.25 s; 75-msg 2.00 s; 287-msg 2.46 s (345 replay updates, ~19.6 KB agent text). |
| ⚠ | Bonus hazard | `session/resume` with an **unknown sessionId does NOT error** — it logs `resume_session: session <id> not found, creating new` and silently returns a **new** session. Detect via `result._meta.hermes.sessionProvenance.acpSessionId != requested id`. |

---

## 1. Resume after kill mid-turn

**Method:** Child A: `initialize` → `session/new` (sid `bf7a9611-2ba9-420d-9dca-126001aa050b`) → `session/prompt` ("Count from 1 to 30…"). After ≥20 chars of `agent_message_chunk` output streamed (~4.9 s in), SIGTERM to A (exit code −15). Fresh child B: `initialize` → `session/resume`.

**Evidence:**

```
partial output before kill (4.9s): '1\n2\n3\n...\n30'    # model had actually finished streaming
child A exit code: -15
resume dt=2.31s
resume response: {"jsonrpc":"2.0","id":2,"result":{"_meta":{...},"models":{...}}}   # success, no error
replay notifications: 3
update types: {'user_message_chunk': 1, 'available_commands_update': 1, 'usage_update': 1}
replayed agent text (0 chars): ''
B stderr: acp_adapter.session: Restored ACP session bf7a9611-... from DB (1 messages)
```

**Findings:**
- `session/resume` on a mid-turn-killed session **succeeds cleanly**; no corruption, no error, no dangling lock.
- The killed turn's assistant output was **not persisted**: even though the full "1..30" had streamed to the client, replay contained only the *user* message (1 message in DB). Hermes appears to persist the assistant message at turn completion, not incrementally per chunk. (Caveat: the model finished streaming ~instantly before the kill, so we killed during the post-stream window; either way, the streamed text never reached the DB.)

**Implication for envoy handover:** a crash mid-turn loses the in-flight assistant turn. The Hall must treat any turn in flight at handover time as *aborted and unrecorded* — either re-send the prompt after resume or surface the loss. The client-side transcript (chunks the old envoy relayed) is the only record of the partial output.

## 2. Double-attach

**Method:** Child A: new session `31370478-4e32-4900-b297-2dc537a11b5b`, one prompt ("Reply with just: ok" → `ok`). While A stayed alive and idle, child B resumed the same sid; then prompts through both.

**Evidence:**

```
== B resume while A alive: dt=3.96s ok=True     # no lock/conflict error
B replay updates: 4  replayed agent text: 'ok'
A proc poll: None                                # A still alive
A prompt after B-resume ok=True dt=1.7s stop=end_turn
B prompt while A alive ok=True dt=2.5s stop=end_turn
A proc poll after B prompt: None                 # A still alive after B's turn
B stderr: conversation turn: session=31370478-... platform=acp history=2 ...
```

**Findings:**
- B's resume succeeded with A alive. No lock, no `session busy`, no error anywhere.
- A remained alive and fully functional (prompted successfully *after* B attached).
- B prompted successfully too. Note B's turn ran with `history=2` — B had A's pre-resume history plus its own turn, but **not** A's post-resume turn ("A2"): each child holds an independent in-memory conversation and appends to the shared DB. A later fresh resume (child C) replayed **all** turns from both: `'okA2B1'` — the DB interleaves writes from both children.
- No writer arbitration exists. Concurrent prompting from two children doesn't crash but produces interleaved history where neither child saw the other's turns in its own context.

**Implication:** the new envoy does **not** need the old child dead before resuming — resume is safe at any time. But the Hall must guarantee **at most one child prompts** a session at a time (stop routing prompts to the old envoy before the new one takes over), otherwise histories silently diverge/interleave. Kill-before-resume is unnecessary; *quiesce-before-prompt* is mandatory.

## 3. State locality

**Method:** In session A: `session/set_model {modelId:"anthropic:claude-opus-4-8"}` → `{}` (ok) and `session/set_mode {modeId:"dont_ask"}` (valid; available modes were `default`, `accept_edits`, `dont_ask`). Kill child, resume in a fresh child, inspect `result.models.currentModelId` / `result.modes.currentModeId`.

**Evidence:**

```
A new_session: current model anthropic:claude-fable-5, mode default
A set_model → {} ; A set_mode → {}
B resume (A alive):  models.current = anthropic:claude-opus-4-8   # model change visible immediately
C resume (A dead):   models.current = anthropic:claude-opus-4-8   # model PERSISTED
set_mode dont_ask → {}; kill; fresh resume: currentModeId = default   # mode LOST
```

Also confirmed in old-child stderr after model switch: `Stored system prompt … has stale runtime identity; rebuilding for model=claude-opus-4-8` — the model choice lives in the session store.

| State | Survives kill+resume? | Where it lives |
|---|---|---|
| Conversation history (completed turns) | ✅ | state.db |
| `currentModelId` | ✅ | Hermes session store |
| `currentModeId` (permission mode) | ❌ resets to `default` | dead child's memory only |
| In-flight turn output | ❌ | dead child's memory only |
| cwd / mcpServers | n/a — re-supplied as `session/resume` params by the new client | envoy-owned |

**Implication:** after resume the new envoy owes: re-send `session/set_mode`, re-supply `cwd` + `mcpServers` in the resume params, and reconcile any in-flight turn. It does **not** need to re-apply the model.

## 4. Resume cost

**Method:** Wall-clock from writing the `session/resume` request line to receiving its response (child already initialized). Long sessions taken from `~/.hermes/state.db` (read-only), `source='acp'`.

| Session | Messages in DB | Resume time | Replay updates | Replayed agent text |
|---|---|---|---|---|
| probe session (exp 2/3) | ~6 | **2.10 s / 2.25 s** (2 runs) | 8 | 'okA2B1' |
| `6ef30386-…` | 75 | **2.00 s** | 110 | 3,342 chars |
| `71b47c5d-…` | 287 | **2.46 s** | 345 | 19,639 chars |

**Findings:** resume latency is ~2.0–2.5 s and essentially **flat with history size** — dominated by fixed session-restore overhead (tool registry, provider init inside the already-running child), not replay. The full transcript is replayed as `session/update` notifications *within* the resume request lifetime (345 updates for the 287-msg session), so the envoy must drain notifications concurrently with awaiting the response. Add ~3–5 s of child process startup + `initialize` on top for total handover budget.

**Failed/inconclusive notes (not papered over):**
- First long-session attempt used Hermes-internal IDs (`mqvw189y0xteuv`, 7034 msgs, non-ACP source). Resume did **not** error — it silently created a fresh session (stderr: `resume_session: session mqvw189y0xteuv not found, creating new`; `_meta` provenance carried a different sessionId). So 4(b) numbers above use genuine ACP-sourced sessions instead; no 1000+-message ACP session existed to test extreme scale. Extrapolation from 6→287 msgs suggests replay cost grows slowly, but that is extrapolation.
- Whether ACP-vs-non-ACP source or the ID itself caused the "not found" was not isolated; either way the silent-new-session behavior is confirmed and is itself a handover hazard.
- Exp 1 caveat: the model finished streaming "1..30" faster than expected (~5 s), so SIGTERM landed at end-of-stream rather than mid-stream; the persisted-state conclusion (assistant turn absent from DB) holds regardless.

## Envoy handover design consequences

1. **Kill-before-resume is NOT required.** Resume works with the old child alive; no lock exists. Ordering can be: new envoy spawns + resumes → Hall flips prompt routing → old envoy drains/dies.
2. **But single-writer discipline is on the Hall.** Nothing in Hermes stops two children from prompting one session; histories interleave silently. Enforce exactly-one-active-prompter at the control plane.
3. **In-flight turns die with the child.** Handover should wait for turn completion (`session/prompt` response) when possible; on crash handover, mark the in-flight turn lost and let the client decide to re-prompt.
4. **Post-resume re-apply checklist:** `session/set_mode` (always — resets to `default`), cwd + mcpServers (resume params). Model persists; skip.
5. **Verify resume identity.** Always compare `result._meta.hermes.sessionProvenance.acpSessionId` to the requested sessionId — a mismatch means Hermes silently created a new session and the handover must be treated as failed.
6. **Budget ~2.5 s** for resume itself (+child spawn/init); drain `session/update` replay notifications concurrently, keyed on `agent_message_chunk`/`user_message_chunk`, before considering the session live.
