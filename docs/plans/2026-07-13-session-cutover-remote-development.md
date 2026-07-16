# Olympus Session Cutover and Sandbox Development Implementation Plan

> **For Hermes:** execute directly or through isolated Kanban worktrees; use an adversarial reviewer before each merge. Do not use implementation subagents.

**Goal:** Make Olympus reliable enough to become the operator’s primary session environment and let capability-scoped Olympus agents build, deploy, expose, and roll back a candidate Olympus stack on an fxcluster sandbox node.

**Architecture:** Stable Hall remains the single authority. An enrolled sandbox
Envoy owns remote processes and host effects over iroh. Agents call typed Hall
operations through an Envoy-mediated runtime gateway bound to the runtime
attempt, using native MCP tools or the Rust `olympus` CLI; they never receive
SSH, Hall network access, or the installation token. Durable session ingress
and runtime reconciliation land before durable jobs, agent operations,
deployment attempts, and apps.

**Tech stack:** Rust/Tokio/Axum, SQLite event log and projections, iroh QUIC,
bubblewrap, systemd/cgroups, Caddy, React/Vite, Envoy-mediated runtime UDS,
MCP, a `clap`-based Rust CLI with generated man pages/completions, Maestro/real
Chromium.

**Doctrine:** Hall owns truth and policy; Envoy owns effects; agents receive typed capabilities, not shells.

---

## Current-state blockers verified on 2026-07-13

| Surface | Exists | Blocking gap |
|---|---|---|
| Remote sessions | `RemoteRuntime`, iroh Envoy transport, explicit `node` in session API | UI has no node choice; remote fork unsupported; remote resumability fails closed |
| Jobs | `JobRunner`, `DispatchJob`, cancellation, bounded output/time, Envoy spool | Hall job map is volatile; output is ACKed before durable job storage; operator-only argv REST API |
| Capabilities | Signed narrowing envelopes and one `CapabilityAuthorizer` seam | No job/deploy/app capability vocabulary wired to agent calls |
| Agent tooling | Runtime setup can inject MCP definitions | No runtime-bound Olympus operation gateway, Hall MCP adapter, or `olympus` CLI |
| Managed apps | ADR 0015 and APP-1 card | No `AppHost`, `ServiceTable`, service frames, app projection, or lifecycle routes in merged tree |
| Edge | Caddy driver source and deploy assets | Live Hall reports edge missing; real route churn/WebSocket coverage incomplete |
| Deployment | Local hash-suffixed binary install and symlink flip | No remote artifact protocol, environment lease, deployment journal, or rollback service |

## Dependency graph

```text
0.1 trustworthy baseline + 0.2 canonical PKG-1

0.1 -> SESSION-SAFE 1.1 -> 1.2 -> 1.3 -> 1.4 -> crash oracle 1.5
1.2 + 1.3 + 1.4 -> runtime gateway 3.1 -> injection 3.2 -> CLI core 3.3

0.1 -> JOBS 2.1 attempt inventory + 2.2 durable projection
2.2 -> 2.3 JobService scaffold
2.1 + 2.2 + 2.3 scaffold -> 2.4 Envoy terminal/spool durability
2.4 -> 2.3 startup/reconnect reconciliation enabled
2.1 + 2.4 -> 2.5 process lifecycle
2.3 reconciled + 2.5 -> provider 4.1 -> hostile sandbox 4.2
4.1 + 4.2 + 3.3 -> typed job CLI/MCP 4.3 -> artifacts 4.4

WF-1 / 4.5 requires:
  0.2 canonical PKG-1 + 1.4 CAPS + JOBS 2.1-2.5 + gateway/CLI 3.1-3.3
  + provider/sandbox/job/artifact gates 4.1-4.4

0.1 -> durable EDGE 6.1
1.4 + 2.1-2.5 + 0.2 contributions.apps + 6.1 -> APP-1 6.2
4.1-4.4 + 6.1 -> DEPLOY contract 7.1 -> provider 7.2 -> CLI/MCP 7.3 -> UI 7.4

Self-development proof 8.2 requires:
  SESSION-SAFE 1.1-1.5 + AGENT-IFACE 3.1-3.3 + JOBS 2.1-2.5/4.1-4.4
  + WF-1 4.5 + EDGE 6.1 + APP 6.2 + DEPLOY 7.1-7.4 + hostile real-substrate gates

8.1 bootstrap + all preceding gates -> 8.2 self-development proof
8.2 + every ADR 0017 cutover gate -> 8.3 seven-day dogfood gate
```

These are dispatch-blocking dependencies, not narrative suggestions. Before any
worker starts, the Kanban card must mirror the relevant incoming edges; a green
phase number does not override an unmet task/gate dependency.

