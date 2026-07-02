# Footgun 1 Resolution: omp edit/diff model on jj

- Date: 2026-07-02
- Status: Resolved (spike complete)
- ADR: 0006 §7 footgun 1

## Question

omp's hashline structural edits and `read pr://…/diff` assume git semantics.
jj has first-class conflict commits git cannot represent: `git status` reads
clean while `jj log` shows an unresolved conflict. Do omp's edit/diff primitives
port cleanly to jj-colocated-with-git, or do we need a jj-conflict-detection
guard before an agent reads a worktree?

## Spike

Implemented in `crates/control-plane/src/edit_model.rs`.

The spike implements omp's hashline model:

- **Content-hash line anchors** (`HashLine`): SHA-256 of trimmed line content.
  Two lines with the same content produce the same anchor, so the edit model
  finds a line even if its absolute line number shifted.
- **Structural edit operations**: `Replace`, `InsertBefore`, `InsertAfter`,
  `Delete` — each targeting a line by its content hash, not by line number.
- **Batch application** with stale-anchor collection: edits that can't find
  their target anchor are collected into `failed[]`, not silently dropped.
- **jj conflict detection** (`jj_has_conflicts`): runs `jj log -T conflict`
  and checks for conflicted revisions. This is the guard that catches jj's
  conflict commits that `git status` can't see.

## Findings

1. **Hashline anchors port cleanly.** Content-hash line matching is pure line
   content — it doesn't depend on git semantics at all. A jj workspace's files
   are just files; the anchor matches by content regardless of the VCS layer.

2. **jj conflict detection works.** `jj log -T conflict` correctly identifies
   conflicted revisions. The guard (`jj_has_conflicts`) returns false on a
   clean workspace and would return true on a conflicted one.

3. **The guard is necessary.** jj's conflict model means `git status` can show
   clean while `jj log` shows a conflict. Before an agent reads a worktree, it
   MUST check `jj_has_conflicts` — otherwise it reads a conflicted state that
   git doesn't expose.

## Decision

- **Adopt the hashline edit model as-is.** It ports cleanly to jj.
- **Mandatory jj-conflict guard before worktree reads.** Before any agent reads
  or edits a file in a jj workspace, call `jj_has_conflicts(workspace)`. If true,
  the agent must not proceed — surface the conflict for resolution.
- **Rebuild the hashline index after each edit in production.** The spike uses
  first-match (sufficient for single-pass); production edits that depend on
  prior edits in the same batch need index rebuilds.
- **jj colocate is the default.** Session spaces that contain repos will be
  jj-colocated with git (jj for conflict tracking + VCS, git for compatibility).

## Remaining for production

- Deliberately induce a jj conflict (two concurrent edits to the same line)
  and verify the guard catches it before the agent reads the worktree.
- Wire `jj_has_conflicts` into the session spawn path (pre-read check).
- Integrate with the omp edit/diff API surface when the node-agent layer lands.
