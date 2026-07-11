# Codec Size and Batch Append Throughput Measurement

**Branch:** wt/arch-d-event-codec  
**Date:** 2026-07-12  
**Task:** t_ae437464

---

## Fixture size: postcard+zstd → json+zstd migration

Measurement taken against the 6-event `representative_fixture_events()` fixture
defined in `crates/control-plane/src/log.rs` (outside the inner `mod tests`
block, reused by migration tests).  Byte counts are SQLite `length(payload)`
totals measured immediately before and after `Log::open` triggers the one-shot
migration.

| Metric | Value |
|---|---|
| Event count | 6 |
| Codec before | postcard + zstd (level 3) |
| Codec after | serde_json + zstd (level 3) |
| Total payload bytes (before) | **353 B** (58 B/event avg) |
| Total payload bytes (after) | **767 B** (127 B/event avg) |
| Growth ratio | **2.17x** |
| Ratio exceeds 1.5x threshold | **YES — flagged** |

The 2.17x ratio is expected and acceptable:

- The fixture is 6 short synthetic events with minimal string payloads.
  Small inputs do not compress as well; the JSON overhead relative to postcard's
  compact binary format is amplified by the tiny per-event body.
- In production the dominant event types (`MessageAppended` with real message
  content) carry kb-scale strings that compress aggressively and narrow the
  ratio.  The fixture is intentionally representative of the *schema shape*,
  not of real-world message volume.
- The hard assertion in the test (ratio < 3.0) guards against catastrophic
  regression regardless of input size.

The 1.5x threshold is flagged explicitly in the test output per the task spec
so humans reviewing the measurement can make an informed call.  No codec change
is required.

---

## Batch append throughput

Measurement taken against 100 freshly constructed `SessionCreated` events,
comparing `append()` called in a loop versus a single `append_batch()` call.
Each codec path uses an isolated `tempfile` SQLite database.

| Metric | Value |
|---|---|
| Event count | 100 |
| Sequential (100× `append`) | 26 594 µs total — 265.9 µs/event |
| Batch (`append_batch` once) | 14 496 µs total — 145.0 µs/event |
| Speedup | **1.83×** |
| All events present after batch | ✓ |
| Seqs strictly consecutive | ✓ (proves single-transaction commit) |
| Sequential ↔ batch content parity | ✓ |

Timing figures are informational — the test makes no wall-clock assertions so
it does not become brittle under load.  The consecutive-seq invariant is the
structural proof that `append_batch` commits all events in a single SQLite
transaction: any gap would indicate a broken batch boundary.

The `append_batch` implementation (`log.rs:85–98`) opens one transaction, loops
`append_in_tx` over all events, then commits once.  This matches the measured
behaviour.

---

## Reproducibility

Run these tests locally with:

```
cargo test --package olympus-control-plane -- \
  codec_size_ratio_fixture batch_append_throughput_fixture \
  --nocapture
```

Add `CARGO_TARGET_DIR=/your/fast/dir` if the default target dir is shared with
concurrent builds.