## Phase 0 — Freeze a trustworthy baseline

### Task 0.1: Integrate and commit the already-verified production fixes

**Objective:** Ensure production is reproducible from Git before adding migration-critical work.

**Files already changed:**
- `Cargo.toml`
- `crates/control-plane/src/log.rs`
- `crates/envoy/src/main.rs`
- `docs/postmortems/0020-event-codec-float-roundtrip-deploy-failure.md`
- `docs/postmortems/0021-envoy-replay-starved-heartbeats.md`
- Vault ADR/plan/tab changes currently in the main worktree

**Steps:**
1. Separate coherent commits without dropping any verified production fix.
2. Run `CARGO_BUILD_JOBS=1 make test`.
3. Run `CARGO_BUILD_JOBS=1 make lint` and `cargo fmt --check`.
4. Run `cd ui && bun run test && bun run typecheck && bun run build`.
5. Confirm live Hall and Envoy binaries can be rebuilt from the resulting commit.

**Gate:** no live binary contains source changes absent from Git.

### Task 0.2: Reconcile PKG-1 and ARCH-D before new protocol work

**Objective:** Land one package implementation and remove duplicate branches before APP-1 depends on it.

**Files:** determined from cards `t_356b59e9`, `t_0a3aa9ce`, and `t_4d2b8455`.

**Steps:**
1. Compare both PKG implementations against ADR 0012/0015 requirements.
2. Select one coherent package manifest/registry implementation; do not merge duplicate models.
3. Merge ARCH-D only after proving the production codec marker gate remains valid.
4. Run the full workspace gate under the shared Cargo lock.

**Gate:** `PackageManifest` has one canonical definition and supports `contributions.apps` extension without app lifecycle code leaking into package parsing.

## Phase 1 — SESSION-SAFE migration foundation

### Task 1.1: Make session ingress and transport ACK atomic

**Objective:** Remove the ACKed-but-uncommitted transcript loss window before
building agent operations on remote sessions.

**Files:**
- Modify: `crates/control-plane/src/log.rs`
- Create: `crates/control-plane/src/views/turn_ingress.rs`
- Modify: `crates/control-plane/src/server/envoy_conn.rs`
- Modify: `crates/control-plane/src/server/routes/sessions.rs`
- Test: crash-point integration test under `crates/control-plane/tests/`

**Required behavior:** persist each frame payload/reference and its watermark in
one SQLite transaction; project assistant/tool/reasoning content from durable
ingress; ACK only the committed watermark.

**RED:** inject a crash after any text/tool/done frame is received but before the
old background accumulator commits; current code loses bytes.

**GREEN gate:** for crashes at receive, ingress commit, projection, and ACK,
restart/replay produces the exact producer sequence and byte hashes with no
duplicates.

### Task 1.2: Report and project authoritative runtime attempts

**Objective:** Reconcile real runtimes even when their spool is empty.

**Files:**
- Modify: `crates/envoy/src/runtime_table.rs`
- Modify: `crates/envoy/src/main.rs`
- Modify: `crates/proto/src/frames.rs`
- Create: `crates/control-plane/src/views/runtime_attempt.rs`
- Modify: `crates/control-plane/src/node.rs`
- Test: `crates/control-plane/tests/iroh_envoy_integration.rs`

**Inventory:** attempt ID, logical session, harness/provenance, child identity,
state, resumability, in-flight/pending-permission state, and last sequence.

**Gate:** Hall restart reattaches or explicitly classifies idle empty-spool,
in-flight, and pending-permission runtimes without starting a second child.

### Task 1.3: Introduce one durable remote runtime-control service

**Objective:** Route prompt, cancel, steer, permission, stop, drain, and recovery
through the same reconstructed remote attempt.

**Files:**
- Create: `crates/control-plane/src/server/runtime_control.rs`
- Modify: `crates/control-plane/src/server/routes/sessions.rs`
- Modify: `crates/control-plane/src/server/envoy_conn.rs`
- Modify: `crates/control-plane/src/server/bridge_mgr.rs`
- Test: remote REST/UI control integration tests

**Gate:** after browser refresh and Hall restart, a real remote adapter receives
permission response, steer, and cancel; cancellation changes the child/turn
state rather than returning a false success.

### Task 1.4: Harden node identity and capability semantics

**Objective:** Make session authority fail closed before MCP exists.

**Files:**
- Modify: `crates/control-plane/src/server/capability.rs`
- Modify: `crates/control-plane/src/server/routes/sessions.rs`
- Modify: `crates/control-plane/src/node.rs`
- Modify: enrollment/allowlist persistence as required
- Test: capability and iroh integration suites

