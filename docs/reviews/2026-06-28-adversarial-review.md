1. VERDICT: NOT READY — fundamental issues

2. BLOCKERS (would cause wrong/broken implementation)

1) Claim: Hermes ACP exposes a uniform command API with resume-by-id, `steer`, `slash`, cancel, model switching, and streaming.
   Evidence it is wrong:
   - ACP's generated method map has `session/prompt`, `session/cancel`, `session/resume`, and `session/set_model`, but no `steer` or generic `slash` method: `/home/rpw/.hermes/hermes-agent/venv/lib/python3.11/site-packages/acp/meta.py:3-17`.
   - Hermes advertises `steer` as an available slash command, not as an ACP method: `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:449-459` and `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:461-501`.
   - Slash handling is text interception inside `session/prompt`: `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:1352-1363`; the slash dispatcher maps `/steer`, `/model`, etc. at `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:1721-1741`.
   - `hermes acp --help` exposes no `--resume`; source agrees: `/home/rpw/.hermes/hermes-agent/hermes_cli/subcommands/acp.py:16-52`.
   - ACP resume only restores rows Hermes considers ACP sessions. `SessionManager._restore` returns `None` unless `row.get("source") == "acp"`: `/home/rpw/.hermes/hermes-agent/acp_adapter/session.py:471-490`.
   Impact: an engineer implementing ADR 0002 §19 literally will call nonexistent ACP methods (`steer`, `slash`, possibly `hermes acp --resume`) and will fail to resume arbitrary external Hermes sessions. This breaks the MVP bridge.
   Suggested fix: rewrite the ACP contract in the docs as the actual wire contract: `session/resume` for ACP-owned rows, `session/prompt` for prompts and slash text (`/steer ...`, `/model ...`), `session/cancel` for cancel, `session/set_model` for model switching, and `session/update` notifications for streaming. If Olympus needs real `steer`/`slash` methods, add them to Hermes ACP first or use ACP `ext_method` deliberately and document it.

2) Claim: Olympus can fork any external session by writing a new session row + copied messages + a system marker directly into live `~/.hermes/state.db`, then `hermes acp` resumes it.
   Evidence it is wrong/unsupported:
   - The live schema has app-level invariants not captured by “insert row + messages”: sessions have counters/title/cwd/model_config/parent lineage; messages have `active`, `observed`, `compacted`; FTS is maintained by triggers; no trigger updates `sessions.message_count` or `tool_call_count`: command output from `sqlite3 -readonly /home/rpw/.hermes/state.db ".schema"` showed `messages.id INTEGER PRIMARY KEY AUTOINCREMENT`, `messages.active`, `messages.compacted`, FTS triggers, and `sessions.message_count` columns.
   - Hermes’s own insert path updates counters explicitly in `append_message`: `/home/rpw/.hermes/hermes-agent/hermes_state.py:2709-2815`. Raw SQL inserts would not.
   - Hermes branch visibility depends on `_branched_from` in `sessions.model_config`, not a message marker: `/home/rpw/.hermes/hermes-agent/hermes_state.py:41-59`; TUI branch creation writes `model_config={"_branched_from": old_key}` and `parent_session_id=old_key`: `/home/rpw/.hermes/hermes-agent/tui_gateway/server.py:7645-7657`.
   - ACP resume will ignore non-`acp` rows: `/home/rpw/.hermes/hermes-agent/acp_adapter/session.py:488-490`.
   - The proposed `<olympus .../>` system marker will be loaded into provider conversation history by `get_messages_as_conversation` (`role` is copied through): `/home/rpw/.hermes/hermes-agent/hermes_state.py:3303-3339`, but ACP UI replay ignores `system` rows because `_replay_session_history` only handles `user`, `assistant`, and `tool`: `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:1049-1085`.
   Impact: forks may be invisible in Hermes lists, have wrong counts, fail ACP resume, pollute model context with a mid-thread system message, or corrupt the user’s live production session DB.
   Suggested fix: do not raw-write the live DB in MVP. Add or upstream a Hermes-owned `SessionDB.create_fork(...)` helper that performs the full transaction with Hermes invariants (`source='acp'`, `_branched_from` where appropriate, counters, cwd/model_config, active flags, FTS, lineage). First prove it on a copied DB and only then allow live writes with backup/rollback.

