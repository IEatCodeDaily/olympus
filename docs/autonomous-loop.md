# Olympus Autonomous Migration Loop

Status: **PAUSED** — built but cron NOT enabled. One tick will be hand-run to prove
the flow before any cron is activated. (Per owner directive.)

> **Task source (CURRENT):** the long-horizon roadmap
> `docs/plans/2026-06-29-olympus-long-horizon-roadmap.md` is the source of truth
> for *what to build next* (epics A→P, milestone briefs, gates, Status Ledger).
> Granular Epic A/B tasks live in `docs/plans/2026-06-28-olympus-mvp.md`
> (Phases 4–5). The Convex/bun references below are from the superseded
> migration plan and apply only to that legacy track — the live substrate is
> Rust (gates: `make verify`, not `bun test`).

## Roles (model-pinned profiles)

| Role | Profile | Model | Tools |
|---|---|---|---|
| Orchestrator | (this agent / controller) | — | kanban, git, verification |
| Coder | `coding-agent` | glm-5.2 (zai, max thinking, no model fallback) | gitnexus, coderabbit, terminal/file |
| Reviewer | `code-reviewer` | gpt-5.5 (openai-codex, xhigh; delegates gpt-5.4-mini) | gitnexus, coderabbit CLI |
| Validator | `tester` | claude-sonnet-4-6 (+vision; vision-only fallback) | maestro web e2e, vision |

## Board

- Board: `olympus-migration`
- Default workdir: `/home/rpw/.hermes/profiles/coding-agent/workspace/olympus` (coding-agent's isolated clone — NEVER the owner deploy repo `/home/rpw/olympus`)
- Source of truth tasks: `docs/plans/2026-06-24-react-convex-bun-migration.md`

## Flow (per task)

```text
1. Orchestrator seeds a card (assignee=coding-agent) from a migration-plan task.
2. coding-agent implements in its clone, runs bun test + oxlint + typecheck,
   pushes a feature branch, opens a PR (gh pr create). Does NOT merge.
   -> signals review-required.
3. code-reviewer triggers (see triggers) -> reviews the PR diff:
   - coderabbit CLI review + gitnexus impact analysis
   - Convex args+returns on ALL functions, authz gates, fail-closed defaults
   - adapter-boundary rule (no Hermes imports in React/Convex)
   -> posts PR review: approve / request-changes.
4. tester triggers -> checks out the PR branch in tester's clone, spins an
   EPHEMERAL convex dev backend (port 3240) + web (5180), runs Maestro web e2e
   + vision checks, tears down. -> posts pass/fail.
5. MERGE GATE (orchestrator-enforced): merge to main ONLY when
   code-reviewer = approved AND tester = green. Orchestrator does the merge.
   Nothing auto-merges.
```

## Reviewer triggers (both)

- **On PR change**: a new PR or new commits to an open PR enqueue a review card.
- **Periodic full-system review**: a scheduled card (e.g. daily) for a whole-repo
  review pass (architecture drift, cross-cutting issues), independent of any single PR.

## Guardrails (hard)

- Per-task `--max-runtime 90m`; expect glm-5.2 subagent timeouts (salvage pattern).
- Resource-limited builds (host stays responsive) — bun/convex/maestro capped.
- Workers DO NOT merge to main. Workers open PRs; orchestrator merges after the gate.
- Workers operate ONLY in their own profile clone + reserved ports (see docs/ports.md).
- coding-agent never binds owner ports (3210/8791/5177).
- Branch naming: `feat/olympus-<task-code>`.

## Cron (NOT yet enabled)

When the proof tick passes and the owner approves:
- A dispatch cron (`every 15m`, scoped toolsets) runs `kanban dispatch --max 3`
  and posts `kanban stats`. It reports/dispatches only — it is NOT the controller.
- A periodic full-system review card is scheduled (daily) assigned to code-reviewer.

## Proof tick (before enabling cron)

1. Seed ONE task (OLY-1: convex tests / first slice hardening).
2. Hand-run: dispatch coding-agent -> verify PR opened -> dispatch code-reviewer
   -> dispatch tester -> confirm gate logic -> orchestrator merges.
3. Only then create the cron jobs.
