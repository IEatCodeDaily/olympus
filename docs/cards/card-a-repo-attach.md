# Card A — Repo attach + jj worktree materialization

## Goal
Let an Olympus session attach a git repo at any time (suggested at start, but
editable whenever the session is unbound-or-bound). When a repo is attached, the
session space (`~/.olympus/spaces/<session_id>/`) becomes a **jj worktree of that
repo** instead of a bare empty dir. This completes the "directory system" half of
Olympus: each session gets a scoped working tree, not a grep of the whole host.

This builds directly on commit `2c651e9` (per-session spaces + durable ids). Read
that commit and `crates/control-plane/src/server/bridge_mgr.rs` FIRST — the
`ensure_space`, `space_path`, `remove_space`, and `RuntimeSpec.cwd` machinery
already exists. You are extending it, not rebuilding it.

## Settled decisions (do NOT re-litigate)
- Space root is `~/.olympus/spaces/<session_id>/` — already wired via
  `BridgeManager::with_spaces_root(home.join("spaces"))` in `main.rs`.
- A bare space (no repo) stays an empty dir. The worktree only appears when a
  repo is attached. This is intentional — pure chat sessions need no repo.
- Repo is configurable ANY time (operator's explicit decision: "repo can be
  configured any time, but suggested to be configured at start"). So: allow
  attach via `PATCH /api/sessions/:id` AND via `POST /api/sessions` body.
- Use **jj (Jujutsu)** as the worktree engine — it's already the chosen VCS
  layer per ADR 0004 (committed). `jj` binary is on PATH. A jj worktree is
  created with `jj workspace add` against a colocated repo, OR for the MVP a
  simpler `jj git clone` / `jj workspace` flow — pick the SIMPLEST correct
  approach and document why. If jj is genuinely unavailable or the repo isn't a
  jj/git repo, fail CLOSED with a clear error surfaced to the API — do NOT
  silently fall back to a bare dir (that would hide a misconfiguration).

## Contract discipline (HARD RULE — from AGENTS.md + memory)
`server/dto.rs` is the ONLY place view rows become wire JSON (camelCase). A
contract change touches THREE files together:
1. `docs/api-contract.md` — document the new `repo` field on session + the
   attach behavior on PATCH/POST.
2. Rust DTOs + event + view (`event.rs`, `views/session.rs`, `server/dto.rs`).
3. `ui/src/types.ts` — add `repo` to the `Session` type.
View rows are snake_case and NOT `Serialize`; only the DTO is.

## Concrete steps
1. Add `repo: Option<String>` to `Event::SessionUpdated` (and to
   `Event::SessionCreated` if you allow attach-at-create). Update the postcard
   roundtrip tests + the `StoredVariant` mapping in `log.rs`.
2. Add `repo` to the session view row (`views/session.rs`) — `SessionUpdated`
   patches it like `title`/`model`; `SessionCreated` seeds it.
3. Add `repo: Option<String>` to `SessionDto` (`server/dto.rs`) + `from_row`.
4. Add `repo` to `PatchSessionBody` and `CreateSessionBody` (`server/mod.rs`),
   thread it into the `SessionUpdated`/`SessionCreated` events.
5. On repo attach (in `patch_session` / `create_session`), call a new
   `BridgeManager::attach_repo(session_id, repo_url_or_path) -> Result<PathBuf>`
   that materializes the jj worktree INTO the existing space dir. Idempotent:
   re-attaching the same repo is a no-op; attaching a different repo is an error
   for the MVP (changing repos = fork a new session — note this in the error).
6. `remove_space` already exists; ensure it also tears down the worktree cleanly
   (jj workspace forget if needed).
7. Add `repo` to `ui/src/types.ts` `Session`, and surface a minimal repo field
   in the ChatView composer assign-row (next to Agent/Model) — a text input
   "Repo (optional)" that PATCHes `repo`. Editable while unbound; locked after
   first send like agent/model (reuse the `bound` gate). Add the field to the
   MSW fixtures + handler so e2e/mocks stay green.

## Out of scope (do NOT do)
- Cloning private repos / credential handling — assume local path or public URL
  for the MVP. Note the gap in api-contract.md.
- Changing the repo of a live session (that's a fork — error for now).
- Branch selection UI — default branch only for MVP.

## Verification (REQUIRED before signaling done)
- `make verify` must print `ALL CANONICAL GATES GREEN`. It runs ~2.5min (e2e);
  run it in the background.
- Add Rust unit tests: attach_repo materializes a jj worktree in the space;
  re-attach same repo is idempotent; attach different repo errors; SessionDto
  serializes `repo` as camelCase.
- The standalone patch-tool linter FALSELY reports `async fn is not permitted in
  Rust 2015 (E0670)`. IGNORE it. Verify with real `cargo build` / `cargo test`,
  which use edition 2021 and exit 0.
- Do NOT claim UI works from build alone — but a real-browser pass is the
  controller's job; you just keep e2e + typecheck + build green.

## Signal
When done and `make verify` is green on your worktree, set the card to
`blocked: review-required` with a comment summarizing: files touched, the
attach_repo approach you chose (and why), test names added, and the
`make verify` result. The controller (Zephyr) re-runs the gate on the merged
tree and commits. Do NOT commit or push yourself.

## Attribution (if you do commit in your worktree for the controller to cherry-pick)
Commit trailer: `Authored-by: Zephyr (AI Assistant) <raisalpwardana+zephyr@gmail.com>`
plus `Co-authored-by: <your-profile> (<your-model>) via Olympus swarm`.
Use `git -c commit.gpgsign=false`. Avoid the words "reboot"/"shutdown" in commit
messages (trips a hardline blocklist) — say "restart" instead.
