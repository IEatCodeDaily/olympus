# ADR 0019 — Agent and human CLI interface over the Olympus operation seam

Status: proposed · Date: 2026-07-13
Relates to: ADR 0011 (jobs/MCP/capabilities), ADR 0012 (programmable operating
environment), ADR 0013 (workflow kernel), ADR 0017 (session cutover).
Supersedes: ADR 0011 §3's decision that agents use MCP but not CLI.

Review chain:

- `docs/reviews/0019-agent-cli-adversarial-review.md` — initial NO-GO.
- `docs/reviews/0019-agent-cli-adversarial-rereview.md` — one remaining B4.
- `docs/reviews/0019-agent-cli-approval.md` — **APPROVED as a proposed
  architecture/plan**; implementation and cutover evidence remain open.

## Context

Olympus needs an interface that agents can discover, invoke, compose with Unix
pipes, and debug without generating ad-hoc HTTP requests. MCP remains useful for
native model tool calls, but it is a poor exclusive operations interface:

- agents are consistently good at invoking documented CLIs and inspecting
  `--help` output;
- shell composition, redirection, capture, and local reproduction are natural;
- a CLI invocation is easy for a human to replay exactly;
- not every harness has equally capable MCP support;
- workflows have runtime-defined typed inputs, so their invocation help must be
  generated from the immutable workflow schema rather than frozen at build time.

A CLI must not become an authorization bypass or a disguised remote shell. It
must call the same typed operations and capability decisions as MCP and REST,
and an agent runtime must not receive a Hall token or general network route.

## Decision

Olympus ships a first-class Rust binary named `olympus`. It is available to
humans and injected into eligible agent runtime sandboxes. CLI and MCP are two
protocol adapters over one typed Hall operation seam; neither implements policy
or host effects.

```text
agent process
  ├─ MCP adapter ───────────────┐
  └─ olympus CLI ─ runtime UDS ─┼─ Envoy runtime gateway ─ iroh ─ Hall operations
                                │                              ├─ policy/capabilities
human shell ─ operator adapter ─┘                              ├─ durable services
                                                               └─ Envoy activities
```

Hall owns operation semantics, durable records, authorization, idempotency, and
audit. Envoy owns the runtime-bound local gateway and host effects. The CLI owns
argument parsing, schema-aware help, rendering, and exit status only.

### 1. Command vocabulary is noun + explicit verb

The canonical workflow invocation is:

```bash
olympus workflow run <workflow-slug> [workflow inputs]
```

The explicit `run` is intentional. `olympus workflow <slug>` would make workflow
slugs collide with management verbs such as `list`, `show`, `get`, `cancel`, and
`signal`. A shorthand may be added later only if it introduces no ambiguous
parse or reserved-slug rule.

Initial command tree:

```text
olympus session info
olympus node list|get
olympus job run|get|logs|cancel
olympus workflow list|show|run|get|watch|cancel|signal
olympus operation get <operation-id>
olympus artifact get
olympus deployment plan|apply|status|rollback
olympus app list|status|install|start|stop
olympus capability list|check
olympus completion <shell>
olympus man [command]
```

The compiled command grammar is static and always appears in static help. Hall's
runtime operation catalog reports whether each operation is available and why.
Calling a compiled but unavailable operation returns typed
`operation_unavailable` in exit class 6; it is not a protocol-incompatibility
error. There is no `olympus exec`, arbitrary `api`, raw argv, raw HTTP, raw SSH,
or caller-chosen Hall endpoint in agent mode.

Canonical operation mapping:

| CLI | Operation ID | MCP adapter name |
|---|---|---|
| `session info` | `session.info` | `olympus_session_info` |
| `node list`, `node get` | `nodes.list`, `nodes.get` | `olympus_nodes_list`, `olympus_nodes_get` |
| `job run/get/logs/cancel` | corresponding `jobs.*` | `olympus_jobs_*` |
| `workflow list/show/run/get/watch/cancel/signal` | corresponding `workflows.*` | `olympus_workflows_*` |
| `operation get` | `operations.get` | `olympus_operations_get` |
| `artifact get` | `artifacts.get` | `olympus_artifacts_get` |
| `deployment plan/apply/status/rollback` | corresponding `deployments.*` | `olympus_deployments_*` |
| `app list/status/install/start/stop` | corresponding `apps.*` | `olympus_apps_*` |
| `capability list/check` | corresponding `capabilities.*` | `olympus_capabilities_*` |

