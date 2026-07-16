# Olympus Robustness Plan — Correctness Spine First, Then Features

Date: 2026-07-14
Owner: Terminus
Status: Draft (provisional transport decision confirmed by user: keep WebSocket;
WebRTC parked as future envoy→client transport)

## 0. Framing

The user reported flaky sessions: sent messages whose responses vanish on
navigation and then reappear out of order, plus a Claude Code
`session/set_model` error, plus a wish to "stream full system state to the
client." They floated WASM+WebRTC.

Three independent adversarial reviews (agent/session identity, terminal, and
project/repo/context) were run read-only against HEAD `0a73a86` and all three
returned **NO-GO for building features on the current substrate**, each naming
the same missing guarantee: a server "success" today proves neither durability
nor runtime truth, and there is no ordered/fenced/reconcilable path between
Hall, Envoy, and the client.

Therefore the plan is **spine first**. No new surfaces are built on a substrate
that loses messages.

Reviews (durable):
- `/home/rpw/.hermes/workspace/reviews/olympus-agent-session-review.md`
- `/home/rpw/.hermes/workspace/reviews/olympus-terminal-review.md`
- `/home/rpw/.hermes/workspace/reviews/olympus-project-repo-context-review.md`

## 1. Decisions locked

- Keep WebSocket. Fix correctness with sequenced frames + client watermark
  reconcile over the existing event log (ADR 0020). WebRTC is a future
  envoy→client transport optimization only; it does not define correctness.
- Hall owns durable truth + policy; Envoy owns host effects; the client is a
  projection, never a second authority.
- A launch target is a structured **placement** `(org, node, agentKind, agentId)`,
  not a fleet-global agent id. Placement is validated by one Hall resolver and
  pinned immutably at creation.
- Terminal is an operator-only human action with a shared Envoy PTY primitive
  and two closed launch profiles; it is not an agent capability, not a job, not
  raw shell. It is gated behind node-key binding + sandbox prerequisites.
- Project is a copy-on-apply configuration template, not a resource owner or
  workdir. Repo is org-owned; repo/wiki/issues/PRs are four independently
  synced sources; sync host effects belong to Envoy over durable jobs.

## 2. Phases (each phase gates the next)

### Phase 0 — Correctness spine (ADR 0020 v2) + honest logs  [BLOCKS ALL FEATURES]

> Revised after gpt-5.6 adversarial review of ADR 0020
> (`/home/rpw/.hermes/workspace/reviews/olympus-adr0020-gpt56-review.md`).
> The v1 global-seq/streamEpoch/per-session-contiguous-seq envelope was
> withdrawn as unbuildable (single global AUTOINCREMENT log with retain_native
> holes; `/api/events` is global/org-unscoped/delta-less; no serialized
> append+apply seam). v2 is smaller and ordered by the EXISTING per-session
> `message_id`.

0.1 **set_model harness fix** — DONE, deployed, live-verified (postmortem 0029).
0.2 **Durable-first `MessageDone`** (PRIMARY FIX): reorder `sessions.rs:1516`
    broadcast to AFTER the durable append + view apply (`:1532-1543`). Fixes the
    refetch-misses-assistant-row race outright.
0.3 **Deliver-on-(re)subscribe**: on `Subscribe`, force a messages refetch for
    that session (drop `staleTime:Infinity` for the active session). Fixes
    navigate-away-and-back.
0.4 **Transcript ordered by existing `message_id`**; optimistic user msg re-keyed
    to server `message_id` on its `message.appended` echo — never sorts above a
    higher id.
0.5 **Optimistic reconcile by durable `clientMsgId`**: add `clientMsgId` to the
    durable `Event::MessageAppended` (NOT wire-only — the done→refetch path reads
    the DB); reducer falls back to content/id match when absent (old events).
0.6 **message_count projection fix**: increment in `SessionView::apply`
    (`views/session.rs:201`), decrement on remove; reconcile sync double-count
    (absolute-set on sync, increment on managed append, never both); replay test
    for BOTH session kinds.
