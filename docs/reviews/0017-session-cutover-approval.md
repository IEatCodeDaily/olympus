# ADR 0017 session cutover — approval

**Verdict: APPROVED**

The two remaining items from the final approval review are resolved:

- **Task 2.3/2.4 ordering blocker:** the dependency graph now explicitly orders attempt inventory → Envoy terminal/spool correctness → JobService reconciliation (`docs/plans/2026-07-13-session-cutover-remote-development.md:50`). Task 2.3 also has a hard prerequisite forbidding startup/reconnect reconciliation from being enabled, or Task 2.3 from being completed, until Task 2.4's real-Envoy durability gate is green (`docs/plans/2026-07-13-session-cutover-remote-development.md:229-232`). This satisfies the correction required by `docs/reviews/0017-session-cutover-final-approval-review.md:26-28`.
- **Non-skippable hostile-gate precision edit:** Task 2.5 now makes its process-tree gate non-skippable in the cutover profile (`docs/plans/2026-07-13-session-cutover-remote-development.md:287-289`), and Task 5.2 does the same for its runtime/process hostile gates (`docs/plans/2026-07-13-session-cutover-remote-development.md:430-433`). This resolves the precision edit identified at `docs/reviews/0017-session-cutover-final-approval-review.md:34`.

No remaining approval blocker or requested precision edit was found within this limited verification scope.