Names are adapter syntax; the operation ID and typed descriptor are the semantic
contract.

### 2. Workflow input flags are generated from the published schema

A published workflow definition declares an immutable, JSON-Schema-compatible
input object. `olympus workflow run` performs a two-phase parse:

1. parse static flags and the workflow slug;
2. resolve the slug to an active immutable definition digest plus schema
   profile/dialect version, fetch that exact input schema through the runtime
   gateway, then parse and validate schema-derived flags locally;
3. submit the definition digest with the request; Hall either starts that exact
   retained definition or returns a conflict—it never silently substitutes a
   newly activated version—and validates the typed input again before appending
   `WorkflowRunStarted`. Audit and result envelopes carry the same definition
   digest/profile version.

Examples:

```bash
olympus workflow run olympus-verify \
  --revision 1a2b3c4d \
  --node sandbox-dev \
  --profile release

olympus workflow run deploy-candidate \
  --input-json request.json \
  --detach \
  --output json

olympus workflow show olympus-verify --schema
olympus workflow run olympus-verify --help
```

Schema mapping is deterministic:

- string/number/integer: `--name VALUE`;
- boolean: `--name` and `--no-name`;
- array: repeated `--name VALUE` preserving order;
- enum: accepted values shown in help and shell completion;
- object or ambiguous union: supplied through `--input-json FILE|-` or a
  named JSON input flag, never guessed from shell text;
- required/default/range/pattern/deprecation information is rendered in help;
- unknown, duplicated scalar, mistyped, or undeclared flags fail before a run is
  created.

`--input-json` and schema-derived flags are mutually exclusive in v1. This avoids
hidden precedence rules. Secret values are never accepted on argv or stdin;
workflows receive named secret/resource bindings authorized by Hall.

Workflow input names that collide with static CLI controls (`help`, `output`,
`detach`, `timeout`, `input-json`, `no-color`, and future reserved names) are
rejected when a definition is published. Dynamic inputs have no short flags.
Input documents and individual inline values have explicit byte/depth limits;
larger data moves through capability-checked artifact/resource references.

The accepted profile is not arbitrary JSON Schema. Published workflow inputs use
the versioned `olympus.workflow-input/v1` profile:

- root is a closed object (`additionalProperties: false`);
- property names match `[a-z][a-z0-9-]{0,62}`, are NFC-normalized, cannot begin
  with `no-`, and cannot collide with static/reserved flags;
- properties are string, signed 64-bit integer, finite IEEE-754 number, boolean,
  enum, or bounded arrays of those scalar forms;
- nested objects, tuples, nullable unions, `$ref`, recursion, `allOf`, `oneOf`,
  `anyOf`, conditionals, dependent fields, and custom executable validation are
  rejected in v1;
- patterns use the bounded Rust `regex` dialect without backreferences or
  look-around; schema, description, example, enum, pattern, input, depth, and
  rendered-help sizes have fixed publication/runtime limits;
- `format: olympus-resource-ref` and `format: olympus-secret-binding` accept
  opaque identifiers only. Raw secret values are never a valid schema type;
- `--input-json` accepts exactly the same closed profile as dynamic flags, not a
  wider language;
- `--name=value` is the unambiguous spelling for a hyphen-leading value;
  whitespace form consumes the next token for a declared scalar even if it
  begins with `-`; duplicate scalar flags fail and repeated array flags preserve
  order.

One publication-time compiler/validator rejects unrepresentable schemas and
produces a canonical schema artifact. The CLI parser/help renderer, Hall
validator/default resolver, and MCP schema adapter consume one shared
conformance corpus. Hall alone applies defaults and computes the normalized
input and digest using a versioned canonical JSON representation; local CLI
validation is fail-fast assistance, not authority.

