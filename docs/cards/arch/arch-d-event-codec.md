# ARCH-D · Self-describing event payloads — JSON-only end state (postcard removed)

## Goal
The SQLite `events` table stores postcard-positional BLOBs — silent-corruption
fragility on any enum reshape. Operator directive: dev phase, nuke the bad
decision. End state: **json+zstd is the ONLY codec**, postcard is fully removed
from the workspace. Existing rows are re-encoded ONCE by an idempotent boot
migration.

## Read FIRST
- `crates/control-plane/src/event.rs` — the Event enum.
- `crates/control-plane/src/log.rs` — append/read, `events` schema, the
  existing idempotent-migration pattern (see commit `925f7c2` for the
  organization-column precedent).
- `crates/control-plane/src/compress.rs` — zstd layer stays.
- ARCH-C's merged result: legacy redb code is already gone.

## Build on
Branch from main AFTER ARCH-C merges.

## Deliverables
1. Boot migration, idempotent, guarded by a `meta`/pragma marker: read every
   events row, decode postcard, re-encode `serde_json::to_vec` + zstd, rewrite
   the payload in place inside one transaction (batched). On a fresh DB it's a
   no-op. This is the ONE exception to append-only immutability — it changes
   encoding, never content; assert event-count and per-event equality
   (decode-old == decode-new) during the migration.
2. After migration lands: all writes json+zstd; the postcard decode path and
   the `postcard` dependency are DELETED from the workspace (check crates/proto
   and envoy for other postcard users first — if the wire protocol uses it,
   scope deletion to control-plane and say so in the summary; proto frames are
   ADR 0008 JSON-lines, so postcard there is unlikely but VERIFY).
3. Round-trip property test: every Event variant encodes→decodes identically.
   Migration test: build a small postcard-encoded fixture DB (vendor the old
   encode helper into the test module only), run the migration, assert
   equality + idempotency on second boot.
4. Report in summary: events table size before/after on the fixture, and
   batch-append throughput unchanged (the batched path stays batched).
5. ADR 0009 addendum: json+zstd sole codec; event schema evolution uses
   `#[serde(default)]` / additive fields — positional reshaping is dead.

## Settled decisions — do NOT re-litigate
- The event log stays append-only, sole source of truth (the one-time
  re-encode migration is encoding maintenance, not history mutation).
- JSON chosen for sqlite3-CLI debuggability; zstd absorbs size. If the fixture
  shows >1.5× on-disk growth after zstd, report it — don't switch formats
  unilaterally.
- No dual-codec steady state. One codec, one decode path.
- Do not touch proto wire frames' shape — storage layer only (removing an
  unused postcard dep from proto is fine if verified unused).

## Gates
- `cargo test --workspace` + clippy `-D warnings` + fmt green.
- Fixtures only — do NOT run against `~/.olympus/olympus.db`. The controller
  runs the live-DB migration at deploy.
- Do NOT start/restart olympus services.
- Do not push to main. Green → `blocked: review-required`.
