# ADR 0018 OpenTelemetry observability — final approval review

**Review date:** 2026-07-13  
**Verdict:** **APPROVED / GO — 0 BLOCKER, 0 P1, 1 P2.**  
**Scope:** focused final review of the current ADR 0018 and implementation plan
for the last sequence-loss state-machine correction and ADR 0017 Task 1.4
ordering correction, including only contradictions newly introduced by those
changes. The review also checked the relevant ADR 0017 plan and current Hall
node-enrollment source. This is architecture/plan approval, not implementation
approval.

## Executive verdict

The two findings from the focused adversarial re-review are resolved.

The range-consuming coverage design now has one coherent sequence authority and
an end-to-end crash/replay/ACK state machine. A durable journal reservation
consumes a range before the data append; append failure converts the same range
to `dropped`, unresolved finalization recovers as `unknown`, and no range is
rolled back or reused. Data and coverage items merge in source order, coverage
consumes its complete range, Hall commits items plus the contiguous watermark
atomically, and exact duplicate, overlap, ACK-cleanup, exhaustion, and manifest
rules are stated. Consequently an ENOSPC failure can consume a durable loss
range and later successful data can follow it without a hole or reused source
position.

ADR 0017 Task 1.4 is now a hard predecessor consistently: it appears in ADR
0018's dependency chain, the plan DAG, the no-remote rule, PRE-OBS evidence, and
OBS-3 as a completed prerequisite that OBS-3 consumes and re-verifies rather
than owns.

One stale terminology fragment remains in the plan. It does not override the
normative source-journal rules or remove a test oracle, so it is P2 editorial
cleanup and does not block approval.

## Resolution of the prior findings

### Prior BLOCKER — resolved: range-consuming loss coverage is coherent

The corrected contract is internally consistent across ADR and plan:

1. **Durable allocation authority.** ADR 0018 defines the preallocated source
   journal as sequence authority and requires a fsynced `pending` reservation,
   including range and expected item digests, before a data-spool append
   (`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:279-282`).
   Without that reservation no position is consumed.
2. **Append and finalization outcomes.** A complete data batch is appended and
   fsynced before journal state becomes `received`; append/fsync failure
   finalizes the already-reserved range as `dropped`; failed finalization leaves
   durable `pending`, which means `unknown`, not complete (`ADR 0018:283-286`).
3. **Recovery and non-reuse.** Recovery deterministically merges journal and
   segmented spool. Matching complete data wins; pending without valid complete
   data becomes range-consuming `unknown`; finalized dropped state becomes one
   coverage item over the full reserved range; the range is never reused
   (`ADR 0018:287-291`).
4. **Ordered range model.** Wire items are adjacent, ordered, and non-overlapping
   from envelope `first_seq` through `last_seq`; data consumes one position and
   coverage may consume many. Replay merges both item classes in exactly that
   order (`ADR 0018:292-295`). The schema likewise gives an item both
   `item_first_seq` and `item_last_seq` and gives coverage an explicit range
   (`ADR 0018:207-221`).
5. **Atomic Hall commit.** Hall validates source ownership, contiguity, IDs,
   digests, size, and redaction, then inserts every item, coverage state, and the
   new contiguous watermark in one transaction. A coverage item advances through
   its range end; partial/corrupt batches receive no ACK (`ADR 0018:296-300`).
6. **Duplicate and overlap contract.** A wholly duplicate batch is ACKed without
   reinsertion only when every committed range, ID, and digest matches. Partial
   overlap and same-range/different-identity replay are rejected
   (`ADR 0018:301-303`).
7. **ACK cleanup.** ACK is only for committed contiguous `through_seq`; cleanup
   advances journal and data segments together. Duplicate ACK is idempotent, and
   cleanup failure retains replayable state without advancing local deletion
   metadata (`ADR 0018:301-306`).
8. **Journal exhaustion and unadmitted data.** Journal capacity and reserve are
   outside the telemetry data budget. Admission stops before exhaustion;
   unadmitted observations consume no positions and are reported as unknown via
   source health/reconnect and the permanent loss fact rather than being assigned
   fictitious dropped sequence numbers (`ADR 0018:307-315`).
9. **Crash and manifest oracles.** Tests cover reservation, append/fsync,
   finalization, merged replay, Hall transaction stages, ACK and cleanup, pending
   recovery, journal exhaustion, duplicate/partial overlap, and later successful
   data after a dropped range. Producer/Hall manifests compare source/epoch,
   consumed data and coverage ranges, normalized digests, trace/span IDs, and
   watermark (`ADR 0018:317-322`; plan:263-284).

