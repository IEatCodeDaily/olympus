# ADR 0002: Olympus is a multi-node agent fleet control plane

- Status: Accepted
- Date: 2026-06-27 (Rust-native substrate revision: 2026-06-28, see ADR 0003)
- Supersedes: the "thin host-effect runtime" framing of ADR 0001 (see §21).
- Substrate: per **ADR 0003**, Layer 1 is a **Rust-native single-binary control
  plane** (redb event log + in-memory views + single-writer scheduler + iroh/UDS
  transport). Convex has been removed entirely. Earlier drafts of this ADR were
  written on Convex; every "Convex" concept has been reframed to its Rust-native
  equivalent below.

> **Read this document in full before writing any Olympus code.** It is the
> authoritative architectural specification. Where this ADR and any other doc
> (ADR 0001, `docs/architecture/architecture.md`, the migration plan, or
> `docs/autonomous-loop.md`) disagree, this ADR wins; where this ADR and ADR 0003
> disagree on the substrate, ADR 0003 wins. Every section is written to be
> implementable without further interpretation. If something here is ambiguous,
> that is a bug in this ADR — stop and get it clarified rather than guessing.

---

## 1. Doctrine (the one sentence)

> **Olympus is a multi-node agent fleet control plane: a Rust-native single-
> binary control plane owns all durable truth (an append-only event log),
> orchestration intent, workflows, and reactive views; one node agent ("envoy")
> per host owns every host effect — process supervision, workspace, skills/MCP,
> credentials, desired-state, vault sync; and heterogeneous agents (Hermes,
> Claude Code, Codex, Droid, OpenClaw) run as supervised processes behind a
> stable `AgentRuntime` boundary.**

Everything below elaborates that sentence into precise, non-negotiable rules.

### 1.1 What Olympus IS

- A **control plane** for a fleet of agents running across one or more host
  machines ("nodes").
- The **interface** through which a human (the operator) and intelligent
  orchestrator agents drive, observe, steer, and interrupt work.
- The **source of truth** for fleet state, configuration, secrets, budgets,
  workflows, sessions, messages, cards, knowledge vaults, and audit history —
  held as an append-only event log with derived views.
- The **owner of every agent's working directory**, so agents operate in a
  scoped workspace (a jj worktree) instead of grepping the whole host — directly
  fixing the Hermes Studio cross-contamination problem.

### 1.2 What Olympus is NOT

- **Not an agent runtime.** Olympus does not implement an agent loop, context
  management, or tool execution. Agents (Hermes et al.) remain external programs.
- **Not a Hermes Studio UI replacement.** It superficially resembles one (a new
  React UI) but the product is the orchestration/control layer, not the chat UI.
- **Not a fork of Paperclip or Fusion.** It is a clean-room product. Its
  differentiators are stated in §1.3.
- **Not built on an external backend service.** Per ADR 0003 the control plane is
  a self-contained Rust binary; there is no Convex, no DB server, no external
  connection terminus.

### 1.3 The differentiators (why this exists despite Paperclip/Fusion)

1. **Correct, owned substrate.** A single-writer scheduler over an append-only
   log gives the coordination correctness (no double-claim, no slot leak,
   fencing) that Fusion and Paperclip patch by hand — from the topology, not from
   a distributed-ACID dependency. Correctness is a property of the design, not of
   defensive code or an external service.
2. **Interactive agency.** Unlike Paperclip's heartbeat/ticket model (agents are
   black boxes that produce output on a schedule), every Olympus agent session is
   a live conversation the operator can enter at any point — read, steer,
   interrupt, ask "why".
3. **Heterogeneous complementary runtimes.** Provider harnesses (Claude Code,
   Codex, Droid) handle well-scoped directive tasks; intelligent agents (Hermes,
   OpenClaw) handle orchestration and judgment. Olympus routes by task shape.
4. **Identity/context/session isolation.** One operator identity across many
   isolated contexts (personal vs corporate) — how a real knowledge worker
   operates across realms. Paperclip's multi-company is flat isolation.
5. **Subscription-aware budgeting.** Tracks finite subscription quotas (Claude
   Code Max weekly cap, Codex quota, multiple API keys) and routes to the
   subscription with remaining capacity — not just token cost.
6. **Shippable & self-contained.** Single Rust binary control plane, all
   dependencies permissively licensed (redb, tantivy, iroh, jj-as-tool); no BSL/
   AGPL/fair-code, no external backend to operate. (ADR 0003.)

### 1.4 Cross-cutting invariants (the load-bearing rules)

These hold everywhere; violating any one is a bug. Keep this list in view.

1. **Layer boundary:** orchestration/state → control plane; any host effect
   (process, file, PTY, port, install) → the host's envoy. Never blur it (§2.1).
2. **One durable process per host:** the envoy is the only durable, stateful
   host-level process; every agent process is disposable and reconstructible from
   the control plane (§11).
3. **All agents are host processes** — orchestrators included. No agent loop runs
   inside the control-plane process (§2.3).
4. **The control plane is the sole source of truth**, as an append-only event
   log; views and search indices are derived, rebuildable projections (§2.4,
   §10A).
5. **Single-writer serialization + fencing:** the scheduler is the one writer of
   contended state; `availableSlots` is decremented on assign and incremented
   exactly once on terminal/sweep; `claimEpoch` fences every claim/complete so a
   stale node cannot write a result for a reassigned command (§10.5, §10.6,
   §14.4).
6. **Card ↔ worker-session is 1:1**; reassignment happens only on
   node-unreachable or agent-changed, and the prior trace is forwarded as a
   "previous attempt" block (§6).
7. **Capability is enforced at the kernel/transport, not by instruction:** worker
   network egress is a per-context netns allowlist; node identity is the
   transport (iroh `NodeId` / UDS peer creds); orchestrator-only `gh`/artifact
   access follows from it (§5.5, §12.4, §2.5).
8. **Context isolation is hard at the data layer, soft at the reasoning layer**
   (§3.2). Credentials/workspace/network are constructionally isolated; memory
   placement is convention + audit.
9. **Workflows orchestrate sessions, never run agent code** (§15.3).
10. **Truth is small + hot; bulk is paged + compressed; search is a derived
    index.** Views and the log's hot set stay in memory bounded; message/artifact
    bytes live compressed in redb / blob store and are paged on demand; search is
    tantivy (§10A, §11).

---

## 2. Three-layer architecture

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ LAYER 1 — CONTROL PLANE  (single Rust binary; + React UI over WSS)         │
│   - Sole source of truth: append-only EVENT LOG in redb                    │
│   - In-memory MATERIALIZED VIEWS (reactive reads) → delta broadcast        │
│   - SINGLE-WRITER scheduler (contended state; fencing; group-commit)       │
│   - Durable WORKFLOW engine (embedded, checkpoint-based, redb-backed)      │
│   - tokio timers (cron), axum (HTTP/webhooks + browser WSS)                │
│   - Does NOT perform host effects (it is one authority over many hosts)    │
└───────────────────────────────┬──────────────────────────────────────────┘
                                 │ command/event protocol over a Transport:
                                 │   • iroh (remote nodes; NodeId = identity)
                                 │   • Unix domain socket (local nodes)
        ┌────────────────────────┼────────────────────────┐
        │                        │                        │
┌───────▼────────┐      ┌────────▼───────┐      ┌─────────▼──────┐
│ LAYER 2 ENVOY  │      │ LAYER 2 ENVOY  │      │ LAYER 2 ENVOY  │
│ (node agent,   │      │                │      │                │
│  one per host) │      │                │      │                │
│  - sole host-  │      │                │      │                │
│    level proc  │      │                │      │                │
│  - PID super-  │      │                │      │                │
│    vision      │      │                │      │                │
│  - port resv   │      │                │      │                │
│  - sandbox     │      │                │      │                │
│  - PTY bridge  │      │                │      │                │
│  - artifact    │      │                │      │                │
│    serving     │      │                │      │                │
│  - DESIRED     │      │                │      │                │
│    STATE       │      │                │      │                │
│    (prereqs)   │      │                │      │                │
│  - VAULT SYNC  │      │                │      │                │
│  - workdir /   │      │                │      │                │
│    skills/mcp/ │      │                │      │                │
│    creds MGMT  │      │                │      │                │
└───────┬────────┘      └────────┬───────┘      └─────────┬──────┘
        │ spawns + supervises    │                        │
