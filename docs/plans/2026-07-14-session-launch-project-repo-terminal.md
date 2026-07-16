# Session Launch, Project/Repo Context, and Terminal Implementation Plan

> **For Hermes:** Execute this plan through isolated Kanban worktrees only after
> Phase 0 is approved and the integration base is checkpointed. Do not use
> `delegate_task` for implementation. Read-only adversarial review is allowed.

**Status:** PROVISIONAL — architecture workshop draft, 2026-07-14. The operator
was asked how duplicate agents should be grouped and did not answer within five
minutes. This plan therefore provisionally groups by logical agent/profile with
exact node installations nested underneath. Confirm Gate D0 before code.

**Goal:** Restore the topbar search and notification affordances, make session
creation deterministic across the full fleet, let operators curate which agents
appear in the creation dialog, introduce the final context-composer shape,
clarify Project as a reusable context template, add first-class repository
management under Projects, and deliver secure reattachable terminals in Session
and Fleet views.

**Architecture:** Hall remains the sole durable truth and policy authority.
Envoys remain the only components allowed to perform host effects. A launch
target is an exact `(node_id, agent_id)` installation, not a flattened agent id.
Projects are versioned templates that contribute defaults to a versioned Session
Context; repos and vaults remain first-class organization resources. One
Envoy-owned PTY subsystem serves both session-scoped and break-glass node
terminals through different authorization profiles.

**Tech stack:** Rust 2021, SQLite event/product store, JSON/zstd event payloads,
FTS5, axum, tokio, iroh/UDS Hall↔Envoy transport, React 18, TanStack Query and
Router, TypeScript, Vite, Vitest, Maestro/Chromium, `@xterm/xterm` after the PTY
contract is approved.

---

## 1. Source-grounded current state

### 1.1 Topbar

- `ui/src/AppShell.tsx:134` renders an empty `.tb-center`.
- The search pill existed immediately before commit `ded9a53`; the authentication
  change removed it.
- `.tb-search`, palette CSS, and Zustand `paletteOpen` still exist, but there is
  no React command-palette component.
- The approved concept contains both search and notification controls at
  `docs/design/concept/olympus-app-concept.html:361-366`.
- There is no Hall notification entity today.

### 1.2 Agent discovery and launch

- Live production evidence on 2026-07-14 showed two online nodes:
  `terminus` over UDS and `Fx-ZephyrusM16` over iroh.
- Both nodes report overlapping profile ids such as `default`, `coding-agent`,
  `code-reviewer`, and `claude-code`.
- `NodeRegistry::all_agents()` deduplicates by `agent.id` and discards node
  identity (`crates/control-plane/src/node.rs:185-195`).
- `AgentPicker` consumes that flat list and `SessionSidebar` creates with only
  `{agent}`.
- When a session has no node, the first prompt routes to
  `envoy_conns.first_node()` (`server/routes/sessions.rs:1158-1166`). Map
  iteration/connection order therefore decides placement.
- Hall does not validate that the selected exact node is ready, non-draining,
  role-capable, and currently reporting the selected agent before recording a
  session.

### 1.3 Project and session context

- Accepted ADR 0005 defines Project as a collection of configuration and
  federated resource access, not a workdir.
- `ProjectRow` already references vault, repo, and board ids.
- `SessionRow` has `project_id`, but `SessionDto` and `ui/src/types.ts` do not
  expose it.
- Session runtime setup calls `effective_for_project(&org_slug, "")` and therefore
  ignores the attached Project (`server/routes/sessions.rs:1051-1054`).
- Project attachment records `SessionProjectAttached` and attempts a symlink,
  but does not resolve/materialize the project template.
- `ui/src/views/ProjectsView.tsx` is currently the global Kanban board despite
  the surface being named Projects.

### 1.4 Repositories

- Event-backed repo register/list/get/remove routes exist.
- `RepoStore` can clone, fetch, and attach jj workspaces, but explicitly states
  that it is not wired to handlers.