### 3. Waiting, interruption, and piping have explicit semantics

`workflow run` waits for a terminal run by default because this matches normal
CLI and pipeline expectations. `--detach` returns immediately with the durable
run reference. Disconnecting or pressing Ctrl-C detaches the client but does not
cancel the durable run; cancellation requires an explicit
`olympus workflow cancel <run-id>`. Before exit, an interrupted waiter prints
the durable run ID and an exact reconnect command only after acceptance is
confirmed. If reconciliation cannot reach Hall before the client deadline, it
returns typed `acceptance_unknown` with the already-known operation ID and
`olympus operation get <operation-id>` recovery command; it never invents or
drops the unknown run identity.

`--timeout` bounds client waiting only. It does not alter the workflow's durable
step timeouts. The command prints progress and diagnostics to stderr. stdout is
reserved for the selected result representation.

Client durations use `[1-9][0-9]*(ms|s|m|h|d)` and are capped at seven days;
zero, negative, fractional, overflowed, or compound values fail locally. Wait
timeout returns `client_wait_timeout` in exit class 6 and always includes the
recoverable run ID/reconnect command when acceptance is confirmed. Otherwise it
returns `acceptance_unknown` in class 6 with the operation ID and exact operation
lookup command on stderr or in the machine envelope.

The waiter is cursor-based over durable workflow-run events. It reconnects with
`(run_id, last_event_sequence)`, deduplicates replayed events by sequence, and
emits the terminal result at most once. Starting an effectful operation creates
a client request ID before the first send; reconnect/retry reuses that ID, and
Hall resolves it to the original run/job/deployment rather than creating a
second effect.

The shared `workflows.run` operation is always non-blocking and returns
`{run_id, definition_digest, operation_id, accepted_sequence}`. CLI waiting is
adapter-side composition over durable `workflows.get` and
`workflows.watch(after_sequence)`. Watch envelopes contain run ID, monotonic
sequence, stable event ID, event type/version, payload, and terminal status.
Ctrl-C/timeout before a response first reconcile by operation ID; after
acceptance they stop only the watch. Neither path sends cancellation.

Workflow run events and their per-run sequences are non-expiring in v1; they are
read from permanent event-log truth, not a bounded broadcast buffer. Therefore
`workflows.watch(after_sequence)` can resume any valid sequence for the lifetime
of the Olympus store. A future retention/compaction design must version the
operation, return typed `cursor_expired`, and fall back to `workflows.get`'s
durable terminal snapshot without redispatch; silent cursor reset is forbidden.

Machine composition:

```bash
olympus workflow run discover-release --output result-json |
  olympus workflow run deploy-candidate --input-json - --output json

olympus workflow watch wr_123 --output jsonl |
  jq -c 'select(.kind == "stepFailed")'
```

Output modes:

- `human` — concise TTY-oriented status and final result;
- `json` — one versioned operation envelope;
- `jsonl` — one versioned event envelope per line, valid for watch/follow;
- `result-json` — terminal workflow output object only, valid for a successful
  waited run;
- `yaml` — human inspection, not the stable automation contract.

No color, pager, spinner, or progress text is emitted when stdout is not a TTY.
Inline results are bounded; larger values are returned as typed artifact
references and retrieved through `olympus artifact get`.

On failure, `result-json` emits no stdout bytes and exits with the durable
failure class; `json`/`jsonl` emit a typed error/terminal envelope. A broken
stdout pipe detaches the waiter and never cancels the run. Unix builds retain
normal SIGPIPE-compatible status rather than translating it into workflow
failure.

`result-json` buffers the bounded terminal JSON value before its first write.
Usage, schema, authorization, transport, timeout, cancellation, indeterminate,
and failed-run outcomes emit zero result bytes. A successful run with no result
emits `null`; artifact-only output emits a typed artifact-reference object. If a
downstream closes during the final write, Unix may observe a truncated write and
SIGPIPE; the downstream `--input-json -` parser buffers and validates the whole
document before creating a run, so partial JSON cannot trigger an effect.

