# ADR 0017 — Session cutover and remote development plane

Status: proposed · Date: 2026-07-13
Relates to: ADR 0008 (Hall/Envoy), ADR 0011 (jobs/capabilities), ADR 0013
(workflows), ADR 0014 (edge), ADR 0015 (managed apps), ADR 0019 (agent CLI).

Review chain:

- `docs/reviews/0017-session-cutover-adversarial-review.md` — initial NO-GO.
- `docs/reviews/0017-session-cutover-adversarial-rereview.md` — narrowed blockers.
- `docs/reviews/0017-session-cutover-final-approval-review.md` — final ordering fix.
- `docs/reviews/0017-session-cutover-approval.md` — **APPROVED as a proposed
  architecture/plan**; implementation and cutover gates remain open.

## Context

The next product gate is not another isolated feature. The operator must be able
to use Olympus as the primary session environment while Olympus develops and
deploys its own candidate builds on a disposable sandbox LXC/VM in fxcluster.
That requires one trustworthy path from a managed session to remote host effects,
plus a separate operator recovery path.

The current tree contains useful substrate, but it is not yet a safe agent-facing
remote execution plane:

- Hall already drives remote `AgentRuntime`s through Envoy over iroh, and session
  creation accepts an explicit node (`routes/sessions.rs`, `RemoteRuntime`).
- Session frames are currently ACKed after only the transport watermark is
  durable; assistant/tool bytes remain in an in-memory turn accumulator until
  `Done`. A Hall crash can therefore lose ACKed transcript bytes permanently.
- Envoy hello derives runtime inventory from spool files rather than the actual
  `RuntimeTable`; Hall ignores later runtime inventory frames. Fully ACKed live
  runtimes can disappear from reconciliation.
- Remote prompt uses a task-local runtime object while cancel, steer, and
  permission handlers consult the local bridge table. Those controls do not yet
  reliably reach remote runtimes.
- Envoy advertises `JobRunner`, accepts argv-based `DispatchJob`, bounds time and
  output, and spools output frames (`proto/frames.rs`, `envoy/job_table.rs`).
- The jobs REST surface is operator-only, stores job records in a process-global
  in-memory map, and acknowledges Envoy output after mutating only that map
  (`routes/jobs.rs`, `node.rs`). Hall restart therefore loses job state even
  though Envoy has been told it may truncate the spool.
- Job execution has no session principal, capability decision, immutable input
  bundle, durable artifact record, namespace isolation, or agent-facing MCP
  endpoint.
- The session setup path can inject MCP definitions into runtimes, but Hall does
  not yet expose its own session-tool MCP server.
- Missing capability envelopes currently mean legacy full authority, resource
  matching is raw string-prefix based, and capability assignment does not always
  use the authenticated principal. None of those semantics are acceptable for
  agent-facing tools.
- APP-1 is specified but `NodeRole::AppHost`, `ServiceTable`, and managed-app
  lifecycle frames are not in the merged tree.
- The UI creates a session after choosing an agent but does not offer node
  placement at creation.
- Remote runtime fork is explicitly unsupported and Hall currently reports
  remote runtimes as non-resumable.
- Production Hall currently reports its external edge as missing. Managed app
  routes cannot be a migration gate until Caddy is actually deployed and
  health-gated.
- Edge desired state is in memory and cannot converge routes after Hall restart.

## Doctrine

**Hall owns session, job, deployment, app, policy, and audit truth; Envoy owns all
host effects; agents receive typed, capability-scoped tools—not SSH.**

The fxcluster sandbox is an Olympus node, not a special deployment backend. It
runs Envoy with `AgentRuntime`, `JobRunner`, and later `AppHost` roles. Routine
build, test, deploy, and app operations travel over the existing authenticated
iroh connection.

## Decisions

### 1. Stable Hall and candidate Olympus are separate trust and failure domains

During cutover, stable Hall remains the authority. The sandbox Envoy connects to
it and hosts builds, sessions, and managed apps. Separate directories are not a
boundary: stable components, build jobs, candidate services, and edge run under
different OS identities/cgroups with explicit filesystem and socket allowlists.
Candidate code cannot signal stable processes, read stable Envoy/Hall/Caddy
state, access the systemd manager, or call Caddy's admin API.

Recommended sandbox identities:

```text
olympus-envoy       stable enrolled supervisor; owns only Envoy state
olympus-build       transient/DynamicUser jobs; declared bind mounts only
olympus-candidate   candidate Hall/Envoy services and candidate state
olympus-edge        stable Caddy; admin socket reachable only by stable Hall
```

Envoy is a system service with narrowly delegated ability to create/control
candidate units; candidate processes never receive that authority. If the LXC
cannot enforce these identities/cgroups, use a VM or a nested rootless container
boundary before accepting untrusted agent-built code.