**Requirements:** explicit envelopes on agent-facing sessions; actual
authenticated principal at assignment; session/org/audience/runtime-attempt/key
version/revocation binding; typed exact resources or explicit segment wildcards;
logical node ID bound to enrolled iroh public key; duplicate/takeover hello
rejected.

**Gate:** no-envelope, wrong-org, prefix-collision (`node-1` vs `node-10`),
archive, revocation, key rotation/restore, stolen cross-session authority, and
concurrent revocation all fail closed.

### Task 1.5: Add the session crash/recovery oracle

**Objective:** Prove payload durability, not merely watermark monotonicity.

**Files:**
- Create: `crates/control-plane/tests/session_transport_crash_matrix.rs`
- Create: deterministic producer fixture under `fixtures/sessions/`
- Add fault-injection hooks behind test-only features

**Gate:** producer and restored consumer manifests match exact frame sequence,
message IDs, text bytes, tool/reasoning hashes, runtime attempt, and terminal
state across Hall/Envoy/network crash points.

## Phase 2 — JOBS-2 durable remote execution

### Task 2.1: Define job attempt identity and retained Envoy inventory

**Objective:** Make retry/reconciliation possible before Hall's `JobService`
claims to reconcile anything.

**Files:**
- Modify: `crates/proto/src/frames.rs`
- Modify: `crates/envoy/src/job_table.rs`
- Modify: `crates/envoy/src/main.rs`
- Test: Envoy restart and ambiguous-dispatch integration tests

**Requirements:** `(job_id, attempt_epoch)` wire identity; retained active and
recent terminal attempts; hello/update inventory; idempotent duplicate dispatch;
explicit at-least-once dispatch semantics with fenced attempt effects.

**Gate:** disconnect after Envoy spawn but before response never starts a second
process for the same attempt.

### Task 2.2: Define durable job events and projection

**Objective:** Replace process-global job truth with event-backed records.

**Files:**
- Modify: `crates/control-plane/src/event.rs`
- Create: `crates/control-plane/src/views/job.rs`
- Modify: `crates/control-plane/src/views/mod.rs`
- Modify: `crates/control-plane/src/log.rs`
- Test: `crates/control-plane/src/views/job.rs`

**Events:** `JobPlanned`, `JobDispatched`, `JobOutputRecorded`, `JobCompleted`, `JobCancelRequested`, `JobLost`.

**Required fields:** organization, initiating principal/session, activity/provider, node, attempt, timestamps, input digest, resource policy, status, terminal reason, output/artifact references.

**RED:** projection rebuild after restart reproduces a running and a completed job exactly.

**GREEN gate:** replay is idempotent and unknown/out-of-order terminal events fail closed.

### Task 2.3: Replace static REST state with `JobService`

**Objective:** Give REST, MCP, and workflows one application service.

**Hard prerequisite:** Task 2.4's real-Envoy terminal/spool durability gate must
be green before startup/reconnect reconciliation is enabled or this task is
completed. Implementation may scaffold service APIs earlier, but cannot claim
reconciliation against the pre-2.4 transport.

**Files:**
- Create: `crates/control-plane/src/server/job_service.rs`
- Modify: `crates/control-plane/src/server/routes/jobs.rs`
- Modify: `crates/control-plane/src/server/mod.rs`
- Modify: `crates/control-plane/src/server/tests.rs`

**Rules:**
- Persist intent before wire dispatch.
- Generate stable job ID and attempt number.
- Verify node is connected and has `JobRunner`.
- Scope reads/cancel to owning organization/principal.
- Reconcile non-terminal records on startup/reconnect.

**RED:** Hall restart after accepted dispatch retains job metadata and status.

**GREEN gate:** no `OnceLock<HashMap<...>>` remains in jobs routes.

### Task 2.4: Make Envoy output ACK and terminal ordering durability-correct

**Objective:** Never truncate the Envoy spool before Hall has durable output truth.

**Files:**
- Modify: `crates/control-plane/src/node.rs`
- Modify: `crates/control-plane/src/server/job_service.rs`
- Modify: `crates/control-plane/src/server/envoy_conn.rs`
- Modify: `crates/envoy/src/job_table.rs`
- Modify: `crates/envoy/src/main.rs`
- Modify: `crates/envoy/src/spool.rs`
- Test: Hall crash tests plus real Envoy spool/job tests

**RED:** crash Hall between receiving output and committing it; reconnect must
replay the chunk exactly once. Also prove final stdout/stderr bytes cannot arrive
after the terminal result.

**GREEN gate:** ACK is emitted only after the event/output-reference transaction
commits; both output drains join before terminal; terminal is the final sequence.
Sequence reservation+append is atomic. ENOSPC, cap, fsync, and ACK-rewrite
failures stop/backpressure without creating a permanent sequence gap.
Tests cover final-byte ordering, cap exhaustion, ENOSPC/read-only spool, fsync
failure, corrupt tail, ACK rewrite failure, and restart against the real spool.

