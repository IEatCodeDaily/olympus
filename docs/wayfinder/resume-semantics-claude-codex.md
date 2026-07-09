# Resume/handover semantics — Claude Code & Codex ACP adapters

**Ticket:** olympus wayfinder research #10 · **Date:** 2026-07-09
**Complements:** `resume-semantics.md` (hermes acp, ticket #2)
**Method:** claude-code-acp probed empirically (live child processes, real ACP
frames); codex-acp analyzed from vendored source (`/tmp/codex-acp-src`, the
`@zed-industries/codex-acp@0.16.x` Rust crate) because the `codex` CLI is not
installed on this host. A prior background subagent hung on this work; findings
below were produced by direct probing.

## Verdict table (all three harnesses)

| Capability | hermes acp | claude-code-acp 0.16.2 | codex-acp 0.16.x |
|---|---|---|---|
| Advertises `loadSession` | ✅ | ✅ | ✅ (`codex_agent.rs:455`) |
| `sessionCapabilities.resume` | ✅ | ✅ (`resume:{}` in init response) | ✅ (`:461`) |
| `session/resume` from a fresh child | ✅ works, ~2.5 s | ✅ works, **1.8 s** (verified with dead auth — restore is local) | ✅ implemented (`resume_session` → `restore_session`, `:627`) |
| History replay on re-attach | on resume (full transcript) | on `session/load` (resume returned only 1 update in probe; load is the replay path, matching ACP spec) | `session/load` replays; `session/resume` restores WITHOUT replay (`replay_history: bool`, `:652,706`) |
| Session state store | Hermes state.db | `~/.claude/` project session files | Codex rollout files under `codex_home`, discovered by `find_thread_path_by_id_str` (`:661`) |
| Resume of unknown id | ⚠ silently creates NEW session | not verified (auth wall) | ✅ errors: `resource_not_found` (`:668`) — fail-closed, better than hermes |
| Auth checked at resume | n/a (runtime creds) | ✅ 401 surfaces on prompt | ✅ `check_auth()` before restore (`:614,634`) |

## Part A — claude-code-acp (empirical)

**Initialize response (captured):**
```json
{"agentCapabilities":{"promptCapabilities":{"image":true,"embeddedContext":true},
 "mcpCapabilities":{"http":true,"sse":true},"loadSession":true,
 "sessionCapabilities":{"fork":{},"list":{},"resume":{}}},
 "agentInfo":{"name":"@zed-industries/claude-code-acp","version":"0.16.2"},
 "authMethods":[{"id":"claude-login","name":"Log in with Claude Code"}]}
```
Full parity with hermes on the capability surface: loadSession + fork/list/resume.

**Handover probe (child A → SIGTERM → child B):**
- `session/new` on A succeeded (sessionId issued) even though the Claude OAuth
  token on this host had expired — session creation is local.
- A's prompt failed with `401 authentication_error` (expired token), so no
  transcript content was written.
- B's `session/resume` of A's sessionId **succeeded in 1.8 s** with a full
  models/modes result — the session store round-trip and re-attach mechanics
  work independent of API auth, from a *different process with the original
  child dead*. That is the handover primitive, and it holds.

**Not verified (auth wall — Claude OAuth token expired on this host, `claude
/login` is interactive):** history-replay fidelity, double-attach behavior,
mid-turn kill semantics, unknown-id behavior. These need one re-run of
`/tmp/cc_handover.py` after `claude /login`. The capability surface + successful
cross-process resume make uniform handover *likely*, but replay/mid-turn
verdicts are **inference, not evidence**, until re-run.

## Part B — codex-acp (source analysis, quotes from `/tmp/codex-acp-src`)

- **Capabilities:** `initialize` builds `.load_session(true)` and
  `session_capabilities = … .close(…).list(…).resume(…)` (`codex_agent.rs:455-461`).
- **Both re-attach paths implemented:** `load_session` (`:607`) and
  `resume_session` (`:627`) both call `restore_session(…, replay_history)` —
  load replays history to the client, resume restores silently (`:652`, `:706-708`).
- **Persistence:** sessions live as Codex **rollout files**; restore resolves
  the path via `find_thread_path_by_id_str(&self.config.codex_home, …)` (`:661`)
  and rebuilds the thread with `resume_thread_from_rollout` (`:689`). State is
  on disk in `codex_home`, not in the dead child.
- **Unknown session id → `Error::resource_not_found`** (`:668`): codex-acp
  fails closed where hermes silently creates a new session. The provenance
  check from #2 remains necessary only for hermes.
- **Auth:** `check_auth()` gates both load and resume (`:614`, `:634`) — an
  unauthenticated envoy fails the handover loudly at resume time, not at the
  next prompt.
- **Untested:** everything here is code-reading; no runtime verification
  (codex CLI absent). Mid-turn kill behavior (is a partial turn flushed to the
  rollout file?) is unknown — rollout appends per event, so partial-turn
  persistence is *plausibly better* than hermes, but unverified.

## Consequences for envoy handover design

1. **Uniform handover is viable.** All three adapters advertise and implement
   cross-process session resume backed by on-disk state. #4's
   resume-then-flip state machine needs no per-harness forks in its happy path.
2. **But gate on capabilities anyway.** The envoy should read
   `agentCapabilities.loadSession` / `sessionCapabilities.resume` from each
   adapter's initialize response and report per-runtime `resumable` flags to
   Hall. Handover requires `resumable`; a future/broken adapter without it
   degrades to: session pinned to its envoy, drain waits for turn boundary,
   then the session goes runtime-less (revived later by lazy ensure_runtime,
   accepting context rebuild). Capability-driven, not name-driven — harness
   agnosticism means no `match harness { … }` in the drain path.
3. **Provenance verification stays** (hermes silent-new-session hazard);
   codex errors properly, claude unverified — the check is cheap and uniform.
4. **Post-resume re-apply checklist is per-adapter data, not code:** hermes
   needs `set_mode` re-applied; claude/codex return modes in the
   resume/load response so the envoy reconciles against desired state the
   same way for all three.
5. **Open items before ADR freeze (#9):** re-run the claude probe after
   `claude /login` (replay fidelity, double-attach, mid-turn kill); optionally
   install codex CLI to runtime-verify Part B. Neither blocks the *design* —
   both refine constants (replay budgets, mid-turn loss messaging).