- `POST /api/sessions/:id/repos` only appends `SessionRepoAttached`; it performs
  no Envoy host effect.
- Repo events are not durably organization-owned in their payload/projection.
- There is no wiki companion, replica state, sync policy, issue/PR cursor, cache,
  rate-limit state, or retry model.

### 1.5 Terminal

- `BottomPanel.tsx:304-312` is an honest terminal placeholder.
- Fleet has no terminal affordance.
- Proto has raw-argv JOBS-1 frames but no PTY frame family.
- The UI does not depend on xterm.js yet.
- ADR 0002 §18 requires Envoy PTY ↔ xterm.js and an audited operator capability.
- ADR 0017 forbids exposing arbitrary shell/SSH/argv as an agent operation.

### 1.6 Foundation drift

- `README.md` still describes the removed Convex/Bun architecture.
- `AGENTS.md` and the old long-horizon roadmap still contain redb/tantivy and
  older runtime claims superseded by accepted ADRs.
- The integration tree is heavily dirty. Worktrees created from HEAD will not
  contain the uncommitted runtime, enrollment, Vault, OTel, and session-log work.
- A long-running unrelated Cargo workspace test currently holds the shared Cargo
  lock. Do not bypass or kill it without ownership verification.

---

## 2. Locked architecture doctrine

1. **Hall owns truth and policy.** Launch visibility, project templates, session
   context revisions, repo registrations/sync intent, terminal authorization,
   and durable audit records are Hall-owned.
2. **Envoy owns effects.** Agent spawn, PTY processes, repo clone/fetch,
   workspace materialization, vault mounts, plugin runtimes, and credentials are
   Envoy effects.
3. **Exact launch target.** A human-created session targets an exact online agent
   installation. Production must never route an unbound session to an arbitrary
   first connection.
4. **Availability is not presentation.** `reported`, `ready`, `schedulable`, and
   `launch_visible` are distinct states.
5. **Project is a template.** It is a reusable wrapper around repo/vault/setup
   defaults. It never owns workdirs and does not absorb Repo or Vault identity.
6. **Session owns effective context.** The resolved context used by a runtime
   attempt is versioned and digest-bound. Project edits do not silently mutate a
   live attempt.
7. **Repo truth stays external.** Git history and GitHub issue/PR truth remain at
   their source. Hall owns registration, policy, cursors, bounded query mirrors,
   and observed sync state.
8. **One PTY primitive, two policies.** Session terminal is workspace-scoped;
   Fleet terminal is an explicit break-glass node terminal. Both are human-only.
9. **No shell for agents.** PTY, raw HTTP, raw SSH, and arbitrary argv are not
   added to the agent CLI/MCP operation registry.
10. **Honest recovery.** Browser disconnect may reattach to a live Envoy PTY.
    Envoy loss terminates that PTY; no live process migration is claimed.

---

## 3. Domain model

### 3.1 Agent group and installation

```text
AgentGroupKey = (organization_id, harness_kind, agent_id)
AgentInstallationKey = (organization_id, node_id, harness_kind, agent_id)

AgentGroup
  key
  launch_visible              # Hall policy, default true
  installations[]

AgentInstallation
  key
  provider
  model
  version?
  ready
  reported_at
  connection_epoch
  node_status
  node_roles
  slots_used / slots_total
```

`AgentInstallation` is observed fleet state and may disappear. `launch_visible`
is durable user policy on the logical group, so a profile intended only for
subagents stays hidden when installed on a new node. A later exact-installation
override is deferred until a real use case exists.

If installations grouped under one logical key disagree on provider/model, the
UI must show the divergence. It must not synthesize one canonical model.

### 3.2 Exact session launch request

```json
{
  "launchTarget": {
    "nodeId": "terminus",
    "kind": "hermes",
    "agentId": "default"
  },
  "context": {
    "project": null,
    "overrides": {
      "repos": {"add": [], "remove": []},
      "vaults": {"add": [], "remove": []},
      "skills": {"add": [], "remove": []},
      "plugins": {"add": [], "remove": []}
    }
  }
}
```

