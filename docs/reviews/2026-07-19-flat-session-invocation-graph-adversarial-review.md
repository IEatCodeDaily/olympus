# Adversarial review — flat session spaces and typed invocation graphs

- Date: 2026-07-19
- Reviewer: delegated adversarial architecture agent
- Reviewed for: ADR 0027
- Source session: `20260719_210023_2d24f6`
- Disposition: blocking findings incorporated into ADR 0027 before acceptance

## Verdict

**NOT READY**

The direction is sound—flat independent workspaces, canonical graph lineage, derived context files, and noncanonical symlinks are preferable to filesystem-encoded ancestry—but the proposal is not ADR-ready until it closes several policy-bypass and consistency holes.

Most critically:

- It contradicts accepted ADR 0005’s nested subsession layout without explicitly superseding it.
- “Workflow triggers do not consume agent depth” permits unlimited effective recursion unless workflow transitions preserve rather than reset inherited agent depth and carry a separate bounded workflow ancestry.
- The current session model cannot represent the proposed typed graph safely: it has one untyped `parent_session_id`, stringly typed fork kinds, and non-atomic creation/lineage updates.
- Authorization, budgets, retention, node ownership, context generation, and concurrent mutation semantics are not yet defined strongly enough to keep Hall canonical.

## Source-aware observations

- ADR 0005 explicitly makes subsessions nested under their parent and places the whole tree on one node: `docs/adrs/0005-org-scoped-local-first-resource-model.md:145-163`. The proposed flat layout directly supersedes this.
- ADR 0013 forbids workflow recursion and runtime sub-DAG generation: `docs/adrs/0013-workflow-kernel-bounded-chains.md:55-63,89-100`. Indirect recursion through a workflow-created agent would otherwise bypass that prohibition.
- ADR 0012 requires workflow authority to be an intersection and forbids elevation: `docs/adrs/0012-programmable-operating-environment.md:69-88`.
- ADR 0017 already requires exact typed authorization, child capability narrowing, and explicit runtime-attempt binding: `docs/adrs/0017-session-cutover-and-remote-development-plane.md:132-179`.
- Current lineage is not a typed graph:
  - `SessionForked` stores a string `fork_type` and a single parent/child pair: `crates/control-plane/src/event.rs:115-137`.
  - `SessionView` collapses forks and handovers into one `parent_session_id`: `crates/control-plane/src/views/session.rs:66-74,236-258,290-300`.
  - SQLite likewise stores only `parent_session_id`: `crates/control-plane/src/log.rs:744-766`.
- Current subsession creation is not an acceptable atomic lineage write:
  - The draft session is persisted first.
  - Views are then mutated directly.
  - `SessionForked` is appended separately, and append failure is only logged before the edge is still applied to the view.
  - Capability assignment is another separate append.
  See `crates/control-plane/src/server/routes/sessions.rs:2048-2125`.
- The current route has no depth check at all: `crates/control-plane/src/server/routes/sessions.rs:1993-2046`.
- The repository has already encountered cross-org lineage traversal as a security defect: `docs/postmortems/0004-subsession-cross-organization-lineage.md`.

---

# Blocking findings

## B1. The proposal silently contradicts accepted filesystem semantics

ADR 0005 currently mandates:

```text
<parent>/sessions/<child>/sessions/<grandchild>/...
```

and states that the entire tree is colocated on one node. Flat workspaces at:

```text
<org>/sessions/<session_id>
```

are a materially different placement and portability model. The new ADR must explicitly supersede ADR 0005 §4.2 and all code/docs that treat ancestry as a path or imply descendant colocation.

Without that declaration, both layouts are normative.

## B2. Workflow-to-agent transitions create a depth-reset bypass

“Workflow triggers do not consume agent spawn depth” is safe only if it means **do not increment**. It must not mean **start a new depth counter**.

Otherwise:

```text
agent depth 3
  -> workflow
  -> workflow agent depth 0
  -> three more subagents
  -> workflow
  -> reset again
```

creates unbounded agent expansion while every local check reports depth ≤3.