3) Claim: `SELECT * FROM messages WHERE id > last_seen_id` is sufficient because Hermes state.db is append-only.
   Evidence it is wrong:
   - The schema has mutable message-state columns: `observed`, `active`, `compacted`: `/home/rpw/.hermes/hermes-agent/hermes_state.py:640-660`.
   - Hermes has in-place compaction that updates old rows to `active=0, compacted=1` and inserts compacted live rows: `/home/rpw/.hermes/hermes-agent/hermes_state.py:2928-2978`.
   - Hermes rewind/undo soft-deletes old rows with `UPDATE messages SET active = 0`: `/home/rpw/.hermes/hermes-agent/hermes_state.py:3433-3505`.
   - Hermes has destructive transcript rewrite (`replace_messages`) that deletes all rows for a session and reinserts: `/home/rpw/.hermes/hermes-agent/hermes_state.py:2898-2927`. ACP persistence calls it after turns and model changes: `/home/rpw/.hermes/hermes-agent/acp_adapter/session.py:423-468`.
   - The live DB already has non-contiguous message IDs: `COUNT(messages)=108584` but `sqlite_sequence.messages=119538`; `id > last_seen` catches future inserts, not deletes/updates.
   Impact: Olympus will miss rewinds, compactions, deletes, counter/title changes, and some ACP rewrites. Its event log will diverge from Hermes, and search/UI will show stale or duplicate messages.
   Suggested fix: treat state.db as a mutable source, not an append-only log. Tail new IDs for latency, but add reconciliation: per-session row checksums/counts, periodic changed-session scans, explicit handling of `active/compacted`, and deletion/rewrite detection. Better: ask Hermes to expose an append-only changefeed table for session mutations.

4) Claim: The MVP can be “single-node; no envoy” while still implementing ADR 0002.
   Evidence it is wrong:
   - ADR 0002 makes the boundary absolute: any host effect goes to the envoy; the control plane must not spawn agents or touch the filesystem “even when co-located”: `/home/rpw/olympus/docs/adrs/0002-olympus-fleet-control-plane.md:167-175`.
   - ADR 0002 says all agents are Layer-3 host processes and no agent loop runs inside the control plane: `/home/rpw/olympus/docs/adrs/0002-olympus-fleet-control-plane.md:184-191`.
   - The MVP plan says “no envoy” and puts the Hermes bridge inside `crates/control-plane/src/bridge/*`: `/home/rpw/olympus/docs/plans/2026-06-28-olympus-mvp.md:9-12` and `/home/rpw/olympus/docs/plans/2026-06-28-olympus-mvp.md:397-407`.
   Impact: one engineer will build a Rust monolith that spawns Hermes; another following ADR 0002 will build an envoy boundary. They will not converge.
   Suggested fix: either include a minimal local envoy in MVP, or explicitly amend ADR 0002 to say “single-node MVP collapses L1+L2 into one process but keeps an internal envoy module boundary.” The cleaner fix is a local UDS envoy from day one.

5) Claim: The bridge is ACP-only and gateway-free.
   Evidence it is stale/inconsistent:
   - ADR 0002 rejects Hermes gateway `/api/pty` as the bridge: `/home/rpw/olympus/docs/adrs/0002-olympus-fleet-control-plane.md:1700-1705`.
   - The MVP plan still says “Hermes bridge (gateway socket or CLI subprocess)” and “spike the gateway protocol”: `/home/rpw/olympus/docs/plans/2026-06-28-olympus-mvp.md:9-12`, `/home/rpw/olympus/docs/plans/2026-06-28-olympus-mvp.md:672-679`.
   Impact: the riskiest integration point is described two incompatible ways. A gateway implementation would violate the ADR and likely reproduce the unstructured PTY problem the ADR rejected.
   Suggested fix: delete every gateway-bridge reference from the MVP plan. The only accepted drive lane should be ACP over stdio, with the actual method mapping from blocker 1.