### Task 2.5: Harden process lifecycle

**Objective:** Make timeout/cancel terminate complete process trees and survive duplicate dispatch.

**Files:**
- Modify: `crates/envoy/src/job_table.rs`
- Modify: `crates/envoy/src/main.rs`
- Modify: `crates/proto/src/frames.rs`
- Test: `crates/envoy/src/job_table.rs`

**Requirements:** process group/cgroup kill, duplicate `(job_id, attempt)` idempotency, explicit terminal state, output sequence continuity, orphan reconciliation in Envoy hello.

**Gate:** a test job that forks/daemonizes/double-forks leaves no process or
populated cgroup after cancel, timeout, or Envoy restart. This process-tree gate
is non-skippable in the cutover profile.

## Phase 3 — AGENT-IFACE-1 Envoy-mediated CLI and MCP operations

### Task 3.1: Add the runtime-bound local operation gateway

**Objective:** Resolve every agent CLI/MCP operation to one authenticated Envoy
peer and runtime attempt without exposing Hall networking or installation
credentials.

**Files:**
- Create: `crates/envoy/src/runtime_gateway.rs`
- Create: `crates/proto/src/operations.rs`
- Create: `crates/control-plane/src/server/operations/mod.rs`
- Create: durable operation intent/projection under `crates/control-plane/src/`
- Create: `crates/control-plane/src/server/mcp/mod.rs` as an adapter
- Modify: `crates/proto/src/frames.rs`
- Modify: `crates/envoy/src/runtime_table.rs`
- Modify: `crates/control-plane/src/server/runtime_control.rs`
- Test: local UDS plus iroh round-trip integration tests

**Rules:** V1 is private UDS only, with the host/guest paths, owner/mode,
per-runtime UID/GID or user namespace, cgroup, `SO_PEERCRED`/PID-start/cgroup,
listener-generation, accepted-connection tracking, and close/unmount rules from
ADR 0019. Runtime subprocesses intentionally share one attempt authority;
cross-runtime FD delegation is denied by OS namespaces/identity. Hall resolves
Envoy key + gateway generation + runtime attempt + session from the accepted
connection, never request fields; every call rechecks current durable authority.
The gateway accepts only versioned registered operations and does not forward
arbitrary Hall REST or network traffic.

The operation registry exhaustively defines types, effect/read classification,
capability/resource resolver, scope, idempotency, revocation, availability,
protocol, and audit/redaction policy. CLI, MCP, and REST call the same Hall
operation modules; policy cannot live in an adapter. Each effectful call reserves
its stable client operation ID, canonical input digest, current authority epoch,
durable resource/attempt, and fence in one Hall transaction before dispatch.
Same-ID/same-digest retries attach; same-ID/different-digest fails. Envoy checks
the authority/fence epoch before first host effect.

**Gate:** a gateway for session A cannot name B, survives Hall reconnect through
attempt reconciliation, and is closed on archive/revoke. Secrets do not appear
in argv, environment, logs, artifacts, or model-visible tool results. Real tests
cover copied path/open FD, attempted cross-runtime FD transfer, same-UID
wrong-cgroup, PID reuse, listener replacement, stale generation, namespace
escape, already-accepted revocation, both concurrent-revocation orders, response
loss/retry, same-ID/different-input, and side-effect-free negotiation.

### Task 3.2: Inject the runtime gateway and non-effectful adapters

**Objective:** Make the tools available without hand-editing harness configuration.

**Files:**
- Modify: `crates/control-plane/src/server/routes/sessions.rs`
- Modify: `crates/envoy/src/runtime_table.rs`
- Modify: `crates/proto/src/runtime.rs`
- Modify/test adapters under `crates/envoy/src/adapter/`

**Gate:** real Hermes plus adapter-level Codex/Claude coverage receives the same
gateway through a private runtime UDS and native MCP configuration; before Phase
4 completes, only non-effectful `session.info` is registered. The real sandbox
harness calls it through both MCP and the CLI, Hall restarts, and the reconciled
gateway still works. Agent mode cannot select an operator profile, endpoint, or
token. The runtime contains no operator config/credentials and has no Hall
network route even if it bypasses or modifies CLI mode detection.

### Task 3.3: Ship the `olympus` CLI core and generated documentation

**Objective:** Establish the stable CLI grammar, transport, output, and error
contracts before adding effectful command groups.

