# Olympus — Business Requirements Document (MVP)

- Version: 1.0 (MVP scope)
- Date: 2026-06-28
- Status: Draft
- Owner: rpw (operator/principal)
- Related: ADR 0002 (architecture), ADR 0003 (substrate), PRD (`docs/prd/0001-olympus-mvp.md`)

## 1. Problem

The operator runs Hermes Agent across multiple channels — CLI (1,038 sessions),
Telegram (285), WebUI (22), Discord (9), cron (133), API server (70), and
subagents (69). **1,626 sessions totaling 108,169 messages**, today, across
`~/.hermes/state.db` (1.4 GB). These sessions are siloed: a conversation started
on Telegram cannot be resumed from the WebUI or CLI without manual handoff; the
WebUI session list and the Telegram gateway maintain separate views; there is no
single searchable, filterable history across all channels.

The underlying cause is architectural: Hermes's session store is append-only per
channel with no reactive sync layer, and the WebUI (Hermes Studio) is a
separate Vue/Koa app bolted on top of SQLite with bespoke Socket.IO plumbing —
state ownership is diffuse, realtime relies on hand-wired sockets, and
correctness is hard to guarantee.

Additionally, Hermes Studio accumulates cross-contamination bugs because agents
run against the whole host filesystem rather than scoped workdirs, and Hermes's
own auth/gateway/profile system is flaky under multi-provider load.

## 2. Business objective

**A single, sync-native control plane where every session from every channel
lives in one place and is resumable from anywhere.** The operator opens Olympus,
sees all 1,626 sessions (CLI, Telegram, Discord, WebUI, cron, subagents)
unified, searches across them, and resumes any one — continuing the conversation
in Olympus's interface regardless of which channel originated it.

Olympus is **not** a rewrite of Hermes's agent loop. Hermes remains the execution
engine behind an `AgentRuntime` boundary. Olympus replaces Hermes Studio as the
*interface and session-control layer*, and adds the reactive sync that Studio
never had.

## 3. MVP scope (what "1:1 parity" means here)

"1:1 feature parity with Hermes" is narrowed to the **sync-native session
interface** — not a reimplementation of Hermes's tools, skills, MCP, or memory
(those stay in Hermes). The MVP delivers:

| Requirement | What it means | Success criterion |
|---|---|---|
| **Unified session store** | Every session from every Hermes channel synced into Olympus's event log from `state.db` (WAL-mode concurrent reads); all new external sessions appear live | All 1,629 existing sessions visible in Olympus; new CLI/Telegram/Discord sessions appear live |
| **Cross-channel resume = fork** | Continuing any external session forks it into an Olympus-managed session (never in-place — avoids cross-channel divergence) | Fork a Telegram-started session into Olympus; continue it there; the Telegram session is untouched |
| **Sync-native reactivity** | Session list, messages, tool calls update live as Hermes runs — no refresh, no polling the UI (control plane tails state.db) | Send a message from CLI; see it appear in Olympus in real time |
| **Chat interface** | Full chat UI with streaming, markdown, tool-call rendering, model switching — parity with Hermes Studio's chat | Operator can drive Hermes from Olympus the same way they drive it from Studio today |
| **Session management** | List, filter, search, archive sessions; filter by source (channel), model, date; fork-lineage node graph | Search "esp32" across 108k messages and find the right session in <1s |
| **Hermes bridge (ACP)** | Olympus drives managed sessions via `hermes acp` (ACP over stdio) — prompt/steer/cancel/model/streaming; observes external sessions via state.db | A message sent from Olympus produces a real Hermes response, streamed back; steering a managed session works mid-turn |

## 4. Out of scope (MVP)

These are in ADR 0002 but explicitly **deferred past the MVP**:

- Multi-node fleet (cards, board, envoy on remote nodes, iroh transport) — MVP is
  single-node: Olympus + Hermes on one host.
- Budget/subscription tracking (§16).
- Workflow engine / n8n-like builder (§15).
- Knowledge vaults (§8).
- Chat rooms (§14).
- Artifact management / blob store (§17) — beyond basic file refs.
- Skills/MCP management UI (§9) — Hermes's existing skills/MCP continue to work
  as-is; Olympus doesn't manage them yet.
- Vector/semantic search (§10A.4) — tantivy keyword FTS only for MVP.
- bwrap/Docker sandboxing (§12) — MVP runs host-direct.
- SSH/terminal (§18).

## 5. Non-functional requirements

| NFR | Target |
|---|---|
| Import time | 1,626 sessions / 108k messages imported in <10 min |
| Search latency | Keyword search across all messages <500ms p95 |
| Session list load | <200ms for the full list (paginated, virtualized) |
| Streaming latency | Token-to-UI <100ms over local WSS |
| Memory (Olympus process) | <200MB with all sessions imported |
| Memory (per active session) | <20MB (sliding window, not full history resident) |
| Availability | Single-process; restart-on-crash via systemd; no HA in MVP |

## 6. Success metrics

1. **The operator stops opening Hermes Studio.** Olympus is the primary
   interface for all Hermes interaction within 2 weeks of MVP ship.
2. **Zero sessions lost in migration.** 1,626 in → 1,626 in Olympus.
3. **Cross-channel resume works.** Operator resumes a Telegram session from
   Olympus at least once per day and it just works.
4. **Search finds what Studio couldn't.** Operator runs a search that spans
   channels and gets a result that would have required grepping multiple
   SQLite tables manually.

## 7. Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Hermes bridge complexity | Low (de-risked) | ACP already exposes prompt/steer/cancel/streaming over stdio — verified in Hermes source. Drive `hermes acp`; no Hermes fork change needed for the MVP bridge. |
| Fork integrity (state.db write of forked session) | Medium | Olympus writes the forked session into state.db (WAL); dry-run + verify the forked session resumes cleanly via ACP before relying on it. Marker tag makes forks self-describing. |
| Import/sync edge cases (tool-call JSON, reasoning fields) | Medium | Read from state.db (complete archive, not JSONL); dry-run import to a temp store; diff counts; spot-check 10 sessions across sources |
| Concurrent state.db access (Hermes writes while Olympus reads) | Low | state.db is WAL mode — concurrent reads are safe and lock-free; Olympus only READS state.db for sync (writes only for fork-prep, also WAL-safe) |
| Tauri webview inconsistency on WSL | Low | WSL is dev host; verify early; prod is a real Linux box |
| Scope creep into deferred features | High | This BRD is the gate; deferred items require a new BRD entry |

## 8. Timeline

MVP target: **4–6 weeks** from implementation start. Phased per ADR 0002 §23
phases 1–5 (control-plane core, Hermes bridge, session store, UI, reactive sync).
The implementation plan (`docs/plans/`) breaks this into bite-sized tasks.

## 9. Stakeholders

- **Operator (rpw):** sole user for MVP. Drives requirements, tests, accepts.
- **Zephyr (AI):** architecture, implementation, documentation.
- **Hermes (the agent):** execution engine behind the bridge — not a stakeholder,
  but the system Olympus must integrate with.
