# ADR 0019 agent CLI adversarial architecture review

**Verdict: NO-GO**

ADR 0019 has the right doctrine—Hall owns truth and policy, Envoy owns effects, and CLI/MCP are adapters rather than alternate authorities—but the current text is not yet safe to implement. The local gateway authority, effectful request/revocation state machine, dynamic schema language, start/wait operation contract, workflow dispatch substrate, shared operation registry, and dependency DAG still have release-blocking gaps or contradictions.

This verdict is for the reviewed architecture/plan, not for an implementation that does not yet exist.

## Review snapshot

References below are against these exact contents:

| Source | SHA-256 |
|---|---|
| `docs/adrs/0019-agent-and-human-cli-interface.md` | `90685ba0b1745282accf4ed18c5697d282d7add6243c9583483cde403b8ff1e4` |
| `docs/adrs/0017-session-cutover-and-remote-development-plane.md` | `fed8e8bc022c936d18ceb869aea641988e35eb8f125ebcfed4a166858a0a693e` |
| `docs/adrs/0013-workflow-kernel-bounded-chains.md` | `5050c477085b783b127860b59da23231dfc1c063f5c64b84803a253d2123cfeb` |
| `docs/plans/2026-07-13-session-cutover-remote-development.md` | `f81f797293d8da81debc659e4a370965ab7cb8cb0960fef7b39c1f655f6540a8` |

## Blocking findings

### B1 — The private UDS is described as an authority boundary without specifying Unix identity and delegation semantics

**Categories:** security bypass; ambiguous Unix behavior; transport identity gap

**Sources:**

- `docs/adrs/0019-agent-and-human-cli-interface.md:182-194` makes a mode-0700 directory, an Envoy-owned endpoint, local-peer authentication, and a permanent `(node key, runtime attempt, session)` binding the security basis.
- `docs/adrs/0019-agent-and-human-cli-interface.md:190-199` asserts that copying the path grants nothing and that agent-mode detection prevents switching to operator credentials.
- `docs/adrs/0019-agent-and-human-cli-interface.md:201-203` requires archive/revocation to close the gateway.
- `docs/adrs/0017-session-cutover-and-remote-development-plane.md:132-145` repeats the private-UDS and runtime-attempt binding but does not define how Envoy authenticates the local runtime.
- `docs/plans/2026-07-13-session-cutover-remote-development.md:315-324` permits “private stdio/inherited-FD **or** UDS,” while `docs/plans/2026-07-13-session-cutover-remote-development.md:336-341` and `:356-363` require a private UDS and wrong-UID/cgroup tests.

**Failure:** The Unix rules needed to make these claims true are absent. A process must have directory search permission and socket write permission to connect. A mode-0700 directory and Envoy-owned socket do not explain how a differently identified sandbox runtime is admitted. Conversely, giving the runtime ownership/access can let same-UID siblings connect unless each runtime has a unique OS identity or Envoy performs a robust peer/cgroup check. An already-open Unix socket FD can be inherited or sent with `SCM_RIGHTS`; unlinking the path or closing the listener does not revoke accepted connections. The plan's inherited-FD alternative is a materially different capability model and invalidates the ADR's path, peer-credential, and copied-socket gates.

“Agent runtime detection always wins” is not a security boundary if inferred from mutable environment variables or a path the agent can unset. The sandbox must lack operator credentials/routes regardless of CLI mode selection.

**Required correction:**