**Files:**
- Create: `crates/cli/Cargo.toml`
- Create: `crates/cli/src/main.rs`
- Create: CLI command/client/render/error modules under `crates/cli/src/`
- Include `operations.get` / `olympus operation get <operation-id>` as the
  generic ambiguous-acceptance recovery surface
- Modify: workspace `Cargo.toml`
- Generate/package man pages and Bash/Zsh/Fish/PowerShell completions
- Test: black-box CLI tests under `crates/cli/tests/`

**Rules:** static grammar uses `clap`; agent mode uses only the private runtime
UDS; stdout is machine/result data, stderr is progress/diagnostics; no raw API,
exec, SSH, argv, token, or endpoint command. Stable exit classes and versioned
JSON/JSONL envelopes follow ADR 0019. Commands map exhaustively to the canonical
operation registry. Static help is offline; dynamic schema help is
organization-scoped, bounded, and terminal/shell escaped. Runtime schema values
never become executable completion source.

**Gate:** `olympus session info` succeeds in a real sandbox runtime without Hall
credentials. Cross-session socket, wrong UID/cgroup, archive/revoke, protocol
downgrade, broken pipe, JSON/stdout, help, man-page, and completion drift tests
pass. The CLI and MCP probe produce the same typed result and authorization
decision. Hostile ANSI/control/bidi/newline/shell-metacharacter fixtures and
compiled-but-unavailable operation behavior pass. Commit+lost-response with Hall
and gateway unavailable at Ctrl-C/timeout returns `acceptance_unknown` with the
stable operation ID and exact lookup command; it never claims or loses a run ID.

## Phase 4 — JOBS-3 sandboxed activity providers

### Task 4.1: Introduce the activity-provider seam

**Objective:** Separate agent intent from host command construction.

**Files:**
- Create: `crates/envoy/src/activity/mod.rs`
- Create: `crates/envoy/src/activity/command.rs`
- Modify: `crates/envoy/src/job_table.rs`
- Modify: `crates/proto/src/frames.rs`

**Initial providers:** `command.checked`, `olympus.checkout`, `olympus.verify`, `olympus.release.build`.

**Gate:** only Envoy providers produce argv/env/cwd; neither CLI nor MCP requests
can submit executable, raw argv/env/cwd, SSH, or raw HTTP fields. The legacy raw
operator job DTO is absent from the agent operation registry.

### Task 4.2: Enforce bubblewrap, OS identity, and resource policy

**Objective:** Make capabilities real at the host boundary.

**Files:**
- Create: `crates/envoy/src/sandbox.rs`
- Modify: `crates/envoy/src/job_table.rs`
- Modify/create hardened system units under `deploy/systemd/`
- Test: `crates/envoy/src/sandbox.rs`

**Mounts:** read-only toolchain/runtime roots; explicit repo/workspace mounts;
writable artifact/output roots; no SSH agent, Hall/Envoy/Caddy state, Hermes
global state, systemd manager, control sockets, or unrelated home paths.

**Gate:** escape, symlink, `/proc`, undeclared home, undeclared network, memory, CPU, output, and wall-time tests fail closed.
These hostile fixtures are non-skippable in the cutover profile.

### Task 4.3: Expose typed job operations through CLI and MCP after provider+sandbox gates

**Objective:** Deliver `nodes.list`, `jobs.run`, `jobs.get`, `jobs.logs`, and
`jobs.cancel` through both agent adapters only after Tasks 4.1 and 4.2 pass.

**Files:**
- Create: `crates/control-plane/src/server/mcp/tools/jobs.rs`
- Modify: `crates/control-plane/src/server/mcp/mod.rs`
- Create: corresponding `crates/cli/src/commands/{node,job}.rs`
- Modify: `crates/control-plane/src/server/capability.rs`
- Test: CLI/MCP equivalence plus provider/sandbox integration tests

**Rules:** agent chooses a registered activity and typed input, not arbitrary
argv. Envoy provider—not caller data—constructs argv/env/cwd. Hall checks `job.*`
capability, linked repositories, writable paths, node/pool, and resource limits.
Effectful job tools remain compile/runtime disabled until provider and hostile
sandbox gates are green.

**Gate:** denial tests cover missing/tampered authority, cross-session/org,
node/path/child expansion; first real `jobs.run` proves provider construction and
non-skippable hostile mount/network/process isolation. CLI and MCP invocation
produce equivalent durable events, capability decisions, and outputs.

### Task 4.4: Register durable job artifacts

**Objective:** Return build/test outputs without embedding unbounded bytes in the event log.

**Files:**
- Create/modify artifact service under `crates/control-plane/src/server/`
- Modify: `crates/control-plane/src/server/job_service.rs`
- Modify: `crates/envoy/src/job_table.rs`
- Test: control-plane job/artifact integration test