┌───────▼────────────────────────▼────────────────────────▼──────┐
│ LAYER 3 — AGENTS  (supervised processes, many per node)          │
│   Orchestrator agents (Hermes/OpenClaw): full env, control tools │
│   Worker agents (Claude Code/Codex/Droid): scoped, sandboxed     │
│   Behind the AgentRuntime adapter boundary                       │
└──────────────────────────────────────────────────────────────────┘
```

### 2.1 The hard boundary rule

**Orchestration and durable state = Layer 1 (control plane). Any host effect
(spawn a process, write a file, hold a PTY, bind a port, install a program) =
Layer 2 (envoy) required.** This dividing line is absolute and is the reason
Layer 2 exists. The control plane is one logical authority coordinating many
hosts; host effects are inherently per-host and belong to that host's envoy. The
control plane process has no business spawning agents or touching a node's
filesystem even when co-located — it issues commands; the envoy performs them.

### 2.2 React/UI never import agent internals

The React UI talks to the control plane only over its WSS API (queries +
subscriptions). It MUST NOT import Hermes, Claude Code SDK, or any agent
internals. All host execution flows through envoys via the command/event protocol
(§10). This keeps the agent layer swappable.

### 2.3 Where agents run (orchestrators included) — definitive

**All agents — orchestrators AND workers — are Layer-3 host processes.** No agent
loop runs inside the control-plane process. The orchestrator is Hermes (or
another harness) running as a supervised host process in a main session, exactly
like a worker, but configured with control-plane tools (§19). It reaches the
control plane only through those tools (which issue commands / read views); it
does not run *inside* the control plane.

The control plane MAY make pure LLM inference calls in two narrow, non-agent
roles: (a) optional utility inference inside a workflow step (e.g. a
summarization node), and (b) embedding generation for search (deferred, §10A.4).
Neither is "an agent" in the orchestration sense — neither is an agent loop.

### 2.4 Truth model: event log + derived views (event-sourced)

The control plane is **event-sourced**. Every state change is an **append-only
event** in redb; current state is a **deterministic projection** of the event
log. Rationale: the product requires full auditable history (sessions, cards,
workflows, who-triggered-what) — the log *is* that history, for free. Two
consequences used throughout this ADR:

- **In-memory materialized views** (cards-by-status, node registry, a session's
  recent messages, scheduler state) are projections held in RAM for fast reads
  and reactivity. On restart they are rebuilt from the log (or a snapshot + tail).
- **Search indices** (tantivy FTS; later a vector index) are also derived
  projections — rebuildable from the log, never the source of truth (§10A).

### 2.5 Transport & node identity (the security primitive)

Node identity is the transport, not an application token:

- **Remote nodes → iroh.** An iroh endpoint is an Ed25519 keypair; its `NodeId`
  is the public key. A node connecting over iroh is cryptographically identified
  by the transport (spoofing requires the private key). Fingerprint verification
  = "is this `NodeId` in the operator's allowlist?". iroh also provides NAT
  traversal + encryption + relay fallback.
- **Local/co-located nodes → Unix domain socket** at
  `~/olympus/system/control.sock` (mode 0600). The OS authenticates the peer
  (filesystem permission; `SO_PEERCRED` for uid/pid). No token needed locally.
- **Browser ↔ control plane → WSS** (browsers cannot be raw iroh peers). This is
  the one unavoidable second transport; it carries operator auth (operator login,
  not node identity).

One wire protocol (length-prefixed frames; `postcard` or messagepack) behind a
`Transport` trait with `UnixSocket`, `Iroh`, and `BrowserWss` impls. The control
plane reads frames; it does not care which pipe they arrived on. This **replaces
the former `nodeToken` scheme** — node identity is now transport-native (iroh
NodeId / UDS peer creds). A `nodeToken` application-layer secret is no longer
required; the allowlist of authorized `NodeId`s (and the per-context grants of
§10.7) is the authorization layer on top of transport identity.

### 2.6 Two planes: authoritative coordination + (future) local-first content

Olympus adopts **position C** (hybrid local-first). Data splits into two planes
with different consistency models — the right model per data shape, not one
engine for everything:

- **Control plane (coordination) — authoritative, centralized, NOW.** Live agent
  coordination — the scheduler, `hostCommands`, leases, fencing, node registry,
  cards/board state — needs single-writer correctness (atomic task claim, no slot
  leak). This CANNOT be CRDT: "exactly one agent claimed this task" is a
  consensus property, not a mergeable one. It stays the central Rust control plane
  (redb event log + single-writer scheduler).
- **Document plane (content) — local-first, P2P, FUTURE (north star).** The
  knowledge/notes/vault/message-content layer — the Notion-like, own-your-data,
  offline-capable, P2P-synced surface — wants automatic convergence of concurrent
  offline edits. That is CRDT, not git-merge. Built on **iroh-docs** (CRDT KV
  sync) + **iroh-blobs** (content-addressed, fetch-on-demand binary transfer) over
  the iroh transport already in §2.5.

**Sync-engine assignment by data shape (decided):**

| Plane | Data | Engine | Why |
|---|---|---|---|
| Code | session repos, origin repos, worktrees (§5) | **jj** (git-colocated) | conflicts *should* surface to a human/agent; agents speak git; text |
| Document | notes, vault docs, message content | **CRDT** (Automerge/Loro-class) over **iroh-docs** | concurrent offline edits must AUTO-converge (Notion-style), not raise conflicts |
| Blob | `.pdf/.xlsx/.pptx`/image/video attachments | content-addressed store (blake3) over **iroh-blobs** | dedup by hash; selective fetch-on-demand; no "every version on every device" |
| Coordination | scheduler/cards/leases/nodes | central single-writer (redb) | atomic claims/fencing; not mergeable |

**jj is the CODE plane only.** It is NOT the document-plane sync engine:
verified — jj has no Git-LFS and no partial/shallow clone (every replica stores
every version of every blob), and jj surfaces *conflicts* rather than
*auto-converging* concurrent edits. Both properties are wrong for a local-first
knowledge layer; both are right for code. (This corrects the earlier "jj is the
file-sync substrate, no CRDT layer" stance — see §21.)

**MVP reality:** the document plane is NOT built in the MVP. Content tables
(§3.5.4) live in the central event log for now; they are kept free of hard
dependencies on the single-writer (§3.5.4 rule) so the CRDT document plane is an
**additive** future, not a rewrite. The iroh transport (§2.5) is the same
substrate both planes ride, so adding iroh-docs/iroh-blobs later reuses it.

---

## 3. Identity / Context / Session isolation

Three layers of scope, with different isolation rules. This is the model that
lets one operator run both personal and corporate work without contamination.

| Layer | Name | Crosses contexts? | Isolation | Holds |
|---|---|---|---|---|
| 1 | **Identity** | Yes (always present) | None — travels everywhere | Agent persona, baseline operator preferences, working style |
| 2 | **Context** | No — the hard boundary | Convention-enforced (§3.2) | Credentials, accessible workspaces, skill library, project memory, budget, audit trail |
| 3 | **Session** | No — belongs to one context | Lives/dies on one node | Conversation history, workspace dir, card/subagent tree |

### 3.1 Definitions (exact)

- **Identity**: the agent persona (e.g. "Zephyr") plus operator-level facts true
  in every context. Identity memory is injected into every session regardless of
  context. Example fact: "operator prefers terse responses."
- **Context**: a named realm such as `personal`, `noovoleum-rde`,
  `noovoleum-mkt`. A context owns its own credentials, secret references, skill
  set, project/domain memory, budget, and audit trail. A session is bound to
  exactly one context at creation and cannot change context.
- **Session**: one conversation/work unit within a context. A **main session**
  (an operator chat / orchestrator session) owns a top-level workspace directory
  (`~/olympus/sessions/<session_id>/`, see §5); a **worker session** (the agent
  that executes a delegated card) does NOT get a top-level directory — its workdir
  IS the card directory under its parent main session (§5.2, §6). A session has
  its own message thread and is pinned to one node (§5.6).

### 3.1.1 Context / project / session hierarchy (decided)

The containment hierarchy is **context › project › session**:

- A **project belongs to exactly one context.** A project does not span contexts.
  (Cross-context sharing would breach the context isolation boundary of §3.2; if
  the same codebase is worked in two contexts, it is two projects.)
- A **session belongs to exactly one context** and **optionally to one project**
  within that context. A session with no project is valid (e.g. a personal
  one-off chat).
- **Memory scoping** (§3.3) is by context and identity. Project-level facts are
  stored as context memory tagged with the `projectId`; there is no separate
  project-memory store in v1.

> DESIGN DECISION (flag for veto): "project belongs to one context." If you want a
> project shareable across contexts, this is the line to change — but it reopens
> the isolation question.

### 3.2 Enforcement model: convention, not system

The boundary is **enforced by convention at the reasoning/memory level and by
construction at the credential/workspace/network level.**

- **Hard-enforced (by construction):**
  - Credentials: a session only receives credentials scoped to its context. The
    envoy injects only the context's secrets into a worker's environment.
  - Workspaces: a session can only read/write within its own
    `~/olympus/sessions/<session_id>/` tree and its context's designated
    workspace/project roots.
  - Network: worker netns egress allowlists are per-context (§12.4).
- **Convention-enforced (by instruction + audit):**
  - Memory placement: when a fact is saved, the operator/agent chooses
    identity / context / session scope. There is NO system guard preventing a
    fact being saved to the wrong scope. Mitigation: every memory record carries
    its `contextId` so misplacement is auditable after the fact (§3.3).
  - Reasoning bleed: an LLM that once knew a corporate fact may reason about it in
    another context. This is accepted; it cannot be structurally prevented
    without separate deployments, which would destroy the single-identity value.

Data and access are hard-isolated; knowledge and reasoning are soft-isolated. You
can prevent a process from *reading* data it should not, but you cannot prevent a
model from *reasoning* about things it learned — so don't pretend to.

### 3.3 Memory model

Three memory stores, matching the three layers, each an event-sourced projection:

- `identityMemory` — keyed by identity (agent). Injected into every session.
- `contextMemory` — keyed by `contextId`. Injected only into sessions in that
  context.
- `sessionMemory` — keyed by `sessionId`. The session's working state; may be
  promoted to `contextMemory` on archival (explicit, logged).

Every memory record carries `contextId` (or `__identity__` sentinel) so the UI
can surface "this fact was saved in context X" and the operator can detect
misplacement.

### 3.4 Crossing contexts

Nothing crosses a context boundary without an **explicit, logged promotion**. A
finding in `noovoleum-rde` the operator wants personally must be explicitly
copied to `personal` with an audit record. A personal skill to be used on
corporate work must be explicitly installed into that context's skill library.
The friction is the security model. Facts true in *all* contexts belong in
identity memory, not context memory.

### 3.5 Tenancy, RBAC, and resource ownership

Olympus is multi-tenant and human-in-the-loop by design. The full scope
hierarchy, above the §3.1.1 context›project›session, is:

```text
Org          personal | noovoleum        ← TENANT ROOT; hard isolation; RBAC realm
 └─ Context  rde | mkt | com (within org) ← credential / budget / skill / memory realm
     └─ Project  a codebase / initiative
         └─ Session  a conversation / work unit
             └─ Card / sub-session  delegated work
```

- **Org is the tenant root.** `personal` is an org-of-one; `noovoleum` is an org
  with contexts `rde`/`mkt`/`com`. **Complete data isolation between orgs** — a
  query scoped to one org can never see another org's rows. This is Paperclip's
  "multi-company isolation," but combined with the single-identity-across-realms
  model (§3.1) Paperclip cannot do.
- **Context** keeps its §3 job (credential/budget/skill/memory boundary) but now
  lives *inside* an org. `noovoleum-rde` = org `noovoleum`, context `rde`.

#### 3.5.1 The decided model (5 forks resolved)

1. **Org sits above Context** — sub-realms (rde/mkt) share a company tenant
   boundary while isolating credentials/budgets.
2. **Identities are org-scoped by default, plus operator-global identities.**
   `identity.orgId` is nullable: `null` = operator-global (Zephyr, available in
   every org the operator belongs to); set = an org-owned agent that does not
   leak across orgs.
3. **Org-level roles only (for now):** `owner` (everything incl. nodes/billing),
   `admin` (manage projects/contexts/members), `member` (create/run/fork/steer
   own + shared work), `viewer` (read-only). Per-project ACLs are a deferred
   additive table (`projectMemberships`), designed-not-enforced.
4. **Boards are org-scoped, optionally project-bound** (not forced 1:1 with a
   project). See §6.
5. **Schema now, enforce later.** Every durable record carries `orgId` +
   `ownerId` from day one; MVP runs single-org (`personal`) / single-user (`rpw`
   = owner) with authz stubbed to "allow the operator." Flipping on real RBAC +
   multi-user later requires NO schema migration or backfill.

#### 3.5.2 RBAC tables

```text
users:        id, name, email, ...                       # humans
orgs:         id, name, ...
memberships:  userId, orgId, role                        # role ∈ owner|admin|member|viewer
projectMemberships: userId, projectId, role              # DEFERRED (table exists, empty in MVP)
```

Authz is a single seam: `can(user, action, resource) -> bool`. Rule: the caller
must hold a role in `resource.orgId` that grants `action`; owner-only actions
(delete, change-ownership) additionally require `user.id == resource.ownerId` OR
an org `admin`/`owner`. In MVP `can(...)` returns `true` for the operator; the
call sites exist everywhere so enforcement is a later flip, not a refactor.

#### 3.5.3 Resource ownership (every durable entity)

Every durable record carries **`orgId` + `ownerId`** (creator) plus its natural
parent scope. Applies to: `orgs, users, memberships, identities, contexts,
projects, sessions, cards, boards, origin-repos, vaults, artifacts,
subscriptions, budgets, nodes`.

- **`assignedId` + `assignedKind: "agent" | "user"`** on cards/sessions/tasks:
  work may be assigned to an agent OR a human. Olympus is agent-native but
  human-in-the-loop — "review this," "approve this," "do this manually" are
  human assignments; the rest are agent assignments. `ownerId` (who organizes
  it) is distinct from `assignedId` (who does it).
- **Nodes/envoys** are granted to orgs by an org owner (`nodes.orgs`; extends the
  §10.7 `nodes.contexts` grant). A node serves only granted orgs — the
  kernel-enforced tenant boundary (§12.4) extends to compute.
- **Subscriptions/budgets** are org-scoped (in addition to context-scoped, §16),
  so cross-org cost isolation is automatic — the corporate/personal cost-
  separation requirement.

#### 3.5.4 Coordination vs content (the local-first cut — keeps §C reachable)

Every table is classified as **coordination** or **content**. This is the
data-model cut that lets Olympus stay centralized for the MVP while keeping the
local-first document plane (§2.6, position C) reachable as an additive future,
not a rewrite:

| Class | Tables | Consistency model | Future |
|---|---|---|---|
| **Coordination** | scheduler state, `hostCommands`, leases, `nodes`, `cards`, board state, `memberships`, budgets/subscriptions | authoritative, single-writer (central control plane) — needs atomic claims/fencing | stays central; live agent coordination cannot be CRDT |
| **Content** | `messages`, session transcripts, knowledge-vault docs, notes, memory entries | could become local-first CRDT later (§2.6) | becomes the document plane (iroh-docs / CRDT) post-MVP |

**Rule for all builders:** content tables MUST NOT take a hard dependency on the
single-writer scheduler (a note's correctness must never require the scheduler).
In MVP both classes live in the central redb event log; the classification is a
boundary we honor now so the content plane can migrate to CRDT later without
touching coordination.

---

## 4. Source-of-truth matrix

All "owned by control plane" rows mean: an append-only event type in the redb log
with a derived in-memory view (§2.4). "Owned by envoy" rows are host effects.

| Concern | Owner | Notes |
|---|---|---|
| Users / authz (operator) | Control plane | operator login over WSS |
| Identities, contexts, projects | Control plane | realm/scope definitions |
| Sessions / threads | Control plane | canonical session records (log) |
| Messages / history | Control plane | log + zstd-compressed bodies; §10A |
| Cards / board | Control plane (`cards`) | durable units of work; lifecycle; §6 |
| Tool-call lifecycle | Control plane | queued/running/succeeded/failed/cancelled |
| Streaming to UI | Control plane | view deltas over WSS; see §10.3 |
| Workflows (durable) | Control plane (embedded engine) | checkpoint-based; journaled; §15 |
| Scheduled work / cron | Control plane (tokio timers) | sweepers, periodic workflows, vault-sync backstop |
| Webhooks (inbound) | Control plane (axum) | feed workflow triggers; §15.5 |
| Node registry + heartbeats | Control plane (`nodes`) | §5.6, §10.6 |
| Node desired-state (prereqs) | Control plane (`nodeDesiredState`) | §7; envoy reconciles |
| Host command queue + leases | Control plane (`hostCommands`) | §10, §10.5–10.6 |
| Skill/MCP library catalog | Control plane (`skills`,`mcpServers`) | §9; bytes materialized to disk by envoy |
| Skill/MCP activation | Control plane (scoped config) + project/session dir | §9; Claude-Code/Codex-style config |
| Knowledge vaults (index) | Control plane | §8; bytes in jj-synced vault dir |
| Budgets + subscriptions | Control plane | §16 |
| Chat rooms | Control plane | §14 |
| Artifact index (files, PRs, builds) | Control plane | §17; bytes in blob store/object storage |
| Full-text search | Control plane (tantivy index) | derived, rebuildable; §10A |
| Vector/semantic search | Control plane (deferred index) | additive; needs embeddings; §10A.4 |
| **Process spawn / supervision** | **Envoy** | Layer 2 |
| **PTY / terminal** | **Envoy** | `Bun.Terminal` or Rust PTY; §18 |
| **Filesystem authority + workdir lifecycle** | **Envoy** | path-guarded; §5 |
| **Skill/MCP/cred materialization to disk** | **Envoy** | library + scoped activation + creds; §5, §9 |
| **Knowledge-vault sync** | **Envoy** | jj pull/push on webhook + cron; §8 |
| **Desired-state reconciliation (installs)** | **Envoy** | gh/git/jj/gitnexus/…; §7 |
| **Port allocation** | **Envoy** | netns + forwarding; §12.4 |
| **Sandbox lifecycle** | **Envoy** | host-direct / bwrap / Docker; §12 |
| **Artifact byte serving + blob store** | **Envoy** | HTTP file server + content-addressed blobs; §17 |
| Agent loop / context mgmt / tools | The agent (Hermes etc.) | NOT Olympus |

---

## 5. Filesystem hierarchy and workspace management

Olympus (via the envoy) owns every agent's working directory. On a new session the
envoy creates a scoped **session space** and runs the agent inside it, so the
agent operates on local, scoped repos rather than grepping the whole host. This
directly fixes the Hermes Studio failure mode (full-system grep instead of
scoping to a local repo).

### 5.0 Terminology (locked — "workspace" was overloaded)

| Term | Meaning |
|---|---|
| **Session space** | A session's whole directory: `~/olympus/sessions/<session_id>/` — holds repos, artifacts, metadata, symlinks. NOT itself a repo. |
| **jj workspace** | A jj working copy of an **origin repo**, living *inside* a session space at `repos/<org>/<repo>/`. Created with `jj workspace add`. |
| **Origin repo** | `~/olympus/repos/<org>/<repo>/` — the canonical jj repo all sessions fork (workspace-add) from. One shared history per repo. |

A session space is the container; jj workspaces are repos *inside* it; the origin
repo is the shared history they branch from. Artifacts and scratch output live in
the session space's `.olympus/artifacts/`, **never inside a `repos/<org>/<repo>/`
working copy**, so session output never pollutes a repo's `jj status`.

### 5.1 The `~/olympus/` tree (exact; this layout is normative)

```text
~/olympus/                               # Olympus-managed root, one per node
├── repos/                               # ORIGIN repos (canonical jj history; fork source)
│   └── <org_name>/<repo_name>/          #   a jj repo (git-colocated); sessions workspace-add from here
├── sessions/                            # SESSION SPACES (main sessions only, FLAT here)
│   └── <session_id>/                    # a session space (operator chat / orchestrator main session)
│       ├── .olympus/                    # session metadata — NOT a repo
│       │   ├── session.json             # → hermes_session_id, source, contextId, projectId?, nodeId,
│       │   │                            #     fork lineage (forked_from, fork_point, fork_type)
│       │   ├── skills.json              # ACTIVE skills for this session (refs into library)
│       │   ├── mcp.json                 # ACTIVE MCP servers (refs into library)
│       │   ├── steer/                   # (reserved) envoy-local control scratch; control lane is stdio (§19)
│       │   └── artifacts/               # session-scoped artifacts (§17) — ISOLATED from repos/
│       ├── repos/                       # jj WORKSPACES (forks of origin repos)
│       │   └── <org_name>/<repo_name>/  #   jj workspace add'd from ~/olympus/repos/<org>/<repo>
│       │       ├── .jj/                 #   this workspace's jj state (one bookmark per attempt, §6.4)
│       │       └── .git/                #   colocated git view for git-expecting agents (§5.4)
│       ├── project -> ~/olympus/projects/<project_id>/   # symlink (only if project-bound)
│       └── <sub_session_id>/            # SUB-SESSION space (nested; §6) — same shape as a session space
│           ├── .olympus/                #   sub-session metadata (incl. fork lineage to parent)
│           ├── repos/<org>/<repo>/      #   sub-session's own jj workspace (forked from parent's commit)
│           └── artifacts/
├── projects/                            # project-scoped SHARED assets — NOT workdirs, NOT session spaces
│   └── <project_id>/
│       ├── vault/                       # knowledge vault (llm-wiki); own jj repo; live-synced (§8)
│       │   └── .jj/
│       ├── skills.json                  # project default ACTIVE skills (refs into library)
│       ├── mcp.json                     # project default ACTIVE MCP servers
│       └── config.json                  # project config, materialized from control plane
├── skills/                              # Olympus-managed skill LIBRARY (ALL managed skills)
│   └── <skill_id>/                      #   bytes materialized by envoy; READ-ALL (§9)
├── mcp/                                 # Olympus-managed MCP LIBRARY (ALL managed servers)
│   └── <mcp_id>/                        #   READ-ALL for discovery (§9)
├── creds/                               # credential materialization
│   └── _context-<context_id>/           #   envoy WRITES scoped; agents READ their context only
└── system/                             # node-local state
    ├── control.sock                     # UDS to the control plane (local nodes; §2.5)
    ├── logs/
    ├── node-agent.json                  # this node's registration mirror (incl. iroh NodeId)
    └── desired-state.json               # installed-software manifest cache (§7)