### 4. Agent transport is a private runtime gateway, not bearer auth

V1 uses UDS only—no inherited descriptor or stdio authority variant. Each
eligible runtime receives a dedicated endpoint hosted at
`/run/olympus-envoy/runtime-gateways/<gateway-generation>/gateway.sock` and
read-only bind-mounted inside only that runtime namespace at
`/run/olympus/runtime-gateway.sock`. The host directory is Envoy-owned mode 0750
with a unique per-runtime GID; the socket is Envoy-owned, that GID, mode 0660.
The sandbox runs with a unique per-runtime UID/GID (or an equivalent private user
namespace) and a dedicated cgroup. All subprocesses inside that one runtime are
intentionally delegated the same session-attempt authority.

On accept, Envoy checks `SO_PEERCRED` UID/GID/PID, PID start identity, expected
cgroup membership, mount/runtime generation, and listener generation. It binds
the accepted connection—not request fields—to `(node iroh key, gateway
generation, runtime attempt, session, organization)`. Hall validates that tuple
against the durable attempt projection on every call. PID exit/reuse, listener
replacement, stale generation, or peer mismatch closes the connection.

The socket path is location, not authority. Copying the path outside its mount
namespace grants nothing. Agent mode provides no `--token`, `--endpoint`, or
profile override. Runtime sandbox images contain no operator credential/config,
have no Hall network route, and cannot switch transport even if an environment
variable is unset. A CLI process cannot name another session or runtime attempt;
Hall derives them from the accepted gateway binding and rechecks the current
capability envelope on every call.

Human/operator use is a separate adapter and credential context. It may use the
local Hall UDS or authenticated HTTPS, but resolves the same operation types and
Hall services. The sandbox lacks operator credentials/routes regardless of CLI
mode detection.

Envoy tracks every accepted connection. Archive, revocation, runtime fencing,
PID/cgroup exit, or gateway replacement closes the listener and all accepted
connections, unmounts the endpoint, and retires the generation. FD inheritance
within the runtime is intentional delegation; cross-runtime FD transfer is
blocked by distinct user/mount/PID/network namespaces and cgroups. Hall per-call
authorization remains authoritative even for an already accepted connection.
CLI access cannot outlive the session authority that created it.

Tests cover copied paths, an inherited/open FD within the same runtime, attempted
cross-runtime FD transfer, same-UID wrong-cgroup compatibility fixtures, PID
exit/reuse, listener replacement, namespace escape, Hall reconnect, and
archive/revoke while a connection is accepted.

### 5. Effectful operations reserve durable identity before dispatch

Every effectful adapter creates a stable random `operation_id` before first send
and reuses it across retries. Hall's single writer executes one transaction that:

1. validates the gateway-derived context, current authority epoch, operation
   descriptor, typed input, and resource scope;
2. applies Hall-owned defaults/canonicalization and computes the input digest;
3. reserves unique `(organization, initiating principal/session, operation_id)`
   and appends the operation intent plus durable resource/attempt/fencing epoch.

A duplicate operation ID with the same digest attaches to and replays the stored
resource/result; the same ID with another digest fails closed. Only committed
intent may dispatch. Dispatch carries the durable resource ID and fencing epoch,
so response loss and retry cannot create a second effect. Version negotiation,
catalog lookup, schema fetch, and retries are side-effect free.

The transaction is the authorization linearization point. Revocation increments
the durable authority epoch. Every operation descriptor declares one revocation
policy: `fence-before-effect`, `cancel-running`, or
`finish-committed-and-reconcile`. Agent-initiated host effects default to
`fence-before-effect + cancel-running`; irreversible activation may use the last
policy only with explicit rollback/restore reconciliation. Envoy rechecks the
intent's authority/fence epoch immediately before the first host effect and
rejects stale work. Concurrent-revocation tests deterministically cover both
orders around intent commit and first effect.

