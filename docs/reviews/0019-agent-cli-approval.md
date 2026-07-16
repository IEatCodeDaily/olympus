# ADR 0019 agent CLI B4 approval

**APPROVED**

The sole remaining B4 from `docs/reviews/0019-agent-cli-adversarial-rereview.md:49-68` is fully resolved. This approval does not reopen B1-B3 or B5-B7.

## Reviewed snapshot

| Source | SHA-256 |
|---|---|
| `docs/reviews/0019-agent-cli-adversarial-rereview.md` | `63b0b9edbab3dcf20fd0d02f13744a4281fd657d9920d921d91d41cf5cfe81da` |
| `docs/adrs/0019-agent-and-human-cli-interface.md` | `0f06d88bab818f9d3ab1c1a86de6065eaa494e7278c1f30ad252939aad35baa9` |
| `docs/plans/2026-07-13-session-cutover-remote-development.md` | `bd5f5c3f8c03736c30273779fb99a2615327e3778e25b09c8c17bde9be4cc977` |
| `docs/cards/arch/cli-1-agent-olympus-interface.md` | `fa237c3e3b91f504843e8934e6442a890125292b06ba5fb2a3e83cc43f806dba` |

## B4 verification

1. **Unknown acceptance has executable terminal semantics.** A waiter reports a run ID only after confirmed acceptance; failed reconciliation before the client deadline returns typed `acceptance_unknown` in exit class 6 with the stable operation ID and exact `olympus operation get <operation-id>` recovery command (`docs/adrs/0019-agent-and-human-cli-interface.md:183-205`). The operation is part of the canonical command/operation mapping (`docs/adrs/0019-agent-and-human-cli-interface.md:59-97`). Task 3.3 requires that exact recovery surface and a commit+lost-response+unavailable-Hall/gateway Ctrl-C/timeout black-box gate (`docs/plans/2026-07-13-session-cutover-remote-development.md:371-401`).

2. **V1 cursor lifetime and future expiry behavior are explicit.** Workflow events and per-run sequences are non-expiring event-log truth in v1 and resumable from any valid sequence for the store lifetime. Any future compaction must version the operation, return typed `cursor_expired`, and use `workflows.get` terminal-snapshot fallback without redispatch; silent reset is forbidden (`docs/adrs/0019-agent-and-human-cli-interface.md:207-227`). Task 4.5 repeats the same contract (`docs/plans/2026-07-13-session-cutover-remote-development.md:502-506`).

3. **Both failure modes have black-box gates.** Task 4.5 explicitly tests commit+lost-response+unavailable reconciliation at Ctrl-C/timeout and reconnect from old cursors with a fixture proving no v1 compaction (`docs/plans/2026-07-13-session-cutover-remote-development.md:512-524`). CLI-1 carries `operation get`, durable non-expiring waits, unknown-acceptance recovery, and old-cursor resumability into its deliverables and gates (`docs/cards/arch/cli-1-agent-olympus-interface.md:28-58`).

**Conclusion:** all three corrections required by `docs/reviews/0019-agent-cli-adversarial-rereview.md:62-66` now have normative contracts and executable gates. B4 is closed.