Hall validates the target atomically against the current node connection epoch
before recording launch intent. Validation failure returns a typed conflict and
creates no runnable draft.

### 3.3 Project template and Session Context

```text
ProjectTemplate
  project_id
  organization_id
  revision
  name
  repo_refs[]
  vault_refs[]
  skill_refs[]
  plugin_refs[]             # MCP and managed App contributions remain typed
  defaults                  # sandbox/model/resource-policy defaults

SessionContextDesired
  session_id
  project_id?
  project_revision?
  overrides                 # explicit add/remove/replace operations

SessionContextResolved
  session_id
  revision
  digest
  effective_repos[]
  effective_vaults[]
  effective_skills[]
  effective_plugins[]
  effective_policy
  resolved_at

RuntimeAttempt
  ...
  context_revision
  context_digest
```

Merge order:

```text
organization baseline → selected project revision → explicit session overrides
```

Project selection applies defaults. Session overrides may add or remove values.
Capability/resource-policy expansion still requires the appropriate human
principal; a child/subsession can only narrow its parent envelope.

Changing context creates a new desired/resolved revision. A running attempt keeps
its bound digest. Applying the new revision requires an explicit runtime restart
or the next new attempt. The UI exposes "Update from project" rather than
silently following the latest project revision.

### 3.4 Repository model

```text
Repository
  repo_id
  organization_id
  host / owner / name
  clone_url
  default_branch
  credential_ref?
  sync_policy

RepositoryCompanion
  repo_id
  kind = wiki
  clone_url                 # separate <repo>.wiki.git repository

RepositoryReplica
  repo_id + node_id
  desired
  state
  revision?
  last_attempt_id?
  last_success_at?
  error_code?

RepositorySyncCursor
  repo_id + source_kind     # git, github_issues, github_pulls
  opaque_cursor / etag
  observed_at
  rate_limit_reset?

Repo query cache (bounded/disposable)
  issue summaries
  pull-request summaries
  source revision/cursor
  observed_at / expires_at
```

Git and wiki bytes are Envoy-owned replicas. Issue/PR summaries are a bounded
Hall query cache, not an append-only permanent duplicate of GitHub. Registration,
desired replicas, sync policy, cursors, and audit remain durable product truth.
Detailed command output belongs in TTL telemetry.

### 3.5 Terminal model

```text
TerminalRecord
  terminal_id
  organization_id
  target = Session(session_id) | Node(node_id)
  node_id
  actor_id
  policy = session_scoped | node_break_glass
  connection_epoch
  terminal_epoch
  state = opening | running | exited | lost | closed
  cols / rows
  opened_at / ended_at?
  exit_code?
  close_reason?

Terminal attachment
  browser attachment id
  last acknowledged output sequence
  attached_at / detached_at
```

Permanent audit stores metadata, not terminal bytes. Redacted terminal bytes may
enter the ADR 0018 TTL store once implemented. Until then, keep only a strict
bounded in-memory/Envoy replay window and clearly label diagnostics ephemeral.

---

## 4. API and protocol contracts

### 4.1 Agent catalog

Replace the flattened response; do not preserve it as a second truth:

```text
GET /api/agents
  → { groups: AgentGroup[] }

PATCH /api/agents/:kind/:agentId/launch-policy
  { launchVisible: boolean }
  → AgentGroup

GET /api/agents/:kind/:agentId/installations/:nodeId/models
  → models for that exact installation
```

Node Hello continues to report observed agents only. Envoy never sends or owns
launch visibility.

### 4.2 Session launch/context

```text
POST /api/sessions
  { launchTarget, context }
  → 201 Session
  → 409 target_offline | target_stale | target_draining |
        agent_not_reported | agent_not_ready | no_capacity

PATCH /api/sessions/:id/context
  { expectedRevision, project?, overrides }
  → resolved context revision

POST /api/sessions/:id/context/reapply-project
  { expectedRevision }
  → resolved context revision
```