The implementation plan carries the same state machine in OBS-3: source-journal
reservation, received/dropped/unknown finalization, ordered range merge without
reuse, full-envelope contiguity, atomic Hall item/coverage/watermark commit,
exact duplicate acceptance, partial-overlap rejection, journal/data cleanup,
and high-water stop (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:230-256`).

**ENOSPC/later-success conclusion:** the first durable reservation fixes the
failed batch's source range before the potentially failing data append. That
range is subsequently represented by `dropped` coverage, or conservatively by
`unknown` after unresolved finalization. The journal remains the allocator, so
a later successful reservation starts after that consumed range. Ordered merge
places the coverage range before later data, Hall advances atomically through
both, and ACK cleanup cannot erase either side early. There is therefore no
missing position and no position reuse in the specified state machine.

### Prior P1 — resolved: ADR 0017 Task 1.4 is a hard predecessor everywhere

The executable ownership and ordering now agree:

- ADR 0018's hard chain places ADR 0017 Tasks 1.2–1.4 before local telemetry and
  the authenticated remote source protocol (`ADR 0018:68-90`).
- ADR 0018's ingest-authority section explicitly depends on Tasks 1.2–1.4
  (`ADR 0018:324-348`).
- The plan DAG places Tasks 1.1–1.4 before OBS-0/OBS-1/OBS-3
  (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:26-45`).
- The no-remote rule requires Tasks 1.1–1.4 to be implemented and verified
  (`plan:51-54`).
- PRE-OBS requires a Task 1.4 artifact proving authenticated peer-key/OS-identity
  to logical-node binding, duplicate/takeover rejection, and durable enrollment
  lookup (`plan:56-74`).
- OBS-3 consumes and re-verifies completed Tasks 1.2–1.4 artifacts and explicitly
  says it does not reimplement them (`plan:219-227`).
- This matches ADR 0017's actual Task 1.4 ownership: logical node ID is bound to
  the enrolled iroh key and duplicate/takeover hello is rejected
  (`docs/plans/2026-07-13-session-cutover-remote-development.md:152-171`).

No alternate path in the reviewed ADR/plan permits remote telemetry to bypass
Task 1.4.

## Remaining finding

### [P2-1] Remove two stale dropped-ledger terms from the plan

`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:110-113`
still calls the preallocated structure a “gap ledger,” while the corrected
architecture and OBS-3 call it the source journal. Lines 263-266 also contain an
orphaned fragment, “dropped-ledger update,” before the rewritten crash matrix.
These are stale terms from the superseded design.

They are not P1 because the normative OBS-3 work, crash list, and exact-manifest
gate unambiguously require the source journal and its reservation/finalization
state machine; no per-record rollback rule remains, and journal finalization is
already a named crash point. Clean these phrases in the next documentation edit
to prevent implementer confusion, but they do not change the approved design.

## Implementation-start decision

**Architecture/plan review: GO. Implementation: still blocked.**

Approval of ADR 0018 does not prove that ADR 0017 Tasks 1.1–1.4 have landed. In
particular, current Hall source still accepts the logical `node_id` supplied by
`EnvoyFrame::Hello` and registers that ID alongside an optional peer iroh ID
rather than deriving the logical node from a durable enrollment binding
(`crates/control-plane/src/node.rs:638-701`). Thus the Task 1.4 prerequisite is
still open in current source.

Under PRE-OBS and the dependency graph, only paper design may continue until the
required ADR 0017 test artifacts exist. No ADR 0018 implementation card,
remote-telemetry protocol, or completeness/query claim may be dispatched merely
because this document review is GO.

## Reviewed snapshot

Repository HEAD at review time:
`f784b04aa6d04e6759eef999012f48ac3f0f1622`.

The ADR 0018 and observability plan files are untracked in the current worktree,
so this review identifies their exact content hashes:

- ADR 0018: `03856b7aee09d9e339d7cf7c9deacfb4092e512a5ff7d236bfd97c2e139ed50e`
- OBS plan: `8a4629a66b6834dd7e212207f991f4195610c8ecfcdd3c86cead44184cbff7be`
- ADR 0017: `02d6cdfbd5516831e8131c442725b125f2216ab8440f570362d95290e987842d`
- ADR 0017 plan: `22577cf643520bb6fc4aaf92d0c1f4eeb274260518fe31dff84ce0012437b4e2`
- Hall `node.rs`: `63c7ac94c74ea4af019cd936073a13cf5601a75fcde2272928d26a7f7d3f056d`

**Final verdict: APPROVED / GO — 0 BLOCKER, 0 P1, 1 P2.**