1. Choose the UDS design for v1 and delete the stdio/inherited-FD alternative from Task 3.1.
2. Specify the host path, read-only bind mount, directory owner/mode, socket owner/mode or ACL, runtime UID/GID model, and exact peer evidence Envoy checks (`SO_PEERCRED` plus stable runtime/cgroup/process identity or a stronger equivalent).
3. State whether runtime subprocesses intentionally share the attempt authority. If not, use a per-runtime OS/container identity; pathname secrecy and one shared UID are insufficient.
4. Make the listener object—not request fields—the source of session/attempt context. Hall must bind an Envoy peer key plus gateway-instance/connection generation to the projected attempt/session.
5. Track and forcibly close accepted connections on archive/fence, while retaining Hall's per-call authorization as defense in depth. Define how FD inheritance/delegation is prevented or bounded.
6. Add real tests for copied paths, copied/open FDs, same-UID wrong-cgroup peers, PID exit/reuse, listener replacement, namespace escape, Hall reconnect, and archive/revoke with an already accepted connection.

### B2 — Client request IDs were added, but effectful calls still lack a complete durable authorization/idempotency state machine

**Categories:** security bypass; transport identity gap; durability

**Sources:**

- `docs/adrs/0019-agent-and-human-cli-interface.md:42-44` assigns authorization, idempotency, durable records, and audit to Hall.
- `docs/adrs/0019-agent-and-human-cli-interface.md:146-151` now requires a client request ID and says retries resolve to the original effect.
- `docs/adrs/0019-agent-and-human-cli-interface.md:201-203` delegates in-flight behavior to each provider's cancellation/fencing policy.
- `docs/adrs/0019-agent-and-human-cli-interface.md:218-225` requires shared authorization/idempotency and fail-closed protocol negotiation.
- `docs/adrs/0019-agent-and-human-cli-interface.md:269-283` audits operation/idempotency identifiers and forbids caller identity fields, but does not define ID scope, input-digest conflict, or authorize/commit/dispatch ordering.
- `docs/adrs/0017-session-cutover-and-remote-development-plane.md:166-173` requires current revocation and identity checks at the seam.
- `docs/plans/2026-07-13-session-cutover-remote-development.md:315-324` says authority is rechecked and the channel closes, but supplies no linearization point.
- `docs/plans/2026-07-13-session-cutover-remote-development.md:462-469` tests one client request ID producing one workflow run, but not a conflicting replay or concurrent revocation.

**Failure:** The new request ID closes only the simplest duplicate-run case. A capability check followed by a later durable append or Envoy dispatch still has a check/revoke race. The text does not say whether the request ID is unique per principal/session/organization, what happens when the same ID is replayed with different typed input, whether the ID reservation and operation intent are one transaction, or how the durable operation ID maps to provider attempt/fencing identity. “Hall resolves it” is not enough to prevent a stale-authority dispatch or cross-context collision.

**Required correction:** Define one state machine for every effectful operation:

1. The adapter generates a stable client request ID before first send and reuses it across transport retries.
2. Hall atomically validates authenticated context/current authority epoch, validates and canonicalizes typed input, and appends a unique idempotency reservation plus operation intent/result.
3. Duplicate `(authority scope, request ID)` with the same canonical input digest attaches/replays the stored result; the same ID with a different digest fails closed. A request ID from another principal/session/org never aliases it.
4. Dispatch uses the durable resource/attempt ID and fencing epoch; response loss never creates another effect.
5. Define the revocation linearization point and, per provider class, whether a committed attempt may finish, must be fenced before first effect, or must be cancelled. “Concurrent revocation fails closed” needs a deterministic oracle, not only channel closure.
6. Protocol negotiation and retries are side-effect free.

Add crash/concurrency tests at request reservation, authority decision, intent append, dispatch send, provider acceptance, result append, response send, and concurrent revoke—with same-ID/same-input and same-ID/different-input cases.

### B3 — “JSON-Schema-compatible” is still not a deterministic dynamic CLI language

**Categories:** dynamic-schema parsing hazard; ambiguous CLI behavior; security/resource exhaustion

**Sources:**