Candidate layout:

```text
/var/lib/olympus-sandbox/                 # operator-owned host root
  releases/<content-hash>/                # immutable extracted release bundles
  environments/dev/
    current -> ../../releases/<hash>/
    state/                                 # candidate OLYMPUS_HOME
    artifacts/
    deploy-journal.jsonl
```

The exact root may be user-scoped when the LXC is unprivileged, but release and
state ownership remain separate.

### 2. SSH is bootstrap and break-glass, never the agent API

Operator SSH is allowed for initial host preparation, recovery, and the future
audited PTY surface from ADR 0002 §18. SSH credentials are not mounted into agent
sessions and no general `ssh` MCP tool is introduced.

Once Envoy is enrolled, routine operations use typed tools backed by Hall
services:

- `nodes.list`
- `jobs.run`, `jobs.get`, `jobs.cancel`, `jobs.logs`
- `deployments.plan`, `deployments.apply`, `deployments.status`,
  `deployments.rollback`
- `apps.install`, `apps.start`, `apps.stop`, `apps.status`

This preserves intent, permits capability checks, makes retries idempotent, and
keeps a complete audit trail. A raw shell string or arbitrary remote target is
not a deployment contract.

### 3. Hall owns typed operations; Envoy mediates agent CLI and MCP transport

Agents do not open general network access to Hall. Envoy injects a local
runtime gateway into each eligible runtime. Native MCP tools and the `olympus`
CLI are adapters over that gateway. It is bound to the runtime attempt by a
private UDS and tunnels typed calls over the already authenticated Hall↔Envoy
iroh/UDS channel. Hall resolves `(peer identity, runtime attempt, session)` and
evaluates the current durable capability grant on every call. CLI and MCP invoke
the same Hall operation modules; neither carries authorization or host policy.
ADR 0019's UDS path/ownership, per-runtime identity/cgroup, peer-evidence,
gateway-generation, accepted-connection revocation, durable operation-ID,
authorization linearization, and canonical operation-registry rules are
normative for this cutover; no inherited-FD/stdio authority alternative exists
in v1.

Agent CLI mode has no Hall token, endpoint override, raw HTTP, SSH, or arbitrary
argv operation. This avoids bearer-token refresh and leakage through process
arguments, logs, artifacts, or model output. Archive/revoke closes the local
gateway and fences in-flight effects according to each provider's policy. A
future direct HTTP transport may use audience-bound proof-of-possession
credentials, but it is not the cutover path. ADR 0019 defines command grammar,
schema-aware workflow help, piping, output, man pages, and exit semantics.

Minimum capability vocabulary:

```text
job.run:<node-or-pool>
job.read:<job-id-or-session-scope>
job.cancel:<job-id-or-session-scope>
workflow.run:<workflow-id-or-scope>
workflow.read:<run-id-or-session-scope>
workflow.control:<run-id-or-session-scope>
deployment.plan:<environment>
deployment.apply:<environment>
deployment.rollback:<environment>
app.install:<node-or-pool>
app.control:<app-id>
```

A child session only receives the intersection of requested and parent
capabilities. Agent tools require an explicit envelope—missing never means full
authority. Capability resources are parsed typed values with exact IDs or
explicit segment-aware wildcards; raw string-prefix matching is forbidden.
The authenticated principal, organization, session, runtime attempt, audience,
issuer/key version, and revocation state are checked at the seam. Human REST
Human REST routes, MCP tools, and CLI commands call the same typed Hall
operations; neither agent adapter wraps operator-only HTTP with a shared
installation token.

### 4. Session transport durability and runtime control precede agent operations

Before Olympus is used as the primary session surface:

- Persist each remote turn frame (or a content-addressed durable reference) and
  its transport watermark in one SQLite transaction before ACK.
- Build assistant/tool/reasoning projections from durable ingress, not an
  in-memory broadcast accumulator.
- Report actual `RuntimeTable` inventory in hello and updates: runtime attempt,
  harness provenance, child identity, state, resumability, in-flight/pending
  permission state, and last sequence.
- Durably project runtime attempt/location in Hall and reconcile explicit
  `attached`, `detached`, `orphaned`, and `recoverable` outcomes.
- Use one Hall runtime-control registry/service for prompt, cancel, steer,
  permission, stop, drain, and recovery. It reconstructs remote controls from
  Envoy inventory; no task-local runtime object is authority.
- Bind logical node ID to its enrolled iroh public key and reject duplicate or
  takeover hellos.

Transport semantics are precise: payload+watermark ingestion is exactly-once by
sequence; host/runtime effects are at-least-once dispatch with idempotent,
fenced attempt execution. Olympus does not claim exactly-once process effects.

### 5. JOBS-2 makes job truth durable before CLI/MCP exposure

Before `jobs.run` is agent-callable:

- Replace the global jobs map with event-backed job records/projections.
- Persist dispatch intent before sending `DispatchJob`.
- Persist each output chunk or a durable chunk/artifact reference before ACK.
- Make dispatch idempotent by a Hall-issued job ID plus attempt number.
- Record owner organization, initiating session/principal, node, package/activity,
  argv, cwd binding, resource policy, timestamps, status, and terminal reason.
- Reconcile running jobs after Hall or Envoy restart. Unknown terminal state is
  represented honestly; it is not silently changed to success or failure.
- Execute in a process group/cgroup; cancellation and timeout terminate the
  complete tree.
- Run under bubblewrap with only declared workspace/repository/artifact mounts.
  Environment values come from named policy bindings; clients never submit
  secret values.

A job may use argv internally, but the agent-facing interface selects a
registered activity/provider. An unrestricted `argv` tool would be SSH under a
different name.

Envoy retains attempt state and reports it in hello before Hall implements
reconciliation. `(job_id, attempt_epoch)` is the wire identity. Sequence
allocation and spool append are one durable operation. stdout/stderr drains join
before the terminal result, making the result the final sequence. Spool cap,
ENOSPC, fsync, or rewrite failure backpressures/stops the producer and records a
terminal loss fact without advancing across a gap.

### 6. Deployments are durable, fenced activities—not ad-hoc scripts

The first deployment provider is `olympus.release.deploy`. Its input is a signed
or locally trusted release manifest containing artifact hashes, protocol version,
target environment, migrations, health probes, and rollback metadata.

Apply lifecycle:

```text
planned -> staging -> preflight -> activating -> verifying -> healthy
                                      |              |
                                      +-> failed <----+
                                             |
                                          rollback
```

Rules:

1. Build outputs are immutable, content-addressed release bundles.
2. Deployment acquires one environment lease; duplicate requests attach to the
   same attempt rather than run concurrently.
3. Database-bearing deployments make an application-consistent backup and run
   migrations against a copy before activation.
4. Activation is a symlink/unit flip, not an in-place overwrite.
5. Health verifies process, API, protocol identity, Envoy reconnect, and required
   edge route.
6. Every Envoy effect carries the deployment attempt epoch; stale effects are
   rejected.
7. Hall's event-backed attempt is authoritative. Envoy's fsynced effect journal
   is a subordinate replay ledger keyed by attempt+epoch, reconciled before new
   effects after reconnect.
8. Migrations declare `backward-compatible`, `forward-only`, or
   `restore-required`. Automatic binary rollback is forbidden when the active
   schema is not backward-compatible. Writes are quiesced/fenced where required,
   with explicit RPO/RTO and operator restore state.
9. Bundle bytes, symlink, parent directory, unit state, edge route, and journal
   transitions have specified fsync/crash points.
10. The deployment journal and restore-required status are visible to the
    initiating session.

Initial sandbox bootstrap may be implemented as an operator-run SSH script. That
script ends by enrolling Envoy; it is not reused as the normal deployment path.

### 7. Managed apps remain separate from deployment environments

APP-1 remains the service lifecycle primitive described by ADR 0015:
`ServiceTable`, health, restart, drain, state directory, and edge registration.
A candidate Olympus deployment may be exposed through the same Caddy edge, but
it is an environment composed of services, not reclassified as an ordinary
single-process app. This avoids forcing multi-service control-plane upgrades into
an app manifest designed for arm's-length product services.

Stable Hall is the only writer to the stable Caddy route subtree. Desired routes
are durable Hall truth and reconcile on boot. Candidate Hall cannot administer
stable Caddy; candidate services request routes through stable Hall. APP-1
service principals use explicit least-privilege grants and Envoy-mediated,
short-lived credentials/channels; they do not inherit installer authority or
receive a long-lived environment bearer.

### 8. Session placement becomes an explicit user choice

New-session UX selects:

1. agent/harness,
2. node or eligible pool,
3. model,
4. capability preset/project context.

Hall validates readiness and role support before accepting the first prompt.
Placement is sticky for the runtime attempt. Node failure does not imply live
migration: Hall preserves the trace and offers recovery as a new attempt on a
healthy node, consistent with ADR 0002 §20.

### 9. “Move to Olympus” is an acceptance gate, not a date

Olympus becomes the primary environment only when all gates below pass against
the sandbox and survive a soak period.

## Cutover gates

### Session gate

- Create a managed session on the sandbox from the UI.
- Prompt, stream text/tool/reasoning, steer, cancel, answer permission, switch
  model, archive, reopen, and fork/subsession where supported.
- Refresh browser, restart Hall, restart Envoy, and interrupt the network without
  losing or duplicating output. A producer-side manifest records every frame
  sequence plus text/tool/reasoning byte hash; the restored transcript must match
  it exactly at deterministic crash points around durable ingress and ACK.
