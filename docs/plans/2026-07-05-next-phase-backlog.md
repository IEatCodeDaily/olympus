# Olympus — State Analysis & Next-Phase Backlog

> Date: 2026-07-05 · Author: Zephyr · Ground truth verified against `main` @ `8cec5bd`

## Part 1 — Current state (verified, not assumed)

### ✅ Working & verified

| Area | State | Evidence |
|---|---|---|
| Sessions core | Chat, model passthrough, spinner, optimistic dedupe, error persistence | `5e7fd7a`, live-tested |
| Pin / archive | Manual pin (user-only), archive, PATCH `/api/sessions/:id` | `a8fb9ae`, e2e green |
| Recent-5 + History | Sidebar caps at 5, History data table w/ node/agent/channel/time filters | `a8fb9ae`, `dcd2628` |
| Agent detection | `ready` flag via credential probe (`~/.codex/auth.json`, `~/.claude/.credentials.json`) | `a8fb9ae` |
| Vaults: notes CRUD | jj-colocated markdown, frontmatter, wikilinks | live-tested via curl |
| Vaults: content addressing | BLAKE3 `cid` injected in frontmatter on write | `1d193cf`, live-verified |
| Vaults: graph | `GET /api/vaults/:id/graph` + force-directed canvas UI | `8cec5bd`, live-verified (3 nodes/3 edges) |
| Vaults: collections | frontmatter `collection: true` → sortable table UI | live-verified |
| Projects (context container) backend | `projects.rs`: create/update/read/delete, `attach_symlink` into session space; full event set | `6549616` |
| E2E pipeline | 48 mocked tests + evidence bundles (MP4/screenshots) + live smoke tier + CI + nightly cron | `5045e58`, `2ace824` |
| Kanban board UI | `ProjectsView.tsx` (misnamed — it's the card board) | working |

### 🔴 REGRESSION found during this analysis

**The repos API routes are GONE from main.** Commit `4533616` added
`/api/repos` (list/register), `/api/repos/{slug}` (get/remove),
`/api/sessions/{id}/repos` (attach) + handlers. The manual merge in `6549616`
**removed** them from `server/mod.rs` while keeping `repos.rs`, `views/repo.rs`,
and the `RepoRegistered`/`SessionRepoAttached` events. The module compiles but
is dead code — zero routes reference it.

**Subsessions were never merged.** `518f6d9` (`wt/subsessions` branch) has the
three routes (`POST/GET /api/sessions/:id/subsessions`, `POST :id/complete`
check gate) — `git merge-base --is-ancestor` confirms it is NOT in main.
The `6549616` commit message claims it was merged; it wasn't. Only the
projects half of that reconciliation landed.

### 🟡 Present but incomplete

| Area | Gap |
|---|---|
| Vault editor | Textarea still; CodeMirror deps installed but not wired |
| Projects | Backend only — no UI, no vault/repo/board **bindings** (manifest holds name only) |
| Repo wiki | Nothing exists (no `<repo>.wiki` management at all) |
| UI naming | `ProjectsView.tsx` = kanban; real Projects (context container) has no surface |
| Subsession test | `list_subsessions_returns_only_direct_children` was `#[ignore]`'d on the branch |
| 29 stale worktrees | `.worktrees/` has 29 dirs from past kanban runs |

## Part 2 — Next phase: "Orchestrator-ready"

Goal (user's words): *"implement these features until it's possible for you to
create subagents using olympus — we want to move over to olympus as the
orchestrator platform."*

The critical path to that goal is: **subsessions (spawn+gate) → repos (jj
workspaces) → projects (context binding) → wiki (derived knowledge)**. Vault
polish and editor upgrades are off the critical path.

### P0 — Restore what was lost (small, immediate)

**B-1. Re-land repos routes** — cherry-pick the route/handler block from
`4533616` into current `server/mod.rs` (the store, views, events are all still
there; only the axum wiring is missing). Add a regression test that asserts
`GET /api/repos` returns 200, so a future merge can't silently drop it again.

**B-2. Merge `wt/subsessions` (518f6d9)** — rebase onto main, resolve
`server/mod.rs` conflicts, fix the ignored test (`create_session` returns 200
not 201 in the fixture), remove `#[ignore]`. This gives: spawn child session,
list children, complete-with-verdict check gate.

**B-3. Merge-reconciliation guard** — the root cause of B-1 was hand-grafting
diffs between worktrees. Add `tests/routes_contract.rs`: one test that walks
the router and asserts the full expected route table. Any dropped route fails
the build. Cheap insurance, kills the whole failure class.

### P1 — Orchestration loop (the actual goal)

**B-4. Repo attach → jj workspace materialization.** `attach_repo` currently
only records the event. Implement: on attach, `jj workspace add` (or clone
then workspace) from `~/.olympus/<org>/repos/<slug>` into the session space.
On subsession spawn with a repo-attached parent, create a nested workspace.
This is the isolation primitive everything else rides on.

**B-5. Subsession spawn from agent tooling.** Expose spawn/list/complete as
an agent-callable surface (bridge tool or documented curl contract in the
session's system context). Acceptance test: an Olympus-managed session spawns
a child, the child does work in its jj workspace, completes with `pass`, the
parent receives the system message. **This test passing = "Olympus can create
subagents" = phase goal met.**

**B-6. Projects as real bindings.** Extend `ProjectManifest` to
`{ vaults: [], repos: [], boards: [] }`; `PATCH /api/projects/:id` accepts
them. On session attach: symlink project dir (exists) **plus** materialize
bound repos as jj workspaces and symlink bound vaults. Session create accepts
`project_id`.

**B-7. Projects UI + naming fix.** Rename `ProjectsView.tsx` → `BoardsView.tsx`
(it's the kanban). New thin ProjectsPage: list projects, create, bind
vault/repo/board via pickers, show attached sessions. Also fixes the topbar
chip naming per the Olympus View→Page hierarchy.

### P2 — Derived knowledge (after the loop closes)

**B-8. Repo wiki v1.** On repo register (and on-demand refresh): create
`<slug>.wiki` as a sibling jj repo; generate `README.md` summary, file-tree
index, and symbol outline (tree-sitter or ctags — decide at implementation).
Session with attached repo also gets a `.wiki` workspace. Keep generation
pluggable — the "AST/indexing and many more" part grows over time.

**B-9. CodeMirror editor upgrade.** Deps already installed. Swap textarea →
CodeMirror 6 (markdown lang, one-dark), add `[[` wikilink autocomplete from
the note tree. Contained change to `NotePage.tsx`.

**B-10. Housekeeping.** Sweep stale `.worktrees/` (keep last N or completed
cards only); wire vault graph/collections e2e specs into the mocked suite
(mock handlers already written).

### Sequencing

```
Week focus 1: B-1 → B-3 → B-2         (restore + guard, all small)
Week focus 2: B-4 → B-5               (isolation + spawn loop = GOAL GATE)
Week focus 3: B-6 → B-7               (projects binding + UI)
Then:         B-8 → B-9 → B-10        (wiki, editor, cleanup)
```

Dependencies: B-2 blocks B-5. B-1 blocks B-4 blocks B-5/B-6. B-7 needs B-6.
B-8 needs B-4. Nothing blocks B-3/B-9/B-10 — they're fill-in work.

### Explicitly deferred (with reasons)

- **cr-sqlite structured vault data** — collections-from-frontmatter covers
  current need; add the engine when multi-writer table merge is actually hit.
- **iroh sync / multi-node vaults** — single-node correctness first.
- **Obsidian-grade graph physics** — current force layout is adequate.
- **Milkdown/WYSIWYG** — ADR 0004 keeps editor swappable; CodeMirror is the
  right step, WYSIWYG is not requested.