`SessionDto` gains exact launch-target and context summary fields. Do not expose
an ambiguous standalone `agent` without `node` as the authoritative binding.

### 4.3 Projects and repos

```text
GET/POST /api/projects
GET/PATCH/DELETE /api/projects/:id
GET /api/projects/:id/revisions

GET/POST /api/repos
GET/PATCH/DELETE /api/repos/:id
POST /api/repos/:id/sync
GET /api/repos/:id/replicas
GET /api/repos/:id/issues
GET /api/repos/:id/pulls
```

All browser routes are organization-scoped and membership filtered. Current
unscoped repo routes must not be aliased into browser organization routes until
repo ownership is durable.

### 4.4 Terminal REST and WSS

Use a dedicated browser WSS endpoint so terminal backpressure cannot block the
normal application delta stream:

```text
POST /api/terminals
  { target, cols, rows, reason? }
  → { terminalId, state, wsPath }

GET /api/terminals/:id
DELETE /api/terminals/:id

WSS /ws/terminals/:id?after=<output-sequence>
  client: attach, input(bytes), resize(cols,rows), ack(sequence), detach
  server: snapshot, output(sequence,base64), exited, lost, error
```

Hall↔Envoy adds typed PTY frames with exact terminal/node/connection epochs.
PTY output uses bounded chunks and a binary-safe encoding. Control and heartbeat
frames have priority over bulk terminal output. A dedicated iroh/UDS stream is
preferred; if the existing connection is reused, it must have weighted bounded
queues and hostile starvation tests.

---

## 5. UX target

### 5.1 Topbar

- Restore search pill in the center.
- `Ctrl/Cmd+K` opens a real global palette.
- Search queries existing Hall search and includes navigation commands.
- Restore bell icon next to search.
- Bell opens an attention drawer. First useful projection includes:
  input-required sessions, runtime-start errors, offline/draining nodes, and repo
  sync failures. No badge is shown when count is zero.

### 5.2 Agent management

The Agents page groups logical profiles and shows their node installations.
Each group has `Show in New Session` toggle. Installation rows show exact node,
provider/model, readiness, node status, slots, and divergence warnings.

### 5.3 Session creation

One dialog, not a multi-page wizard:

1. Search/filter.
2. Group logical agents by harness, then profile; exact nodes nested beneath.
3. Context selectors: Project, Repositories, Vaults, Plugins (MCP/App), Skills.
4. Effective-context summary with inherited vs override markers.
5. Advanced section for model, sandbox, capabilities, and resource limits.
6. Create remains disabled until one exact ready target is selected.

Placeholder context controls may be visible before their backends land, but they
must be explicitly disabled/"coming next" and use no fake persisted state.

### 5.4 Projects navigation

```text
Projects
  Templates
  Repositories
  Boards
```

- `/projects` lists Project templates.
- `/projects/:projectId` edits the wrapper/template.
- `/projects/repositories` is the organization repo-management page.
- `/projects/boards` preserves the current Kanban surface.

Visual placement under Projects does not change Repo into a child-owned entity.
Projects store references only.

### 5.5 Terminal surfaces

- Session: real Terminal tab in the existing bottom panel, target fixed to the
  session/node/workspace.
- Fleet node detail: `Open terminal` action and terminal pane/dialog clearly
  labeled `Break-glass node terminal`.
- Both use one shared React xterm workbench and reconnect hook.
- Node terminal requires reason entry, short lease, and prominent target label.

---

## 6. Phased task graph

### Phase 0 — Foundation freeze and decision gates

#### Task 0.1: Approve/checkpoint the integration base

**Objective:** Produce one commit/branch point containing the current intended
uncommitted work before creating implementation worktrees.

**Files:** Existing dirty tree; no feature edits.

