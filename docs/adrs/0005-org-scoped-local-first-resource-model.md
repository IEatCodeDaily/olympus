# ADR 0005: Org-scoped local-first resource model (supersedes ADR 0002 §3 & §5)

- Status: Accepted
- Date: 2026-07-01
- Supersedes: **ADR 0002 §3** (Identity/Context/Session model) and **§5**
  (Filesystem hierarchy) in their entirety. Those sections describe a
  `~/olympus/sessions/<id>/` flat layout with a "context" isolation boundary
  and soft/convention-enforced isolation. **That model is withdrawn.**
- Keeps intact: **ADR 0003** (Rust-native substrate — redb event log,
  single-writer scheduler, iroh/UDS transport) and **ADR 0004** (vaults are
  markdown-first, jj is the merge engine). This ADR relocates and re-scopes
  *where* those resources live, not *how* they are implemented.
- Relates to: ADR 0002 §2 (layer boundary), §15 (workflows), §17 (artifacts).

> **This ADR is authoritative for the organization-scoped resource model.**
> Where ADR 0002 §3/§5 and this document disagree, this document wins, except
> for the session-space and invocation-graph amendments below.
>
> **Partially superseded by ADR 0027 (2026-07-19).** Session spaces remain under
> `~/.olympus/<org>/sessions/`, but every session—including subagents—now has one
> flat `<session_id>/` directory. ADR 0027 replaces §3's nested-session wording,
> §4's nested tree, and §4.2's nesting rule. Hall's typed invocation graph is
> authoritative for lineage; directories are not.

> **Partially superseded by ADRs 0024–0026 (2026-07-17).** Organization remains
> the hard ownership/isolation boundary and Project, Repo, Vault, and Session
> remain distinct resources. ADR 0024 now owns external connections,
> credentials, grants, and Secret Store boundaries. ADR 0025 makes Repo a
> durable provider-bound resource. ADR 0026 requires each Project to reference
> one same-org home Vault, makes Board durable, and replaces the “project has no
> working files,” optional-vault, and “two databases” summaries with its
> normative project-content layout and authority matrix. A Project references
> its home Vault; it does not own or delete the Vault.

## 1. Why this supersedes §3 and §5