- `docs/adrs/0019-agent-and-human-cli-interface.md:79-91` requires local schema-derived parsing and a second Hall validation.
- `docs/adrs/0019-agent-and-human-cli-interface.md:110-124` calls the mapping deterministic but leaves object/union handling and a “named JSON input flag” open.
- `docs/adrs/0019-agent-and-human-cli-interface.md:126-130` now rejects reserved names and bounds bytes/depth, but does not define the schema dialect/subset or an injective property mapping.
- `docs/adrs/0019-agent-and-human-cli-interface.md:223-225` versions only the operation schema, not the workflow input schema profile.
- `docs/plans/2026-07-13-session-cutover-remote-development.md:443-469` schedules implementation without a publication-time schema-to-CLI compiler/validator contract.

**Failure:** JSON Schema permits constructs that cannot be mapped by the listed rules without ambiguity: `$ref`/recursive refs, `allOf`/`oneOf`/`anyOf`, nullable types, tuples, nested arrays/objects, `additionalProperties`, conditionals, dependent fields, multiple numeric forms, and arbitrary property names. The reserved-name edit catches direct static collisions but not boolean negation (`x` versus `no-x`) or normalization collisions (`foo-bar` versus `foo_bar`, case, Unicode). Hyphen-prefixed string values and negative numbers interact with option parsing. Pattern/default semantics can diverge between Rust CLI and Hall. Byte/depth limits alone do not bound ref expansion, enum/help cardinality, or hostile regex cost.

The secret guarantee at `docs/adrs/0019-agent-and-human-cli-interface.md:122-124` is not enforceable against an arbitrary string/object schema unless workflow publication has a distinct opaque binding type and rejects raw secret fields.

**Required correction:** Define and version a closed v1 workflow-input schema profile and publication-time validator. It must specify:

- supported JSON Schema dialect and keywords, ref policy, depth/size/enum/pattern limits, numeric domain, and regex dialect;
- an injective JSON-property-to-flag mapping, complete reserved names/prefixes, boolean-negation collision rules, `--` and hyphen-leading value behavior, duplicate semantics, and what “named JSON input flag” means;
- whether `--input-json` supports the full profile or the same flaggable subset;
- opaque secret/resource-binding types that carry identifiers only;
- Hall as sole authority for canonicalization/default application and normalized input digest, so CLI and MCP omission of a default cannot produce different durable events;
- one shared conformance corpus consumed by the CLI parser, Hall validator, schema-help renderer, and MCP schema adapter.

Reject an unrepresentable/colliding schema at **publish time**, not when an agent tries to run it.

### B4 — Cursor/retry behavior was added, but start versus wait is still not one shared typed operation contract

**Categories:** ambiguous CLI behavior; workflow durability mismatch; operation-contract drift

**Sources:**

- `docs/adrs/0019-agent-and-human-cli-interface.md:132-151` defines waiting, interruption, durable cursors, and request-ID retries.
- `docs/adrs/0019-agent-and-human-cli-interface.md:163-180` defines output modes, failure output, and SIGPIPE behavior.
- `docs/adrs/0019-agent-and-human-cli-interface.md:205-225` says adapters translate one typed operation request/result.
- `docs/adrs/0013-workflow-kernel-bounded-chains.md:102-107` says MCP starts non-blocking while CLI waits.
- `docs/adrs/0017-session-cutover-and-remote-development-plane.md:322-338` requires detach/reconnect and a real pipeline.
- `docs/plans/2026-07-13-session-cutover-remote-development.md:455-469` requires byte-tested wait/reconnect behavior but does not identify canonical start/watch/get operation types.

**Failure:** The only coherent common effectful operation is a non-blocking start returning a durable run reference. CLI waiting must then be adapter-side composition over durable get/watch operations. The documents still do not state that decomposition. As written, “same typed results” can mean either the MCP start reference or the CLI terminal result, and equivalence tests have no exact comparison point.