- Node loss produces an explicit orphan/recover flow, never a falsely running
  session.
- Restart Hall with an idle empty-spool runtime, an in-flight turn, and a pending
  permission. It must reattach or report a precise recoverable state without
  spawning a second harness.

### Agent-operation gate

- A managed session receives the runtime-bound Olympus operation gateway
  without installation credentials. Both MCP and `olympus session info` work.
- Its `jobs.run` builds and tests a checked-out Olympus revision on the sandbox.
- Output is live, bounded, durable across Hall restart, attributable to the
  session, and downloadable as artifacts.
- A child session cannot expand the parent’s node, path, app, or deployment
  authority.
- No-envelope, wrong-org, prefix-collision, archived, revoked, rotated-key,
  cross-session, and concurrent-revocation calls fail closed.
- A real sandbox harness calls the Envoy-mediated operation gateway through CLI
  and MCP, survives Hall restart, and loses access immediately on
  archive/revoke.
- CLI and MCP calls for the same operation produce equivalent durable events,
  capability decisions, and typed outcomes. Schema-invalid workflow flags create
  no run; a real `workflow run --output result-json | workflow run
  --input-json -` pipeline survives client detach/reconnect.

### Deployment gate

- The agent plans a candidate deployment; a capability-approved session applies
  it to `sandbox-dev`.
- Binary-only failure triggers automatic rollback only when migration
  compatibility permits it. Forward-only/restore-required failures enter an
  explicit operator state rather than attempting an unsafe binary rollback.
- A valid candidate remains reachable after stable Hall and sandbox Envoy
  restarts.
- Stable Hall remains available throughout a failed candidate deployment.
- Crash/fault tests cover stale attempt, Hall loss after activation, Envoy loss
  during flip, Caddy loss after route change, concurrent writes/backup fencing,
  ENOSPC, corrupted rollback target, and failed restore/rollback health.

### App/edge gate

- APP-1 installs and supervises a reference app on the sandbox.
- Caddy exposes `/app/<slug>/`, Hall forward-auth works, WebSocket/streaming works,
  and unhealthy/stopped services fail closed.
- Route churn and restart recovery are integration-tested with real Caddy.
- Real Caddy coverage is non-skippable in the cutover profile. Stable Hall is the
  sole writer; durable desired routes converge before exposure after restart.

### Operational gate

- Full restore is rehearsed into an isolated home, including WAL-consistent DB,
  keys/identity policy, artifacts, routes, Envoy/MCP reconnect, measured RPO/RTO,
  and protocol compatibility.
- Version and protocol skew fail closed with actionable status.
- Disk quota, job concurrency, output quota, restart storm, and credential
  rotation are tested.
- A hostile candidate attempts to read stable keys/DB/spool, call Caddy admin or
  control sockets, inspect `/proc`, signal stable processes, alter units/routes,
  and exhaust CPU/RAM/PIDs/disk/ports. Every control attempt fails and stable
  service SLOs remain inside the gate.
- Seven consecutive days of primary-session dogfooding complete without
  unrecovered message loss, privilege bypass, or manual database repair. The
  soak runs a scripted daily fault schedule, minimum workload counts, exact
  sequence/hash reconciliation, disk-growth limits, and explicit clock-reset
  criteria after any blocker.

## Rejected alternatives

### Give agents SSH

Rejected. It bypasses session capabilities, node/path scoping, audit semantics,
resource limits, idempotency, and deployment rollback. Operator SSH remains a
recovery mechanism.

### Make the candidate Hall supervise its own deployment

Rejected. A failed candidate would own the mechanism needed to observe and roll
it back. Stable Hall and its enrolled Envoy own candidate lifecycle during the
migration period.

### Expose the existing jobs REST endpoint directly through MCP

Rejected. It is operator-only, volatile, argv-shaped, and acknowledges output
without durable job truth. MCP exposure follows JOBS-2, not precedes it.

### Treat the entire candidate Olympus environment as one APP-1 app

Rejected. APP-1 is a long-lived arm's-length service primitive. A control-plane
environment has coordinated Hall/Envoy/edge/state migration semantics and needs a
deployment record above ServiceTable.

## Sandbox prerequisites supplied by the operator

Before remote bootstrap, record:

- hostname/IP and operator SSH user,
- OS/version and whether the LXC is privileged or unprivileged,
- CPU/RAM/disk limits and filesystem type,
- outbound access to GitHub/crates/npm and inbound reachability policy,
- systemd user-session availability and lingering policy,
- whether rootless Podman is allowed,
- DNS name intended for the candidate edge route,
- snapshot/backup mechanism exposed by fxcluster.

No application design depends on a fixed private IP; enrollment identity is the
iroh public key and the logical node ID.