**Gate:** logs and release bundle survive Hall restart and are attributable to job, session, revision, node, and content hash.

### Task 4.5: Implement WF-1 and the schema-aware workflow command surface

**Objective:** Make bounded durable workflows the primary pipeable composition
surface for agents without growing a second workflow implementation in the CLI.

**Sources:** ADR 0013, ADR 0019, and
`docs/cards/arch/wf-1-workflow-kernel.md`.

**Prerequisites:** canonical PKG-1, hardened CAPS, durable JobService, activity
providers, runtime gateway, and CLI core. Effectful workflows additionally
require the hostile sandbox gate.

**Files:**
- Implement WF-1 event/projection/scheduler/routes from its card
- Implement `olympus.workflow-input/v1` publication compiler/validator and one
  shared parser/help/Hall/MCP conformance corpus
- Add workflow operation types under `crates/proto/src/operations.rs`
- Add Hall workflow operation handlers and MCP adapter
- Create: `crates/cli/src/commands/workflow.rs`
- Generate/check static help, man pages, completions, and machine schemas
- Test: workflow kernel plus black-box CLI/MCP/pipeline integration

**Command contract:** canonical execution is
`olympus workflow run <slug>`. It waits by default; `--detach` returns the
durable run reference; Ctrl-C detaches without cancellation. Dynamic flags and
`--help` derive from the pinned published input schema. `--input-json FILE|-`
is mutually exclusive with dynamic flags. Progress goes to stderr; human,
JSON, JSONL, and `result-json` outputs follow ADR 0019.

Underlying `workflows.run` is non-blocking and returns run/digest/operation/cursor
identity. CLI wait composes durable get/watch with monotonic resume sequences.
V1 workflow event sequences are non-expiring event-log truth; reconnect from any
valid sequence is required. Future compaction must version the operation and use
typed `cursor_expired` plus terminal-snapshot fallback without redispatch.
Hall atomically appends `StepDispatchPlanned` plus JOBS-2 intent before dispatch
and reconciles the exact attempt; non-idempotent ambiguity becomes
`StepIndeterminate`. Cancel request, quiescence, indeterminate, and terminal
cancel are distinct durable states.

**Gate:** unknown/type-invalid flags create no run; schema help exactly matches
the pinned definition; reserved/oversized inputs fail before creation; an active
definition change cannot substitute the submitted digest;
wait/detach/cursor-reconnect/failed-run exits are byte-tested; ambiguous response
loss and retry with one client request ID create one run; CLI and MCP calls
create equivalent durable run events. A real sandbox pipeline
passes one successful workflow's `result-json` into another workflow's
`--input-json -` without credentials or unbounded inline output. Tests cover
schema-profile rejection/conformance, republish race, canonical defaults/digest,
schema fetch/before-send/after-commit interruptions, terminal-before-output,
commit+lost-response+unavailable reconciliation at Ctrl-C/timeout, reconnect from
old cursors (with a fixture proving no v1 compaction), truncated pipe input,
non-idempotent ambiguous dispatch, and honest cancellation quiescence.

## Phase 5 — Session cutover UX and recovery

### Task 5.1: Add explicit agent/node/model/capability selection

**Objective:** Let the operator intentionally place a new session on the sandbox.

**Files:**
- Modify: `ui/src/views/sessions/components/AgentPicker.tsx`
- Create: `ui/src/views/sessions/components/NewSessionDialog.tsx`
- Modify: `ui/src/views/sessions/components/SessionSidebar.tsx`
- Modify: `ui/src/api.ts`
- Modify: `ui/src/types.ts`
- Test: corresponding `*.test.tsx` files

**Gate:** unavailable/draining nodes and unsupported agent/node combinations cannot be selected; selected node reaches `POST /api/sessions`.

### Task 5.2: Expose recovery semantics in the product

**Objective:** Build operator UX over the Phase 1 runtime-attempt truth rather
than reimplementing recovery late.

**Files:**
- Modify: `crates/control-plane/src/server/envoy_conn.rs`
- Modify: `crates/envoy/src/runtime_table.rs`
- Modify: `crates/proto/src/frames.rs`
- Modify: `crates/control-plane/src/server/routes/sessions.rs`
- Test: `crates/control-plane/tests/iroh_envoy_integration.rs`

**Decision:** no live process migration. Reattach the same attempt only when the
same Envoy proves from durable local attempt+cgroup+process identity that the
original child remains the unique owner; do not spawn. Otherwise force/prove the
old cgroup empty, terminalize/orphan the old attempt, and resume provenance or
trace-seed a **new attempt epoch**. A new node always means a new attempt.

