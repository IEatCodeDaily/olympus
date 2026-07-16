# ADR 0019 agent CLI adversarial rereview

**Verdict: NO-GO**

The rewrite closes B1-B3 and B5-B7 and all eight non-blocking precision items. B4 remains incomplete: the CLI still promises a recoverable run ID in a response-loss window where it may know only the client operation ID, and resumable watch behavior still has no cursor-retention/compaction contract.

## Review snapshot

| Source | SHA-256 |
|---|---|
| `docs/reviews/0019-agent-cli-adversarial-review.md` | `de88e6a26e9097158e6d51384e8ceb84e461b29e404f93d9cd39586d3d5d3322` |
| `docs/adrs/0019-agent-and-human-cli-interface.md` | `250247b2102e3af8d261f09e17cae6dcb656da614e8cf99dc1ee55a3422a31b0` |
| `docs/adrs/0017-session-cutover-and-remote-development-plane.md` | `52c47dd96348825563624cb750b595b53849af71f774cee1da454743907a112c` |
| `docs/adrs/0013-workflow-kernel-bounded-chains.md` | `da782a222b3a97c9cc18eac1ad89663d41cba2f58438fe71a9bd8b641a41eec9` |
| `docs/cards/arch/wf-1-workflow-kernel.md` | `29c6d800259fbf363e5605e1d8e5d101344aa650ba9fffe4ad9130213b0880d8` |
| `docs/cards/arch/cli-1-agent-olympus-interface.md` | `9a647f540bd8ac8a88a316725b2a556044f1b5b3ac5cd063a223170d2819657f` |
| `docs/plans/2026-07-13-session-cutover-remote-development.md` | `3905876c3c809c145339a1b967288d1e3170d9018f5fb5755ed97e9c05e440e6` |

The source review contains B1-B7 only: B7 ends at `docs/reviews/0019-agent-cli-adversarial-review.md:167-188`, and P1 begins at `:190-196`. There is no B8 to reconcile; the B8 row below records that source discrepancy rather than inventing a finding.

## B1-B8 reconciliation

| ID | Status | Current reconciliation and exact sources |
|---|---|---|
| B1 | **Resolved** | V1 is UDS-only; path/mount/DAC, per-runtime identity, peer evidence, connection-derived context, FD delegation, accepted-connection revocation, and hostile tests are explicit (`docs/adrs/0019-agent-and-human-cli-interface.md:252-295`; `docs/adrs/0017-session-cutover-and-remote-development-plane.md:132-150`; `docs/plans/2026-07-13-session-cutover-remote-development.md:309-351`). |
| B2 | **Resolved** | Hall atomically authorizes, canonicalizes, reserves scoped operation identity plus intent/fence, defines same-ID digest behavior and revocation linearization, and gates crash/concurrency cases (`docs/adrs/0019-agent-and-human-cli-interface.md:297-322`; `docs/plans/2026-07-13-session-cutover-remote-development.md:336-351`). |
| B3 | **Resolved** | `olympus.workflow-input/v1` is a closed, publication-validated profile with injective names, bounded types/regex/rendering, opaque bindings, exact JSON scope, canonical defaults/digest, and shared conformance fixtures (`docs/adrs/0019-agent-and-human-cli-interface.md:97-179`; `docs/plans/2026-07-13-session-cutover-remote-development.md:481-516`; `docs/cards/arch/cli-1-agent-olympus-interface.md:39-46,55-57`). |
| B4 | **Open** | Non-blocking `workflows.run` plus adapter-side get/watch is now explicit, but ambiguous interruption and cursor lifetime remain underspecified (`docs/adrs/0019-agent-and-human-cli-interface.md:181-213`; `docs/plans/2026-07-13-session-cutover-remote-development.md:491-516`). See remaining blocker below. |
| B5 | **Resolved** | ADR 0013 and WF-1 now require JOBS-2 planned intent, exact fenced attempt reconciliation, and `StepIndeterminate`; WF-1's build dependencies are dispatch-blocking (`docs/adrs/0013-workflow-kernel-bounded-chains.md:64-78,102-113`; `docs/cards/arch/wf-1-workflow-kernel.md:3-29,41-52`; `docs/plans/2026-07-13-session-cutover-remote-development.md:52-55,469-516`). |
| B6 | **Resolved** | The canonical mapping and exhaustive versioned operation registry define types, scope, policy, idempotency, revocation, availability, audit, and adapter equivalence; raw operator argv DTOs remain outside agent operations (`docs/adrs/0019-agent-and-human-cli-interface.md:81-95,324-356`; `docs/plans/2026-07-13-session-cutover-remote-development.md:326-351,371-397,434-455`). |
| B7 | **Resolved** | The top-level graph names exact WF-1, self-development, and soak predecessors and makes them dispatch-blocking; both CLI-1 and WF-1 repeat the hard edges (`docs/plans/2026-07-13-session-cutover-remote-development.md:36-70`; `docs/cards/arch/cli-1-agent-olympus-interface.md:17-26`; `docs/cards/arch/wf-1-workflow-kernel.md:26-29`). |
| B8 | **N/A** | No B8 exists in the source review (`docs/reviews/0019-agent-cli-adversarial-review.md:167-196`). |

