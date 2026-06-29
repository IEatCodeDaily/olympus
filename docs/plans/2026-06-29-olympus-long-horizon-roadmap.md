# Olympus — Long-Horizon Product & Implementation Roadmap

> **For the swarm:** This is the durable, multi-month roadmap. It is the source
> of truth for *what to build next and why*, structured so a Hermes-kanban swarm
> can execute it **without per-task hand-holding from the controller**. Each
> milestone below is a self-contained brief: goal, why, dependencies, the
> riskiest unknown (spike-first), bite-sized tasks, the gates, and an explicit
> **Done =** line. An agent should be able to claim a milestone, read only its
> brief + the linked ADR section, and ship it.
>
> **How to use this with the swarm** (see `docs/SWARM_WORKFLOW.md` +
> `subagent-driven-development` skill):
> 1. Pick the lowest-numbered milestone whose dependencies are all `DONE`.
> 2. If it has a `SPIKE (resolve first)`, do the spike, capture results in
>    `docs/reviews/`, and only then seed implementation cards.
> 3. Seed one kanban card per task (`--project olympus --workspace worktree`),
>    assignee by role (coder/reviewer/validator — see role table below).
> 4. Workers signal `blocked: review-required`; the controller verifies the
>    merged tree against the milestone's **Gates**, commits, and marks the
>    milestone `DONE` here (update the Status Ledger at the bottom).
> 5. Never start a milestone whose `Depends on` is unmet. Never build a later
>    epic speculatively (ADR 0002 §23).

**Goal:** Take Olympus from "unified read-only session browser + dedicated
chat channel" to the full ADR 0002 vision — a Rust-native, multi-node,
sync-native AI-agent fleet control plane that replaces Hermes Studio and
becomes a local-first agent-native collaboration platform.

**Architecture (unchanged anchor):** Rust single-binary control plane (redb
event log = truth → in-memory materialized views → tantivy search → axum
REST/WS) + Hermes bridge (ACP over stdio) + React/Tauri UI. Multi-node,
workflows, vaults, sandboxing are **later deployment/feature changes, not
refactors** — the envoy module seam and the AgentRuntime boundary exist from
the MVP so they don't require rewrites. Canonical spec: **ADR 0002** (24 §),
substrate decision: **ADR 0003**.

**Tech stack:** Rust (redb, tantivy, zstd, tokio, axum, serde/postcard,
rusqlite, async-trait), React + Vite + TypeScript (+ Tauri later), jj +
iroh-blobs (vaults), Sayiir or equivalent (workflows). Hermes integration is
**patch-not-fork** via `hermes-patches/`.

---

## North Star (the end state, so every milestone has a direction)

Olympus is the **operator's single pane of glass over a fleet of agents**:

- **One unified, searchable, resumable history** across every Hermes channel
  (done at MVP read-level).
- **Drive agents from Olympus** — start, steer, fork, switch model, cancel — in
  a dedicated `olympus` channel, plus observe every other channel live.
- **A board/cards model** where work is durable, assignable, and 1:1 with worker
  sessions; the swarm that builds Olympus eventually *runs on* Olympus.
- **Multi-node fleet**: agents run on many machines; Olympus coordinates them
  over a transport-native identity layer (UDS local, iroh remote).
- **Knowledge vaults**: local-first, access-gated, offline-first docs/notes the
  agents read and propose edits to.
- **Workflows**: durable, n8n-like graphs (e.g. the code-review loop) that
  outlive any single process.
- **Local-first collaboration** (the Anytype/Notion-like horizon): CRDT content
  plane, P2P sync, mobile companion.

Everything below sequences toward that, in dependency order, never speculatively.

---

## Roles (model-pinned profiles — see `docs/autonomous-loop.md`)

| Role | Profile | Use for |
|---|---|---|
| Orchestrator | controller (this agent) | seed cards, verify merged tree, commit, merge gate |
| Coder | `coding-agent` / `glm52` (glm-5.2/zai) | implementation tasks |
| Reviewer | `gpt55` / `code-reviewer` (gpt-5.5) | spec + protocol review, adversarial source-review, spikes |
| Validator | `tester` (claude-sonnet, when credits allow; else glm-5.2) | browser e2e, vision checks |