**Gate:** Hall/Envoy restart, partition, and node loss each have deterministic
UI-visible outcomes. Idle, in-flight, pending-permission, post-spawn,
daemon/double-fork, stale-effect, and single-prompter/cgroup oracles are covered.
These runtime/process hostile gates are non-skippable in the cutover profile.

### Task 5.3: Add the primary-session reliability journey

**Objective:** Turn “move over” into an executable release gate.

**Files:**
- Create: `ui/e2e/session-cutover.spec.ts` or the repository’s current Maestro equivalent
- Create: `scripts/session-cutover-soak.sh`
- Update: `docs/harness/validation.md` if present, otherwise `Makefile`

**Journey:** create remote session → prompt → stream → tool → permission → steer → cancel → reload → Hall restart → Envoy restart → resume/recover → archive/reopen.

**Gate:** evidence bundle contains desktop/mobile screenshots and video, and the
deterministic producer/consumer manifest proves exact session bytes and sequences.

## Phase 6 — Durable edge and APP-1

### Task 6.1: Make edge desired state durable and deploy real Caddy

**Objective:** Replace live `edge: missing` with a real, tested edge.

**Files:**
- Modify as needed: `deploy/caddy/caddy.json`
- Modify as needed: `deploy/systemd/olympus-caddy.service`
- Modify: `crates/control-plane/src/edge/mod.rs`
- Add edge events/projection under `crates/control-plane/src/`
- Create: real Caddy integration tests under `crates/control-plane/tests/`
- Create: operations runbook under `docs/operations/`

**Rules:** stable Hall is the only writer to stable Caddy; desired routes are
durable Hall truth and reconcile before exposure after restart; Caddy admin is
not available to candidate/build identities.

**Gate:** route add/update/remove, Hall forward auth, cookie stripping,
WebSocket, streaming, Caddy restart, Hall restart, stale writer, and unhealthy
upstream behavior pass non-skippably with real Caddy ≥2.11.1 and
`enforce_origin` enabled.

### Task 6.2: Amend PKG-1 and APP-1, then implement APP-1

**Objective:** Add `contributions.apps`, `AppHost`, `ServiceTable`, binary
runtime, health/restart/drain, durable app projection, state directory,
least-privilege service principal, and edge registration.

**Sources:** ADR 0015 and `docs/cards/arch/app-1-servicetable.md`.

**Prerequisites:** hardened CAPS, JOBS plumbing, PKG-1 explicitly amended and
replay-tested for `contributions.apps`, and Task 6.1 durable real edge.

**Additional requirements:** lifecycle reconciles after Hall/Envoy restart
before exposure; service identity is durable but runtime authority is explicit,
short-lived/audience-bound or Envoy-mediated; install authority is not copied;
remove/quarantine revokes access before process/route teardown.

## Phase 7 — DEPLOY-1 remote release environments

Phase 7 depends on Task 6.1 durable edge, not Task 6.2 APP-1. APP-1 and DEPLOY-1
converge only in the final sandbox journey.

### Task 7.1: Define release and deployment contracts

**Objective:** Make deployments content-addressed, idempotent, and recoverable.

**Files:**
- Create: `crates/proto/src/deployment.rs`
- Modify: `crates/proto/src/lib.rs`
- Add events: `crates/control-plane/src/event.rs`
- Create projection: `crates/control-plane/src/views/deployment.rs`
- Test all wire/event/projection round trips

**Entities:** `ReleaseManifest`, `Environment`, `DeploymentAttempt`,
`AttemptEpoch`, `MigrationClass`, `HealthCheck`, `RollbackPlan`,
`RestoreRequired`.

**Rules:** Hall event log is authoritative; Envoy effect journal is subordinate
and fsynced; every effect carries a fenced epoch; migrations are classified
backward-compatible/forward-only/restore-required; write quiescence and RPO/RTO
are explicit.

### Task 7.2: Implement Envoy deployment provider and crash journal

**Objective:** Stage, preflight, activate, verify, and roll back candidate releases without SSH.

**Files:**
- Create: `crates/envoy/src/activity/olympus_deploy.rs`
- Create: `crates/envoy/src/deployment.rs`
- Modify: `crates/envoy/src/main.rs`
- Add fixtures under `fixtures/deployments/`

**Gate:** deliberate bad binary/protocol, migration classes, concurrent writes,
Hall loss after activation, Envoy loss during link/unit flip, Caddy loss, ENOSPC,
stale epoch, corrupt rollback target, and failed restore/rollback health produce
the specified durable state. Automatic binary rollback occurs only when schema
compatibility allows it.

### Task 7.3: Expose deployment tools to approved sessions

**Objective:** Add `deployments.plan/apply/status/rollback` to the typed Hall
operation seam and expose equivalent CLI and MCP adapters.