**Steps:**
1. Inventory every modified/untracked file by feature owner.
2. Verify no secrets/build outputs are included.
3. Run the smallest relevant gates for each accumulated feature.
4. Ask the operator for commit authorization.
5. Commit/checkpoint with the required Terminus trailer.
6. Create all feature worktrees from that exact commit.

**Gate:** `git status --short` is understood and the approved base SHA is recorded
in every Kanban card. No swarm starts from uncommitted prerequisites.

#### Task 0.2: Resolve decision gate D0

**Decisions:**
- Confirm provisional grouping: harness → logical agent/profile → node installs.
- Confirm project snapshot/reapply semantics.
- Confirm Fleet terminal is break-glass and human-only.
- Confirm group-wide launch visibility keyed by `(org, kind, agent_id)`.

**Gate:** Decisions recorded in the ADRs; no implementation card may reinterpret
them.

#### Task 0.3: Repair agent-facing architecture docs

**Files:**
- Modify `README.md`
- Modify `AGENTS.md`
- Modify `docs/architecture/architecture.md`
- Modify `docs/plans/2026-06-29-olympus-long-horizon-roadmap.md`

**Objective:** Remove active Convex/Bun/redb/tantivy and arbitrary first-node
claims so workers do not implement superseded architecture.

**Gate:** Cross-doc source-of-truth matrix agrees with ADRs 0005 and 0011–0019.

#### Task 0.4: Write and adversarially approve three ADRs

**Files:**
- Create `docs/adrs/0020-agent-installations-and-session-context-composer.md`
- Create `docs/adrs/0021-project-templates-and-repository-reconciliation.md`
- Create `docs/adrs/0022-operator-terminal-plane.md`
- Create corresponding reports under `docs/reviews/`

**Gate:** Read-only adversarial review returns READY after corrections.

### Phase 1 — Restore shell parity

#### Task 1.1: Restore functional global search

**Files:**
- Modify `ui/src/AppShell.tsx`
- Create `ui/src/components/GlobalSearchPalette.tsx`
- Create `ui/src/components/GlobalSearchPalette.test.tsx`
- Create `ui/src/components/global-search.css`
- Modify `ui/src/api.ts` only if the existing search helper is insufficient

**TDD acceptance:** Search pill renders; keyboard shortcut opens; Escape closes;
debounced Hall results navigate to a session; empty/loading/error states are
honest; mobile keeps the icon reachable even when the pill collapses.

#### Task 1.2: Restore the notification bell as an attention drawer

**Files:**
- Create `ui/src/components/AttentionCenter.tsx`
- Create `ui/src/components/AttentionCenter.test.tsx`
- Create `ui/src/components/attention-center.css`
- Modify `ui/src/AppShell.tsx`

**Acceptance:** Bell is visible, accessible, and opens a drawer; count is derived
from real query state; zero produces no badge; every item links to its source.
Persistent read/ack is deferred unless the ADR adds a user-state event.

**UI gate:** Focused Vitest, typecheck, build, real desktop/mobile Chromium shots
in both supported themes when color behavior changes.

### Phase 2 — Exact agent installation and launch policy

#### Task 2.1: Add Hall-owned launch policy events/projection

**Files:**
- Modify `crates/control-plane/src/event.rs`
- Create `crates/control-plane/src/views/agent.rs`
- Modify `crates/control-plane/src/views/mod.rs`
- Modify replay/tests

**TDD acceptance:** Policy survives replay; key is organization + kind + agent
id; default visible; Envoy reports cannot overwrite it.

#### Task 2.2: Replace flattened agent catalog API

**Files:**
- Modify `crates/control-plane/src/node.rs`
- Modify `crates/control-plane/src/server/routes/agents.rs`
- Modify `crates/control-plane/src/server/dto.rs`
- Modify `docs/api-contract.md`
- Modify route tests

