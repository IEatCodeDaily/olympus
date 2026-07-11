# ARCH-D · Self-describing event payloads (versioned envelope)

## Goal
The SQLite `events` table stores `event_type TEXT` + a postcard-positional BLOB
— the same silent-corruption fragility class the old redb log had. Move NEW
events to a self-describing encoding with an explicit version, keeping full
replay compatibility for existing rows.

## Read FIRST
- `crates/control-plane/src/event.rs` — the Event enum.
- `crates/control-plane/src/log.rs` — append/read paths, payload
  encode/decode, the `events` schema.
- `crates/control-plane/src/compress.rs` — zstd layer.
- ARCH-C's result in your base: legacy redb code is feature-gated; do not
  touch it.
- `docs/adrs/0009-redb-to-sqlite-memory-reduction.md` §schema.

## Build on
Branch from main AFTER ARCH-C merges.

## Deliverables
1. Add a `payload_codec INTEGER NOT NULL DEFAULT 0` column to `events`
   (0 = postcard+zstd legacy, 1 = json+zstd). Schema migration must be
   idempotent and run automatically at open (follow the existing migration
   pattern in log.rs — see `925f7c2` for the organization-column precedent).
2. Writes: encode `serde_json::to_vec(&event)` + zstd, codec=1.
3. Reads: dispatch on codec per row. Old rows decode exactly as before. NO
   rewrite of existing rows — history is immutable.
4. Measure and report in your summary: events table size before/after codec
   flip on a synthetic 10k-event fixture (postcard vs json, both zstd'd), and
   append throughput (the existing batch-append path must stay batched).
5. A round-trip property test: every Event variant encodes(1)→decodes
   identically; plus a pinned-bytes test for one codec-0 row to prove legacy
   decode never breaks.
6. Two-paragraph ADR addendum (0009) recording codec versioning and the rule:
   event schema evolution now uses `#[serde(default)]` — never positional
   reshaping.

## Settled decisions — do NOT re-litigate
- The event log stays append-only, sole source of truth.
- No migration/rewrite of historical payloads.
- JSON chosen over other self-describing formats for sqlite3-CLI
  debuggability; zstd absorbs the size cost. If your fixture shows >1.5×
  on-disk growth AFTER zstd, report it in the summary rather than switching
  formats unilaterally.
- Do not touch proto frames or the wire protocol — this is storage-layer only.

## Gates
- `cargo test --workspace` + clippy `-D warnings` + fmt green.
- Do NOT run against `~/.olympus/olympus.db` (live data) — fixtures only.
- Do NOT start/restart the olympus server.
- Do not push to main. Green → `blocked: review-required`.