6) Claim: MVP import acceptance can hard-code “1,626/1,629 sessions and 108,169 messages.”
   Evidence it is wrong now:
   - BRD uses both 1,626 and 1,629: `/home/rpw/olympus/docs/brd/0001-olympus-mvp.md:11-14`, `/home/rpw/olympus/docs/brd/0001-olympus-mvp.md:48-55`, `/home/rpw/olympus/docs/brd/0001-olympus-mvp.md:88-91`.
   - PRD acceptance hard-codes 1,626 and 108,169: `/home/rpw/olympus/docs/prd/0001-olympus-mvp.md:254-258`.
   - The live DB read-only query returned `sessions_count=1633`, `messages_count=108584`, source distribution `cli=1042, telegram=285, cron=133, subagent=72, api_server=70, webui=22, discord=9`.
   Impact: acceptance tests will fail or, worse, someone will filter/drop real sessions to match stale docs.
   Suggested fix: define acceptance as “counts equal the read-only snapshot taken at import start,” not fixed constants in prose. Store the snapshot in the import report.

7) Claim: MVP can defer auth/profile handling while exposing a browser UI over WSS/REST to the complete Hermes archive.
   Evidence it is unsafe/underspecified:
   - ADR 0002 says browser WSS “carries operator auth”: `/home/rpw/olympus/docs/adrs/0002-olympus-fleet-control-plane.md:224-226`.
   - PRD F8 explicitly says “No profile/auth management in MVP”: `/home/rpw/olympus/docs/prd/0001-olympus-mvp.md:165-175`.
   - The MVP exposes read/write endpoints including `POST /api/sessions/:id/messages`: `/home/rpw/olympus/docs/plans/2026-06-28-olympus-mvp.md:347-352`.
   Impact: any local web origin/process that can reach the port can read all session history and drive Hermes, including host-direct tools. This is not acceptable even for a single-user local MVP.
   Suggested fix: ship an MVP auth gate: bind localhost by default, require a random per-install token or OS-authenticated local socket for mutations, set strict CORS/origin checks, and make remote binding fail-closed.

3. Two load-bearing verifications

(a) ACP capability verification against Hermes source

| Capability | Verdict | Evidence |
|---|---|---|
| prompt | CONFIRMED | ACP method map has `session/prompt`: `/home/rpw/.hermes/hermes-agent/venv/lib/python3.11/site-packages/acp/meta.py:11-13`. Hermes implements `prompt(...)`: `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:1292-1304`. |
| structured streaming | CONFIRMED | ACP client `session_update` is the notification channel: `/home/rpw/.hermes/hermes-agent/venv/lib/python3.11/site-packages/acp/interfaces.py:85-101`. Hermes sends message/tool/thought updates through callbacks: `/home/rpw/.hermes/hermes-agent/acp_adapter/events.py:87-100`, `/home/rpw/.hermes/hermes-agent/acp_adapter/events.py:114-180`, `/home/rpw/.hermes/hermes-agent/acp_adapter/events.py:266-279`; server wires those callbacks before running the agent: `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:1398-1415`. |
| cancel | CONFIRMED | ACP method map has `session/cancel`: `/home/rpw/.hermes/hermes-agent/venv/lib/python3.11/site-packages/acp/meta.py:3-7`. Hermes `cancel` sets `cancel_event` and calls `agent.interrupt()`: `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:1211-1223`. |
| model switch | CONFIRMED | ACP method map has `session/set_model`: `/home/rpw/.hermes/hermes-agent/venv/lib/python3.11/site-packages/acp/meta.py:14-16`. Hermes implements `set_session_model`: `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:1987-2021`. Slash `/model` also exists: `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:1760-1779`. |
| resume-by-id | REFUTED AS STATED | ACP has `session/resume`, and Hermes implements it: `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:1176-1209`. But `hermes acp` has no `--resume` CLI flag: `/home/rpw/.hermes/hermes-agent/hermes_cli/subcommands/acp.py:16-52` and `hermes acp --help` output. More importantly, ACP restore ignores rows whose `source` is not `acp`: `/home/rpw/.hermes/hermes-agent/acp_adapter/session.py:488-490`. It is confirmed only for ACP-owned/correctly-crafted ACP rows, not arbitrary external sessions. |
| mid-turn steer | REFUTED AS STATED | There is no ACP `steer` method in the generated method map: `/home/rpw/.hermes/hermes-agent/venv/lib/python3.11/site-packages/acp/meta.py:3-17`. Hermes supports `/steer` as a slash command inside `session/prompt`; `_cmd_steer` calls `state.agent.steer(...)` only when a turn is running: `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:1956-1973`. So the capability exists only as “send a concurrent `session/prompt` whose text is `/steer ...`,” not as the documented ACP method. |
| slash command | REFUTED AS STATED | There is no generic ACP `slash` method. Hermes advertises slash commands through `available_commands_update`: `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:1692-1704`, and handles them by intercepting prompt text starting with `/`: `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:1352-1363`, `/home/rpw/.hermes/hermes-agent/acp_adapter/server.py:1721-1741`. |

