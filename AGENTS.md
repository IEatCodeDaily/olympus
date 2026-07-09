# Agent Map — Olympus

Short map for coding agents working on Olympus. Detailed guidance lives in `docs/`.

## What Olympus is

A clean-room, Rust-native AI control plane for Hermes Agent: a single-binary
**event-sourced control plane** (redb log → in-memory materialized views →
tantivy search → axum REST/WS API) plus a **Vite + React UI** under `ui/`. It
unifies all Hermes sessions from every channel into one searchable, resumable
interface. NOT a fork of Hermes Studio. The earlier Convex/Bun/TS design was
removed (ADR 0003); do not reintroduce it. See `docs/architecture/architecture.md`,
`docs/adrs/0002-olympus-fleet-control-plane.md`, and
`docs/adrs/0003-remove-convex-rust-native-substrate.md`.

## First reads

- `docs/plans/2026-06-29-olympus-long-horizon-roadmap.md` — **START HERE.** The
  durable roadmap (epics A→P, milestone briefs, gates, live Status Ledger). Tells
  you what to build next and why; built for autonomous swarm execution.
- `docs/adrs/0002-olympus-fleet-control-plane.md` — authoritative spec (~24 §).
- `docs/adrs/0003-remove-convex-rust-native-substrate.md` — substrate decision.
- `docs/api-contract.md` — UI↔backend wire contract (REST + WSS + shared TS types).
- `docs/plans/2026-06-28-olympus-mvp.md` — granular Epic A/B tasks (phases 0-8).

## Workspace

- `crates/control-plane/src/` — the Rust control plane:
  - `event.rs`, `log.rs`, `compress.rs` — event-sourced append-only log (redb + zstd).
  - `views/` — in-memory materialized projections (session + message views).
  - `search.rs` — tantivy full-text index.
  - `import.rs` — read-only bulk import from Hermes `state.db`.
  - `auth.rs` — per-install token + Bearer/Origin gate.
  - `server/` — axum REST + `/ws` delta stream + camelCase DTOs + CORS.
  - `main.rs` — boot: import → build views/search → serve.
- `ui/` — Vite + React + TypeScript client (own bun setup; MSW mock mode toggled
  by `VITE_USE_MOCKS`, real backend via `.env.local`).
- `docs/` — ADRs, plan, api-contract, reviews. `hermes-patches/` — patch-not-fork
  registry for required Hermes changes.

## Commands (canonical)

```bash
make verify          # ALL gates: Rust (test/clippy/fmt) + UI (typecheck/build/e2e)
make verify-rust     # cargo test --workspace && clippy -D warnings && fmt --check
make verify-ui       # cd ui && bun run typecheck && bun run build && playwright e2e
make test            # cargo test --workspace (fast inner loop)
make run             # cargo run --release (imports state.db, serves API on :8787)
make deploy          # build + install both hall + envoy binaries (symlink flip)
make deploy-hall     # build hall → flip → restart olympus-hall.service
make deploy-envoy N=2 # build envoy → flip → start olympus-envoy@2 → health gate

# Direct equivalents (what `make` runs):
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cd ui && bun run typecheck && bun run build && bun run test:e2e
```

There is NO `bun run test` / `bun run lint` / Convex command — those were the old
TS scaffold (removed in fe7580b). The UI test target is `test:e2e` (Playwright).

## Production services (ADR 0008 S6)

Olympus runs as two systemd user services:
- `olympus-hall.service` — the control plane (event log, views, REST/WS API).
- `olympus-envoy@1.service` — the local agent runtime holder (hermes acp children).

Binaries live at `~/.olympus/bin/olympus-{hall,envoy}-<gitHash>` with stable
symlinks (`olympus-hall` / `olympus-envoy`) as the deploy pointer. The
`scripts/deploy.sh` script handles build → copy → symlink flip.

## Hard rules

- The redb event log is the sole source of truth; views are pure projections.
  Never mutate view state outside an `apply(event)` path.
- `state.db` is read ONLY (open `SQLITE_OPEN_READ_ONLY`); never write the live
  Hermes DB. Cross-channel continuation is a FORK, never an in-place edit.
- Patch Hermes, never fork — via `hermes-patches/patchctl.sh`.
- The auth gate (token + loopback Origin) applies to all `/api/*` and `/ws`.
  Bind `127.0.0.1` by default; remote bind is opt-in and fails closed.
- UI and backend share `docs/api-contract.md`; a contract change updates both
  sides. The DTO layer (`server/dto.rs`) is the only place view rows become wire
  JSON (camelCase).
- `make verify` must be green before a PR.

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **olympus** (2886 symbols, 5482 relationships, 241 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/olympus/context` | Codebase overview, check index freshness |
| `gitnexus://repo/olympus/clusters` | All functional areas |
| `gitnexus://repo/olympus/processes` | All execution flows |
| `gitnexus://repo/olympus/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