### 6. CLI, MCP, REST, and UI share operation contracts and services

The implementation introduces one exhaustive, versioned operation registry in
`olympus-proto`. Each descriptor defines canonical operation ID,
request/result/error and stream/cursor types, read/effect classification,
capability/resource resolver, principal/organization scope, idempotency scope,
revocation policy, protocol range, availability gate, and audit/redaction
policy. Hall operation handlers accept a gateway-derived authenticated context
plus the descriptor's typed input and return its typed output/error. Adapters
translate only:

```text
CLI args/stdin       -> operation request -> operation result -> stdout/exit
MCP tool call        -> operation request -> operation result -> MCP response
REST request         -> operation request -> operation result -> HTTP response
UI action            -> REST adapter      -> operation result -> view state
```

Authorization, organization scoping, idempotency, dispatch, durable persistence,
and host command construction never live in CLI or MCP adapters. This is a deep
module: callers learn a small typed operation interface while Hall hides policy,
recovery, persistence, and transport complexity.

The operation schema carries a version and supports additive evolution. CLI and
Hall negotiate compatible protocol versions before any effectful call. Version
or schema mismatch fails closed with an upgrade instruction.

CLI and MCP names are generated or exhaustively matched against the registry.
Equivalence tests submit the same canonical request and compare authorization,
normalized input digest, durable events, and non-blocking operation result before
CLI-specific waiting/rendering. The legacy raw operator job DTO is not registered
as an agent operation and is removed or remains separately named operator-only;
no agent descriptor accepts executable, argv, env, cwd, SSH, or raw HTTP fields.

### 7. Help, manuals, discovery, and errors are product interfaces

Static commands use `clap`. Build/release generation uses `clap_mangen` and
`clap_complete` from the exact same command definition, producing:

- `olympus <command> --help`;
- `man olympus`, `man olympus-workflow`, and command pages;
- Bash, Zsh, Fish, and PowerShell completions.

Runtime-defined workflow help is fetched from Hall:

```bash
olympus workflow run <slug> --help
olympus workflow show <slug> --schema --output json
```

Help includes description, immutable definition digest/version, inputs, types,
required/default values, examples, required capabilities, expected output
schema, and whether the run can perform effects. Help retrieval is read-only and
organization-scoped.

`olympus workflow run --help` is static generic help and never contacts Hall;
adding `<slug>` requests dynamic schema help. Missing and unauthorized slugs use
the same non-enumerating response. Runtime text is length-bounded and escapes
control/ANSI/bidi/newline content. Shell completions treat runtime values as
quoted data returned by a completion protocol; they never interpolate schema
text into generated executable shell source. Hostile terminal and shell
metacharacter fixtures are mandatory.

Errors have a stable machine code, human message, retryability, and optional
field violations. Human mode prints the concise message and remediation. JSON
modes print the complete typed error to stdout only when the operation result is
the requested machine output; transport/usage diagnostics stay on stderr.

Stable exit classes:

```text
0  accepted/completed successfully
2  CLI usage or local schema validation
3  authentication/capability denied
4  resource not found
5  conflict, fenced attempt, or invalid state transition
6  unavailable, transport failure, or retryable remote failure
7  durable run/job/deployment completed unsuccessfully
8  incompatible CLI/protocol/schema version
```

Exact error codes inside these classes are the automation contract; scripts
must not parse English messages.

### 8. Audit records intent, not shell accidents

Hall records the initiating principal/session/runtime, surface (`cli`, `mcp`,
`rest`, or `ui`), operation ID, normalized typed input digest, capability
decision, idempotency key, resulting resource ID, and terminal outcome.

Principal, organization, session, runtime attempt, and node identity are not
fields accepted from an agent operation request. Hall derives them from the
authenticated gateway context; any compatibility field carrying identity is
ignored or rejected before authorization.

Hall computes the digest over its post-validation/default, versioned canonical
representation. Equivalent CLI/MCP requests intentionally differ only in the
audit `surface`; they share operation ID semantics, resource identity, canonical
input digest, authorization outcome, and durable domain events while producing
separate invocation audit records.