Decisive summary: ACP is usable, but the docs describe the wrong API. The MVP bridge is viable only if implemented against the actual ACP protocol, not the invented uniform method names.

(b) state.db foreign-write safety verification against schema/source

Verdict: REFUTED AS STATED.

Evidence:
- The live DB is WAL mode (`PRAGMA journal_mode` returned `wal`), and `messages.id` is `INTEGER PRIMARY KEY AUTOINCREMENT`; `sqlite_sequence.messages=119538` while `COUNT(messages)=108584`, so IDs are monotonic but not contiguous.
- WAL gives concurrent readers plus one writer, not magic multi-writer safety. Hermes’s own write helper uses `BEGIN IMMEDIATE`, a short timeout, and application-level jitter retry: `/home/rpw/.hermes/hermes-agent/hermes_state.py:1008-1058`.
- Hermes enables `PRAGMA foreign_keys=ON` and runs schema reconciliation/FTS setup only through `SessionDB`: `/home/rpw/.hermes/hermes-agent/hermes_state.py:770-819`, `/home/rpw/.hermes/hermes-agent/hermes_state.py:1193-1422`.
- FTS triggers exist and will index raw message inserts, but session counters are updated only in Hermes methods, not triggers: `/home/rpw/.hermes/hermes-agent/hermes_state.py:2709-2815`.
- `active`/`compacted` are part of live semantics. Search excludes rewound rows but includes compacted archived rows by default: `/home/rpw/.hermes/hermes-agent/hermes_state.py:3691-3728`, `/home/rpw/.hermes/hermes-agent/hermes_state.py:3760-3767`.

Reasoning:
- A foreign writer can probably make a resumable ACP fork only if it exactly mimics Hermes internals: `source='acp'`, valid `model_config` including `cwd`, correct counters, active rows, branch markers, timestamps, FK-valid parent, and a transaction/retry strategy compatible with Hermes.
- The docs do not specify those invariants.
- Writing a system marker as a message is especially suspect: ACP history replay ignores it, while the model may still receive it.
- Therefore the claim “WAL-safe concurrent write” is too broad. The safe claim is only “SQLite can serialize a carefully written transaction if the writer follows Hermes SessionDB invariants and handles busy retries.” That is not what the MVP docs currently say.

4. CONTRADICTIONS & STALE TEXT

1) ADR 0003 still says Sayiir adopt-vs-build is pending, while ADR 0002 says Sayiir was adopted after source review.
   - Pending/open: `/home/rpw/olympus/docs/adrs/0003-remove-convex-rust-native-substrate.md:66-70`, `/home/rpw/olympus/docs/adrs/0003-remove-convex-rust-native-substrate.md:108-110`.
   - Adopted: `/home/rpw/olympus/docs/adrs/0002-olympus-fleet-control-plane.md:1311-1318`.
   - Stale rejected-alt phrasing also says “Sayiir (MIT) or in-house”: `/home/rpw/olympus/docs/adrs/0002-olympus-fleet-control-plane.md:1726-1728`.