The new interruption promise is impossible in one important window: `docs/adrs/0019-agent-and-human-cli-interface.md:138-140` says an interrupted waiter prints the durable run ID, but Ctrl-C can arrive after Hall commits the request and before the first response, when the CLI knows only the client request ID. The text does not require reconciliation by request ID before exit. Cursor semantics also omit event-envelope identity/version, cursor retention/expiry, and the terminal snapshot rule when events have compacted. Finally, `result-json` requires an output object (`:168-169`) while ADR 0013 does not establish that all successful workflows have an object result.

**Required correction:**

1. Define `workflow.start` as a non-blocking typed operation returning `{run_id, definition_digest, operation_id}` for CLI and MCP.
2. Define durable `workflow.get` and `workflow.watch(after_sequence)` operation types. Event envelopes need run ID, monotonic sequence, stable event ID/type/version, terminal status, cursor retention/expiry, and a terminal snapshot fallback.
3. State that `olympus workflow run` is adapter composition: schema lookup -> `workflow.start` -> optional `workflow.watch/get` -> rendering. Equivalence applies to the canonical start request/result/events before adapter-specific waiting.
4. On Ctrl-C/timeout after send but before response, reconcile the client request ID to a run ID before claiming detachment. If reconciliation is unavailable, emit an explicit ambiguous-acceptance error plus the recovery request ID; do not promise a run ID the client cannot know.
5. Define successful no-result, non-object, and artifact-only workflows, or require an object output schema at publish time.
6. Test interrupt/crash at schema fetch, before send, after Hall commit/before response, during watch, after terminal/before output, and during downstream pipe closure.

The current digest binding (`docs/adrs/0019-agent-and-human-cli-interface.md:84-91`), cursor concept (`:146-151`), zero stdout on failed `result-json` (`:176-180`), and plan race gates (`docs/plans/2026-07-13-session-cutover-remote-development.md:462-469`) are good corrections; they are not sufficient to define the complete typed state machine.

### B5 — Accepted ADR 0013 still depends on the unsafe JOBS-1 transport that ADR 0017 says must not back agent workflows

**Categories:** workflow durability mismatch; dependency contradiction

**Sources:**

- `docs/adrs/0013-workflow-kernel-bounded-chains.md:64-73` says activity dispatch uses JOBS-1 and the “existing seq/ack/spool exactly-once delivery.”
- `docs/adrs/0013-workflow-kernel-bounded-chains.md:99-106` says these are already-existing patterns and exposes operations through ADR 0019.
- `docs/adrs/0017-session-cutover-and-remote-development-plane.md:37-45` says current jobs are volatile, ACK output before durable Hall storage, and lack identity/capability/sandbox truth.
- `docs/adrs/0017-session-cutover-and-remote-development-plane.md:198-225` requires JOBS-2 durability, retained attempt identity, reconciliation, atomic spool sequencing, and terminal ordering before agent exposure.
- `docs/plans/2026-07-13-session-cutover-remote-development.md:192-295` implements those missing JOBS-2 guarantees.
- `docs/plans/2026-07-13-session-cutover-remote-development.md:443-445` correctly makes durable `JobService` and activity providers prerequisites for WF-1, contradicting ADR 0013's JOBS-1 substrate.

**Failure:** ADR 0013 is accepted and says it is the workflow spec, yet its dispatch premise is explicitly false in ADR 0017. `max_retries: 0` does not make a non-idempotent activity fail closed across an ambiguous first dispatch: a crash after process spawn but before durable `StepDispatched`/job correlation can duplicate the effect or strand the run. A key containing `attempt` is safe only if restart reconciles and reattaches to the same durable attempt; incrementing the attempt changes the key and can repeat the effect.

**Required correction:** Amend ADR 0013 before WF-1 implementation to require JOBS-2, not JOBS-1. Specify the order `authorize + append StepDispatchPlanned/job intent -> dispatch durable (job_id, attempt_epoch) -> reconcile/attach -> append terminal step event`. A restart must never infer “dispatch next” from absence of completion alone. For non-idempotent providers, ambiguous effect state becomes an explicit indeterminate/operator-resolution state, never automatic redispatch. Update the WF-1 card/source references at the same time so no implementer can legally follow the stale accepted text.