```

### 5.2 Key structural rules

- **A session space holds repos AND artifacts, kept separate.** `repos/<org>/<repo>/`
  is a jj workspace (versioned repo work); `.olympus/artifacts/` is session output
  (build outputs, screenshots, scratch). Artifacts NEVER live inside a repo
  working copy, so they never pollute `jj status` or a commit. Applies identically
  to main and sub-sessions.
- **Only main sessions get a top-level `sessions/<session_id>/` session space.** A
  worker/sub session (executing a delegated card, §6) does NOT get a top-level
  space — it nests under its parent as `<sub_session_id>/`.
- **Sub-sessions nest** as `sessions/<main>/<sub_id>/` with the same session-space
  shape (own `.olympus/`, own `repos/`). The whole session tree lives under one
  `sessions/<main_session_id>/` root on one node (§5.6).
- **Sessions are flat under `sessions/`, not nested under projects.** A
  project-bound session records `projectId` in `session.json` and reaches the
  project's shared files via the `project ->` symlink, NOT by filesystem nesting.
- **`repos/<org>/<repo>/` is a jj workspace of `~/olympus/repos/<org>/<repo>/`**
  (the origin). Multiple sessions working the same repo are multiple jj workspaces
  of one origin history (§6.4).
- **`projects/<id>/` holds shared assets (vault, config, default activation), not
  workdirs.**
- **`skills/` and `mcp/` are libraries** of *all* managed skills/MCP; activation is
  per-scope config referencing the library (§9).
- **`creds/` is written by the envoy**, scoped per context; an agent reads only its
  own context's credentials.

### 5.3 Workdir lifecycle is owned by the envoy

- On session creation: the envoy creates the session space `sessions/<session_id>/`
  with `.olympus/`, `jj workspace add`s any needed origin repos into
  `repos/<org>/<repo>/`, materializes active skills/MCP, injects scoped creds, sets
  the `project ->` symlink if project-bound, then spawns the agent inside the
  session space.
- On sub-delegation: the envoy creates `sessions/<main>/<sub_id>/` and
  `jj workspace add`s from the parent's commit (§6).
- On archival/cleanup: see §17.4.

### 5.4 Version control: jj with git colocate

- Origin repos and their jj workspaces are **Jujutsu (jj)** colocated with git,
  maintaining both `.jj/` and `.git/` so agents that shell out to `git` see a
  correct git view while jj manages history.
- A session's repo working copy is a **jj workspace** (`jj workspace add
  <session_space>/repos/<org>/<repo> --revision <fork_point>`) of the origin repo.
  Commits flow into the origin's shared history; abandoning a session's work is a
  `jj` abandon / `jj workspace forget`.
- **KNOWN HAZARD (must be handled):** jj has first-class conflict commits git does
  not represent. With colocate, `git status` may read "clean" while `jj log` shows
  an unresolved conflict, so the agent reasons from a false premise. The envoy
  MUST detect a jj-conflict state on a worktree and surface it (block the worker,
  or translate the conflict into git-visible markers) before a git-expecting agent
  reads the tree. Do not ship colocate without this guard.

### 5.5 Orchestrator-only GitHub access

- Worker agents have **no `gh` CLI and no GitHub network egress** (netns
  allowlist, §12.4); they produce commits in their local jj worktree only.
- The **orchestrator** is the sole agent with `gh` and `jj git push`. It reviews
  worker output and decides what to push and when to open a PR.
- **Consequence (accepted):** push throughput is bounded by orchestrator
  availability; the orchestrator is the merge gate. GitHub only ever sees curated,
  orchestrator-approved branches.

### 5.6 Session-node affinity

A session tree (a main session plus all its card/subagent workdirs) lives on
**one node**, pinned at creation, and does not migrate.

**Rationale (not mere simplicity):** co-location enables filesystem-native
inter-agent context — `rg` across sibling card worktrees, shared `jj` history,
local SDK comms at memory speed. Splitting across nodes loses these and forces
network-routed fallback (§13.6). Multi-node fan-out is a deliberate degradation,
reserved for when a card needs a capability the home node lacks (e.g. a GPU on
another node). Most cards run on the same node as the main agent, so reassignment
(§6.2) is rare.

---

## 6. Cards and the board (kanban)

A **card** is the durable unit of work on a board. It carries lifecycle state, the
attached session, the jj bookmark, artifacts, and message history.

### 6.1 Card ↔ session is 1:1 by default

- A card attaches to **exactly one session**. The card's workdir
  (`sessions/<main>/<card_id>/`) and the attached worker session are bound for the
  card's whole life.
- The card's directory and jj history are **keyed by `card_id`** and are stable;
  the *attached session* is what changes on the (rare) reassignment.

### 6.2 Reassignment — the only two triggers

A card may be reset and reassigned a new session ONLY when the current session
cannot continue:

1. **Node unreachable.** The node running the session became unreachable (§20).
   Uncommon (most cards co-locate with their main agent).
2. **Agent changed.** The operator/orchestrator deliberately swaps the agent
   (Zephyr → Talos, or Claude Code → Codex).

In **both** cases: any prior session output Olympus retained (at minimum the
message trace; the card's files too if the node is reachable) is **forwarded to
the new agent as a "previous session attempt" text block** in the new session's
first prompt. The new attempt is a new jj bookmark in the same card repo (§6.4).

### 6.3 What a card holds

```text
cards:
  _id  (= card_id, stable)
  orgId, ownerId                         # tenancy + creator (§3.5.3)
  contextId, projectId?
  boardId                                # the board this card lives on (§6.8)
  title: string
  status: string                         # a board column key (§6.8), e.g. "todo"
  assignedId: string                     # who DOES it (agent OR human)
  assignedKind: "agent"|"user"           # Olympus is agent-native + human-in-the-loop
  currentSessionId?: string              # the attached worker session (1:1)
  blockedBy: string[]                    # card dependencies
  baseBookmark: string                   # jj bookmark this card branches from
  currentBookmark?: string               # jj bookmark of the current attempt (§6.4)
  priority: number
  attempts: { sessionId, assignedId, bookmark, startedAt, endedAt?, outcome }[]
  createdAt, columnMovedAt: number
