# ARCH-C · Quarantine the redb legacy log behind a migration-only seam

## Goal
Remove `legacy_log.rs` (redb + postcard StoredVariant machinery, ~1.3k loc) from
the default build. It exists only to migrate the old `~/.olympus/eventlog.redb`
into SQLite (`olympus.db`, ADR 0009). Make it a cargo feature so the default
dependency graph drops redb entirely.

## Read FIRST
- `docs/adrs/0009-redb-to-sqlite-memory-reduction.md` — the migration ADR.
- `crates/control-plane/src/legacy_log.rs` — what you're quarantining.
- `crates/control-plane/src/log.rs` — `migrate_from_redb()` (~line 85) is the
  only production call path into legacy_log. Verify with grep before assuming.
- `crates/control-plane/src/compress.rs` — check what legacy vs live log share.
- `crates/control-plane/src/main.rs` — boot sequence: where migration is invoked.

## Build on
Commit `3fd7d2f` (main).

## Deliverables
1. Cargo feature `migrate-redb` on olympus-control-plane (default OFF... unless
   boot currently auto-migrates when eventlog.redb exists — in that case default
   ON for one release, and document the flip date in the ADR as an addendum).
   Decide by reading main.rs, state your choice in the card summary.
2. `legacy_log.rs` + the redb dependency + any postcard StoredVariant
   stored-shape code used ONLY by it are `#[cfg(feature = "migrate-redb")]`.
3. Boot path: when the feature is off and an un-migrated `eventlog.redb` is
   present, FAIL CLOSED with a clear error telling the operator to run a
   migrate-enabled build — never silently ignore un-migrated data.
4. `cargo tree -p olympus-control-plane | grep -c redb` == 0 with default
   features. Include this proof in your summary.
5. Doc: 2-paragraph addendum in ADR 0009 recording the quarantine + deletion
   window.

## Settled decisions — do NOT re-litigate
- SQLite is the sole source of truth (ADR 0009). No redb revival.
- Do NOT change the live events schema or payload encoding — that is card
  ARCH-D, which runs after you and builds on your worktree result.
- Deletion (not just feature-gating) happens in a later pass — do not delete
  the file in this card.

## Gates
- `cargo test --workspace` (default features) + `cargo test -p
  olympus-control-plane --features migrate-redb` + clippy `-D warnings` + fmt.
- Migration test: build a tiny redb fixture (or reuse existing test helpers in
  legacy_log.rs tests) and prove migrate_from_redb still works under the flag.
- Do NOT start/restart the olympus server or touch `~/.olympus/eventlog.redb`
  (live operator data).
- Do not push to main. Green → signal `blocked: review-required`.