### B6 — The “one typed operation seam” remains an assertion rather than a drift-proof operation registry

**Categories:** operation-contract drift; authorization bypass risk

**Sources:**

- `docs/adrs/0019-agent-and-human-cli-interface.md:28-44` declares CLI and MCP adapters over one Hall seam.
- `docs/adrs/0019-agent-and-human-cli-interface.md:59-77` lists CLI commands and gives conflicting availability rules (“appear only” versus static unavailable groups).
- `docs/adrs/0019-agent-and-human-cli-interface.md:205-225` introduces a typed vocabulary but specifies only generic handler shape and additive versioning.
- `docs/adrs/0017-session-cutover-and-remote-development-plane.md:119-130` names plural typed operations such as `jobs.run`, `deployments.status`, and `apps.status`.
- `docs/adrs/0013-workflow-kernel-bounded-chains.md:102-107` names MCP tools `run_workflow`, `get_run`, and `signal_run` with different adapter behavior.
- `docs/plans/2026-07-13-session-cutover-remote-development.md:305-320` creates `operations.rs` and shared modules, but does not require one canonical operation registry/manifest.
- `docs/plans/2026-07-13-session-cutover-remote-development.md:400-421`, `:447-469`, and `:604-617` add adapters separately and test “equivalence” without defining the common request/result being compared.

**Failure:** Separate CLI and MCP implementations can drift on operation ID, request fields, defaults, capability, organization/resource scope, idempotency, effect classification, streaming, or error mapping while still calling the same Hall service. Names need not be identical across adapters, but their mapping must be explicit. The current “equivalent durable events/results” gate is not executable when MCP start returns a reference and the complete CLI invocation returns a terminal result.

There is also a dangerous migration edge: the existing operator REST job request is argv-shaped (`docs/adrs/0017-session-cutover-and-remote-development-plane.md:37-45`), while agent `jobs.run` must be activity/provider-shaped (`docs/adrs/0017-session-cutover-and-remote-development-plane.md:198-218`). Sharing `JobService` must not make the raw operator DTO part of the agent operation vocabulary.

**Required correction:** Define one versioned operation registry in `olympus-proto`, generated or exhaustively matched by every adapter. Each entry must include canonical operation ID, request/result/error types, stream item/cursor type where applicable, effect/read classification, capability/resource resolver, organization/principal scope, idempotency requirement/scope, protocol range, availability gate, and redaction/audit policy. CLI command and MCP tool names map to that ID; they do not define separate contracts. Keep any raw operator-maintenance job route as a separately named operator-only operation—or remove it—and prove no agent registry entry accepts argv/env/cwd/executable fields. Equivalence tests submit the same canonical typed request and compare normalized authorization, durable event, and typed operation result before adapter-specific waiting/rendering.

### B7 — The dependency graph allows SANDBOX/WF work before prerequisites required by its own tasks and ADR 0017 gates

**Categories:** dependency-order contradiction; security bypass; durability

**Sources:**

- `docs/plans/2026-07-13-session-cutover-remote-development.md:36-60` makes `PKG-1 + JOBS + CAPS` sufficient for WF-1 and `APP-1 + DEPLOY-1 + SESSION-SAFE` sufficient for SANDBOX-1.
- `docs/plans/2026-07-13-session-cutover-remote-development.md:443-445` additionally requires activity providers, runtime gateway, CLI core, and the hostile sandbox gate for effectful workflows.
- `docs/plans/2026-07-13-session-cutover-remote-development.md:650-667` makes the SANDBOX proof use schema-aware workflow CLI, result-json piping, and typed deployment CLI/MCP.
- `docs/adrs/0017-session-cutover-and-remote-development-plane.md:175-225` requires SESSION-SAFE and JOBS-2 ordering before agent operations.
- `docs/adrs/0017-session-cutover-and-remote-development-plane.md:322-338` makes gateway, MCP/CLI, durable jobs, capability denial, workflow schema validation, and pipeline reconnect part of the agent-operation gate.
- `docs/adrs/0019-agent-and-human-cli-interface.md:323-341` forbids exposing an effectful command before corresponding prerequisites are green.