```

- **Board columns / ordering** (cribbed from Fusion's battle-tested rules): `todo`
  orders priority-desc → oldest `createdAt` → cardId; `done` by completion
  recency. `blockedBy` gates eligibility.
- **`ownerId` (organizes) vs `assignedId`+`assignedKind` (executes)** are distinct:
  a human owner can assign a card to an agent, or to another human for manual
  work / review / approval (human-in-the-loop). Who may create/assign/move/delete
  a card is governed by the org role (§3.5.2).
- Artifacts (§17) and message history attach to the card and are inherited across
  attempts; the live jj working copy belongs to the current attempt's session.

### 6.4 jj bookmarks per attempt (decided)

Each session attempt on a card is its own **jj bookmark** in the same card repo
(`attempt-1`, `attempt-2`, …). A reassigned session starts a fresh bookmark from
the card's base (or, at the orchestrator's choice, from a prior attempt's tip) and
can `jj log` to inspect all prior attempts. This gives continuity (dir + history
persist) and a clean slate per attempt, while the "previous session attempt" text
block (§6.2) gives the new agent the narrative of what was tried.

### 6.5 Workflows operate on cards

The workflow `agent-run` node (§15) starts or resumes a **card's current
session**. Workflows move cards across columns and trigger on card events.

### 6.6 Session forking (the model for cross-channel continuation + parallelism)

Olympus **never continues a session in place across channels.** A session started
on Telegram, continued "in place" from Olympus, would diverge — the Olympus turns
would not be reflected back in Telegram, producing two inconsistent truths.
Instead, **continuing a non-Olympus session forks it.** Forking is also the
mechanism for deliberately parallelizing work. Two fork types:

| Fork type | Where it lives | jj | Lineage record | Use |
|---|---|---|---|---|
| **Sub-session** (nested) | `sessions/<parent>/<sub_id>/` | `jj workspace add` from the **parent session's** commit | `parent_session_id` (also maps to Hermes's native `parent_session_id`) | "try a different approach to this same task," exploratory branch in the same context tree |
| **Parallel session** (independent) | `sessions/<parallel_id>/` (own top-level space) | `jj workspace add` from the **origin repo's** commit | `forked_from` + `project_id` (Olympus link; NOT Hermes parent) | "parallelize this project into an independent thread," sibling under the same project |

**The fork point is always recorded** — both types carry `forked_from` (source
session id) and `fork_point` (the source message id / jj commit at which the fork
happened), so the node-graph (§6.6.2) can trace any session back through its
lineage.

#### 6.6.1 How a fork is executed (pure Olympus; zero Hermes change)

Hermes does not need to know about Olympus forking. Verified: Hermes loads a
session to resume from **state.db** (`SessionDB.resolve_resume_session_id`), not
from a side channel. So Olympus forks by preparing a new session row in state.db
and resuming it:

1. Olympus writes a **new Hermes session** into state.db (WAL-safe concurrent
   write): copy the source session's messages `[0..fork_point]` into the new
   session id (the context up to the fork).
2. Inject an Olympus marker as a system message in the new session, so the fork is
   self-describing even from raw state.db:
   `<olympus fork="true" from_agent="<agent_id>" from_session="<src>" fork_point="<n>" olympus_session="<new>"/>`
3. The envoy starts the agent on the new session via `AgentRuntime.start`
   (`hermes acp`, §19) — Hermes resumes it, oblivious that it is a fork; it just
   sees history + a system tag.
4. Olympus drives the fork; the **source session is never modified** (the Telegram
   JSONL/state.db rows are untouched).

Lineage is recorded in two places: **primary** in Olympus's event log
(`forked_from`, `fork_point`, `fork_type`) which drives the node graph; **redundant**
in the in-session marker, so the graph can be reconstructed from raw state.db if
Olympus's log is ever lost.

#### 6.6.2 Node-graph visualization

Every session is a node; every fork is an edge from `forked_from` at `fork_point`.
Sub-sessions render as branches off their parent; parallel sessions render as
sibling roots under a shared project node. The operator sees the full branch
structure of any line of work and can open any node.

#### 6.6.3 Steering applies ONLY to Olympus-managed sessions

Olympus can steer/cancel/switch-model (§19) **only on sessions it manages** —
sessions it created or forked, where the envoy holds the live stdio handle. Olympus
has **no way to interrupt or inject into a live external-channel session** (e.g. a
running Telegram session); it can only **observe** it (§6.7) or **fork** from it.
Steering a "Telegram session" therefore means: fork it into Olympus, then steer the
fork.

> Future (post-MVP): on fork, inject a marker into the *source* channel's session
> history noting "forked into Olympus session `<id>`", so the origin channel knows
> and can reference the fork. Out of MVP scope.

### 6.7 External-channel session sync (observe everything, own nothing)

Sessions originating outside Olympus (Telegram, Discord, CLI, cron, api_server,
subagent) are **observed read-only**, live, with no coupling to the Hermes gateway:

- **Source of truth is `~/.hermes/state.db`** (the Hermes SQLite store), NOT the
  JSONL session files. Verified: state.db holds the complete archive (1,629
  sessions) while only ~135 JSONL files exist on disk (pruned/rotated) and carry
  thinner metadata. state.db runs in **WAL mode**, so Olympus reads it
  concurrently while Hermes writes, with no lock contention.
- **Live sync:** Olympus tails `state.db` incrementally —
  `SELECT * FROM messages WHERE id > <last_seen_id>` on a short poll (~1–2 s),
  optionally triggered by an inotify watch on `state.db-wal`. New rows become
  `MessageAppended` events in Olympus's log; the UI updates live. A new
  `sessions` row becomes a `SessionCreated` event.
- This one mechanism covers **all** channels, because every Hermes channel writes
  to the same state.db. No gateway, no per-channel integration.
- Observed sessions are read-only in Olympus. To act on one, the operator forks it
  (§6.6) into an Olympus-managed session.

### 6.8 The board data model

A **board** is the kanban surface cards live on. It is **org-scoped, optionally
project-bound** (not forced 1:1 with a project — §3.5.1 fork 4).

```text
boards:
  _id
  orgId, ownerId                         # tenancy + creator (§3.5.3)
  projectId?                             # optional binding; null = free-form org board
  name: string
  columns: ColumnDef[]                   # ordered; card.status is one column's key
  createdAt: number
  # ColumnDef = { key, label, order }
```

- **MVP columns are fixed** to the §6 lifecycle:
  `planning → todo → in-progress → in-review → done → archived`. The `columns`
  field is already a list, so **configurable / workflow columns** (the
  Fusion-style "workflow columns," the n8n-like surface) are an additive future,
  not a schema change.
- A card's `status` is a `column.key` on its board (§6.3). Moving a card = changing
  `status` to another column key; ordering rules per §6.3.
- Boards, cards, and their column moves are **coordination** state (§3.5.4) —
  authoritative, central, single-writer; never CRDT (two agents must not both claim
  or both move a card inconsistently).
- RBAC (§3.5.2): create/delete a board and create/assign/move/delete cards are
  governed by the caller's org role; MVP allows the operator.

---

## 7. Node desired-state management

Olympus declares the desired system state of each node; the envoy reconciles to
it. This is how "I want gitnexus" becomes "gitnexus is installed on every node
that should have it."

### 7.1 Model

```text
nodeDesiredState:
  _id
  appliesTo: "all"|"context:<id>"|"node:<id>"   # scope of this requirement
  programs: { name: string, minVersion?: string, installRecipe?: string }[]
  updatedAt: number

nodeActualState:                                 # reported by each envoy
  nodeId
  programs: { name, version, present: boolean }[]
  lastReconciledAt: number
  drift: { name, want, have }[]                  # computed diff, surfaced in UI
```

### 7.2 Reconciliation loop

- On startup and on a schedule, the envoy reads its applicable `nodeDesiredState`,
  checks installed programs, installs/updates what is missing (via `installRecipe`),
  and writes `nodeActualState` with residual `drift`.
- Drift is surfaced per node. A node that cannot satisfy a hard requirement is
  marked `draining` (no new sessions assigned) until reconciled.
- Kubernetes-controller pattern (desired vs actual, reconcile, report). The
  control plane holds desired state; only the envoy can install software (Layer 2
  boundary). **Skill/MCP library materialization (§9.1) is part of this loop.**

---

## 8. Knowledge vaults (project-scoped, live-synced)

A knowledge vault is a project-scoped, live-synced **LLM-wiki**: documentation and
reference material agents in that project can read, including non-text references
(`.pdf`, `.xlsx`, `.pptx`, images) alongside markdown.

### 8.1 Storage and sync (decided)

- A vault lives at `~/olympus/projects/<project_id>/vault/` and **is its own jj
  repo** (git-colocated). jj is the sync substrate. The canonical copy is a remote
  the envoy pulls from / pushes to (a git remote the operator owns; for remote
  nodes that traffic rides the iroh tunnel).
- **Write path:** an agent (or the operator via the UI) writes vault files
  locally. The envoy commits and pushes to the canonical remote, then emits a
  `vault.updated` event to the control plane (updating the index). Writes are
  serialized per vault by the envoy; a genuine push conflict surfaces to the
  operator rather than auto-merging.
- **Read/sync path:** other nodes serving the same project pull on a
  `vault.updated` webhook plus a periodic cron backstop (e.g. every 5 min). No
  Syncthing, no separate object store for the canonical copy.
- The control plane holds the vault **index/metadata** (document list, titles,
  tags, extracted-text refs, embeddings refs, last-synced commit); **bytes** live
  in the jj-synced vault dir. Non-text references are stored as files and indexed
  by path + extracted text (§10A.3).

### 8.2 Access

- Vaults are **project-scoped**: only sessions bound to that project may read the
  vault. Part of the context/project isolation surface (§3).
- Agents read the vault from the local path; the envoy guarantees the local copy
  is current within the sync interval.

---

## 9. Skills and MCP management

Skills and MCP servers are Olympus-managed and scoped. **MCP is treated as "a
skill on how to use a CLI/API"** — same management surface (library + scoped
activation).

### 9.1 Library vs activation (the core distinction)

- **Library** (`~/olympus/skills/`, `~/olympus/mcp/`): **all** managed skills/MCP
  definitions, materialized to disk by the envoy from the control-plane catalog.
  **Read-all**: every agent can read the library for discovery/search. Kept
  current as part of **desired-state reconciliation** (§7): when the catalog
  changes, the envoy re-materializes affected entries on its next reconcile tick
  (and on a catalog-change webhook). Stale library bytes are reconciliation
  drift, not a separate mechanism.
- **Activation** (which skills/MCP are *active* for a scope): configured in the
  relevant `skills.json` / `mcp.json` referencing the library — exactly how Claude
  Code / Codex consume project-scoped skills/MCP. Layered:

  ```text
  global default → context default → project default → card/session override
  (control plane)  (control plane)   projects/<id>/     sessions/.../.olympus/
                                     skills.json|mcp.json  skills.json|mcp.json
  ```

  More specific overrides broader. The envoy materializes the effective set into
  the card/session `.olympus/skills.json` and `mcp.json` at spawn.

### 9.2 Why this shape

An agent is spawned with a **specific** active set yet can **discover** the full
catalog. Requesting an additional skill/MCP is an explicit, scoped activation
change, not silent inheritance. MCP being skill-shaped means one catalog, one
activation mechanism.

### 9.3 Tables (event-sourced)

```text
skills:                                  # catalog (library entries)
  _id, name, description, scopeHint, version, contentRef   # bytes materialized to ~/olympus/skills/
mcpServers:                              # catalog (library entries; MCP = skill-shaped)
  _id, name, description, transport: "stdio"|"sse"|"streamable-http",
  configRef, secretRefs[]                # secrets are refs only, never plaintext
```

---

## 10. Command / event protocol (Layer 1 ↔ Layer 2)

### 10.1 Tables (exact shape; field names are normative)

These are projections of the event log (§2.4); the envoy mutates them only via the
scheduler mutations (§10.5), which append events and update views.

```text
nodes:
  nodeId: string                 # for remote nodes this IS the iroh NodeId (Ed25519 pubkey)
  transport: "iroh"|"uds"        # how this node connects (§2.5)
  capabilities: string[]         # e.g. ["claude-code","codex","hermes","gpu"]
  labels: record<string,string>  # {"os":"linux","arch":"x64","gpu":"none"}
  contexts: string[]             # operator-granted contexts this node may serve
  status: "online"|"unreachable"|"draining"|"dead"
  availableSlots: number         # remaining concurrent session capacity
  lastHeartbeat: number          # epoch ms
  healthPort: number             # localhost health endpoint

hostCommands:
  _id
  sessionId: string              # the session this command drives (main or worker)
  cardId?: string                # set when the command runs a card's worker session
  contextId: string
  kind: "agent.run.start"|"terminal.open"|"terminal.input"|"terminal.resize"
        |"terminal.close"|"fs.read"|"fs.list"|"workspace.prepare"|"artifact.serve"
        # No "agent.run.abort" kind: cancel = set cancelRequested on the live row (§10.4).
        # fs.* and workspace.prepare are OPERATOR/ORCHESTRATOR control-plane ops over a
        # node's filesystem; agents do their own file I/O directly in their sandboxed
        # workdir — routine agent writes do NOT round-trip through the control plane.
  payload: object
  requiredCapability: string
  status: "pending"|"assigned"|"claimed"|"running"|"completed"|"failed"|"cancelled"
  assignedNodeId?: string
  claimedBy?: string             # nodeId that holds the lease
  claimEpoch: number             # monotonic fencing token (§14.4)
  leaseExpiresAt?: number        # epoch ms; node must renew before this (§10.6)
  cancelRequested: boolean       # operator/orchestrator sets to request stop (§10.4)
  result?: object
  error?: string
  createdAt, assignedAt?, completedAt?: number

hostCommandOutput:               # streaming buffer for host-agent output (§10.3)
  commandId
  index: number                  # monotonic chunk index
  text: string
  done: boolean
  at: number
