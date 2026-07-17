# 0045 — Per-worktree Rust targets exhausted the dev host disk

Date: 2026-07-17 · Severity: high (build and Git writes failed) · Author: Terminus

## Symptom

Concurrent Olympus lanes repeatedly drove fxcompute-01 to 97–98% disk usage.
Git failed to write `.git/index.lock` with `Out of diskspace`; Rust and UI gates
failed with ENOSPC. Individual worktrees held 5–10 GB `target/` trees containing
mostly duplicate dependencies.

## Root cause

The workflow serialized some Cargo commands but did not standardize
`CARGO_TARGET_DIR`. Every isolated worktree therefore built a complete target
tree. Serialization limited CPU contention but did nothing about duplicated
artifacts. Completed branches also left reproducible targets behind.

## Fix

The agent runbook now requires one shared fxcompute target and lock:

`CARGO_TARGET_DIR=$HOME/.cache/olympus-cargo-target flock $HOME/.cache/olympus-cargo.lock cargo test -j2 -p <crate>`

Inactive targets are removed only after checking that no live `cargo`/`rustc`
command references them. During recovery, verified inactive main and completed
branch targets reclaimed more than 20 GB without touching service artifacts or
active builds.

## Prevention

- Every worker card and runbook uses the same target and lock.
- Disk checks precede broad Rust gates on the constrained dev host.
- Completed worktree cleanup includes its inactive target.
- The dev Hall retains its separate service target under `/var/lib/olympus` so
  test cleanup cannot remove the running binary.