**Failure:** The top-level DAG is the artifact parallel workers and schedulers will follow. It currently permits WF-1 without the runtime gateway/CLI/provider/sandbox prerequisites listed later and permits SANDBOX-1 without AGENT-IFACE-1 or WF-1, although the SANDBOX proof directly invokes both. Phase numbering is not a substitute for explicit hard edges in a plan that advertises isolated parallel worktrees.

**Required correction:** Rewrite the graph with exact task/gate identifiers rather than umbrella words. At minimum:

- WF-1/Task 4.5 depends on Task 1.4 CAPS, JOBS-2 Tasks 2.1-2.5, Tasks 3.1-3.3, Tasks 4.1-4.4, canonical PKG-1, and Task 4.2's hostile gate for effectful workflows.
- SANDBOX Task 8.2 depends on SESSION-SAFE 1.1-1.5, AGENT-IFACE 3.1-3.3, typed job operations/artifacts 4.1-4.4, WF-1 4.5, durable edge 6.1, required APP/DEPLOY tasks, and their real-substrate hostile gates.
- The seven-day gate depends on successful Task 8.2 plus all ADR 0017 cutover gates, not merely APP/DEPLOY/SESSION labels.

Make these dispatch-blocking dependencies in the work tracker, not prose-only prerequisites.

## Non-blocking precision edits

These should be fixed in the same rewrite, but they do not independently drive the NO-GO verdict.

### P1 — Reconcile command nouns and verbs across documents

`docs/adrs/0019-agent-and-human-cli-interface.md:63-68` uses `node list|get`, `job ...`, `deployment ... get`, and `app ... get`; `docs/adrs/0017-session-cutover-and-remote-development-plane.md:122-127` uses `nodes.list`, `jobs.get`, `deployments.status`, and `apps.status`; `docs/plans/2026-07-13-session-cutover-remote-development.md:604-617` uses deployment `status` but creates the singular `deployment` CLI command. Add an explicit command/tool-to-operation mapping table and choose `get` versus `status` consistently where they mean the same thing.

### P2 — Define generic help versus schema-derived help

`docs/adrs/0019-agent-and-human-cli-interface.md:106-107` correctly shows `workflow run <slug> --help`, but `docs/plans/2026-07-13-session-cutover-remote-development.md:654-657` also asks for `olympus workflow run --help` without a slug. State that the latter is static generic help and never fetches a schema, while only the former is dynamic. Define missing/not-found/unauthorized slug behavior without leaking cross-organization existence.

### P3 — Sanitize dynamic terminal and completion content

Descriptions, examples, patterns, property names, and enum values are runtime data rendered into help (`docs/adrs/0019-agent-and-human-cli-interface.md:236-246`). Require control-character/ANSI escaping, bounded rendering, and safe shell quoting. Runtime schema content must never be interpolated into executable completion shell source. Add hostile escape-sequence, bidi, newline, and shell-metacharacter fixtures.

### P4 — Specify timeout syntax and exit/output behavior

`docs/adrs/0019-agent-and-human-cli-interface.md:142-144` does not define duration grammar, zero/negative values, overflow, or timeout exit class. Define one duration syntax, cap it, classify client timeout distinctly within exit class 6, and guarantee the request/run recovery identifier remains available.

### P5 — Separate unavailable operation from incompatible protocol