```

### 10.2 The forward path (intent → host effect)

1. A control-plane mutation appends a `hostCommands` event (`status: "pending"`).
2. The **scheduler** (§10.5) atomically assigns it to a node with the required
   capability and free slots (`status: "assigned"`, `claimEpoch++`,
   `availableSlots--`).
3. The target envoy (subscribed to its assigned commands) **claims** it atomically
   (`status: "claimed"`, sets `claimedBy`, `leaseExpiresAt`).
4. The envoy transitions it to `running` and executes the host effect (spawn
   process / open PTY / read fs).
5. The envoy reports terminal status (`completed`/`failed`/`cancelled`) with
   `result`. The same terminal-status mutation increments the node's
   `availableSlots` back (§10.5) — released exactly once, by the scheduler-owned
   mutation, never by the envoy directly, so the counter cannot leak.

### 10.3 The output path (agent output → control plane)

- **Control-plane utility inference (no host effect)** — e.g. a workflow
  summarization step: produced inside the control plane; streamed into the message
  view directly. (NOT an orchestrator; §2.3.)
- **Host agents** (Claude Code, Codex, Hermes on a node): the envoy captures
  process stdout and writes batched chunks (~100 ms throttle, NOT per-token) into
  `hostCommandOutput`. The control plane appends these as message-delta events on
  the session's thread, so host-agent output lands in the **same** message view as
  any other output. The UI subscribes to one message view regardless of agent
  type. (Delta semantics in §10.3.1.)

**Durability guarantee:** if a node dies mid-run, the control plane retains all
output up to the last flush (≤100 ms loss). Git commits/files are on the node's
disk (best-effort, §11, §20).

### 10.3.1 Reactive views and delta streaming (replaces Convex subscriptions)

The control plane holds **in-memory materialized views** (§2.4). A subscription is
a client (UI over WSS, or an envoy over its Transport) registering interest in a
view or a filtered slice. On each committed mutation the affected view computes a
**delta** (added/changed/removed rows) and the control plane pushes it to
subscribers over their transport. Implementation: per-view `tokio::sync::watch` /
`broadcast` channels; the diff is row-level. This is the reactive layer Convex
used to provide; here it is ours, bounded, and transport-agnostic. For views that
grow joins/aggregations beyond hand-maintained indexes, `differential-dataflow` is
the escape hatch — not needed for v1.

### 10.4 The control path (control plane → running agent)

- **Interrupt/cancel (single mechanism):** the operator/orchestrator mutation sets
  `hostCommands.cancelRequested = true` on the live command. No separate abort
  kind. The envoy (subscribed to its claimed command) stops the run: SIGTERM then
  SIGKILL after a grace period for a process agent, or the gateway abort API for a
  service agent (Hermes). The envoy then writes terminal status `cancelled`, which
  (§10.5) releases the slot.
- **Steer (mid-turn injection):** see §13.

### 10.5 The scheduler (the correctness core — exact)

The scheduler is the **single writer** of contended state. It is a control-plane
component (a dedicated task/actor) that owns all `hostCommands` transitions
touching capacity, leases, or fencing, and appends the corresponding events. It is
the single most correctness-critical component. Budget enforcement (§16.3) is a
*filter inside* the assign step, not a separate scheduler.

```text
# assign: pending → assigned   (on a timer tick AND on enqueue)
assignPendingCommands (single-writer, atomic per command):
  for each oldest pending command (bounded batch):
    1. budget gate (§16.3): if context/project budget exhausted → skip (stays pending)
    2. find an online node where:
         requiredCapability ∈ node.capabilities
         AND contextId ∈ node.contexts
         AND node.availableSlots > 0  AND node.status == "online"
       (tie-break: most availableSlots, then lowest load) → none → skip
    3. subscription gate (§16.4): pick a subscription with remaining quota;
       none → fail command "no_capacity"
    4. append events atomically: status="assigned", assignedNodeId, claimEpoch++,
       reserve budget, bind subscriptionId; node.availableSlots--

# claim: assigned → claimed   (envoy request, validated by scheduler)
claimCommand:
  precondition: assignedNodeId == callerNodeId AND status=="assigned"
  set status="claimed", claimedBy=callerNodeId, leaseExpiresAt = now + LEASE_MS

# renew: keep the lease alive while running   (envoy, periodic)
renewLease:
  precondition: claimedBy==callerNodeId AND claimEpoch matches
  set leaseExpiresAt = now + LEASE_MS

# complete: claimed/running → terminal   (envoy request, validated by scheduler)
completeCommand:
  precondition: claimedBy==callerNodeId AND claimEpoch matches   # FENCING (§14.4)
  set status ∈ {completed,failed,cancelled}, result/error, completedAt
  release: node.availableSlots++ ; release budget reservation delta
  # a stale (old-epoch) completion is REJECTED here — fencing
```

Because all of these run through the single-writer scheduler, no two-phase commit
or distributed lock is needed; serialization is structural. `availableSlots` is
incremented in exactly one place — `completeCommand` (and its sweeper equivalent,
§10.6) — so it is released exactly once and cannot leak. Durability: scheduler
event appends use **group commit** (batch fsync per ~1–5 ms window) so the fsync
barrier, not serialization, sets throughput (>100k mutations/s headroom).
Partition-by-scope writers are the documented forward path if a single writer ever
saturates; v1 is single-writer.

### 10.6 Heartbeat, lease, and sweeper — the three timers (exact)

Two independent liveness timers, one sweeper acting on both:

| Timer | Cadence | Written by | Meaning |
|---|---|---|---|
| `nodes.lastHeartbeat` | every `HEARTBEAT_MS` (default 10 s) | envoy (`heartbeat`) | "this node is alive" |
| `hostCommands.leaseExpiresAt` | renewed every `LEASE_RENEW_MS` (default 20 s); window `LEASE_MS` (default 60 s) | envoy (`renewLease`) | "this command is still being worked" |

A single scheduled control-plane function, `reap` (every `REAP_MS`, default 15 s):

```text
reap (single-writer):
  # node death
  for node where status=="online" AND lastHeartbeat < now - NODE_TIMEOUT_MS (default 35s):
     mark node.status="unreachable"; trigger §20 node-death handling
  # command lease expiry (claimant alive-but-stuck, or node died between ticks)
  for command where status∈{claimed,running} AND leaseExpiresAt < now:
     if assignedNode is unreachable/dead → fail "node_unreachable" (resumes parked workflow)
     else → requeue: status="pending", claimedBy=null, claimEpoch++ (FENCING),
            node.availableSlots++   # release the stuck slot exactly once
```

Node-heartbeat failure is the coarse signal (whole node gone); lease expiry is the
fine signal (one command stuck). Either firing releases the command and its slot.
`NODE_TIMEOUT_MS > HEARTBEAT_MS` and `LEASE_MS > LEASE_RENEW_MS` by a safety
margin. All five constants are config, not hardcoded.

### 10.7 Node authorization (security)

Node *identity* is the transport (§2.5): iroh `NodeId` for remote, UDS peer creds
for local — unspoofable without the private key / filesystem access. *Authorization*
on top of identity:

- A node is admitted only if its `NodeId` is in the operator's **allowlist**
  (`nodes` is operator-curated; a node cannot self-register into service).
- A node serves only contexts the operator **explicitly granted** (`nodes.contexts`
  is operator-set). The scheduler filters `contextId ∈ node.contexts` (§10.5);
  context credentials are injected only on a granted node.
- `claimCommand`/`completeCommand` verify `assignedNodeId`/`claimedBy` == the
  authenticated `NodeId`. With `claimEpoch` fencing (§14.4), this closes spoofing
  and split-brain. (This replaces the former application-layer `nodeToken`; the
  transport provides identity, the allowlist + grants provide authorization.)

---

## 10A. Storage, compression, and search

The truth is the redb event log; everything else here is a derived, rebuildable
projection. redb stores opaque bytes — compression and search are app-layer
concerns layered on top, by design (ADR 0003).

### 10A.1 Tiered storage

```text
SOURCE OF TRUTH — redb (single embedded ACID KV file)
  - operational state (cards, nodes, scheduler, budgets) — small, hot
  - message log: key (threadId, seq) → zstd-dict(message blob)
  - artifact INDEX rows (metadata + content hash; NOT bytes)
  hot read "latest N of thread X" = fast point/range scan
DERIVED PROJECTIONS (rebuildable from the log)
  - in-memory materialized views (reactivity; §10.3.1)
  - tantivy full-text index (§10A.2)
  - vector index (DEFERRED; §10A.4)
BLOB STORE (bytes, content-addressed; §17)
  - blake3-keyed; dedup by hash; fs cache + object storage (R2/S3)