A workflow-created agent must inherit the initiating causal chain’s current agent depth. Its first subagent is `inherited_depth + 1`. A scheduled workflow without an initiating session begins under its service principal at depth 0.

## B3. Workflow recursion remains possible indirectly

ADR 0013 prohibits workflow recursion, but the proposed distinction permits:

```text
workflow A -> agent -> workflow B -> agent -> workflow A
```

This is semantically recursive even though no YAML definition contains a workflow-call step.

The design needs:

1. A durable causal ancestry for workflow runs.
2. A separate workflow nesting counter.
3. Active-ancestry cycle rejection by pinned workflow-definition digest.
4. A hard maximum workflow nesting depth.
5. A prohibition on activity providers starting workflow runs outside Hall’s typed `workflow.run` operation.

Without these, ADR 0013’s scope ceiling is bypassed rather than preserved.

## B4. “Typed graph edges” are underspecified

The ADR must define which edges are causal and which are merely referential. At minimum, these are not interchangeable:

- `agent_spawn`
- `workflow_trigger`
- `session_fork`
- `session_handover`
- `context_reference`
- `completion_to`

Only causal edges should participate in depth, budget, cancellation propagation, and active-ancestry cycle checks.

Each execution node must have at most one immutable **causal parent**. It may have multiple noncausal references. If multiple causal parents are allowed, depth, authority, ownership, cancellation, and budget inheritance become ambiguous.

The current `parent_session_id` projection cannot express these distinctions.

## B5. Lineage creation is not transactional

Creation must durably commit, in one Hall transaction:

- node identity;
- organization and project ownership;
- typed causal edge;
- inherited capability envelope;
- inherited depth values;
- budget reservation;
- workspace materialization intent;
- idempotency key.

No node or edge may become visible independently. Envoy must not materialize the workspace or spawn an agent before that transaction commits.

This is a current-source blocker, not theoretical: existing subsession creation can expose an orphan session or an edge that exists only in memory (`routes/sessions.rs:2048-2125`).

## B6. Authorization propagation is incomplete

Every causal transition must be authorization checked using durable endpoints and the authenticated initiating principal.

Required rules:

- Both edge endpoints must belong to the same organization unless a future explicit cross-org transfer protocol is designed.
- Child-agent authority is:
  `initiator ∩ parent_session ∩ requested_child ∩ scope_policy ∩ runtime/provider_grant`.
- Workflow authority remains ADR 0012’s:
  `caller ∩ workflow_manifest ∩ scope_policy ∩ provider_grant`.
- A workflow-created agent receives no more than the workflow run’s effective authority.
- A scheduled workflow uses a dedicated revocable service principal; it does not inherit the publisher’s or last editor’s authority.
- Every spawn, workflow start, step dispatch, context traversal, completion delivery, resume, and regeneration revalidates current ownership and revocation.
- Generated `lineage.json`, `CONTEXT.md`, filesystem location, and symlinks are never authorization evidence.

This must incorporate the lesson from `docs/postmortems/0004-subsession-cross-organization-lineage.md`.

## B7. Budgets can be laundered across workflow boundaries

Separate per-agent and per-workflow limits are insufficient. A caller could alternate between workflows and agents to evade each local quota.

Hall needs one durable **causal execution budget account** shared by the root and all descendants, regardless of edge type. It must cover at least:

- active and total agent sessions;
- active and total workflow runs;
- workflow steps and retries;
- model tokens or provider spend;
- CPU/memory time;
- wall-clock lifetime;
- output/artifact bytes;
- workspace and context-manifest bytes;
- fan-out/concurrency.

Reservation must occur atomically before spawn or dispatch. Retry and recovery attach to the existing reservation; they do not receive a fresh budget. Child budgets may narrow remaining authority but cannot mint additional capacity.

## B8. Cycles must be rejected in canonical state, not handled only during rendering

The causal subgraph must be a DAG. Hall must reject:

- self-edges;
- an edge whose target is already an ancestor of its source;
- a second causal parent;
- edges to missing, tombstoned, wrong-org, or incompatible node types;
- attempts to reinterpret an existing edge’s type.

