# ARCH-C · DELETE the redb legacy log (no quarantine — remove it)

## Goal
Operator directive: we are in development phase — nuke superseded architecture
outright. Delete `legacy_log.rs`, the redb dependency, and every postcard
StoredVariant stored-shape used only by the legacy log. The live store is
SQLite `olympus.db` (ADR 0009); the redb file is a fossil.

## Read FIRST
- `docs/adrs/0009-redb-to-sqlite-memory-reduction.md`.
- `crates/control-plane/src/legacy_log.rs` — what you're deleting.
- `crates/control-plane/src/log.rs` — `migrate_from_redb()` (~line 85) and its
  call site(s) in `main.rs`/boot. Grep the whole workspace for `legacy_log`,
  `redb`, `StoredVariant` before assuming the blast radius.
- `crates/control-plane/src/compress.rs` — keep whatever the LIVE log uses;
  delete only legacy-exclusive helpers.

## Build on
Commit `3fd7d2f` (main).

## Deliverables
1. Delete `legacy_log.rs`, `migrate_from_redb()`, its boot call path, and the
   `redb` dependency from Cargo.toml (workspace + crate). Delete
   legacy-exclusive postcard stored-shape code. `postcard` may remain a dep
   ONLY if the live log still encodes with it (ARCH-D, a later card, removes
   that too — do not do ARCH-D's work here).
2. Boot behavior when `~/.olympus/eventlog.redb` exists: log ONE warning naming
   the file as obsolete and ignore it. Never fail, never read it.
3. Proof in summary: `cargo tree --workspace | grep -c redb` == 0; workspace
   grep for `legacy_log|migrate_from_redb` == 0 outside docs.
4. ADR 0009 addendum (short): legacy log deleted at this commit; the redb file
   on existing dev installs is inert and can be manually removed.

## Settled decisions — do NOT re-litigate
- No feature flag, no migration bin, no deprecation window — deletion.
- SQLite is the sole source of truth. No redb revival.
- Do NOT change the live events schema/encoding — that is ARCH-D.

## Gates
- `cargo test --workspace` + clippy `-D warnings` + fmt green.
- Do NOT touch `~/.olympus/` (live operator data). Do NOT start/restart
  olympus services.
- Do not push to main. Green → signal `blocked: review-required`.