2) The architecture map has the wrong AgentRuntime interface.
   - It says `startRun/abortRun/listTools/callTool`: `/home/rpw/olympus/docs/architecture/architecture.md:151-153`.
   - ADR 0002 defines `start/send/events/stop` and a uniform command queue: `/home/rpw/olympus/docs/adrs/0002-olympus-fleet-control-plane.md:1605-1624`.

3) The MVP plan still references “gateway socket” and “gateway protocol” even though ADR 0002 rejected gateway bridging.
   - Stale plan text: `/home/rpw/olympus/docs/plans/2026-06-28-olympus-mvp.md:9-12`, `/home/rpw/olympus/docs/plans/2026-06-28-olympus-mvp.md:672-679`.
   - Rejection: `/home/rpw/olympus/docs/adrs/0002-olympus-fleet-control-plane.md:1700-1705`.

4) “No envoy in MVP” contradicts the ADR’s hard L1/L2 boundary.
   - Plan: `/home/rpw/olympus/docs/plans/2026-06-28-olympus-mvp.md:9-12`.
   - ADR hard boundary: `/home/rpw/olympus/docs/adrs/0002-olympus-fleet-control-plane.md:167-175`.

5) PRD Flow B describes an in-place continuation UX after F4 says resume=fork.
   - F4 says non-Olympus continuation forks and source rows are untouched: `/home/rpw/olympus/docs/prd/0001-olympus-mvp.md:77-96`.
   - Flow B says type a message in the Telegram session and “source stays telegram”: `/home/rpw/olympus/docs/prd/0001-olympus-mvp.md:198-206`.
   - Fix the UX language: opening a Telegram session is read-only; “Continue in Olympus” creates/open a fork whose displayed origin is “forked from telegram,” not “source stays telegram” as if it were the same session.

6) Counts disagree across docs and live data.
   - BRD says 1,626 and 108,169 in problem statement, but its table says 1,629: `/home/rpw/olympus/docs/brd/0001-olympus-mvp.md:11-14`, `/home/rpw/olympus/docs/brd/0001-olympus-mvp.md:48-55`.
   - PRD says 1,626/108,169: `/home/rpw/olympus/docs/prd/0001-olympus-mvp.md:105-119`, `/home/rpw/olympus/docs/prd/0001-olympus-mvp.md:254-258`.
   - Plan says 1,626 in one place, 1,629 in another: `/home/rpw/olympus/docs/plans/2026-06-28-olympus-mvp.md:22-29`, `/home/rpw/olympus/docs/plans/2026-06-28-olympus-mvp.md:492-493`.
   - Live DB: 1,633 sessions and 108,584 messages.

7) BRD timeline points to “ADR 0002 §23 phases 1–5” as MVP scope, but ADR §23 phase 5 only reaches reactive WSS/cards and does not include import/fork/search/UI ship.
   - BRD: `/home/rpw/olympus/docs/brd/0001-olympus-mvp.md:108-112`.
   - ADR build order: `/home/rpw/olympus/docs/adrs/0002-olympus-fleet-control-plane.md:1789-1837`.
   - Plan uses 9 MVP phases: `/home/rpw/olympus/docs/plans/2026-06-28-olympus-mvp.md:664-679`.

8) The docs repeatedly say ACP `steer` is an ACP method “already in server.py”; source refutes that. It is a slash command implemented inside prompt handling.
   - ADR claim: `/home/rpw/olympus/docs/adrs/0002-olympus-fleet-control-plane.md:1632-1646`.
   - BRD risk: `/home/rpw/olympus/docs/brd/0001-olympus-mvp.md:97-105`.
   - PRD claim: `/home/rpw/olympus/docs/prd/0001-olympus-mvp.md:134-146`.
   - Plan claim: `/home/rpw/olympus/docs/plans/2026-06-28-olympus-mvp.md:436-447`.

5. SCOPE & FEASIBILITY

The MVP is buildable only after reducing/reshaping the critical path. As written, the 24-task plan is not minimal because it delays the two existential proofs until after substrate/UI work:

- ACP semantics should be Phase 0, not Phase 4. If `steer`/resume/fork semantics fail, the whole product shape changes.
- Fork safety should be Phase 0, not Phase 5. Writing to the live Hermes DB is the highest data-loss risk in the plan.
- The reactive-view + WSS layer is under-specified. The plan defines a broadcast enum, but not subscription lifetimes, replay after disconnect, backpressure, dropped-client behavior, auth, pagination consistency, or “snapshot + delta” ordering.
- State sync is underspecified because Hermes state.db is mutable. A pure high-water poll is not enough.
- The plan collapses the envoy boundary without acknowledging the architectural exception.
- Search is more than “index every message.” Hermes search semantics include active/compacted handling, CJK trigram fallback, source filters, and context windows; Olympus can simplify, but then parity claims must be narrowed.
- The UI scope is not small: virtualized list, chat renderer, markdown, tool calls, reasoning, streaming smoothing, model selector, settings, search, fork graph. That is not a five-task afterthought.
- “Systemd service + Tauri wrapper” is not ship polish if auth/profile/DB path are unresolved.

Better build order:
1. Read-only state.db inspector/importer with live DB counts and mutation semantics documented.
2. ACP wire spike against a throwaway session: new/resume/prompt/stream/cancel/model/`/steer`.
3. Fork spike on a copied state.db using a Hermes-owned helper or a prototype that proves ACP resume and no invariant drift.
4. Minimal local envoy or explicit internal envoy boundary.
5. Only then build redb log, views, WSS, UI, search.

6. SECURITY & GAPS

- UI auth is missing. A local unauthenticated WSS/REST server that can read all history and send prompts is a privilege-escalation surface.
- `state.db` contains secrets, system prompts, tool outputs, file paths, credentials accidentally pasted into chats, and corporate context. Importing it to redb + tantivy duplicates the blast radius. The docs need storage location, permissions, encryption-at-rest decision, backup policy, and deletion policy.
- Foreign writes to `state.db` are a production-data corruption risk. MVP must never develop/test fork writes against the live DB first.
- `<olympus .../>` marker is forgeable and model-visible. It cannot be authoritative lineage. Store lineage in Olympus’s event log and, if Hermes needs a marker, use a structured metadata field (`model_config`) that is not sent to the model.
- ACP child processes run with the user’s privileges in HostDirect mode. That is fine for trusted local MVP only if the UI is authenticated and bound locally.
- Profile/HERMES_HOME selection is underspecified. Docs assume `~/.hermes/state.db`; Hermes has profiles and ACP loads env/config from `HERMES_HOME` (`/home/rpw/.hermes/hermes-agent/acp_adapter/entry.py:101-113`). Olympus must know which profile/home it is operating on and display it.
- Hermes Studio/gateway shutdown is muddled. PRD says Studio goes read-only or shuts down to avoid dual writers: `/home/rpw/olympus/docs/prd/0001-olympus-mvp.md:231-235`. But Telegram/Discord/cron/API channels may depend on Hermes gateway processes continuing to run and write state.db. Define exactly which legacy process is stopped and which remains authoritative.
- CORS/CSRF/origin rules are absent. “Localhost” is not an auth model; hostile local web pages can hit localhost unless origin/token checks exist.
- Backups and recovery are absent. Before any fork write, Olympus should create a SQLite online backup or require an operator-confirmed snapshot.

7. TOP 5 THINGS TO FIX BEFORE WRITING CODE

1. Replace the ACP section with the real source-verified contract. No invented `steer`/`slash` methods; no `hermes acp --resume`. Add a mandatory ACP spike as task 0.
2. Remove raw live-DB fork writes from the MVP until a Hermes-owned fork helper exists and is proven on a copied DB. Specify exact source/model_config/counter/lineage/message semantics.
3. Redesign state.db sync as mutable-source reconciliation, not append-only `id > last_seen` only. Handle `active`, `compacted`, deletes, rewrites, title/counter/model changes.
4. Resolve the “no envoy” contradiction. Either ship a minimal local envoy now or formally amend ADR 0002 for an MVP-only internal envoy boundary.
5. Add MVP auth/profile/storage safety: local bind + token/origin checks, explicit HERMES_HOME/profile selection, redb/tantivy permissions, and backup/restore before any state.db write.