```

### 10A.2 Compression — zstd with a trained dictionary

Messages are highly compressible (repeated JSON structure, role tokens, tool-call
schemas, system-prompt fragments). Generic per-message gzip does poorly on small
docs (no shared context). Use a **zstd dictionary trained on a corpus of real
messages** (the `zstd` crate exposes dictionary training), then compress each
message against the shared dictionary — small JSON docs reach ~10–20× vs ~3× for
naive compression. Per-message compression for hot/random-access; per-block-of-N
for cold (better ratio, slight read amplification). Compression is at the app
layer (we control the dictionary and granularity), not a store feature.

### 10A.3 Full-text search — tantivy (v1)

**tantivy** (MIT, the Rust Lucene): BM25 keyword search across all sessions,
embeddable, with columnar fast-fields for faceting. The index is a derived
projection of the message log; on schema change, rebuild from the log. "Find the
session where we discussed X" is a tantivy query. **Non-text artifacts** are made
searchable by **text extraction at ingest** (calamine for .xlsx, a pdf extractor
for .pdf, XML parse for .pptx/.docx, OCR/vision caption for images); extracted
text → tantivy, tagged with the artifact's content hash so a hit points back to
the blob.

### 10A.4 Vector / semantic search — DEFERRED (additive)

Deliberately deferred to a later phase, NOT because unwanted but because it drags
in an **embedding pipeline** (a local model = a host effect, or an API call = cost
hitting the §16 budget system). When added: embed **selectively** (user/assistant
turns, or session summaries, or on archival — never tool-spam). Because indices
are derived projections of the log, adding a vector index later is **purely
additive** — no migration, no lock-in. Library choice at that time:
**LanceDB** (Apache-2.0; unified columnar vector+FTS+multimodal, object-store
native — could subsume the message/blob store, but pulls in the Arrow dependency
tree) vs a lightweight **usearch/hnsw** index keeping redb as truth. Decide then,
with real data. (Note: LanceDB uses tantivy for its FTS, so v1 on tantivy is not
throwaway either way.)

### 10A.5 Message lifecycle (bounds the bloat)

Compression shrinks; lifecycle bounds. Mirrors §17.4:

1. **Hot** (active session): recent messages, lightly/un-compressed in redb;
   bounded window in the in-memory view.
2. **Warm** (recent sessions): zstd-dict compressed in redb; fully tantivy-indexed.
3. **Cold** (archived): whole session re-compressed into one zstd blob (best
   ratio), optionally pushed to object storage; **tantivy index retained** so it
   stays searchable; bytes paged back on demand for audit/replay.

Invariant: **searchability is permanent (indices retained); byte residency is
tiered.** Sessions stay forever searchable without staying forever resident.

---

## 11. Memory and process disposability

**Invariant: every process is disposable; only the envoy daemon (per host) and the
control-plane process are durable, and the envoy must be stateless across
commands.** Memory grows with session length, so any process must be killable
without losing important state — and truth is in the control-plane log, so killing
a leaky process and restarting it is always safe.

- **Control-plane process:** holds the redb log + in-memory views. View memory is
  **bounded by design**: views hold hot, small, indexed rows; **bulk (message
  bodies, artifact bytes, workflow step payloads) is paged from redb/blob store on
  demand, never fully resident.** A view may hold a bounded recent window per
  *active* session and evict cold sessions (LRU). Each view tracks `entry_count`
  and estimated `bytes` for observability and to catch a "resident bulk"
  regression. Total view memory is tens-to-low-hundreds of MB even at fleet scale.
- **Envoy daemon** (one durable host process): holds NO cross-command cache. State
  is limited to live subscription handles, the health server, the live
  child-process handle set (bounded by `availableSlots`, §10.1), and the port
  allocation table. Monitors its own RSS and self-restarts above a threshold;
  systemd `MemoryMax` + `Restart=on-failure` backstop. Its RSS is effectively a
  leak detector (flat = healthy).
- **Worker processes:** spawned per session, killed on session end. The envoy
  holds the PID and applies an OS memory/CPU limit (cgroup v2 via `systemd-run`,
  or `ulimit` fallback). Worktree stays on disk; process memory reclaimed.
- **Orchestrator sessions:** context lives in the control-plane log; the in-process
  window is a sliding view fetched on demand. An OOM-killed orchestrator is
  re-spawned and reloads from its thread.

### 11.1 Full memory observability (cgroups v2)

Three process classes, one mechanism — **cgroups v2**, unified with the §12.3
limits (`memory.current` is the observability, `memory.max` is the limit, same
cgroup):

- **Control plane / envoy:** own-process RSS (+ per-view byte accounting for the
  control plane).
- **Spawned agents (incl. children):** each agent runs in its own cgroup; the
  envoy reads `memory.current` (exact, incl. children), `memory.peak`,
  `memory.max` (hard cap → kernel OOM-kill), `memory.high` (soft cap → throttle +
  reclaim), `memory.events` (OOM/throttle counter, so the envoy knows an agent was
  killed for memory and reports "card failed: OOM" rather than a mystery exit).
- The envoy polls these on its heartbeat tick and reports them up; the control
  plane aggregates into a memory view (per-node, per-agent, per-session, plus
  control-plane + envoy self-RSS) surfaced in the UI.

**HOST CAVEAT (verify early):** cgroup v2 memory accounting depends on the kernel
exposing the memory controller in the unified hierarchy. On real Linux nodes
(Debian KVM, cloud VMs) this is reliable; **on WSL2 it has historically been
inconsistent.** Design for cgroup v2 as primary with `/proc/<pid>/smaps_rollup`
summation as fallback, and verify on the WSL host with a 5-line spike
(`systemd-run --scope` a process, read `memory.current`) before relying on it.

---

## 12. Sandboxing and port reservation

### 12.1 Three sandbox modes behind one interface

```ts
interface Sandbox {
  prepare(worktree: string, config: SandboxConfig): Promise<SandboxHandle>
  exec(handle: SandboxHandle, command: string[]): Promise<ChildProcessHandle>
  cleanup(handle: SandboxHandle): Promise<void>
}
```

| Mode | When | Properties |
|---|---|---|
| `HostDirect` (DEFAULT) | trusted work, personal sessions, fast iteration | runs on host, no isolation, fastest startup, no auth/image friction |
| `Bubblewrap` | ephemeral workers, parallel sessions needing isolation | private mount + PID + net namespaces; read-only system rootfs; read-write worktree only; <50 ms startup; host toolchain via bind-mount |
| `Docker` (DEFERRED) | untrusted input, reproducible per-project toolchains, sudo-inside-container, GPU/ML | full mutable env; declarative image per context/agent-type; slower startup; image+auth management cost |

- **Default is `HostDirect`.** Docker is opt-in per agent type or context, NOT
  default, and not built in the initial phase.
- **bwrap handles `rm -rf` by construction:** `--ro-bind` on system paths and
  `--bind` only on the worktree → destructive commands hit permission-denied
  outside the worktree (which is jj-recoverable). No tool-call interception layer.
- **bwrap does NOT manage toolchains;** it binds host tools in. Reproducible
  per-context toolchains, if needed, are the trigger to build the Docker backend
  (or Nix-store bind-mounts) — deferred, not built now.

### 12.2 Per-agent-type sandbox assignment (default policy)

```text
envoy daemon            → unsandboxed (it IS the host supervisor)
orchestrator (Hermes)   → HostDirect (trusted; full network for gh/webhooks/model APIs)
worker (Claude Code)    → Bubblewrap (isolation) or HostDirect (fast/trusted)
worker (Codex/Droid)    → Bubblewrap or HostDirect
```

### 12.3 Resource limits — three independent layers (all required)

| Layer | Limits | Mechanism | Granularity |
|---|---|---|---|
| OS / envoy | memory, CPU per worker process | cgroup v2 (`memory.max`) / ulimit | per session |
| Agent harness | wall-clock, tokens per run | Hermes session limits / Claude Code `--max-turns` | per run |
| Control plane | cost + concurrent sessions | scheduler checks budget before assigning | per context/project |

OS catches memory blowups; harness catches infinite loops; control plane catches
cost runaway. None substitutes for the others. Process agents get PID/cgroup-level
supervision; service agents (Hermes) rely on harness limits + gateway-level
`MemoryMax`.

### 12.4 Port reservation

The envoy owns the host's port space:

- **Sandboxed (bwrap `--unshare-net`):** each session gets a private network
  namespace; the agent binds freely on its own loopback; the envoy forwards a
  reserved host-port block in. Worker egress is **default-deny with a per-context
  allowlist** (package registries) forwarded by the envoy; no model APIs, no
  GitHub, no control-plane access for workers.
- **Host-direct:** the envoy allocates a reserved host-port block and tells the
  agent which ports to use.

Port allocation is per-node state in the envoy (mirrored to the control plane for
observability). This is the kernel-enforced capability boundary: the orchestrator
has host network; workers are confined to their allowlist and cannot reach the
artifact system, GitHub, the control plane, or other sessions' ports.

---

## 13. Inter-agent communication (within a session tree)

### 13.1 Model: local-first, filesystem-native, SDK-mediated

A session tree on one node communicates locally, NOT through the control plane:

- **Sibling → sibling:** read each other's card worktrees directly
  (`rg ../<sibling_card_id>/`, `jj log -r <sibling>`). The filesystem IS the shared
  context. Siblings do not message each other.
- **Parent ↔ child:** the orchestrator holds an **SDK handle** (Claude Code SDK,
  Codex SDK equivalent) to each child; streaming, steering, tool interception,
  structured results flow through the SDK locally at memory speed.
- **The control plane is the recording layer, not the routing layer** for local
  trees: the envoy mirrors SDK-streamed output to the control plane for
  observability; the live path is local.

### 13.2 Two communication modes

- **Async notification (fire-and-forget):** "FYI parent." Sender continues;
  receiver processes at its next step boundary. Generalized steer (§13.3).
- **Sync request (ask-and-wait):** "I need access/info." The requesting agent's
  *workflow step* suspends on the workflow engine's signal/await (it does NOT block
  a process); the parent responds; the step resumes.

### 13.3 Steering (mid-turn injection)

Steering is a message append routed to the target and surfaced at its next step
boundary (before its next LLM call):

- **Hermes:** existing steer mechanism.
- **Black-box CLI (Claude Code/Codex):** the envoy relays via the SDK's steering
  method, or stdin, or (if neither is available mid-run) queues for the next turn.
  Degradation for black-box CLIs is "arrives at next turn," not "mid-turn."

### 13.4 Sync-request safety (deadlock prevention)

- Every sync request has a **timeout** (default 5 min). On timeout the requester
  resumes with a timeout error and decides: retry / proceed without / escalate /
  fail. Never hangs forever.
- Every request carries a **`depth`** counter (tree depth); requests over a max
  depth (e.g. 5) are rejected ("max delegation depth exceeded"), bounding circular
  chains. Circular requests are rare, resolved by timeout — survived, not
  structurally prevented.
- If the parent died (orphaned by the sweeper), pending requests resolve
  `parent_unreachable`.

### 13.5 Read access to the whole session tree

Any agent may call `readSessionContext(sessionId, depth)` returning read-only
messages from any thread **in its own session tree** (membership checked by the
control plane). Lazy/on-demand pull, NOT eager push. Reading across session-tree
boundaries is forbidden — the boundary that prevents cross-session bleed.

### 13.6 Multi-node fallback (the exception)

When a card forks to a different node (capability gap, §5.6), local SDK + filesystem
are unavailable. Communication falls back to control-plane-routed: steering via a
`steeringMessages` event type, sync requests via the workflow engine's signals.
Same machinery as chat rooms (§14); built only when the first multi-node case
appears — not speculatively.

---

## 14. Chat rooms (cross-node capability bridges)

Distinct from the session tree. A chat room is a **controlled, observable channel
between named peer agents of differing access levels** — to let an agent obtain a
capability it lacks by asking a peer that has it.

### 14.1 Why it exists

Zephyr (on the rpw host) needs something from fxcluster, which only Talos (on
192.168.69.1) can reach. Zephyr requests it via a chat room; Talos fulfills it.
The room exists *because* of the access asymmetry. Neither Paperclip's hierarchy
(peers can't talk) nor Fusion's open rooms (no access control).

### 14.2 Rules

- Rooms are **created by the human operator only** (initially). An agent that needs
  a channel escalates to the operator; the operator opens the room and sets the
  participant allowlist. Agents cannot self-enroll or auto-open rooms.
- Rooms are **observable** (every message is a control-plane event the operator
  reads in the UI) and **controllable** (the operator can close a room).

### 14.3 Tables (exact)

```text
chatRooms:
  _id, name, status: "active"|"closed"
  participants: string[]         # agent/identity allowlist
  createdBy: string              # operator userId — NOT an agent
  createdAt, closedAt?: number

chatMessages:
  _id, roomId, fromAgentId: string, text: string
  kind: "message"|"request"|"response"
  requiresResponse: boolean      # request → starts a timeout
  status: "delivered"|"read"
  createdAt: number
```

- A `request` with `requiresResponse: true` starts a timeout. Peer responds →
  fulfilled; timeout → requester gets `no_response` and applies its exception logic
  (escalate / try alternative / abandon). The timeout path IS the exception
  handling.

### 14.4 Fencing for host commands

`claimEpoch` on `hostCommands` (§10.1) is the fencing token. When a command is
re-queued after lease expiry, `claimEpoch` increments; a late completion from a
zombie node holding the old epoch is rejected (§10.5 `completeCommand`
precondition). Because the scheduler is single-writer, two nodes can never both
hold a live claim; fencing additionally rejects stale writes from a previously
-claiming node that reconnects.

---

## 15. Workflow system (generic, n8n-like)

> **Generic, node-based, template-able automation engine** — not GitHub-specific.
> GitHub CI/PR loops are one *instance*, not the system.

### 15.1 What it is

A workflow is a graph of **nodes** (steps) connected by edges, executed durably by
an embedded, checkpoint-based engine. Node types (minimum): `trigger`, `agent-run`
(start/resume a card's session and await result), `condition` (branch), `loop`
(bounded), `http-request`, `cp-mutation`, `cp-query`, `await-signal`
(human-in-the-loop / external), `delay`, `sub-workflow`, `chat-room-request`,
`artifact-publish`, `notify`. Edges carry data/control. **Templates** are reusable
parameterized definitions (e.g. "code review loop" per card/PR). The **visual
builder is ours** (the n8n-like product surface); it emits the graph.

### 15.2 Execution substrate (engine: ADOPT SAYIIR — source-review verdict)

Per ADR 0003, the durable-execution engine is embedded, single-process, and
**checkpoint-based** (not deterministic-replay), backed by redb via a custom
persistence adapter. **Decision (2026-06-28, after a source-level review of the
Sayiir codebase): adopt Sayiir.** The review cloned the repo, read the engine
source, built it, and ran its 364 tests; verdict ADOPT-WITH-CAVEATS.

**What the review confirmed against source:**
- **License clean:** MIT across every crate in the dependency path
  (`sayiir-core`, `sayiir-persistence`, `sayiir-runtime`), all-permissive
  transitive tree, zero copyleft. Shippable.
- **Determinism risk eliminated (verified):** `execute_or_skip_task` returns the
  cached output for any already-completed task **without re-running user code**
  (`helpers.rs:456-473`); no purity/replay constraints. This was the *only*
  justification for building our own — Sayiir already solves it correctly.
- **`PersistentBackend` is a clean 10-method trait** (`SnapshotStore` +
  `SignalStore`; `backend.rs:543-545`). The redb adapter is **~300–500 LOC**,
  HIGH feasibility, with the bundled `InMemoryBackend` as reference. (For embedded
  single-process you do NOT implement `TaskClaimStore`/`TaskResultStore` — those
  are distributed-only.)
- **Signal/park/resume verified end-to-end** and maps *exactly* onto Olympus's
  `agent-run`: task enqueues the host command → workflow durably parks at
  `AwaitSignal` (position persisted, survives crash) → the envoy reports
  completion via `WorkflowClient::send_event` → `resume` consumes the event and
  advances. Park-before-completion ordering is favorable (idempotent re-entry).
- **The single-process durable runner is `CheckpointingRunner` in
  `runner/distributed.rs`** (despite the filename); there is no separate
  `DurableEngine` struct in Rust.

**Caveats now baked into our design:**
1. **At-least-once durability** (crash between a step's side effect and its
   checkpoint re-runs the step) — identical to what build-our-own would yield, and
   exactly why `agent-run` MUST be idempotent (below).
2. **Snapshot bloat:** the snapshot persists every completed task's output bytes
   and is rewritten each step → O(n²) without mitigation. The redb adapter MUST
   implement strip/hydrate (the Postgres backend shows the pattern,
   `snapshot.rs:684`), and **agent-run steps MUST return refs/IDs, not payloads**
   (the bulk lives in our message store, §10A).
3. **No task-completion hook:** Sayiir's "OpenTelemetry" is `tracing` spans only —
   no callback API. The §15.6 audit surface must be built as a thin layer that
   subscribes to `tracing` events or reads the snapshot's `completed_tasks`
   journal after each step.
4. **Bus factor:** effectively a single maintainer, 1.0.0 brand new. Mitigated by
   MIT + a small, zero-`unsafe`, vendorable core — **limit our dependency surface
   to `sayiir-core` + `sayiir-persistence` + the `CheckpointingRunner` path** so a
   future fork stays cheap. Do NOT depend on its Postgres/Node/Python/Cloudflare
   surfaces.
5. **Codec:** prefer `CodecId::Json` for the durable store (the engine's own
   recommended durable default) over rkyv, which is layout-fragile across struct
   changes.

We embed the **single-process `CheckpointingRunner`** with our **redb
`PersistentBackend`** (the ~300–500 LOC adapter, doing strip/hydrate) and a thin
**tracing/journal audit layer**. NOT the distributed `PooledWorker` mode — Olympus
nodes execute via our own command/event protocol, not Sayiir workers.

The engine provides: durable checkpoint/resume across restart, signals
(await/send) for human-in-the-loop and cross-step waiting, parallel steps, nested
workflows, cancel, retries, timeouts, delays. **Durability is at-least-once** for
side-effecting steps — so `agent-run` MUST be idempotent: it enqueues a
`hostCommand` keyed by a deterministic `(workflowId, stepId, attempt)` id, so a
re-run reuses the same command rather than spawning a duplicate.

### 15.3 The cardinal rule

**Workflows orchestrate agent SESSIONS; they do not run agent code.** An
`agent-run` node does not execute an LLM loop inside the workflow handler. It
enqueues a `hostCommands` event (idempotent, §15.2) and suspends on a signal until
the envoy reports completion. The workflow is the conductor; agents are the
musicians.

**"Resume a session" means spawn-fresh-and-reload, not literal process resume.**
Because processes are disposable (§11), resuming a card's session is: the envoy
spawns a fresh agent process in the card's existing workdir and reloads the
conversation context from the control-plane thread (plus, on reassignment, the
"previous attempt" block, §6.2). No suspended OS process is woken.

### 15.4 Worked example — code review loop (must be implementable from this)

```text
Template: code-review-loop
Params: { cardId, prBranch, maxIterations }