Cycle detection during `CONTEXT.md` generation is too late: authorization and budget traversal would already be ambiguous.

Noncausal reference edges may form cycles only if they are explicitly excluded from inheritance and rendered with visited-set and size bounds.

## B9. Generated transitive context can leak data and scale quadratically

“Transitive context” cannot mean concatenating all ancestor transcripts or workspaces. That creates:

- O(depth × history) duplication per descendant;
- stale copies after edits or revocation;
- prompt-injection amplification;
- secret leakage from ancestors;
- large regeneration storms when an ancestor changes;
- nondeterministic context under concurrent updates.

Generated context must be a bounded, capability-filtered snapshot containing stable summaries and logical references, not arbitrary ancestor file contents. It needs a graph revision/watermark, schema version, deterministic ordering, byte/node limits, truncation markers, and atomic replacement.

## B10. Node-local path and placement semantics are missing

A durable graph edge must never contain a host path as identity. Paths are Envoy-local projections of logical resource IDs.

The ADR must define:

- session IDs as opaque, path-safe identifiers validated before joining;
- canonical containment beneath the configured org session root;
- no reliance on `/`, `\`, case sensitivity, inode identity, device IDs, or symlink support;
- no assumption that causal relatives are on the same node;
- translation of logical repo/vault/artifact attachments through Envoy’s resolver;
- session move/recovery as a new fenced materialization generation;
- rejection of writes from stale Envoy/runtime attempts.

Flat layout is acceptable only if no correctness path scans the directory. Hall and the Envoy reconciliation index must enumerate sessions; filesystem enumeration is repair-only.

## B11. Retention semantics could destroy required lineage or pinned inputs

Deleting a workspace is not equivalent to deleting its graph node.

At minimum:

- causal nodes and edges, tombstones, ownership, definition digests, authority decisions, budget charges, and audit facts remain durable while any retained descendant/run references them;
- workflow definitions and providers remain pinned while any nonterminal run depends on them, consistent with ADR 0013 lines 79-81;
- transcript/blob retention may expire independently, represented as unavailable—not as “never existed”;
- parent deletion cannot orphan descendants;
- node-local cleanup is driven by a Hall tombstone plus fenced Envoy reconciliation and acknowledgment;
- cleanup of one workspace must never recursively delete descendants based on filesystem nesting or symlinks.

## B12. Concurrent mutation and regeneration semantics are missing

“Hall is the single writer” does not eliminate races between async requests, scheduler actions, cancellation, retention, and Envoy reconciliation.

Every mutation needs:

- operation/idempotency ID;
- expected graph revision or transactional invariant check;
- atomic append/projection commit;
- deterministic duplicate outcome;
- runtime/materialization attempt epoch;
- stale-attempt fencing.

Each session workspace needs one Envoy reconciliation owner per materialization generation. `lineage.json` and `CONTEXT.md` must be written by temporary file plus flush/fsync as supported, atomic rename, and parent-directory durability where required. Readers must see either the prior complete generation or the new complete generation, never a partial file.

---

# Nonblocking findings

## N1. Flat directory fan-out is an operational scaling hazard

Large organizations may accumulate enough direct entries under `sessions/` to make scans, backup tools, antivirus/indexing, and repair expensive. V1 may retain the flat physical shape, but the path must be explicitly nonpublic and resolver-owned so a future sharded layout does not change resource identity.

## N2. `CONTEXT.md` is model-facing untrusted input

Ancestor summaries and labels can contain instructions. The file should clearly delimit provenance, quote or encode untrusted text, and state that it is context data rather than system policy. Secrets and raw tool output should be excluded by default.

## N3. `lineage.json` needs a stable machine schema

Specify:

- schema version;
- node/edge IDs and types;
- root and causal parent;
- graph revision;
- agent and workflow depths;
- truncation status;
- logical resource references;
- generated timestamp as informational only.

Do not expose host paths, credentials, capability signatures, or mutable authorization decisions unnecessarily.

## N4. Optional symlinks need collision policy

If Envoy creates convenience links, their namespace can collide with user files. Define a reserved Olympus-managed directory such as `.olympus/links/`, create links only when supported and policy permits, and never overwrite user content.

## N5. Context refresh should not rewrite every descendant eagerly

Prefer lazy regeneration at runtime creation/resume or explicit invalidation by graph revision. An ancestor update should mark dependent snapshots stale, not synchronously rewrite an unbounded descendant set.

## N6. Existing terminology should be migrated

The repository currently uses “tree,” “fork,” “branch,” “subsession,” and “handover” as partially overlapping parent relationships. The ADR should reserve “causal lineage” for execution ancestry and retain fork/handover as typed edge semantics, not aliases for generic parenthood.

---

# ADR-ready replacement language

The following can replace the proposal’s lineage, depth, workflow, and filesystem sections.

## Session workspaces and resource identity

> Every session is an independent durable resource identified by an opaque, globally unique `session_id`. On a node where it is materialized, Envoy places its workspace at:
>
> ```text
> <org-session-root>/sessions/<session_id>/
> ```
>
> A session’s filesystem location does not encode its lineage. Main sessions, agent-created child sessions, forks, handovers, and workflow-created sessions use the same flat layout.
>
> This decision explicitly supersedes ADR 0005 §4.2’s nested `<parent>/sessions/<child>` layout and its requirement that one lineage tree be colocated on one node.
>
> Session paths are Envoy-local projections, not durable identifiers or API contracts. Hall stores logical resource IDs and placement attempts, never authoritative host paths. Envoy validates `session_id` as one path-safe component, resolves it beneath the configured organization root, and rejects traversal, absolute paths, separator-bearing IDs, containment escapes, and stale materialization attempts.
>
> Correctness paths enumerate sessions from Hall projections or Envoy’s reconciliation index. They MUST NOT depend on scanning `sessions/`. The flat physical layout is therefore replaceable by a future internal sharding scheme without changing session identity or graph semantics.

## Canonical lineage graph

> Hall’s durable event log and projections are the sole authority for lineage. Lineage is a directed typed multigraph whose execution nodes include `Session` and `WorkflowRun`.
>
> V1 edge types are:
>
> - `agent_spawn`: an agent session created by another agent session;
> - `workflow_trigger`: a workflow run initiated by a session or workflow service principal;
> - `workflow_agent`: an agent session created as part of a workflow run;
> - `session_fork`: a new session derived from another session’s conversation state;
> - `session_handover`: a replacement session created for another runtime/harness;
> - `context_reference`: a noncausal reference used only for discoverability or explicitly authorized context;
> - `completion_to`: delivery routing for a terminal result.
>
> `agent_spawn`, `workflow_trigger`, `workflow_agent`, `session_fork`, and `session_handover` are causal edges. `context_reference` and `completion_to` are noncausal.
>
> Every execution node has zero or one immutable causal parent. Causal edges form a DAG. Hall rejects self-edges, a second causal parent, cycle-forming edges, missing or tombstoned endpoints, incompatible endpoint types, and cross-organization edges. Noncausal edges do not affect authority, depth, budgets, cancellation, or retention inheritance.
>
> Edge creation and node creation are one atomic Hall operation. The transaction records node identity, organization/project ownership, typed causal edge, capability envelope, depth values, budget reservation, workspace materialization intent, and operation idempotency key before Envoy receives any materialization or spawn command.

## Agent spawn depth

> `agent_spawn_depth` is a durable value derived from causal ancestry:
>
> - a user-created main session or system/service workflow root has depth `0`;
> - an `agent_spawn` edge sets the child depth to `parent.agent_spawn_depth + 1`;
> - `workflow_trigger`, `workflow_agent`, `session_fork`, and `session_handover` preserve the initiating depth unless the operation also explicitly performs an agent spawn;
> - a workflow transition NEVER resets depth.
>
> Hall rejects any agent spawn whose resulting depth exceeds `3`. Thus the maximum chain is main=`0`, sub1=`1`, sub2=`2`, sub3=`3`.
>
> A workflow-created agent inherits the workflow run’s initiating `agent_spawn_depth`. If that agent spawns a subagent, the normal `+1` rule applies. A scheduled workflow without an initiating session begins under its service principal at depth `0`.
>
> Depth is checked and recorded by Hall in the same transaction that creates the child. Envoy, clients, generated files, and caller-supplied depth values are not authoritative.

## Workflow causality and recursion

> Workflow invocation is distinct from an agent subagent-tool call and does not increment `agent_spawn_depth`; it preserves that depth.
>
> Workflow definitions remain nonrecursive as required by ADR 0013. Activities and providers MUST NOT start workflow runs except through Hall’s typed and authorized `workflow.run` operation.
>
> Hall additionally tracks `workflow_nesting_depth` over the full causal ancestry, including workflow runs separated by workflow-created agents. A root invocation has depth `0`; starting another workflow from any descendant of that active run increments it by `1`.
>
> V1 applies both controls:
>
> 1. A workflow definition digest already present in the active workflow ancestry may not be started again.
> 2. `workflow_nesting_depth` may not exceed `8`.
>
> These checks apply to direct calls, agent-mediated calls, scheduled signals, retries, and recovery. Retry or restart of the same `workflow_run_id` attaches to the existing run and does not increment nesting depth. A new run always receives a new run ID and consumes budget.
>
> Completion of a workflow before a later agent starts an unrelated run does not erase causal ancestry for accounting or audit, but only active ancestry participates in repeated-digest recursion rejection.

## Authorization and ownership

> Hall authorizes every causal transition using durable canonical endpoint ownership and the current authenticated principal.
>
> Effective child-agent authority is:
>
> ```text
> initiator
> ∩ parent session/run authority
> ∩ requested child authority
> ∩ organization/project policy
> ∩ runtime and provider grants
> ```
>
> Effective workflow authority is:
>
> ```text
> caller
> ∩ workflow manifest
> ∩ organization/project policy
> ∩ pinned provider grants
> ```
>
> A workflow-created agent receives at most the workflow run’s effective authority. Scheduled workflows run as dedicated, revocable service principals. Publishing or activating a workflow does not grant its publisher’s authority to future runs.
>
> Hall rechecks ownership, revocation, exact typed resource scope, and runtime attempt at workflow start, each step dispatch, agent spawn, resume, completion delivery, context traversal, and generated-context refresh. Missing capability envelopes fail closed on agent-facing operations.
>
> Both endpoints of every relationship traversal must belong to the same organization. URL scoping, caller-supplied IDs, projected parent fields, generated lineage files, host paths, and symlinks are not authorization evidence.

## Unified execution budgets

> Each causal execution root owns one durable budget account shared by all descendant sessions, workflow runs, steps, retries, jobs, and workflow-created agents.
>
> The account limits, at minimum, active/total agents, active/total workflow runs, workflow steps/retries, model tokens or provider cost, CPU/memory time, wall-clock lifetime, output/artifact bytes, workspace/context bytes, and concurrent fan-out.
>
> Hall atomically reserves budget before committing a spawn or dispatch intent. Insufficient budget rejects the operation without creating a node or side effect. Recovery and retry reuse the existing reservation and attempt identity. Cancellation releases only unused reservations; consumed resources remain charged. A child may receive a narrower sub-budget but cannot increase the root account or reset accounting through a workflow boundary.

## Derived lineage and context files

> Envoy may generate these convenience projections inside a materialized session workspace:
>
> ```text
> .olympus/lineage.json
> .olympus/CONTEXT.md
> ```
>
> They are caches derived from Hall’s canonical graph. They are never accepted as authority for identity, authorization, depth, budget, placement, retention, or recovery.
>
> Each generation records the schema version, Hall graph revision/event watermark, session ID, causal root and parent, typed ancestry, `agent_spawn_depth`, `workflow_nesting_depth`, logical resource references, and any truncation status.
>
> Transitive context is capability-filtered and bounded. It consists of deterministic ancestor summaries and logical references, not concatenated transcripts, raw tool output, secrets, credentials, capability signatures, or host paths. Generation has configurable node and byte limits and emits an explicit truncation marker when either limit is reached.
>
> Envoy renders ancestors in deterministic root-to-parent order, deduplicates visited nodes, and uses a visited set even though the causal graph is required to be acyclic. Noncausal reference cycles are not traversed transitively.
>
> `CONTEXT.md` labels inherited text as untrusted context data, not system policy. Hall-side revocation or ownership changes invalidate affected snapshots. Envoy regenerates lazily at create/resume or before context use against the current graph revision.
>
> Envoy writes each file to a temporary sibling, flushes it, atomically renames it over the prior generation, and durably records the materialization generation as supported by the host filesystem. Readers observe either the prior complete snapshot or the new complete snapshot.

## Symlinks

> Symlinks are optional, node-local conveniences and are noncanonical. No operation may depend on their presence, target spelling, or host support.
>
> Convenience links, when enabled, live only under `.olympus/links/`. Envoy never overwrites user content, never follows a link for authorization or deletion, and resolves all actual access through logical resource bindings and sandbox bind mounts. Unsupported platforms omit the links without changing behavior.

## Placement, recovery, and concurrent updates

> Every workspace materialization and runtime has a Hall-issued attempt epoch. Exactly one Envoy owns reconciliation for `(session_id, materialization_epoch)`. A moved, recovered, cancelled, archived, or tombstoned session fences all prior epochs; stale Envoys and runtimes may not modify the workspace, publish generated context, emit completion, or perform effects.
>
> All Hall graph mutations carry an operation idempotency key and are serialized through the authoritative event-log transaction. Duplicate operations return the prior durable result. Competing mutations re-evaluate graph invariants and budget availability in the committing transaction.
>
> Workspace realization is reconciliation, not authority: Envoy may report `materialized`, `missing`, `stale`, `orphaned`, or `cleanup_pending`, but it may not invent graph nodes or edges from local disk contents.

## Retention and deletion

> Graph records, typed causal edges, tombstones, organization ownership, definition/provider digests, capability decisions, budget charges, and audit facts remain durable while any retained descendant or workflow run references them.
>
> Transcript bodies, artifacts, summaries, and workspaces may have separate retention policies. Expiry changes content state to `unavailable` or `expired`; it does not erase the node or rewrite lineage as if the content never existed.
>
> A session or workflow root with retained descendants may be archived but not hard-deleted. Workflow definitions and provider versions remain pinned until no nonterminal run depends on them.
>
> Workspace cleanup is initiated by a durable Hall tombstone and executed by the currently fenced Envoy materialization owner. Cleanup is acknowledged and retryable. It deletes only the named session workspace and never follows symlinks or recursively infers descendants from filesystem structure.

## Required acceptance gates

> Acceptance requires tests proving:
>
> - direct agent depth boundaries `0..3`, rejection at `4`, and no workflow depth reset;
> - agent→workflow→agent→workflow chains preserve depth and budget;
> - direct and indirect workflow recursion rejection, including `A→agent→B→agent→A`;
> - concurrent duplicate spawn requests create one child and one budget reservation;
> - crash between node/edge/budget commit and Envoy materialization reconciles without orphan authority;
> - cycle, second-parent, wrong-org, missing-parent, tombstoned-parent, and stale-epoch writes fail closed;
> - capability revocation between context generation and use invalidates or regenerates the snapshot;
> - bounded context generation over wide/deep graphs does not grow quadratically or leak unauthorized content;
> - Hall restart reproduces identical graph projections and Envoy regenerates identical lineage artifacts;
> - workspace relocation across nodes uses logical resource bindings and rejects stale-node writes;
> - retention removes workspace/content without breaking lineage, pinned workflow recovery, or audit;
> - operation retries and concurrent completion/cancellation produce deterministic durable outcomes.

## Completion summary

- **Did:** Inspected the current ADRs, session lineage events/projections, subsession routes, workspace path code, authorization doctrine, workflow constraints, and the prior cross-org lineage postmortem.
- **Found:** 12 blocking architecture issues, chiefly depth reset/indirect recursion bypasses, conflict with ADR 0005, untyped/non-atomic current lineage, and missing authority/budget/cycle/path/retention/concurrency semantics.
- **Files modified:** None.
- **Issues encountered:** None.