## P1-P8 reconciliation

All eight precision items were rechecked and are resolved in the current normative text:

| ID | Current source |
|---|---|
| P1 | Canonical CLI/operation/MCP mapping: `docs/adrs/0019-agent-and-human-cli-interface.md:59-95`; deployment adapters: `docs/plans/2026-07-13-session-cutover-remote-development.md:651-664`. |
| P2 | Static generic versus scoped dynamic help and non-enumeration: `docs/adrs/0019-agent-and-human-cli-interface.md:367-385`; proof journey: `docs/plans/2026-07-13-session-cutover-remote-development.md:701-708`. |
| P3 | Bounded terminal escaping and non-executable completion data: `docs/adrs/0019-agent-and-human-cli-interface.md:379-385`; hostile fixtures: `docs/plans/2026-07-13-session-cutover-remote-development.md:384-397`. |
| P4 | Duration grammar/cap and `client_wait_timeout` exit class: `docs/adrs/0019-agent-and-human-cli-interface.md:191-198,392-406`. The remaining unknown-acceptance defect is blocking B4, not a separate precision item. |
| P5 | Static grammar, typed `operation_unavailable` class 6, and incompatibility class 8: `docs/adrs/0019-agent-and-human-cli-interface.md:74-79,392-406`. |
| P6 | Hall-owned post-default canonical digest and surface semantics: `docs/adrs/0019-agent-and-human-cli-interface.md:408-423`. |
| P7 | Cancel-request, quiescence, terminal cancel, and indeterminate truth: `docs/adrs/0019-agent-and-human-cli-interface.md:430-437`; implementation/test contract: `docs/plans/2026-07-13-session-cutover-remote-development.md:498-516`. |
| P8 | Descriptor-level availability plus the operation gate matrix and dispatch-blocking DAG: `docs/adrs/0019-agent-and-human-cli-interface.md:324-356,477-509`; `docs/plans/2026-07-13-session-cutover-remote-development.md:36-70`. |

## Remaining blocker

### B4 — Ambiguous interruption and cursor expiry still lack executable terminal semantics

**Current sources:**

- `docs/adrs/0019-agent-and-human-cli-interface.md:183-189` promises that every interrupted waiter prints a durable run ID and reconnect command.
- `docs/adrs/0019-agent-and-human-cli-interface.md:195-198` makes the same promise for client timeout.
- `docs/adrs/0019-agent-and-human-cli-interface.md:200-213` correctly creates the operation ID before send and says pre-response interruption first reconciles it, but defines neither reconciliation failure nor watch cursor retention/expiry.
- `docs/plans/2026-07-13-session-cutover-remote-development.md:498-516` requires monotonic resume and interruption tests, but no unavailable-reconciliation or expired/compacted-cursor oracle.

**Blocking failure sequence:** The client sends `workflows.run`; Hall commits the run; the response is lost; then Hall or the gateway is unreachable when Ctrl-C or `--timeout` fires. The client has the stable operation ID but cannot obtain the promised run ID. The documents provide no bounded exit or machine error carrying an unknown-acceptance recovery ID, so implementations may block past timeout, print a false run ID, or discard the only reconciliation key. Separately, `watch(after_sequence)` has no declared never-expire guarantee and no `cursor_expired`/terminal-snapshot fallback, so reconnect behavior is undefined if retained events are compacted.

**Required correction:**

1. If operation-ID reconciliation cannot complete within the client deadline, return a typed `acceptance_unknown` result containing the operation ID and exact retry/reconcile command; promise a run ID only after successful reconciliation.
2. Declare workflow watch cursors non-expiring in v1, or define retention, typed expiry, and `workflows.get` terminal-snapshot fallback without redispatch.
3. Add black-box gates for commit + lost response + unavailable Hall/gateway + Ctrl-C/timeout, and for reconnect after cursor expiry/compaction (or prove v1 events are never compacted).

Re-review only B4 after those contracts and gates are added; the other findings do not need reopening unless their source hashes change.
