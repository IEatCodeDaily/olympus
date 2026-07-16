# ADR 0017 session cutover — final approval review

**Review date:** 2026-07-13  
**Scope:** current `docs/plans/2026-07-13-session-cutover-remote-development.md` and `docs/adrs/0017-session-cutover-and-remote-development-plane.md`, checked specifically against the three blockers and three precision edits in `docs/reviews/0017-session-cutover-adversarial-rereview.md`.

## Verdict

**NO-GO — one approval blocker remains, plus one non-blocking precision edit is only partially resolved.**

The effectful-tool and runtime-recovery corrections are now coherent, and Task 2.4 now owns the correct Envoy files and real spool failure tests. However, the plan still orders Task 2.3's Hall reconciliation before Task 2.4's required Envoy terminal/spool correctness, despite the re-review explicitly making that Envoy-side work a prerequisite of Task 2.3 reconciliation. This is the only remaining approval blocker found. The latest edits introduce no additional ordering contradiction.

## Blocker reconciliation

### Blocker 1 — resolved

Effectful `jobs.run` no longer appears in Task 3.2. Task 3.2 exposes only non-effectful `session.info` before Phase 4 completes (`docs/plans/2026-07-13-session-cutover-remote-development.md:307-320`). The activity-provider and sandbox boundaries land in Tasks 4.1 and 4.2 (`:324-353`), and typed effectful job tools are exposed only afterward in Task 4.3 (`:355-374`). Task 4.3 also requires Envoy-provider construction of argv/env/cwd, compile/runtime disabling until both gates are green, and a non-skippable hostile isolation integration test (`:366-374`).

### Blocker 2 — resolved

Task 5.2 now permits same-attempt reattachment only when the same Envoy proves durable local attempt+cgroup+process identity and does not spawn a replacement (`docs/plans/2026-07-13-session-cutover-remote-development.md:416-420`). Every other outcome empties the old cgroup, terminalizes/orphans the old attempt, and creates a new attempt epoch; a new node always means a new attempt (`:418-420`). Its gate names restart, partition, node loss, post-spawn ambiguity, stale effects, daemon/double-fork, and single-prompter/cgroup oracles (`:422-424`). This agrees with the ADR's at-least-once, idempotent fenced-attempt semantics (`docs/adrs/0017-session-cutover-and-remote-development-plane.md:184-186`) and new-attempt node recovery (`:285-288`).

### Blocker 3 — partially resolved; ordering prerequisite remains blocking

Task 2.4 now correctly owns all three Envoy implementation surfaces—`job_table.rs`, `main.rs`, and `spool.rs`—alongside Hall persistence/ACK work (`docs/plans/2026-07-13-session-cutover-remote-development.md:245-256`). Its gate now requires joined output drains, terminal-last sequencing, atomic sequence reservation+append, fail-closed disk/fsync/cap/ACK-rewrite behavior, and real-spool tests for final-byte ordering, ENOSPC/read-only spool, cap exhaustion, fsync failure, corrupt tail, ACK rewrite failure, and restart (`:258-267`). This resolves the file ownership and test-surface defect.

**Remaining blocker:** Task 2.3 still precedes Task 2.4 and requires startup/reconnect reconciliation of non-terminal jobs (`docs/plans/2026-07-13-session-cutover-remote-development.md:224-243`) before Task 2.4 establishes the Envoy terminal/spool contract on which trustworthy reconciliation depends (`:245-267`). The re-review's required correction explicitly stated that “Task 2.3 reconciliation and all MCP exposure remain gated on this Envoy-side task” (`docs/reviews/0017-session-cutover-adversarial-rereview.md:80-82`). MCP exposure is now later, but Task 2.3 reconciliation is not gated on Task 2.4 anywhere in the current plan.

**Required correction:** move the Envoy terminal/spool portion of Task 2.4 before Task 2.3, or explicitly split/gate Task 2.3 so its startup/reconnect reconciliation cannot be completed or enabled until Task 2.4's real-Envoy durability gate is green. Update the dependency graph or task prerequisites so the executable order is unambiguous.

## Precision-edit reconciliation

1. **Resolved:** Phase 7 explicitly says DEPLOY-1 depends on Task 6.1 durable edge and not Task 6.2 APP-1 (`docs/plans/2026-07-13-session-cutover-remote-development.md:479-482`).
2. **Resolved:** the dependency graph now says `JOBS activities/artifacts + durable EDGE --> DEPLOY contract/journal/provider`, correctly making the journal part of DEPLOY rather than an input to it (`:50-52`).
3. **Partially resolved, non-blocking:** Task 4.2's hostile fixtures are explicitly non-skippable in the cutover profile (`:352-353`), and Task 4.3 repeats that requirement for the first real `jobs.run` isolation test (`:372-374`). The runtime/process hostile gates themselves—Task 2.5's process-tree test (`:269-281`) and Task 5.2's daemon/double-fork, stale-effect, and cgroup oracles (`:404-424`)—still do not explicitly say they are non-skippable in the cutover profile. Add that sentence to those gates for the requested precision.

## Ordering consistency

The amended provider/sandbox/tool order is internally consistent: non-effectful bridge injection precedes provider and sandbox construction, while effectful tools follow both. The attach-or-new-attempt recovery rule is also consistent with the ADR and does not create a same-attempt cross-node path. No new ordering contradiction was introduced by those edits.

The sole ordering defect is the still-unmet Task 2.4-before-Task 2.3-reconciliation prerequisite above. Once that dependency is made executable and explicit, the proposal is approvable; implementation and cutover remain separately gated by the plan's real-substrate evidence.
