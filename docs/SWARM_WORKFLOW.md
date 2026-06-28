# Olympus Swarm Workflow (standard — every worker follows this)

> Orchestrator: Zephyr (monitors + validates only, does not implement).
> Workers: model-pinned agents, one task each, isolated.

## Roles & model channels (work around the delegate_task credential bug)

| Role | Model | Channel | Why |
|---|---|---|---|
| Coder | **glm-5.2** | `delegate_task` (provider=zai, model=glm-5.2) | Only reliable delegate_task route today |
| Reviewer | **gpt-5.5** | `code-reviewer` profile (CLI subprocess) | openai-codex creds work; off the broken relay |
| Validator/Tester | **claude-sonnet-4-6** | `tester` profile (CLI subprocess) | anthropic creds work via profile config |

(claude/gpt via `delegate_task` is BROKEN right now — base_url resolves to the zai relay → 401. Use the profile CLI channel for those models until the Hermes subagent-credential bug is fixed.)

## The TDD task lifecycle (RED → GREEN → REFACTOR — mandatory)

Every coding task MUST:
1. **RED**: write the failing test first. Run it. Paste the failure output.
2. **GREEN**: write the minimal code to pass. Run the test. Paste the pass output.
3. **REFACTOR**: clean up; tests stay green.
4. **Gate before "done"**: `cargo test` (all green), `cargo clippy -- -D warnings` (clean), `cargo fmt --check`. For UI: `bun test` + `bun run typecheck`. Paste real output — never claim green without it.
5. **Commit to a feature branch** `feat/olympus-<phase>-<slug>`. Do NOT merge to main. Do NOT touch files outside your assigned ownership.
6. **Report**: what you built, the exact test command + output, the branch name, any deviation from spec, and any bug discovered.

## File ownership (no two concurrent workers touch the same files)

- Backend core → `crates/control-plane/src/**` (further split per-module: `event.rs`, `log.rs`, `views/`, `search.rs`, `import.rs`, `server.rs`)
- UI → `ui/**`
- Workspace root (`Cargo.toml`, `.gitignore`) → ONLY the Phase-0 foundation worker; afterwards root edits go through the orchestrator.

A worker that needs a shared-file change (Cargo.toml dep add, lib.rs module decl) states it in the report; the orchestrator applies it to avoid races.

## Orchestrator validation gate (Zephyr, on every returned task)

A task is accepted ONLY when ALL pass:
1. Re-run the worker's tests independently: `cargo test` green (not trusting the worker's report).
2. `cargo clippy -- -D warnings` clean; `cargo fmt --check` clean.
3. **CodeRabbit review** on the branch diff (`coderabbit review` / CLI) — address criticals.
4. Spec-compliance: does it match the ADR/PRD/plan task? No scope creep, no invented APIs.
5. Functional check where applicable (the thing actually works, not just unit-green).
Only then merge the branch to main and dispatch dependent work.

## Coverage standard

- Every public function/behavior has a test. Aim full line+branch coverage on logic modules (event log, views, scheduler, import, fork, search).
- Functional/integration tests for cross-module behavior (e.g. append→view→delta, import→query, fork→resume).
- No "TODO: test later." Untested code does not merge.

## Post-mortem convention

Any known **Olympus** bug (discovered, worked-around, or shipped) gets a post-mortem at
`docs/postmortems/YYYY-MM-DD-<slug>.md`:
- Symptom, root cause, how found, fix (or workaround + why), prevention.
- Scope: Olympus's own code only. Infra/tooling bugs in Hermes, the delegation
  layer, or other external systems are NOT Olympus post-mortems — note them in the
  task report instead.

## Wave plan (dependency-ordered; fan out only when a wave's deps are met)

- **Wave 1 (solo)**: Phase 0 workspace + Phase 1 event log/redb. Serialization point.
- **Wave 2 (parallel ≤3)**: Phase 2 views · Phase 6 tantivy search · Phase 5 import (read-only half) · UI skeleton. All depend only on Phase 1.
- **Wave 3**: Phase 3 WSS+delta (needs views) · UI session-list/chat (needs API shape).
- **HELD until adversarial review lands**: Phase 4 ACP bridge · Phase 5 fork-via-state.db-write.