[trigger: card.session.completed]
   → [agent-run: reviewer examines prBranch]      # enqueue host cmd (idempotent), await signal
       → [condition: verdict == "approved"?]
            ── yes → [artifact-publish: mark PR ready] → [notify: operator] → END
            ── no  → [agent-run: coding agent revises with feedback]
                       → [loop guard: iteration < maxIterations?]
                            ── yes → back to "reviewer examines"
                            ── no  → [cp-mutation: record max_iterations] → END
```

- The loop bound (`maxIterations`) is the hard execution limit.
- Each step is checkpointed; a crash resumes from the last checkpoint (no re-run of
  completed steps; re-run of an in-flight side-effecting step is idempotent,
  §15.2).
- Audit trail = the workflow run journal (steps + results) + the message view (what
  each agent said).

### 15.5 Triggers (generic)

| Trigger type | Source → mechanism |
|---|---|
| Agent / card event | control-plane event → `workflow.start` |
| Webhook (any provider) | axum HTTP handler → `workflow.start` |
| Chat-room event | chat message event → `workflow.start` |
| Scheduled | tokio timer → `workflow.start` |
| Manual | UI action → `workflow.start` |

GitHub (CI completion, PR review requested) is just the "webhook" row with a
GitHub-shaped payload. Nothing in the engine is GitHub-specific.

### 15.6 Audit surface

`workflowRuns` records `workflowName`, `status`, `startedBy`, `trigger
{kind,source}`, `startedAt`, `completedAt`, and the step journal (derived from
engine checkpoints/events). One view lists runs filterable by trigger / agent /
status; clicking a run shows the full step trace. "What workflow was triggered by
whom, the history, and everything" is this view. **Sayiir exposes NO
task-completion callback** (its "OpenTelemetry" is `tracing` spans only) — so this
audit surface is built as a thin layer that subscribes to `tracing` events or
reads the snapshot's `completed_tasks` journal + `last_completed_task_id` after
each step (§15.2 caveat 3).

---

## 16. Budget and subscription tracking

> Models **finite subscription quotas**, not just token-times-price cost. Tracks
> multiple subscriptions/API keys per provider, their usage, and remaining limits —
> including Claude Code and Codex subscription caps.

### 16.1 Tables (exact)

```text
subscriptions:
  _id, contextId
  provider: "anthropic"|"openai"|"zai"|...
  type: "api_usage"|"subscription_quota"
  apiKeyRef: secretRef                       # NEVER plaintext; control-plane secret store
  pricing?: { inputPerMillion, outputPerMillion }     # for type=api_usage
  quota?: { unit: "tokens"|"messages"|"requests", inputLimit, outputLimit, windowMs }
  used: { input, output, requests, costUsd }
  windowStartsAt: number
  status: "active"|"exhausted"|"expired"|"paused"
  priority: number                            # higher = preferred for routing

budgets:
  _id, scope: "context"|"project"|"session", scopeId: string
  maxCostUsd, maxTokens?: number
  currentCostUsd, currentTokens: number
  periodMs, periodStartsAt: number

usage:                                        # one event per generation/tool call
  subscriptionId, contextId, projectId?, sessionId, cardId?, agentId
  model, inputTokens, outputTokens, costUsd, toolName, timestamp
```

### 16.2 Recording

- Control-plane utility inference: usage recorded at the call site with
  `subscriptionId` and `toolName`.
- Host agents: the envoy parses the CLI's usage metadata (Claude Code/Codex report
  token counts in output) and appends `usage` events + decrements the
  subscription's remaining quota.

### 16.3 Enforcement (in the scheduler, before assignment)

```text
inside assignPendingCommands (§10.5):
  - if context/project budget exhausted → leave pending, notify orchestrator
  - pick subscription with remaining quota for the required model
      (filter status=active, has capacity; order by priority) → none → fail "no_capacity"
  - reserve budget + bind subscriptionId atomically with the assign
```

### 16.4 Subscription-aware routing (the novel logic)

When the orchestrator delegates "run Claude Code," it does NOT name an API key. The
scheduler selects the active Claude subscription with the most remaining quota for
the context; the envoy injects that key into the worker's env. When one is
exhausted, the next routes onward; when all are exhausted, fall back to an API key
if one exists, else "Claude capacity exhausted." Single-writer selection +
reservation means two workers cannot drain the last of a quota concurrently.

### 16.5 UI surface

A subscriptions dashboard: per provider, each key/subscription with used vs limit
and reset window (e.g. "Claude Code Max: week 3 of 4, 340/500 messages, resets in
4 days"); plus per-context budget and provider allocation. Being per-context, it
reports exactly what corporate vs personal work costs.

---

## 17. Artifact management

### 17.1 What an artifact record indexes

An artifact is anything a session produces with value beyond the message log. The
`artifacts` index includes, per session/card:

- **Generated files** — paths produced in the workspace, with type, size, content
  hash, producing session/card/agent.
- **Pull requests** — PRs opened (number, repo, branch, URL, state), linked to the
  card and the orchestrator that opened them.
- **Build outputs / prototypes** — preview builds servable over the tunnel.
- **Other** — any registered media/document.

```text
artifacts:
  _id, sessionId, cardId?, contextId, projectId?
  kind: "file"|"pull_request"|"build"|"document"|"image"|"video"|"other"
  producedByAgentId: string
  contentHash?: string          # blake3; key into the blob store (§10A.1)
  # file:        path?, sizeBytes?, mime?
  # pull_request: prNumber?, repo?, branch?, url?, prState?
  # build:       servePath?, previewUrl?, ephemeral?
  title?, description?: string
  createdAt: number, promotedAt?: number   # promotion = ephemeral → durable
```

### 17.2 Production vs publication (the boundary)

- **Workers PRODUCE** artifacts: write files into their card worktree
  (`sessions/<session_id>/<card_id>/.olympus/artifacts/`). Just filesystem; no
  network.
- **The orchestrator PUBLISHES**: registers them in the index, promotes ephemeral
  builds to durable storage, opens PRs. Workers cannot reach the artifact system
  (netns, §12.4). Mirrors orchestrator-only GitHub access (§5.5): the orchestrator
  is the single chokepoint for anything leaving the local environment.

### 17.3 Serving and the blob store

- **Bytes are content-addressed** (blake3): stored once per hash (dedup), local fs
  as cache/ephemeral tier, object storage (R2/S3) as durable tier. The `artifacts`
  index holds the hash, never the bytes.
- **Ephemeral** (session/card-scoped prototypes): the envoy runs a static file
  server over the card's `artifacts/` dir on a reserved port; the UI proxies
  `…/artifacts/<session_id>/<path>` to the owning node (directly or via
  Netbird/Cloudflare/iroh tunnel). Cleaned up on session archive.
- **Durable** (shared beyond the session): the orchestrator uploads to object
  storage; served from there. Promotion is explicit and logged.

### 17.4 Workspace lifecycle (must be designed in, not retrofitted)

`~/olympus/sessions/<session_id>/` grows unbounded without a lifecycle:

- **Archive:** freeze the workspace, keep it readable, promote durable artifacts
  first, re-compress the session per §10A.5.
- **Cleanup:** delete the workspace after a grace period (configurable per
  context). Index rows + tantivy entries survive; bytes are gone unless promoted.
- The envoy runs cleanup on a schedule driven by control-plane state, so node disks
  do not fill.

---

## 18. SSH / terminal interface from Olympus to each node

The operator must be able to open an interactive shell on any node from the UI.

### 18.1 Mechanism

- The envoy spawns an interactive shell via a **PTY** (`Bun.Terminal` if envoy is
  Bun; a Rust PTY crate such as `portable-pty` if envoy is Rust) — or `ssh
  <target>` for hops beyond the node — and bridges the PTY both ways over its
  Transport (UDS/iroh) to the control plane, which relays to **xterm.js** in the
  UI over WSS.
- A terminal session is opened via a `hostCommands` row of kind `terminal.open`;
  keystrokes flow as `terminal.input`; output streams back; `terminal.resize` /
  `terminal.close` manage lifecycle.
- Terminal access is an **operator capability**, not an agent capability:
  operator-initiated, scoped to nodes the operator may reach, fully audited (every
  terminal session is a control-plane event).

### 18.2 Why this is cheap

The envoy already owns PTYs (Layer 2), so terminal bridging is an extension of its
existing responsibility, not a new subsystem. (Bun ships `Bun.Terminal` natively;
Rust has mature PTY crates — no native addon either way.)

---

## 19. The AgentRuntime boundary (envoy command queue + per-harness stdio adapter)

The envoy holds, per managed session, the **process handle** (PID + lifecycle)
AND the **stdio control channel** to the spawned harness. Olympus does not reach
into an agent's memory; it sends a command to the envoy, and the envoy's
per-harness adapter translates that command into the harness's native
bidirectional stdio protocol. This is the industry-converged pattern: every
modern harness exposes a long-lived JSON-RPC-over-stdio control protocol with a
control lane (steer/cancel) separate from the data lane (prompt/stream).

### 19.1 The interface

```ts
// Envoy owns a COMMAND QUEUE per managed session. Olympus enqueues; the envoy
// dispatches to the right per-harness adapter. Commands are uniform across
// harness types; the adapter maps each to the harness's native protocol.
type AgentCommand =
  | { kind: "prompt"; text: string; model?: string }
  | { kind: "steer"; text: string }          // inject guidance mid-turn
  | { kind: "cancel" }                        // interrupt the current turn
  | { kind: "stop" }                          // end the session, release handle
  | { kind: "switchModel"; model: string }    // change model for the session
  | { kind: "slash"; command: string };       // harness slash command (/usage, etc.)