ADR 0002 introduced a **"context"** isolation layer ("personal / noovoleum-rde /
noovoleum-mkt") with soft, convention-enforced isolation (§3.2: "hard at the
data layer, soft at the reasoning layer"). Two problems made it wrong in
practice:

1. **The soft boundary doesn't actually isolate.** Credentials and network
   egress are enforced, but "context" was a label on shared processes — two
   contexts on one host shared one envoy, one network namespace, one process
   tree. Real isolation wants a **process boundary**, not a label.
2. **The flat session layout (`~/olympus/sessions/<id>/`) collided with the dev
   reality** that `~/olympus/` is where the source repo lives, and provided no
   org-level grouping or ownership scoping for resources.

This ADR replaces "context" with **organization** (a real process boundary —
one envoy per org per node, not one per node), and gives the entire tree an
org-scoped, slug-addressed layout under the dotted `~/.olympus/` root.

## 2. The two roles (unchanged in spirit, sharpened)

| Role | Is | Owns | Runs |
|---|---|---|---|
| **Olympus (control plane)** | The authority + source of truth + UI viewport | durable records: org/project/session/repo/vault/workflow config, session *duplicates*, tombstones, audit | no agent loop; orchestrates |
| **Envoy (node agent)** | The host-effect executor + local-first runtime | the physical files on its node: session spaces, cloned repos, jj workspaces, materialized creds, plugin runtimes, the local sqlite reconciliation index | spawns/supervises agent processes, bwrap sandboxes, jj, the actual work |

**Hard rule (from ADR 0002 §2, restated):** orchestration/state → control plane;
any host effect (process, file, PTY, port, install, sandbox) → that host's
envoy. The control plane never spawns an agent directly; it records intent and
the envoy realizes it on the node. **For the local single-node case, the envoy
is a logical role fulfilled inside the control-plane process** (not a separate
daemon yet); the boundary is preserved so multi-node is additive, not a rewrite.

## 3. The vocabulary (locked — replaces ADR 0002 §3.1)

| Concept | Definition |
|---|---|
| **Organization** | The resource owner: nodes, data, config, secrets, budget. The hard isolation boundary. One org = one envoy per node (two orgs on a host = two envoys, two cred sets, two netns). Replaces ADR 0002 "context." |
| **Project** | A collection of configuration + federated resource access: bundles session/repo/vault attachments and access policy. Exists to scope access later. A project belongs to exactly one org. |
| **Session** | A session space — the working directory where an agent is initiated. Main session or sub-session (nested, max depth 4). One agent per session; sub-sessions are how you get a *different* agent into the same workspace. |
| **Repo** | A git/jj repository used by the org. Canonical under `~/.olympus/<org>/repos/...`; materialized into session spaces as jj workspaces on demand. |
| **Vault** | A knowledge + data store dedicated to knowledge and data (markdown-first per ADR 0004). Distinct from a project so access control can diverge later. |
| **Workflow** | Org-owned automation definition (config, env vars, templates). Some run on all nodes, some olympus-only. Agents invoke workflows (e.g. a "review → iterate → PR → webhook-watch" loop). |
| **Artifacts** | Olympus-managed, agent-generated outputs (static HTML, long-lived docs, plugged-in subsystems). **Discouraged — prefer vaults.** Periodically scrubbed by olympus. |
| **Plugins** | Subsystems Olympus manages that require a runtime: a receipt/accounting module, CRM, MCP servers, OR a host-level install (gitnexus, CLIs). Declared `kind: install \| service`. |

**Removed from the vocabulary:** "context" (→ organization), "identity layer"
(folded into the operator's cross-org persona, not a path segment).

**project vs vault, precisely:** today they are structurally similar (both are
collections under an org). The split is a **forward compatibility seam for
access control**: a project may federate access to repos/vaults/sessions;
a vault is a dedicated knowledge/data store. Separating them now is cheap;
merging them later would require touching every access rule. Keep them apart.

## 4. Filesystem hierarchy (normative — replaces ADR 0002 §5.1)

Everything lives under the **dotted** `~/.olympus/` root (the same root that
holds the control plane's internal state: event log, search index, token). This
avoids the collision with a source checkout at `~/olympus/`.

```text
~/.olympus/                                # olympus-managed root (internal state + all resources)
├── eventlog.redb                          # control-plane append-only log (internal)
├── search-index/                          # tantivy FTS (internal, derived)
├── token                                  # bearer token (internal, 0600)
├── <org_slug>/                            # ── ORGANIZATION ──
│   ├── org.json                           # org config + secrets refs
│   ├── nodes/                             # node registrations for this org (iroh NodeIds)
│   │   └── <node_slug>.json
│   ├── projects/
│   │   └── <project_slug>/                # PROJECT — collection of config + federated access
│   │       ├── project.json               # attached repo/vault/session refs, access policy
│   │       └── (no working files — those live in repos/vaults/sessions)
│   ├── sessions/
│   │   └── <session_id>/                  # SESSION SPACE (main session)
│   │       ├── session.json               # → agent, node, projectId?, repo refs, bwrap profile, R/W access
│   │       ├── repos/<repo_org>/<repo_name>/   # jj workspace (workspace-add'd from the org origin repo)
│   │       ├── project -> ../../projects/<project_slug>/   # symlink if project-bound (bind-mounted into sandbox)
│   │       └── sessions/<sub_session_id>/      # SUB-SESSION (nested, same shape, max depth 4)
│   │           └── ...
│   ├── repos/
│   │   └── <repo_org>/<repo_name>/        # ORIGIN repo (canonical jj history; sessions workspace-add from here)
│   ├── vaults/
│   │   └── <vault_slug>/                  # VAULT (markdown-first per ADR 0004; jj repo)
│   │       ├── docs/
│   │       └── .vault/vault.db
│   ├── workflows/
│   │   └── <workflow_slug>/               # WORKFLOW (config, env, templates)
│   ├── artifacts/
│   │   └── <artifact_slug>/               # ARTIFACT (scrubbed periodically; tombstoned on delete)
│   └── plugins/
│       └── <plugin_slug>/                 # PLUGIN (install script OR service; manifest declares kind)
│           └── plugin.json
└── ...
```

### 4.1 Slug-as-identity (immutable primary key)

Slugs are the primary key. **A slug is locked at creation and cannot be
renamed.** To "rename" a resource, the user recreates it with a new slug and
migrates references. This trades rename ergonomics for:

- **LLM-friendly addressing** — a slug like `noovoleum-rde` is readable and
  typeable in chat; a UUID is not.
- **Trivial directory addressing** — the path *is* the identity; no
  indirection table from id→directory.
- **No rename-detonation** — since slugs never change, no symlink/ref rewrite
  on rename (there are no renames).

Consequence: the local sqlite in each node is **not** an id→path resolver. It
is a **reconciliation index** (what's synced here, what's pending, jj/bwrap
runtime state) — a materialized projection of the control-plane truth for
local-fast queries. It is never the source of truth.

### 4.2 Session spaces

- A session space is created **eagerly on session creation** (the moment a new
  session is recorded). It is the agent's working directory.
- **Main sessions** get a top-level `<org>/sessions/<id>/`. **Sub-sessions**
  nest under their parent as `<parent>/sessions/<sub_id>/`, max depth 4. The
  whole tree lives under one `<org>/sessions/<main_id>/` root on one node.
- **Agent is chosen at session creation** (the user picks the agent). **Node is
  inferred from the agent** (the agent runs on its configured node). The agent
  is **locked at creation** — the only way to get a different agent into the
  same workspace is a sub-session. So the **session id must not bake in the
  node** (it isn't known at creation time under the original ADR model; here it
  *is* known, because node is derived from the chosen agent — but the id stays
  node-free for portability and is `<datetime>-<hash>` or slug-friendly).
- **Repos** materialize as jj workspaces inside the session space at
  `sessions/<id>/repos/<repo_org>/<repo_name>/`, workspace-add'd from the org's
  origin repo at `repos/<repo_org>/<repo_name>/`. If the origin isn't present
  on the node yet, the envoy clones it first, then workspace-adds. Same on-demand
  materialization for project symlinks and vault access.

### 4.3 Sandboxing: bubblewrap + bind-mounts

Each session may run in a **bubblewrap (bwrap) sandbox** if configured. The
sandbox profile is in `session.json`. Two requirements: **network isolation**
(per-session netns) and **file access control** (per-path R/W). 

The envoy builds the bwrap and **bind-mounts** the configured paths (the
session space, the project symlink target, vault refs) with the configured R/W
mode. A symlink inside the session space is the *host* view; inside the sandbox
it must be a bind-mount or it dangles. **Default profiles:** the org's default
agent (e.g. Hermes default profile) may have direct host access; configured
agents (Claude Code, Codex, Hermes sub-agents) run sandboxed.

### 4.4 Origin repo host-qualification

`repos/<repo_org>/<repo_name>` must be host-qualified to avoid collisions
across git hosts (`github.com/foo/bar` ≠ `gitlab.com/foo/bar`). The origin repo
path is `repos/<host>/<org>/<repo>` or a slugged equivalent; the two-segment
`<org>/<repo>` shorthand above is for readability and assumes github.com as the
default host.

## 5. Local-first, one-way sync

| Resource | Sync model |
|---|---|
| **Organization** | Node connection is owned per-org. One envoy per org per node. Two orgs on a node = two envoys. |
| **Project** | Synced based on the agents configured to it — infer the node from the agent's location; sync the project to that node. |
| **Session** | **One-way: node → olympus.** Olympus stores a duplicate of the session for cross-node search. Cross-node session search for an unsynced session goes *through* olympus via an MCP/CLI call from the node's agent (e.g. "user mentions session about X → agent searches locally → misses → calls olympus search"). The agent is never blocked by sync delay. |
| **Repo** | Synced on demand — when a repo is attached to a project or session, the node clones/workspaces it. |
| **Vault** | Synced on demand — when attached to a project or session. (Per ADR 0004, jj is the sync engine.) |
| **Workflow** | Org-owned. Sync is config-driven: some workflows must exist on all nodes, some are olympus-only. Agents invoke workflows. |
| **Artifacts** | Synced to all nodes. Because they're discouraged (→ vault), they are **periodically scrubbed**. Deletion requires a **tombstone** until all nodes confirm scrub. |
| **Plugins** | Synced per node — olympus manages which plugins each node needs (e.g. every node needs gitnexus). |

**Olympus only stores a duplicate of the session.** Actual session files and
artifacts live locally on the node where the envoy is. For an active user
session with envoy installed, streaming is **P2P envoy→user client**; without
envoy (web-only), the user still gets a viewport but the agent runs on whatever
node has an envoy. The agent is never blocked by node→olympus sync delay.

## 6. New-session creation flow (exact)

1. Olympus records the session (mints id) — **no space yet, no agent yet.**
2. User chooses an agent. **Node is inferred from the agent** (agent → its
   configured node). The agent is locked at this point.
3. Envoy (on the inferred node) spins up the session space
   `<org>/sessions/<id>/`.
4. If repo(s)/project are attached and not yet on the node, the envoy sets them
   up (clone origin repo → jj workspace-add into the session space; create
   project symlink; materialize vault access).
5. Envoy builds the bwrap sandbox (if configured) and spawns the agent inside
   the session space as its cwd.

## 7. The two databases (ownership rule)

- **Control-plane redb event log** — owns durable org/project/session/repo/
  vault/workflow/plugin *records* and config. The source of truth for what
  *exists* and its *configuration*.
- **Node local sqlite** — envoy-managed reconciliation index: what's synced
  here, what's pending, jj workspace state, bwrap profile cache, PID/ports.
  The source of truth for "what's physically on this disk" only.

**Rule:** for synced entities, **control plane wins**; for "what's physically
on this disk," the **node wins**. The sqlite is never an alternative source of
record for entity identity (slugs hold that).

## 8. Consequences

- **Gained:** real process-boundary isolation (org = envoy = cred set = netns),
  not a label. Two orgs on a host are isolated by construction.
- **Gained:** org-scoped paths mean no collision with source checkouts, and
  natural multi-tenancy at the filesystem layer.
- **Gained:** slug-addressing is LLM-native (typeable in chat, greppable).
- **Cost:** slugs are immutable; "rename" = recreate + migrate refs. Accepted —
  rename is rare and LLM-friendliness outweighs it.
- **Cost:** the node sqlite must be kept in sync with the control plane for
  reconciliation; stale node indexes are self-healing (re-derive from control
  plane truth + local disk scan).
- **Cost:** artifacts require tombstone-based distributed delete. Accepted.
- **Action required:** existing code using `~/.olympus/spaces/` or the flat
  `~/olympus/sessions/` layout must migrate to `~/.olympus/<org>/sessions/`.
  The session-id scheme drops the node segment (node is a field, inferred from
  agent, not baked into the id).

## 9. Migration note for current code

Commits `2c651e9` and the E1/E2 worktree work predate this ADR. Specifically:
- `BridgeManager::with_spaces_root(home.join("spaces"))` → must become
  `<workspace_root>/<org_slug>/sessions`.
- Session id `<datetime>-<node>-<hash>` → `<datetime>-<hash>` (node is a field).
- E1's "space becomes a jj worktree" → "repo is a jj workspace *inside* the
  session space; space is a container."
- E2's agent detection → still valid; integrates as detected host agents merge
  into the agent list (node is inferred from where a detected agent runs).