0.7 **Delete** ChatPage component-local transcript truth.
0.8 **Honest Envoy operational logs**: session-scoped `session.log` frames
    (variant already exists), gated by `should_deliver` org/session filter. No
    competing permanent store; full OTel remains ADR 0018 (v1 §9's coupling to
    the OTel plane was over-reach and is dropped).
0.9 **(Prerequisite, tracked separately)** serialized append+apply critical
    section (support.rs:22-31 / sessions.rs:1532/1541 take separate locks →
    apply order can diverge from commit order). Named by the project/repo review
    §4.3 and review H3. 0.2 removes the single-turn race; general multi-writer
    correctness needs this seam. Do NOT claim multi-writer correctness before it.

**Optional (only if reconnect robustness demands it):** session-scoped,
org-checked `GET /api/messages?session=&since=<message_id>` — never reuse the
global unscoped `/api/events` for browser catch-up (review H4 cross-org leak).

Gate: ADR 0020 v2 §8 hostile tests pass (incl. concurrent-writers apply-order
test); browser evidence (send→A→B→A, refresh mid-turn); postmortems 0029, 0030.

### Phase 0.C — Operator cockpit shell (ADR 0021, frontend-only, parallelizable)

Ships alongside/after Phase 0 (pure frontend, no backend dep). Floating tabbed
operator-only window mounted ONCE at AppShell root outside the router outlet;
top-right toggle; xterm.js tabs in a top-level Zustand store; toggle=display:none
(never dispose); per-user geometry/tab manifest in localStorage; over a MOCK PTY.
Proves Layer A persistence (navigate + toggle, console stays open) with browser
evidence. No agent path. Real PTY is Phase 3.
Gate: browser evidence of persistence across navigation + toggle.


### Phase 1 — Placement identity + node-aware session creation

Depends on Phase 0. Implements the agent/session review's minimal model:
discovered placement, Hall launch policy (per-placement visibility), immutable
session placement, node-key binding + duplicate/takeover fencing, one Hall
launch resolver used by both "list options" and "create". UI: grouped
new-session modal over server facts (agent → node targets), Agents-page
visibility toggles, draft/configure/confirm flow. Restores topbar search
(real command palette) and notification bell (backed by an attention feed).
Gate: agent/session review acceptance matrix.

### Phase 2 — Projects / Repo / context

Depends on Phase 0 (ordered truth) and Phase 1 (placement + Envoy host-effect
discipline). Implements project/repo review: org-owned Repo v2 identity,
revisioned Project template (copy-on-apply), Session desired-context +
runtime-context-snapshot events, one serialized command seam, Envoy-side
clone/fetch/wiki/issues/PRs over durable jobs, Projects-area Repositories page.
Gate: project/repo review acceptance gates.

### Phase 3 — Terminal (operator-only PTY)

Depends on Phase 0 (sequenced streams), Phase 1 (node-key binding), and sandbox
prerequisites. Implements terminal review: shared Envoy PTY primitive, two
closed launch profiles, durable terminal attempt/reconnect, dedicated operator
WS, xterm.js last. Session + Fleet surfaces.
Gate: terminal review non-skippable tests.

## 3. Execution model

- Durable Kanban worktrees with explicit file ownership per worker (user
  dislikes delegate_task for implementation; delegation only for adversarial
  review, already done).
- Cargo serialized under `flock ~/.cache/olympus-cargo.lock`,
  `CARGO_TARGET_DIR=~/.cache/cargo-target/plain`, `CARGO_BUILD_JOBS=1`, `-j 2`.
- Monitoring durable/session-independent (tmux + files + cron watchdog).
- Every bug → `docs/postmortems/`. Every dev report → a visual
  (UI: screenshots/video; backend: ERD/dataflow).
- Deploy discipline: SQLite backup, migration dry-run on copied prod DB, fresh
  Envoy readiness, health + API + browser verification after restart.

## 4. Immediate next actions

1. Verify + deploy 0.1 (set_model) so Claude Code is testable now.
2. Render `docs/diagrams/client-state-projection.{html,png}` for ADR 0020.
3. Stand up the Phase 0 Kanban lane with file-ownership boundaries:
   proto/ws (envelope), server projector, client reducer, message_count,
   envoy-logs — sequenced by dependency.