**TDD acceptance:** Duplicate ids on two nodes produce one logical group with two
installations; provider/model divergence is preserved; stale/offline/draining
state is represented; policy PATCH is organization-scoped.

#### Task 2.3: Make session launch exact and fail closed

**Files:**
- Modify `crates/control-plane/src/server/routes/sessions.rs`
- Modify session event/view/DTO types
- Modify `crates/control-plane/src/server/envoy_conn.rs`
- Add focused integration tests

**TDD acceptance:** Exact node+agent is persisted; stale epoch, missing role,
draining, offline, not-reported, not-ready, and no-capacity each reject before a
runnable session is created; no production first-node fallback remains.

#### Task 2.4: Wire the UI contract

**Files:**
- Modify `ui/src/types.ts`
- Modify `ui/src/api.ts`
- Modify `ui/src/hooks/queries.ts`
- Modify MSW handlers/fixtures and contract tests

**Gate:** Rust/TS contract fixtures serialize identically.

### Phase 3 — Agent management and Session Context composer

#### Task 3.1: Redesign Agents page

**Files:**
- Modify `ui/src/views/sessions/pages/AgentsPage.tsx`
- Create `AgentsPage.test.tsx`
- Create feature CSS

**Acceptance:** Logical groups, exact installations, readiness/state, divergence,
and `Show in New Session` toggle are visible. A hidden profile remains visible on
the management page and in Fleet.

#### Task 3.2: Replace AgentPicker with SessionCreationDialog

**Files:**
- Delete `ui/src/views/sessions/components/AgentPicker.tsx`
- Create `SessionCreationDialog.tsx`
- Create `SessionCreationDialog.test.tsx`
- Modify `SessionSidebar.tsx`
- Create feature CSS

**Acceptance:** Hidden groups excluded; exact node chosen; groups are keyboard
navigable; offline/draining/not-ready installations are shown but disabled with
reasons; context placeholders have final labels and no fake persistence.

#### Task 3.3: Live two-node launch proof

**Acceptance:** Create one session on `terminus` and one on `Fx-ZephyrusM16`;
each runtime starts only on the selected Envoy; refresh and first prompt preserve
placement; hidden agents remain available to subagent orchestration.

### Phase 4 — Project template and resolved Session Context

#### Task 4.1: Expose existing project binding correctly

**Files:**
- Modify `SessionDto` and `ui/src/types.ts`
- Modify API contract and tests
- Remove stale `contextId` semantics where superseded

#### Task 4.2: Add versioned project/setup fields

**Files:**
- Modify project events/view/DTO/routes
- Modify `crates/control-plane/src/views/setup.rs`
- Modify project manifest compatibility code

**Acceptance:** Every update produces a monotonic revision/digest; Project remains
references/defaults only.

#### Task 4.3: Add Session Context desired/resolved events

**Files:**
- Modify event/view/DTO/routes
- Integrate with ADR 0017 runtime-attempt inventory rather than inventing a
  parallel attempt model
- Add context resolver module with pure tests

**Acceptance:** deterministic merge; explicit add/remove; invalid/missing refs
fail closed; resolved digest is stable; runtime attempt binds the digest.

#### Task 4.4: Implement Project templates UI

**Files:**
- Refactor `ProjectsView.tsx`
- Create project template list/detail components and routes
- Move current board page under `/projects/boards`
- Update router tests and Maestro flows

### Phase 5 — Repository management and periodic reconciliation

#### Task 5.1: Make repo identity organization-safe

**Files:**
- Amend repo events/view/DTO/routes
- Add host-qualified immutable repo identity
- Add organization ownership and browser-scoped routes
- Add production event migration/replay compatibility tests

#### Task 5.2: Move repo effects behind Envoy typed providers

**Files:**
- Move/replace control-plane `RepoStore` effect code with Envoy provider code
- Add typed proto requests/results only after JOBS-2 prerequisites
- Add local bare-repo integration fixtures