**Files:**
- Create: `crates/control-plane/src/server/deployment_service.rs`
- Create: `crates/control-plane/src/server/mcp/tools/deployments.rs`
- Create: `crates/cli/src/commands/deployment.rs`
- Modify: `crates/control-plane/src/server/capability.rs`

**Gate:** read-only sessions can plan/status but cannot apply; only
environment-scoped grants can activate or roll back. Equivalent CLI/MCP calls
produce the same durable attempt and authorization decision.

### Task 7.4: Expose candidate UI safely

**Objective:** Route candidate Olympus through Caddy without sharing stable Hall state or primary cookies.

**Files:**
- Extend deployment provider/edge registration service
- Add candidate environment configuration under `deploy/environments/`
- Add browser E2E against `/app/olympus-dev/`

**Gate:** candidate failure and hostile-candidate fixtures cannot read stable
keys/state/spool, call Caddy admin/control sockets, signal/alter stable services,
or starve stable `/`, Fleet, MCP, and sandbox Envoy beyond defined SLOs.

## Phase 8 — SANDBOX-1 fxcluster bootstrap and cutover

### Task 8.1: Bootstrap the operator-provided VM once over SSH

**Objective:** Install the minimum host substrate and enroll Envoy.

**Files:**
- Create: `scripts/bootstrap-remote-envoy.sh`
- Create: `deploy/systemd/olympus-envoy-remote.service`
- Create: `docs/operations/fxcluster-sandbox.md`

**Script properties:** rerunnable, fail-fast, no secret output, pinned package
checks, systemd/cgroup/disk preflight, distinct `olympus-envoy`, build,
candidate, and edge identities, filesystem/socket allowlists, bwrap/Caddy/Podman
capability report, and rollback of partial unit install.

**Gate:** after SSH disconnect, Fleet shows the logical sandbox node online over iroh with roles and toolchain inventory.

### Task 8.2: Execute the self-development proof

**Objective:** Use an Olympus-managed session—not the operator shell—to build and deploy Olympus.

**Steps:**
1. Start a session on the sandbox node.
2. Inspect `olympus --help`, `olympus workflow run --help`, and the selected
   workflow's schema-derived help.
3. Run the checkout/verify/build chain through
   `olympus workflow run olympus-release --revision <rev>`.
4. Pipe its `result-json` into the candidate deployment workflow.
5. Plan and apply to `sandbox-dev` through the typed CLI or equivalent MCP tools.
6. Exercise candidate Sessions, Fleet, Vault, job, app, and edge journeys.
7. Break health intentionally and verify rollback.
8. Restart stable Hall, sandbox Envoy, Caddy, and candidate services one at a time.

**Gate:** all actions and artifacts are visible from the initiating session; CLI
and MCP audit records are equivalent; no direct SSH is used after enrollment.

### Task 8.3: Seven-day dogfood gate

**Objective:** Establish evidence that Olympus can replace the current primary session surface.

**Daily evidence:** minimum scripted session/job counts; producer/consumer
sequence+byte hashes; failed/recovered turns; scheduled Hall/Envoy/network/disk
faults; job/deployment outcomes; resource/SLO and disk growth; edge errors;
manual SSH interventions; database integrity; backup age; measured restore RPO/RTO.

**Go criteria:** seven days without sequence/hash mismatch, unrecovered message
loss, privilege bypass, stable-service SLO breach, manual DB repair, or an
operation that required giving an agent SSH.

**No-go action:** remain on the current session platform, file a postmortem for each blocker, and fix the class before restarting the soak clock.

## Sandbox information needed from the operator

When the VM is ready, provide only connection metadata—not credentials in chat:

- hostname/IP and SSH username,
- OS/version,
- privileged vs unprivileged LXC or VM,
- CPU/RAM/disk quotas,
- systemd user and lingering availability,
- outbound package/GitHub access,
- intended DNS name,
- rootless Podman allowance,
- fxcluster snapshot/restore mechanism.

Use an existing SSH agent or an operator-installed one-time key. Olympus agents
must never receive that key.

## Canonical verification

For every merged slice:

```bash
flock ~/.cache/olympus-cargo.lock env CARGO_BUILD_JOBS=1 make test
flock ~/.cache/olympus-cargo.lock env CARGO_BUILD_JOBS=1 make lint
cargo fmt --check
cd ui && bun run test && bun run typecheck && bun run build
```

CLI slices additionally run black-box agent-runtime transport, stdout/stderr,
JSON/JSONL, pipeline, exit-code, help/man/completion drift, and CLI/MCP operation
equivalence tests.

Before cutover, additionally require real iroh, Caddy, sandbox Envoy, browser,
restart/reconnect, rollback, and copied-database tests. Unit-green alone is not a
migration gate.
