# 0025 — Envoy spool sequence gap and periodic resend storm

**Status:** Producer fix complete; existing production spool reconciliation pending  
**Incident:** 2026-07-13

## Impact

Hall repeatedly logged observation sequence gaps, including:

> `expected 112, got 528`

The Envoy resent its full unacknowledged suffix every second. One blocked sequence therefore produced a sustained warning/CPU/transport storm while no durable progress was possible.

## Root cause

`EventSpool::next_seq()` advanced the in-memory allocator before the caller appended and fsynced the frame. If append failed, the sequence was consumed without a durable record. Hall correctly required contiguous acceptance and could never ACK beyond the invisible hole.

The periodic replay loop independently reread and retransmitted every unacknowledged frame each second. It had no per-connection sent cursor and treated lack of ACK as permission to flood.

## Additional observation defect

The inspected observation source `observed:cron_cb047f64216d_20260713_154016` had Hall watermark 111 and spool sequences 113–537. Across Hall plus spool, source logical messages were present, but 138 logical IDs were duplicated because `StateDbObserver` can restart and enumerate old rows again under new transport sequences. Another spool contained a real `(13,16)` transport gap.

This is separate from the atomic append defect and requires a durable observer cursor/source-epoch design.

## Corrective actions

- Added `EventSpool::append_next`, which allocates, mutates the frame, appends, fsyncs, persists the counter, and only then advances the in-memory allocator.
- Moved runtime events, observations, job output, and job results to `append_next`.
- Added a regression proving failed append does not consume a sequence.
- Added a per-live-connection periodic replay cursor. Each durable frame is sent at most once unless Hall explicitly requests `ResumeFrom`.
- Kept `ResumeFrom` replay off the socket read loop so replay cannot starve ACKs and heartbeats.
- Added regressions for asynchronous `ResumeFrom` dispatch and per-connection cursor advancement.

## Verification

- Envoy suite: 69 library, 2 binary, and 2 iroh tests passed.
- Envoy Clippy passed with `-D warnings`.
- Earlier deployed asynchronous replay fix drained a 940-frame backlog to zero without taking `terminus` offline.

## Recovery boundary

Existing corrupt/gapped production spools must not be deleted, renumbered, or silently skipped. Recovery requires:

1. Back up Hall DB and every spool/counter file.
2. Produce source/Hall/spool manifests.
3. Preserve each logical source record exactly once.
4. Close the old source epoch as incomplete.
5. Start a new authenticated source epoch with a fresh contiguous sequence.

ADR 0018 telemetry ingestion must use authenticated source identity, source epoch, record sequence, and atomic record-plus-watermark commit. It must not inherit the observer ordinal model.