`docs/adrs/0019-agent-and-human-cli-interface.md:74-77` says commands appear only when backed, yet static help may show future groups. Define whether command presence is build-time, runtime-catalog-driven, or always static with a typed `operation_unavailable` response. Do not overload protocol/schema incompatibility exit 8 (`docs/adrs/0019-agent-and-human-cli-interface.md:253-267`) for a server that simply lacks an additive operation.

### P6 — Define canonical audit digest and surface semantics

`docs/adrs/0019-agent-and-human-cli-interface.md:269-283` should say the normalized digest is computed by Hall over a versioned canonical representation after schema validation/default resolution, never by an adapter. Define whether equivalent CLI/MCP calls intentionally differ only in `surface` and therefore share operation/resource/input identity while producing separate audit events.

### P7 — Clarify cancellation terminal truth

`docs/adrs/0013-workflow-kernel-bounded-chains.md:64-80` includes `RunCancelled`, while `docs/adrs/0019-agent-and-human-cli-interface.md:134-144` distinguishes detach from explicit cancel and `:201-203` allows provider-specific in-flight behavior. Specify whether `RunCancelled` means “no further steps dispatch,” “all in-flight activities are fenced/stopped,” or merely “cancellation requested.” If effects may outlive it, use distinct requested/quiesced/terminal states and surface the honest state in CLI output.

### P8 — Replace the acceptance-gate grab bag with a per-operation gate matrix

`docs/adrs/0019-agent-and-human-cli-interface.md:339-341` lists SESSION-SAFE, capability, job, provider, sandbox, workflow, edge, app, and deployment gates in one sentence. Add a matrix mapping each command/operation to exact prerequisites. This prevents both over-gating read-only commands and under-gating effectful commands.

## Confirmed corrections in this snapshot

The current snapshot correctly improves several previously ambiguous areas:

- Workflow start submits the exact schema digest and forbids silent active-version substitution (`docs/adrs/0019-agent-and-human-cli-interface.md:84-91`; plan gate `docs/plans/2026-07-13-session-cutover-remote-development.md:462-467`).
- Workflow commands now use one `olympus workflow ...` noun (`docs/adrs/0019-agent-and-human-cli-interface.md:59-68`).
- Wait reconnect has a durable cursor and client request ID (`docs/adrs/0019-agent-and-human-cli-interface.md:146-151`).
- Failed `result-json` emits no stdout and broken pipes detach rather than cancel (`docs/adrs/0019-agent-and-human-cli-interface.md:176-180`).
- Agent requests cannot supply principal/org/session/attempt/node identity (`docs/adrs/0019-agent-and-human-cli-interface.md:275-278`).

These controls should remain; none resolves the independent blockers above.

## Coverage summary

| Requested lens | Result |
|---|---|
| Security bypasses | **Blocked:** local gateway delegation/revocation and raw-job seam are not closed (B1, B2, B6). |
| Ambiguous Unix/CLI behavior | **Blocked:** UDS DAC/FD behavior and start/wait interruption semantics remain incomplete (B1, B4). |
| Dynamic-schema parsing hazards | **Blocked:** digest pinning is fixed, but no closed schema/flag profile exists (B3). |
| Transport identity gaps | **Blocked:** local peer-to-attempt binding and authority/idempotency linearization are unspecified (B1, B2). |
| Operation-contract drift | **Blocked:** start versus waited result is not decomposed and no canonical registry exists (B4, B6). |
| Workflow durability mismatch | **Blocked:** ADR 0013 relies on JOBS-1 and lacks ambiguous-dispatch reconciliation (B5). |
| Dependency-order contradictions | **Blocked:** top-level DAG omits prerequisites required by its own tasks and cutover gates (B7). |

## Approval condition

Re-review after B1-B7 are resolved in the ADRs and plan as one coherent change. Approval should require rewritten contracts plus executable gates for UDS/FD isolation, concurrent revocation, conflicting idempotency replay, parser conformance, ambiguous start response, cursor compaction, non-idempotent dispatch ambiguity, adapter equivalence, and dependency dispatch blocking.