**Acceptance:** Hall never runs git/jj/gh; Envoy clone/fetch/workspace is fenced,
idempotent by attempt key, and reports retained status.

#### Task 5.3: Add wiki companion and replica reconciliation

**Acceptance:** `<repo>.wiki.git` is modeled as a separate companion; desired
replicas reconcile per node; missing wiki is an honest `not_available`, not an
error loop.

#### Task 5.4: Add bounded GitHub issue/PR mirror

**Files:** New bounded cache module/schema, typed GitHub sync provider, routes,
and tests.

**Acceptance:** cursor/ETag and rate-limit state survive restart; cache has byte
and age bounds; deletions/closures reconcile; no credential appears in events,
logs, or API responses.

#### Task 5.5: Implement Repository page under Projects

**Acceptance:** Register/edit/remove, manual sync, replica status, wiki status,
issues, and PRs are visible. Periodic schedule and next run are explicit.

### Phase 6 — Terminal control plane

#### Task 6.1: Approve PTY wire contract and auth prerequisite

**Gate:** Browser principal has explicit human terminal capabilities. Do not place
Hall bearer credentials in the browser. Exact frame limits, epochs, and priority
queues are approved before implementation.

#### Task 6.2: Implement Envoy PTY supervisor

**Files:**
- Create `crates/envoy/src/terminal.rs`
- Modify Envoy main dispatch
- Modify proto terminal types
- Add Unix PTY/process-group dependency only after manifest review

**TDD acceptance:** open/output/input/resize/exit/close; bounded process groups;
browser detach grace; output sequence/replay; timeout and forced cleanup; no cwd
escape from session profile.

#### Task 6.3: Implement Hall terminal registry and bridge

**Files:**
- Create Hall terminal service/routes
- Modify Envoy connection routing
- Add dedicated terminal WSS handler
- Add permanent audit events and TTL hook seam

**TDD acceptance:** authorization by target, exact Envoy epoch, reattach cursor,
output bounds, terminal loss on Envoy restart, no heartbeat starvation under
terminal flood.

### Phase 7 — Terminal UI

#### Task 7.1: Add shared xterm workbench

**Files:**
- Add `@xterm/xterm` and fit addon to `ui/package.json`
- Create shared TerminalWorkbench component/hook/tests/CSS

#### Task 7.2: Wire Session terminal

**Files:** Modify `BottomPanel.tsx` and tests.

**Acceptance:** fixed session target/cwd; connect/reconnect/resize; exit and lost
states; no arbitrary node selector.

#### Task 7.3: Wire Fleet break-glass terminal

**Files:** Modify `FleetView.tsx` or split NodeDetail into owned page components.

**Acceptance:** explicit warning/reason; short lease; target always visible;
close action; no terminal for unauthorized users.

### Phase 8 — Integration, deployment, and evidence

1. Focused tests during each card.
2. Serialized canonical gate:
   `flock ~/.cache/olympus-cargo.lock env CARGO_TARGET_DIR=$HOME/.cache/cargo-target/plain CARGO_BUILD_JOBS=1 cargo test --workspace -j 1`.
3. `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` under the
   same serialization discipline.
4. UI Vitest, typecheck, build, and Maestro serially.
5. Real Chromium journeys at desktop and 412×915 mobile.
6. Live two-node exact launch and hidden-agent proof.
7. Live session terminal and Fleet terminal on both UDS and iroh nodes.
8. Disconnect/reconnect, Envoy restart, terminal flood, stale connection epoch,
   and repo sync failure hostile tests.
9. Backup production SQLite before migrations; dry-run copied DB; integrity
   check; deploy Hall then Envoys; wait for fresh Fleet readiness.
10. Store screenshots/video under `docs/evidence/` and backend diagrams under
    `docs/diagrams/`.
11. Seven-day soak begins only after all prerequisite gates pass.

---

## 7. Swarm topology and file ownership

One controller owns shared contracts and merge order. Workers never share a
worktree.