**Gates every milestone must pass before `DONE`** (the controller runs these on
the merged tree — never trust a worker's self-report):
- Rust: `cargo test --workspace` + `cargo clippy --all-targets -- -D warnings` + `cargo fmt --check`
- UI (if touched): `bun run typecheck` + `bun run build` + `bun run test:e2e`
- Or simply: **`make verify`** (runs all of the above).
- **Real browser e2e for any UI change** — never claim UI works from a build alone.
- Adversarial source-review BEFORE building any new Hermes-integration code.

---

## Epic map (the whole arc)

| Epic | Theme | ADR refs | Status |
|---|---|---|---|
| **A** | MVP completion — drive Hermes from Olympus | §10, §19, §6.6 | IN PROGRESS |
| **B** | Reactivity hardening + state.db live sync | §6.7, §10.3.1, §2.5 | TODO |
| **C** | Cards & board (work becomes durable) | §6 | TODO |
| **D** | Scheduler core + AgentRuntime second impl | §10.5/.6, §19, §23.1-2 | TODO |
| **E** | Filesystem hierarchy + workdir lifecycle + jj | §5 | TODO |
| **F** | Sandboxing + port reservation | §12 | TODO |
| **G** | Observability admin surface | §10B | TODO |
| **H** | Workflows (Sayiir / code-review loop) | §15 | TODO |
| **I** | Budget + subscription-aware routing | §16 | TODO |
| **J** | Artifacts + content-addressed blob store | §17, §10A.3 | TODO |
| **K** | Knowledge vaults | §8 | TODO |
| **L** | Multi-node (iroh transport + SSH bridge) | §2.5, §10.7, §18 | TODO |
| **M** | Secret management (scoped broker) | (post-MVP ask) | TODO |
| **N** | Identity/context/project isolation surfaced | §3 | TODO |
| **O** | Semantic search + chat rooms + recovery | §10A.4, §14, §20 | TODO |
| **P** | Local-first content plane + mobile (north star) | §2 (future) | RESEARCH |

Dependency spine: **A → B → C → D** is the critical path (everything real
depends on the scheduler core landing in D). E/F/G can run in parallel after C.
H–O each declare their own deps. P is research-gated and last.

---

# EPIC A — MVP completion: drive Hermes from Olympus

**Why:** The MVP today is read + a dedicated `olympus` channel *foundation*
(the `HERMES_ACP_SESSION_SOURCE` patch is in). What's missing is the bridge that
actually drives `hermes acp`, so "New Chat", "send", "fork-to-continue", and
streaming responses work. This is the last mile of the original MVP acceptance
criterion: **"operator can close Hermes Studio and work from Olympus."**

## Milestone A1 — ACP bridge (drive `hermes acp` over stdio)

**Depends on:** patch `001-acp-configurable-source` (DONE), ACP wire spike
(`docs/reviews/acp-wire-spike.md`, DONE).

**SPIKE (resolve first — already done, re-read before building):** the real ACP
method contract is in `docs/reviews/acp-wire-spike.md` + ADR 0002 §ACP. Methods
that exist: `session/new`, `session/prompt`, `session/set_model`,
`session/cancel` (notification), `session/resume` (acp-source rows only),
`session/update` (streaming). `steer`/`slash` are **prompt text**, not methods.
Build against these only — no invented method names.

**Files:**
- Create `crates/control-plane/src/bridge/mod.rs` — `AgentRuntime` trait +
  `AgentCommand` / `AgentEvent` enums (copy interface from MVP plan §4.1).
- Create `crates/control-plane/src/bridge/acp.rs` — Content-Length-framed
  JSON-RPC stdio client: request/response correlation by id, notification
  handling, `session/update` → `AgentEvent` mapping.
- Create `crates/control-plane/src/bridge/hermes.rs` — `HermesAgentRuntime`:
  spawns `hermes acp` as a child **with `HERMES_ACP_SESSION_SOURCE=olympus`** so
  every session it creates lands in the dedicated channel.

**Tasks (each TDD, hermetic unit tests — no live `hermes acp` in default suite):**
1. `bridge/mod.rs`: the trait + enums + command→method mapping table.
2. `bridge/acp.rs`: frame encode/decode round-trips a JSON-RPC message (test).
3. `bridge/acp.rs`: `AgentCommand::Steer` serializes to `session/prompt` with
   text `"/steer …"`; `Cancel` emits a notification (no id) — tests.
4. `bridge/acp.rs`: a `session/update` notification deserializes into the right
   `AgentEvent` variant — test.
5. `bridge/hermes.rs`: `start()` spawns the child with the olympus source env;
   integration test that actually spawns `hermes acp`, prompts "say PONG",
   asserts a streamed PONG — gate behind `#[ignore]` so default `cargo test`
   stays hermetic; the controller runs it manually as a milestone gate.

**Gates:** `make verify-rust`; plus the controller manually runs the `#[ignore]`
live PONG test once and pastes output.

**Done =** `cargo test bridge` green; the live PONG integration test passes when
run manually; a spawned session appears in `state.db` with `source='olympus'`.

## Milestone A2 — New Chat + send + streaming (wire the bridge to the API/UI)

**Depends on:** A1.

**Files:**
- `server/mod.rs`: add `POST /api/sessions` → bridge `session/new` (returns the
  new `source=olympus` Session DTO); change `post_message` from the 503 stub to
  call the bridge `session/prompt` for managed sessions.
- `server/ws.rs`: forward bridge `AgentEvent`s as `message.delta` / `message.done`
  / `message.appended` frames on the existing `/ws` channel.
- `ui/src/api.ts`: `createSession()`; `ui` New Chat button; composer already
  exists — flip it to live for `managed` sessions.
- `ui/src/lib/format.ts`: `SOURCE_META.olympus` (channel pill + distinct color).

**Tasks:** (1) `POST /api/sessions` route + test (201 + olympus DTO). (2) wire
`post_message` → bridge, return 202. (3) stream `session/update` → `/ws` deltas.
(4) UI New Chat button + `createSession()`. (5) UI `olympus` channel pill/color.
(6) composer live-send for managed sessions.

**Gates:** `make verify`; **real browser e2e**: create a new Olympus chat, send
"say PONG", see the streamed response, confirm the row shows the `olympus`
channel pill and `source='olympus'` in the DB.

**Done =** operator can start a brand-new chat in Olympus, send a message, and
watch the agent stream a reply — all in the dedicated `olympus` channel.

## Milestone A3 — Fork-to-continue (cross-channel continuation)

**Depends on:** A2; **SPIKE (resolve first):** fork recipe is proven on a copy
(`docs/reviews/fork-spike.md`). Needs a Hermes patch `002-sessiondb-create-fork`
(see `hermes-patches/` planned entry) — implement via patchctl, prove on a
**copied** state.db first, back up before any live touch.

**Files:** `hermes-patches/patches/002-sessiondb-create-fork.patch`;
`server/mod.rs` `POST /api/sessions/:id/fork`; `ui` Fork button (already stubbed
in the observed-session composer).

**Tasks:** (1) write + prove the create_fork patch on a copy (13 invariants from
fork-spike). (2) `POST /fork` route → create_fork → returns new managed session.
(3) UI Fork button → opens the fork. (4) verify the source session is untouched.

**Gates:** `make verify`; patchctl status APPLIED + reverse-checks clean; browser
e2e: fork a Telegram session, continue it in Olympus, original unchanged.

**Done =** any observed session (telegram/cli/…) can be forked into an
`olympus`-managed session and continued from Olympus, with the original intact.

**EPIC A DONE =** A1+A2+A3. **This is the real MVP ship** — the operator can
close Hermes Studio and do daily work from Olympus. Tag `v0.1-mvp`.

---

# EPIC B — Reactivity hardening + state.db live sync

**Why:** Today import is a one-time boot snapshot. To truly replace Studio,
Olympus must reflect Hermes activity *live* — new messages on any channel,
compaction, rewind/undo — without a restart.

## Milestone B1 — state.db mutable-source reconciliation (ADR §6.7)

**Depends on:** Epic A. **SPIKE (resolve first):** confirm the mutation surface
— `messages.id` is non-contiguous; Hermes mutates rows via compaction
(`active=0, compacted=1`), rewind (`active=0`), and `replace_messages`
(delete+reinsert). A pure `id > last_seen` tail diverges. Capture the exact
mutation cases in `docs/reviews/statedb-sync-spike.md`.

**Files:** `crates/control-plane/src/sync.rs`.

**Tasks:** (1) fast tail: read-only WAL `SELECT … WHERE id > ?last_seen` poll
(~1–2s) → `MessageAppended`, honoring `active`/`compacted`. (2) reconciliation
sweep (30–60s + on session open): per-session signature `(max(id), row_count,
checksum)` vs the view; on mismatch re-read + reconcile deletes/rewinds/
compaction/title/counter/model changes via `SessionUpdated`. (3) session-meta
reconciliation (message_count/title/model/archived are authoritative in
`sessions`). (4) candidate-patch note: if the sweep is too chatty, record a
Hermes changefeed-table patch as the clean fix.

**Gates:** `make verify-rust`; live test — `hermes -z "test"` in another terminal
appears in Olympus within the poll interval; a rewind/compaction shows the
tombstone, not a stale duplicate.

**Done =** Olympus reflects external Hermes activity live across insert, update,
and delete, with no restart and no divergence.

## Milestone B2 — Reactive view delta correctness under load

**Depends on:** B1. **Tasks:** broadcast backpressure (lagged subscribers
reconcile via REST, already stubbed); delta coalescing for high-rate sessions;
a soak test (100+ msgs/min across N sessions) proving no dropped/duplicated
view rows. **Done =** the `/ws` delta stream is correct + bounded under a soak.

---

# EPIC C — Cards & the board (work becomes durable)

**Why:** ADR §6. Turns Olympus from a chat UI into a *work* control plane:
durable tasks, 1:1 with worker sessions, assignable, reattemptable. This is the
foundation the scheduler (D) and workflows (H) build on, and eventually the
swarm that builds Olympus migrates onto Olympus's own board.

## Milestone C1 — Card data model + event types

**Depends on:** Epic A (event log + views patterns are established).
**Tasks:** `Event::Card*` variants (created/assigned/claimed/blocked/completed/
reassigned); a `CardView` projection (cards-by-status); REST CRUD; per-attempt
bookmarks + "previous attempt" block semantics. **Done =** cards persist in the
event log, project into a board view, and survive restart.

## Milestone C2 — Board UI (cards-by-status, live)

**Depends on:** C1, B (reactive deltas). **Tasks:** a board view in the React UI
subscribed to `CardView` deltas; create/assign/move cards; 1:1 link from a card
to its worker session (open the session from the card). **Done =** operator
manages a live board in Olympus; moving a card updates instantly.

**EPIC C DONE =** Olympus has a working durable board. Tag `v0.2-board`.

---

# EPIC D — Scheduler core + AgentRuntime second impl (the correctness spine)

**Why:** ADR §23.1-2 calls this "the correctness core; everything builds on it."
Single-writer scheduler with slot accounting + claim fencing + a reap sweeper is
what makes multi-agent, multi-node coordination correct without distributed ACID.

## Milestone D1 — Scheduler: assign/claim/renew/complete + reap

**Depends on:** C1. **SPIKE (resolve first):** validate the `availableSlots`
accounting + `claimEpoch` fencing model (§10.5) and the `reap` sweeper (§10.6)
against the echo end-to-end test in §23.1 before building. **Tasks:** scheduler
mutations with slot accounting + epoch fencing; reap sweeper; group-commit
durability; `echo` end-to-end (enqueue→assign→claim→stream→complete→slot
released); the two §23.1 correctness tests (60s stream cancelled at 30s;
kill-the-node proves reap requeues + releases the slot exactly once).
**Done =** the echo round-trip + both correctness tests pass.

## Milestone D2 — AgentRuntime second impl (prove the seam)

**Depends on:** D1, A1 (`HermesAgentRuntime` exists). **Tasks:** add
`ClaudeCodeRuntime` (shell-out) behind the same `AgentRuntime` trait (§19); prove
a card can run on either runtime via the same scheduler path. **Done =** the same
card runs through two different runtimes unchanged — the seam is proven.

**EPIC D DONE =** correctness spine landed. Tag `v0.3-scheduler`.

---

# EPIC E — Filesystem hierarchy + workdir lifecycle + jj  (ADR §5)

**Depends on:** D. **Tasks:** the org/context/project/session dir hierarchy;
task-based workdirs; jj colocate with the conflict guard; workdir lifecycle
(create/claim/cleanup). **Done =** each worker card gets an isolated jj-colocated
workdir; conflicts surface, never silently merge.

# EPIC F — Sandboxing + port reservation  (ADR §12)

**Depends on:** E. **Tasks:** `HostDirect` → Bubblewrap backend; port
reservation via netns; the reserved-range model from `docs/ports.md`. **Done =**
a worker runs sandboxed, binds only its reserved ports, cannot touch owner
deploys.

# EPIC G — Observability admin surface  (ADR §10B)

**Depends on:** Epic A (tracing spans already mandated from MVP). **Tasks:**
the separate server-rendered admin surface on `:8788` (traces/logs/metrics/
event-tail); ring buffer; OTLP export deferred. **Done =** operator can watch
live traces + the event tail in the admin UI without touching the main React app.

# EPIC H — Workflows (Sayiir / code-review loop)  (ADR §15, ADR 0003)

**Depends on:** D (scheduler), C (cards). **SPIKE:** Sayiir redb adapter +
snapshot strip/hydrate (already source-reviewed). **Tasks:** adopt Sayiir
(redb `PersistentBackend`); the code-review-loop template with idempotent
`agent-run`; an audit hook via tracing. **Done =** the code-review loop runs as a
durable workflow that survives a process restart mid-run.

# EPIC I — Budget + subscription-aware routing  (ADR §16, §16.5/.6)

**Depends on:** D. **Tasks:** budget/subscription tracking folded into the
scheduler assign step; the fallback chain (§16.5) + subscription limit mgmt
(§16.6) already specced. **Done =** the scheduler routes by remaining budget +
subscription limits; a model at its limit rolls to the configured fallback.

# EPIC J — Artifacts + content-addressed blob store  (ADR §17, §10A.3)

**Depends on:** D. **Tasks:** content-addressed blob store; artifact lifecycle;
serving; text extraction for search. **Done =** agents produce/consume artifacts
stored content-addressed, served to the UI, indexed for search.

# EPIC K — Knowledge vaults  (ADR §8)

**Depends on:** E (jj), J (blobs). **Tasks:** vault as a separate top-level
resource (`vaults/<id>/`); text = jj, binaries = blobref + content-addressed
(iroh-blobs + R2/S3 backup); `vault.db` (sqlite-vec); access tiers READ/PROPOSE/
WRITE/ADMIN; assignedId+assignedKind (human-in-the-loop). **Done =** an agent
reads a vault and proposes an edit that a human approves; binaries sync via
blobref.

# EPIC L — Multi-node (iroh transport + SSH bridge)  (ADR §2.5, §10.7, §18)

**Depends on:** D (scheduler), F (sandbox). **SPIKE:** iroh transport as a second
`Transport` impl behind the trait UDS already satisfies. **Tasks:** iroh
transport (NodeId allowlist + per-context grants); node register/heartbeat over
iroh; SSH/terminal bridge to each node. **Done =** a card dispatched from one
node runs on a second machine and streams output back; the operator SSHes to a
node from Olympus.

# EPIC M — Secret management (scoped broker)  (post-MVP ask)

**Depends on:** D, F. **Tasks:** scoped broker (Bitwarden/1Password handler);
zero-exposure injection — agents get scoped access, never raw keys; per-context/
session scoping. **Done =** an agent uses a secret for one task without the raw
value ever entering its context or logs.

# EPIC N — Identity/context/project isolation surfaced  (ADR §3)

**Depends on:** threaded through from MVP at the data level (orgId/ownerId
everywhere). **Tasks:** surface contexts/projects in the UI; the
`can(user,action,resource)` seam flips from stub to real RBAC. **Done =** the
operator manages multiple contexts/projects; authz is enforced, not stubbed.

# EPIC O — Semantic search + chat rooms + recovery  (ADR §10A.4, §14, §20)

**Depends on:** J (artifacts), L (multi-node). **Tasks:** vector/semantic search
(sqlite-vec or equivalent); `reapOrphanedMainSessions` orchestrator recovery
(§20); chat rooms (§14) + multi-node fallback comms (§13.6). **Done =** semantic
search returns relevant cross-session results; an orphaned orchestrator recovers;
two nodes bridge a capability over a chat room. **Build last — only when a real
cross-node case exists.**

# EPIC P — Local-first content plane + mobile (north star)  (RESEARCH)

**Depends on:** everything. **Gate:** research-only until a concrete requirement
forces it. **Theme:** CRDT content plane (iroh-docs); P2P sync of docs/notes/
messages; mobile companion. **Done =** N/A yet — keep as a research ADR until the
coordination plane (A–O) is solid.

---

## Cross-cutting standing rules (so agents don't re-derive them)

1. **Patch Hermes, never fork** — every Hermes change is a `hermes-patches/`
   entry, proven on a **copied** state.db first, registered in `manifest.toml`.
2. **The event log is the only source of truth**; views are pure projections.
   Never mutate view state outside an `apply(event)` path.
3. **`state.db` is read-only** to Olympus except via proven Hermes patches.
   Cross-channel continuation is a FORK, never an in-place edit.
4. **Auth gate on all `/api/*` + `/ws`**; bind 127.0.0.1 by default; remote bind
   opt-in + fail-closed.
5. **The DTO layer (`server/dto.rs`) is the only place view rows become wire
   JSON** (camelCase). Contract changes update `docs/api-contract.md` + both sides.
6. **Build in dependency order; never build a later epic speculatively** (§23).
7. **Adversarial source-review before any new Hermes-integration code** — it has
   already caught refuted protocol claims twice.
8. **Every milestone updates the Status Ledger below** when it lands.

---

## Status Ledger (the swarm updates this; it is the live state of the roadmap)

| Milestone | Status | Commit / tag | Notes |
|---|---|---|---|
| (pre-A) MVP read + channel foundation | DONE | `d0ed69c` | import, views, search, REST/WS, UI, auth, CORS, configurable acp source |
| A1 ACP bridge | TODO | | next up; spike already done |
| A2 New Chat + send + streaming | TODO | | depends A1 |
| A3 Fork-to-continue | TODO | | depends A2 + patch 002 |
| B1 state.db live sync | TODO | | |
| B2 delta correctness under load | TODO | | |
| C1 card model | TODO | | |
| C2 board UI | TODO | | |
| D1 scheduler core | TODO | | correctness spine |
| D2 AgentRuntime 2nd impl | TODO | | |
| E filesystem + jj | TODO | | |
| F sandboxing | TODO | | |
| G observability admin | TODO | | |
| H workflows | TODO | | |
| I budget/routing | TODO | | |
| J artifacts | TODO | | |
| K vaults | TODO | | |
| L multi-node | TODO | | |
| M secrets | TODO | | |
| N isolation surfaced | TODO | | |
| O semantic/rooms/recovery | TODO | | |
| P local-first (research) | RESEARCH | | gated |

---

## Execution handoff

Pick the lowest-numbered `TODO` milestone whose `Depends on` is `DONE`
(currently **A1**). Resolve its spike if any, seed one kanban card per task on
the `olympus-mvp` board (`--project olympus --workspace worktree`), dispatch
≤3 workers (host RAM ceiling), verify the merged tree against the milestone
**Gates**, commit, and update the Status Ledger. Repeat. Escalate to the
operator only for: a failed spike that invalidates a milestone, a destructive
Hermes change that can't be proven on a copy, or a genuine architecture fork in
the road.