interface AgentRuntime {
  start(session: SessionSpec): Promise<RunHandle>;   // spawn + open stdio channel
  send(cmd: AgentCommand): Promise<void>;            // enqueue → native protocol
  events(): AsyncStream<AgentEvent>;                 // stream: text, tool calls, reasoning, done
  stop(): Promise<void>;                             // graceful end + handle release
}
```

The control lane (`steer`, `cancel`) and the data lane (`prompt`, streamed
`events`) run over the **same long-lived stdio channel** the envoy holds. Because
the envoy owns the channel, it does the steer/cancel call itself — no file, no
socket, no in-process method call across a boundary, no fork-side change.

### 19.2 Per-harness adapters (all stdio control protocols)

| Adapter | Native protocol | prompt | steer | cancel/stop |
|---|---|---|---|---|
| `HermesAgentRuntime` | **ACP** (Agent Client Protocol, JSON-RPC over stdio) via `hermes acp` | `session/prompt` | `steer` (ACP method — *"Inject guidance into the currently running agent turn"*, already in `acp_adapter/server.py`) | `session/cancel` (sets cancel_event + `agent.interrupt()`, already present) |
| `ClaudeCodeRuntime` | Claude Agent SDK streaming-input mode (CLI subprocess, stdio) | push message | streaming input | `interrupt()` |
| `CodexRuntime` | Codex **app-server** (JSON-RPC 2.0 over stdio) | `sendUserMessage` | streamed input | interrupt |
| `DroidRuntime` / `OpenClawRuntime` | their native stdio protocol | — | — | — |

**KEY FINDING (de-risks the MVP):** Hermes's ACP adapter already exposes
`steer`, `cancel`, and `prompt` with streaming `session/update` notifications.
ACP is the same protocol family Claude Code and Codex use. **The Hermes
integration is "drive `hermes acp` over stdio" — no Hermes fork change required
for steer/cancel/prompt/streaming.** Earlier file-based-steer and
gateway-endpoint proposals are rejected (§21).

### 19.3 Orchestrator vs worker

The orchestrator is the *same* runtime impl as a worker but configured with
control-plane tools (`delegateToWorker`, `listNodes`, `recoverSession`,
`cancelWork`, `publishArtifact`, `requestChatRoom`). "Orchestrator vs worker" is a
tool-set + privilege configuration, not a separate code path.

### 19.4 Where it lives + prove the seam

This boundary lives in the envoy (Layer 2): the envoy holds the PID + stdio
channel and implements `AgentRuntime` per harness type. **Prove the seam with a
second impl early** — `HermesAgentRuntime` (ACP) + a trivial `ClaudeCodeRuntime`
validates that the uniform command queue maps cleanly onto two different native
stdio protocols before orchestration logic assumes one runtime shape. Two real
impls prove it; five stubs do not.

---

## 20. Node death and recovery

Olympus **accepts node death**; it does not fight it with live migration.

- **Detection:** the `reap` sweeper (§10.6) marks a node `unreachable` when
  `lastHeartbeat` is stale, fails its active `hostCommands` (resuming parked
  workflow steps with `node_unreachable`), and orphans its active sessions/cards.
- **What survives:** message history, workflow journal, tool-call records, card
  records, the artifact index — all in the control-plane log. The git worktree
  (files, commits) is on the node's disk and is best-effort (recoverable only if
  the node returns).
- **Recovery:** the card is reset and a new session is created on a healthy node
  (§6.2), **seeded with the orphaned session's message trace** as a "previous
  session attempt" block. The orchestrator (or operator) reads the trace, decides
  what was accomplished, and continues as a new jj attempt-bookmark (§6.4).
  Recovery is an intelligent decision, not a fencing-token re-queue.
- **Recovery base case (who recovers the recoverer):** worker/card sessions are
  recovered by their orchestrator. A **main/orchestrator** session has no parent
  agent, so its recovery is initiated by a **scheduled control-plane function**
  (`reapOrphanedMainSessions`) that detects orphaned main sessions and either (a)
  re-spawns the orchestrator on a healthy node seeded with its thread, or (b)
  surfaces it to the operator — per a per-context `autoRecoverOrchestrator: bool`.
  Terminates the recursion: cards ← orchestrator ← scheduled function ← operator.
- **Accepted limitation (state in user-facing docs):** Olympus guarantees state
  correctness (the log) and execution durability (workflow journal). It does NOT
  guarantee execution continuity across node failures — a dead node's session is
  orphaned and recovered as a new attempt, and filesystem artifacts on a dead node
  are best-effort. The deliberate trade for architectural simplicity (no
  distributed consensus, no fencing-token live migration).

---

## 21. Rejected alternatives

- **Hermes gateway socket / `/api/pty` as the bridge.** The gateway's only
  programmatic surface is `/api/pty` — a raw PTY byte stream for xterm.js, not a
  structured protocol; extracting tool calls would require parsing ANSI. And a new
  structured gateway endpoint would couple Olympus to the gateway's lifecycle.
  Rejected in favor of driving `hermes acp` (ACP over stdio) directly from the
  envoy (§19), which already exposes structured prompt/steer/cancel/streaming.
- **File-based steer drain (envoy writes a `.olympus/steer` file the agent polls).**
  Unnecessary once ACP is the control lane: the envoy owns the stdio channel and
  calls ACP `steer` directly. Rejected as a redundant, non-native channel.
- **One-shot `hermes -z` per message as the bridge.** Works for send/receive but
  cannot steer, switch model mid-session, or run slash commands (those need a live
  in-process agent). Rejected for managed sessions; the persistent ACP process is
  used instead. (`hermes -z` remains usable only for trivial fire-and-forget.)
- **JSONL session files as the sync/import source.** Incomplete — only ~135 JSONL
  files exist on disk vs 1,629 sessions in state.db (JSONL is pruned/rotated and
  carries thinner metadata). Rejected in favor of reading `state.db` (WAL-mode
  concurrent reads), which is the complete archive and the resume source Hermes
  itself uses (§6.7).
- **Continuing an external-channel session in place.** Produces cross-channel
  divergence (Olympus turns absent from the origin channel). Rejected in favor of
  always forking a non-Olympus session (§6.6).
- **Convex (and any external DB-as-server backend).** See ADR 0003: two identity
  systems vs transport-native identity, SPOF connection terminus, distributed-ACID
  we don't need, an external backend to operate, and (for SpacetimeDB) the same
  transport/identity friction reintroduced. Rejected in favor of a Rust-native
  single-binary control plane.
- **Restate / n8n / Windmill (workflow engines).** BSL / fair-code / AGPL — not
  shippable in a closed product. Rejected; the workflow engine is Sayiir (MIT) or
  in-house, and the n8n-like surface is ours (§15, ADR 0003).
- **Port Hermes into the control plane as the orchestrator.** Hermes's value (PTY,
  spawn, filesystem, skills, local model switching) is inseparable from its host
  environment. Rejected; Hermes runs as a host process behind `AgentRuntime` and
  remains the reference implementation for context-eviction and tool-semantics.
- **Adopt Paperclip.** Heartbeat/ticket model makes interactive agency
  structurally impossible. Rejected.
- **Build on Fusion's substrate.** SQLite + SSE + multi-leader replication; its
  self-healing layer is a symptom of that fragility. Rejected as the substrate;
  Fusion remains a reference for failure modes and board/lifecycle rules.
- **Control-plane-only (no envoy).** The control plane is one authority over many
  hosts; host effects are inherently per-host. The envoy is a hard requirement.
- **Docker for every agent by default.** Startup latency, image maintenance,
  in-container auth friction. Host-direct default; bwrap opt-in; Docker deferred.
- **System-enforced context isolation.** Impossible to hard-block an LLM from
  reasoning about facts it learned without separate deployments (destroys the
  single-identity value). Convention-enforced at reasoning, hard at credential/
  workspace/network.
- **Live session migration across nodes.** Distributed consensus + fencing — the
  complexity being avoided. Session-node affinity + orphan-and-recover instead.
- **Card with many concurrent sessions.** 1:1 + stable task-based directory +
  per-attempt jj bookmarks instead; reassignment is the exception.
- **Tool-call interception layer for safety (à la Evonic).** `rm -rf` and egress
  are handled by bwrap + netns at the kernel level. Rejected as wrong-layer.
- **Webhook-driven GitHub workflow engine.** Too narrow; generic node/edge/template
  engine of which GitHub is one instance.
- **Skills/MCP inherited silently per context.** Library is read-all for discovery;
  activation is explicit and scoped. No silent inheritance.
- **iroh-docs / a CRDT layer for cross-node state — REVISED, not rejected.** The
  original stance ("jj is the file-sync substrate; no CRDT layer needed") held for
  a fully-centralized design (position A). Under the adopted **position C** (§2.6),
  iroh-docs (CRDT KV) + iroh-blobs (content-addressed blobs) ARE the **document +
  blob plane** for the future local-first knowledge layer, and **jj is narrowed to
  the code plane only** (it has no LFS/partial-clone and surfaces conflicts rather
  than auto-converging — wrong for local-first docs, right for code). This is
  **deferred to post-MVP** (the document plane is north-star, not MVP); coordination
  state stays central single-writer regardless. See §2.6.

---

## 22. Consequences

- Olympus is fleet infrastructure, not an agent. Hermes and other agents are
  managed, not reimplemented.
- The control plane is a self-contained, shippable Rust binary; the envoy is the
  single durable host-level process and trust boundary per host. Everything else
  is disposable.
- Olympus owning the workdir eliminates the Hermes Studio cross-contamination bug.
- **Owned now (the real cost of removing Convex):** the reactive-view + delta
  -broadcast layer (§10.3.1) and the message/streaming model are ours to build —
  Convex's batteries. We run redb + tantivy + blob store (+ later vector) instead
  of one store; each is the right tool, and the derived ones rebuild from the log.
- **Gained:** transport-native unspoofable node identity; no SPOF terminus; one
  identity system per transport; predictable GC-free memory; a simpler correctness
  model (single-writer for the contended core, deterministic views for reads); full
  shippability (all deps MIT/Apache).
- **Operational reality:** remote nodes need reachability to the control plane over
  iroh (NAT-traversed) or a tunnel; the control-plane process is a per-deployment
  critical component (run it supervised; HA is future work, easier than Convex HA
  because it is a single binary over redb).
- **Behavioral correctness is NOT free.** The substrate guarantees state/view
  correctness; it faithfully records an agent that fakes completion. A verification
  layer (verifiable artifact = mergeable commit / passing tests / accepted diff,
  not self-report) is still required.

---

## 23. Build order (phasing)

Build in this order; each phase is independently verifiable. Do not build later
phases speculatively.

1. **Control-plane core + transport + scheduler.** redb event log + a first
   in-memory view; the `Transport` trait with **UDS** (local) first; node register
   + heartbeat; the scheduler assign/claim/renew/complete mutations with
   `availableSlots` accounting + `claimEpoch` fencing (§10.5); the `reap` sweeper
   (§10.6); group-commit durability. Run `echo` end-to-end (enqueue → assign →
   claim → stream output → complete → slot released). Verify: a 60 s streaming
   command cancelled at 30 s, and a kill-the-node test proving `reap` requeues and
   releases the slot exactly once. **This is the correctness core; everything
   builds on it.**
2. **AgentRuntime + second impl.** `HermesAgentRuntime` (real) + `ClaudeCodeRuntime`
   (shell-out). Prove the seam (§19).
3. **Filesystem hierarchy + workdir lifecycle + jj colocate** with the conflict
   guard (§5).
4. **Cards + board** (1:1 worker session, task-based dirs, per-attempt bookmarks,
   reassignment + "previous attempt" block) (§6).
5. **Reactive views + delta streaming + browser WSS** — the React client subscribes
   to a live view (cards-by-status) and updates on mutation (§10.3.1, §2.5). **This
   is the Convex-replacement risk surface; validate it early.**
6. **Local inter-agent comms** (SDK handles, filesystem reads, control-plane
   mirror) for one orchestrator + worker cards on one node (§13).
7. **Sandbox: HostDirect → Bubblewrap + port reservation via netns** (§12).
8. **Storage tiers: zstd-dict message compression + tantivy FTS** (§10A) — derived
   index, rebuildable.
9. **Skills/MCP library + scoped activation** (§9); **node desired-state
   reconciliation** incl. library refresh (§7).
10. **Workflow engine** — adopt Sayiir (redb adapter) or build equivalent; the
    code-review-loop template with idempotent `agent-run` (§15, ADR 0003).
11. **Budget + subscription tracking + subscription-aware routing** — folded into
    the scheduler assign step (§16, §10.5).
12. **Artifact management + content-addressed blob store + serving + lifecycle +
    text extraction** (§17, §10A.3).
13. **Knowledge vaults** (jj-synced, write+sync paths, non-text refs) (§8).
14. **iroh transport** for remote nodes (second `Transport` impl; NodeId allowlist
    + per-context grants) (§2.5, §10.7); **SSH/terminal bridge** (§18).
15. **Identity/context/session/project isolation + memory stores** (§3) — threaded
    through from the start at the data level, surfaced as a feature here.
16. **Vector/semantic search** (§10A.4); **orchestrator recovery**
    (`reapOrphanedMainSessions`, §20); **chat rooms** (§14) + **multi-node fallback
    comms** (§13.6) — last, only when a real cross-node/capability-bridge case
    exists.

Deferred (build only when a concrete requirement forces it): Docker sandbox backend
(reproducible toolchains / untrusted input), Nix-store toolchains, multi-node
session-tree fan-out, control-plane HA.