```text
S0 foundation/docs/ADRs
  └─ S1 shared contract spine (events/proto/DTO/API contract)
       ├─ S2 Hall launch backend
       ├─ S3 Agents management UI
       ├─ S4 Session creation UI
       ├─ S5 Project context backend
       ├─ S6 Project/Repo UI
       ├─ S7 Envoy repo providers
       ├─ S8 Envoy PTY supervisor
       └─ S9 Hall terminal bridge
             ├─ S10 Session terminal UI
             └─ S11 Fleet terminal UI
  └─ S12 adversarial integration review
  └─ S13 canonical gate + live evidence + deployment
```

Safe parallelism after S1:

- S2, S5, S7, and S8 have separate module ownership.
- S3 and S4 may run in parallel only if they use separate component/CSS files;
  the controller owns `types.ts`, `api.ts`, query keys, and shared shell imports.
- S6 waits for the Project/Repo contract but can build mock-first components.
- S9 waits for the PTY proto contract, not for the Envoy implementation to finish.
- S10/S11 wait for shared TerminalWorkbench and Hall WSS shape.

Shared hotspots owned only by the contract/controller lane:

- `crates/control-plane/src/event.rs`
- `crates/proto/src/frames.rs` or replacement typed terminal modules
- `crates/control-plane/src/server/dto.rs`
- `docs/api-contract.md`
- `ui/src/types.ts`
- `ui/src/api.ts`
- `ui/src/hooks/queries.ts`
- `ui/src/index.css`
- `ui/src/router.ts`

Feature workers create local modules and CSS files; they do not independently
edit these hotspots.

Before card creation run `hermes profile list` and use only profiles that exist.
Use goal-mode only for bounded long implementation cards with explicit acceptance
criteria. Keep a tmux dispatcher/steward loop and durable status files; do not rely
on Studio bridge process handles.

---

## 8. Review and acceptance matrix

| Surface | Required proof |
|---|---|
| Topbar | Search keyboard/mouse navigation; bell opens real attention state; desktop/mobile screenshots |
| Agents | Two nodes with duplicate ids remain distinct; visibility survives Hall restart |
| New Session | Exact target persisted; every invalid state fails before spawn; inherited/override context shown |
| Project | Template revision and session resolved digest are reproducible; no live silent mutation |
| Repo | Hall metadata/cursors durable; Envoy effects fenced; wiki separate; issue/PR cache bounded |
| Session terminal | Correct cwd/node; reattach after browser loss; resize/input/output; Envoy loss honest |
| Fleet terminal | Explicit break-glass auth/reason/lease/audit; inaccessible to agents |
| Transport | Terminal flood cannot starve heartbeat, ACK, session prompt, or control traffic |
| Deployment | copied-DB migration dry run, backup, integrity, health, Fleet fresh epoch, browser evidence |

---

## 9. Explicit non-goals for this wave

- No direct SSH API for agents.
- No `olympus exec` or arbitrary terminal/argv operation in the agent CLI/MCP.
- No live process migration between Envoys.
- No project-owned duplicate Repo or Vault objects.
- No automatic mutation of running attempts when a Project changes.
- No permanent unbounded issue/PR or terminal-output log.
- No raw GitHub tokens in Hall events, API responses, telemetry, or repo config.
- No second scheduler or ad hoc cron loop outside the accepted workflow/job seam.
- No compatibility preservation for the broken flattened agent-launch contract;
  development-phase code should delete the nondeterministic path.

---

## 10. Immediate next batch after approval

1. Finish the three adversarial reports and revise this plan/ADRs.
2. Resolve D0 with the operator.
3. Audit and checkpoint the dirty integration tree with explicit commit approval.
4. Repair the stale agent-facing architecture docs.
5. Seed only Phase 1 and Phase 2 contract cards first.
6. Do not seed Project/Repo or Terminal implementation until their ADRs and the
   ADR 0017/JOBS-2 prerequisites are explicitly green.