Raw command lines are not authoritative and are not logged when they can contain
user content. Secret values are never accepted by the CLI. Re-running a printed
CLI command is safe only where the operation advertises idempotency or requires
an explicit new attempt.

### 9. Cancellation distinguishes request, quiescence, and terminal truth

`workflows.cancel` appends `RunCancelRequested` and prevents new step dispatch.
It then fences/cancels in-flight activities according to their descriptors.
`RunCancelled` is terminal only after every in-flight attempt is durably terminal
or proven quiesced. An ambiguous or potentially live effect produces
`RunIndeterminate` and operator reconciliation, never a falsely terminal cancel.
CLI get/watch/exit output exposes these states honestly.

## Consequences

- Agents gain a highly discoverable, pipeable Olympus interface without gaining
  shell-shaped remote authority.
- MCP remains available for native tool invocation; it is no longer the only
  agent-facing adapter.
- Workflow schema quality becomes user-facing CLI quality. Published definitions
  must carry descriptions, examples, and output schemas.
- The CLI can be tested against an in-memory operation adapter and a real private
  UDS/iroh path without starting a browser.
- A new `olympus-cli` crate/binary and a shared typed operation vocabulary are
  required.
- Man pages cover static commands; runtime workflow help is necessarily dynamic
  and comes from the pinned definition schema.

## Rejected alternatives

### CLI wraps Hall REST with an installation token

Rejected. It leaks broad credentials into runtimes and creates a second policy
surface.

### CLI shells out to MCP

Rejected. It adds another process/protocol hop, weakens streaming and exit-code
semantics, and makes MCP availability a prerequisite for ordinary CLI use. Both
adapters instead share typed operations and the runtime gateway.

### Dynamic flags without a workflow schema

Rejected. Guessing values from strings produces ambiguous booleans, arrays,
numbers, and objects and creates runs before useful validation.

### Arbitrary `olympus api` or `olympus exec`

Rejected. Those surfaces erase semantic capability checks and recreate SSH or a
raw admin API under a convenient name.

## Acceptance gates

1. A real sandbox Hermes runtime invokes `olympus session info` through its
   private gateway without Hall credentials or general Hall network access.
2. Cross-session socket access, copied paths, wrong cgroup/UID, archived session,
   revoked grant, key rotation, and protocol downgrade all fail closed.
3. CLI and MCP calls for the same operation produce equivalent durable events,
   authorization decisions, and typed results.
4. `workflow run <slug> --help` exactly reflects the pinned published schema;
   reserved-name, unknown, oversized, or type-invalid inputs do not create a run;
   an activation change between help and submission cannot substitute a digest.
5. Waiting, detach, Ctrl-C, reconnect, JSON/JSONL, pipeline, bounded-result, and
   failed-run exit semantics are covered by black-box tests. Ambiguous response
   loss/retry cannot create a second run for one client request ID.
6. Generated help, man pages, completions, and machine schemas are checked for
   drift in CI.
7. No effectful CLI command is exposed before the corresponding SESSION-SAFE,
   capability, durable job, provider, sandbox, workflow, edge, app, or deployment
   gate is green.

Per-operation availability matrix:

| Operations | Hard gate |
|---|---|
| `session.info`, capability reads | SESSION-SAFE identity + runtime gateway + fail-closed CAPS |
| `operations.get` | preceding gate; scoped to the initiating principal/session |
| node reads | preceding gate + organization-scoped Fleet read policy |
| job reads | durable JobService/ACK/reconciliation |
| job effects | job reads + provider construction + hostile sandbox/process gates |
| workflow reads | WF-1 durable projection |
| workflow run/control | workflow reads + JOBS-2 + providers + CAPS + runtime gateway; effectful definitions require hostile sandbox gate |
| artifact get | durable artifact registry + scoped read capability |
| deployment effects | DEPLOY-1 contract/journal/provider + durable edge + migration gates |
| app effects | APP-1 + PKG-1 apps + durable edge + CAPS/JOBS gates |
