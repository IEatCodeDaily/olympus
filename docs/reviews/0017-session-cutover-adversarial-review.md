# ADR 0017 session cutover — adversarial source review

**Review date:** 2026-07-13  
**Reviewed worktree HEAD:** `f784b04aa6d04e6759eef999012f48ac3f0f1622` plus the uncommitted worktree containing ADR 0017 and its plan  
**Scope:** `docs/adrs/0017-session-cutover-and-remote-development-plane.md`, `docs/plans/2026-07-13-session-cutover-remote-development.md`, source under `crates/control-plane`, `crates/envoy`, `crates/proto`, the session UI, ADRs 0002/0008/0011/0012/0014/0015, and the PKG-1/APP-1 cards.

## Verdict

**NO-GO as written.** The doctrine is sound, and the proposal correctly recognizes that the current jobs API is volatile and unsafe to expose to agents. The implementation plan does not yet establish a safe migration path, however. Most importantly, the same ACK-before-durable-content defect identified for jobs already exists in the remote **session** stream; Hall restart can permanently lose assistant output while Envoy has legitimately truncated it. Runtime recovery is also substantially less implemented than ADR 0008 and ADR 0017 imply, remote cancel/steer/permission do not route to remote runtimes, the current capability seam is fail-open for sessions without envelopes, and “separate homes” under one Unix user is not a stable/candidate failure boundary.

Do not start the seven-day cutover soak until the P0 items below are fixed and proven with crash-point tests. Do not expose Hall MCP until a fail-closed session principal, exact resource authorization, credential refresh/revocation, and a deliberately permitted network path exist.

## What the proposal gets right

The following load-bearing current-state statements are supported by source:

- Jobs are held in a process-global `OnceLock<RwLock<HashMap<...>>>` and accept caller-supplied argv (`crates/control-plane/src/server/routes/jobs.rs:19-23,39-50,91-116`).
- Hall mutates only that volatile map and then ACKs job output/results (`crates/control-plane/src/node.rs:798-833`).
- Envoy spools output before transport (`crates/envoy/src/main.rs:539-558`; `crates/envoy/src/spool.rs:65-92`).
- `NodeRole` currently contains only `AgentRuntime` and `JobRunner`; no `AppHost` exists (`crates/proto/src/frames.rs:28-33`). No `ServiceTable` or package/app model exists in merged source.
- Session creation accepts a node (`crates/control-plane/src/server/routes/sessions.rs:79-90,428-464`), while the UI only asks for an agent and calls `createSession({ agent })` (`ui/src/views/sessions/components/AgentPicker.tsx:17-24`; `SessionSidebar.tsx:63-86`).
- Remote fork and resumability currently fail closed (`crates/control-plane/src/server/envoy_conn.rs:518-522,606-610`).
- Hall health reports edge readiness from the driver and can report `missing` (`crates/control-plane/src/server/mod.rs:294-302`).

Those facts justify ADR 0017. They do not make the proposed dependency order or gates sufficient.

---

## P0 — migration blockers

### P0.1 Session output is ACKed before the assistant message is durable

**Contradicted claim:** ADR 0008 says Hall derives its applied watermark from the event log and remote replay is exactly-once (`docs/adrs/0008-hall-envoy-split-rolling-deploy.md:73-86`). ADR 0017’s session gate expects Hall restart without lost or duplicated output (`docs/adrs/0017-session-cutover-and-remote-development-plane.md:204-212`). The plan repairs ACK durability only for jobs (`docs/plans/2026-07-13-session-cutover-remote-development.md:124-136`).

**Actual source:** `EnvoyConnection::apply_event` commits only the transport sequence watermark, then forwards the event through an in-memory broadcast channel, then ACKs it (`crates/control-plane/src/server/envoy_conn.rs:157-173`). `Log::accept_envoy_seq` writes only `envoy_watermarks` (`crates/control-plane/src/log.rs:129-157`). The assistant text and tool calls are accumulated in a background task and persisted only when `Done` arrives (`crates/control-plane/src/server/routes/sessions.rs:1302-1307,1349-1371,1487-1539`).

**Failure:** crash Hall after one or more text/tool frames were ACKed but before `Done` and `append_assistant_message`. Envoy may truncate those frames. On restart the watermark says they were consumed, but no message contains their bytes. The transcript has permanent, silent loss. A crash after ACKing `Done` but before the background consumer commits is the worst form: all turn frames may be gone.

**Required correction:** move session frame application into a durable per-turn ingress record or durable chunk table and advance the transport watermark in the **same SQLite transaction**. ACK only that committed watermark. Final message projection can be derived transactionally or rebuilt from durable turn frames. Do not use “broadcast delivery happened” as durability.

**Required gate:** deterministic crash injection at every boundary: frame receive → durable ingress commit → ACK write → assistant projection. After restart/replay, compare the exact event sequence and transcript bytes/hash to a producer-side oracle. “No visible duplicates” is insufficient.

### P0.2 Runtime restart/recovery claims are not implemented, and recovery is scheduled too late

**Actual source:** Envoy hello builds `runtimes` from **spool filenames**, not `RuntimeTable` (`crates/envoy/src/main.rs:292-303`). Fully ACKed live sessions have no spool file and disappear from hello. Reported entries have `hermes_id: None`, `state: "spooled"`, and `resumable: false`, even if a live runtime exists. Hall ignores subsequent `Runtimes` frames (`crates/control-plane/src/node.rs:795-797`). `RuntimeTable` has no status snapshot method and is in-memory only (`crates/envoy/src/runtime_table.rs:45-60`). The Envoy read-loop disconnect aborts its reaper/heartbeat/replay tasks but leaves children in `RuntimeTable`, while Hall has no reliable inventory to reattach to them (`crates/envoy/src/main.rs:317-373,422-428`).

ADR 0008’s promised “Hall relearns who holds what from the envoys” is therefore false for the current tree. The plan puts this repair in Phase 4, after durable jobs, MCP exposure, and activity providers (`docs/plans/...:237-267`). That order allows agent operations to ship atop sessions whose authority/credential/runtime ownership cannot be reconstructed.

**Required correction/order:** make authoritative runtime inventory and Hall reconciliation Phase 1, before MCP and before calling any session primary. Hello and updates must report actual live runtime identity, harness provenance, state, resumability, turn/in-flight state, and last emitted sequence. Hall must durably project runtime attempt/location and produce explicit `attached`, `detached`, `orphaned`, and `recoverable` outcomes.

**Required gate:** restart Hall with (a) an idle fully-ACKed live runtime, (b) an in-flight turn, (c) a pending permission, and (d) an Envoy whose spool is empty. Verify exact reattachment/recovery, single-prompter ownership, and no second harness process.

### P0.3 Remote sessions cannot reliably cancel, steer, or answer permissions

The prompt path creates a `RemoteRuntime` as a task-local value (`crates/control-plane/src/server/routes/sessions.rs:1167-1185`). It is not registered in Hall’s `BridgeManager` runtime table. The REST control handlers consult only that local/in-process bridge table:

- cancel: `routes/sessions.rs:1728-1747`
- steer: `routes/sessions.rs:1750-1784`
- permission response: `routes/sessions.rs:1852-1877`

Consequently, an Envoy-hosted session may stream a permission request, but the response path cannot find that remote runtime. Cancel may return success after doing nothing. This directly invalidates the session acceptance journey’s steer/cancel/permission claims (`docs/plans/...:268-279`) and ADR 0017’s session gate (`:206-210`).

**Required correction:** introduce one Hall-side runtime-control registry/service keyed by durable runtime attempt and node, used by prompt, cancel, steer, permission, stop, recovery, and drain. It must reconstruct remote handles from Envoy reconciliation rather than rely on task-local trait objects.

**Required gate:** for each remote adapter, block on a real permission request, respond through REST/UI after a browser refresh, then steer and cancel. Assert the Envoy receives each typed frame and the child/turn changes state. Restart Hall while permission is pending and prove an honest recoverable outcome.

### P0.4 MCP would inherit a fail-open and ambiguous capability model

ADR 0017 says every MCP call evaluates a session’s signed envelope (`docs/adrs/0017...:95-117`). Current source does not enforce that invariant:

1. A missing capability record is explicitly treated as legacy full authority (`crates/control-plane/src/server/capability.rs:285-314`). The current UI creates sessions without capabilities (`SessionSidebar.tsx:70-75`). Thus “session-bound token” alone can become broad authority unless MCP adds a separate mandatory-envelope rule.
2. `CapabilityAuthorizer` ignores the supplied principal (`capability.rs:290-295`). Session ownership and principal/session binding are not checked at this seam.
3. Resource matching uses raw string prefix matching (`capability.rs:177-187`). A grant such as `job.run:node-1` also authorizes `job.run:node-10`; path/app/environment IDs have analogous prefix-collision risks.
4. `CapabilitySet` has no subject/session, organization, audience, issuer, issued-at, expiry, or key ID (`capability.rs:45-66`). It is a signed permission set, not a self-contained session credential.
5. Session creation signs a caller-supplied set while hard-coding `Principal::Operator` rather than extracting the authenticated principal (`routes/sessions.rs:470-477`). Subsession paths repeat the same pattern (`:2055-2062`).

**Required correction:** MCP authentication must fail closed unless the target session has an explicit valid envelope. Bind the MCP credential to subject/session/org/audience/expiry/key version and compare it to the resource owner on every call. Replace prefix matching with parsed resource types and exact IDs (or explicit segment-aware wildcards). Use the actual authenticated principal at mint/assignment. Migrate legacy sessions deliberately; do not let “missing” mean full authority on an agent-facing surface.

**Required gate:** include no-envelope sessions, prefix-collision IDs, wrong org, archived session, parent revoked during child call, key rotation, stolen token from session A, and concurrent revocation/in-flight calls. The plan’s token-A-cannot-read-B test (`docs/plans/...:154-181`) would not catch fail-open session A or prefix expansion.

### P0.5 “Separate homes” are not separate failure or trust domains

ADR 0017 calls stable Hall and candidate Olympus separate failure domains because they use different homes, ports, databases, service names, and slugs (`docs/adrs/0017...:53-74`). That is namespace separation, not a security/failure boundary.

The current Hall and Envoy units run as the same user and have essentially no filesystem/process hardening (`systemd/olympus-hall.service:5-19`; `systemd/olympus-envoy@.service:7-19`). The proposed APP-1 binary runtime uses a user unit and gives the app a bearer credential in its environment (`docs/cards/arch/app-1-servicetable.md:30-44`). Caddy runs as that same user and grants write access to all `%h/.olympus` (`deploy/systemd/olympus-caddy.service:13-17`). Its unauthenticated localhost admin API can replace the entire Olympus route subtree (`crates/control-plane/src/edge/caddy.rs:12-18,103-131`).

A candidate Hall, candidate app, build artifact, or managed binary under the same uid can read stable keys/spools/state where permissions permit, signal sibling user processes, or rewrite Caddy configuration. A malformed candidate is not the only threat: Olympus is explicitly executing agent-built code.

**Required correction:** stable Envoy/Hall/Caddy and candidate services need distinct OS identities or containers/VM boundaries, explicit filesystem allowlists, `ProtectHome`, `ReadWritePaths`, process/cgroup restrictions, and no access from candidate workloads to stable Envoy state, Hall DB/keys, Caddy admin, systemd user manager, or host control sockets. If the entire candidate VM is the isolation boundary, the stable Envoy supervising it cannot share the candidate uid/filesystem authority. Define which component is allowed to flip candidate units without granting candidate code that authority.

**Required gate:** run a hostile candidate fixture that attempts to read stable Hall DB/capability key/iroh key/Envoy spool, connect to Caddy admin and Hall/Envoy control endpoints, signal stable processes, inspect `/proc`, and alter stable units/routes. Every attempt must fail while deployment and health still function.

---

## P1 — architecture and sequencing defects

### P1.1 JOBS-2 reconciliation is ordered before the protocol needed to reconcile

Task 1.2 requires startup/reconnect reconciliation (`docs/plans/...:103-122`), but the current hello has no jobs inventory (`crates/proto/src/frames.rs:219-242`) and `JobTable` deletes entries at terminal state (`crates/envoy/src/job_table.rs:106-132`). “Orphan reconciliation in Envoy hello” is deferred to Task 1.4 (`docs/plans/...:138-150`). Task 1.2 cannot meet its own requirement before Task 1.4 changes the protocol/state model.

There is also an ambiguous-dispatch window: Hall can durably plan and send `DispatchJob`, Envoy can spawn it, and the connection can fail before Hall receives `Resp`. Retrying against the current Envoy is not idempotent: duplicate active IDs return an error, while completed IDs have been forgotten and rerun (`job_table.rs:53-55,124-125`).

**Correct order:** define `(job_id, attempt)` wire identity, durable/retained Envoy attempt state, and hello/reconnect inventory first; then implement Hall `JobService` reconciliation and dispatch retry; then ACK-correct output; only then MCP. Specify the effect guarantee honestly: dispatch is at-least-once transport with idempotent attempt execution, not magical exactly-once process effects.

### P1.2 Job result ordering and spool-overflow behavior can create false terminal state or permanent gaps

stdout/stderr readers are detached tasks. The waiter sends `JobResult` immediately after child exit without joining the readers (`crates/envoy/src/job_table.rs:85-133,165-196`). Therefore result can be sequenced before final output. A projection that treats terminal as final can miss late chunks, and the plan’s “unknown/out-of-order terminal events fail closed” gate does not define whether late output is legal.

The job forwarding task allocates a sequence, silently drops a frame if spool append fails, and continues (`crates/envoy/src/main.rs:548-558`). Sequence allocation is persisted separately from frame append (`crates/envoy/src/spool.rs:43-63,65-92`). The next successful frame then creates an unrecoverable gap at Hall. Contrary to ADR 0008, overflow currently returns an error; it does not emit a durable `SPOOL_OVERFLOW` marker (`spool.rs:69-79`).

**Required correction:** join both output drains before emitting result; define terminal as the last sequence. Make sequence reservation + frame append one durable operation. On cap/disk/fsync failure, stop or backpressure the producer, persist one terminal loss/overflow fact through a reserved channel, and never advance past a missing sequence. Test ENOSPC, read-only spool, cap exhaustion, corrupted tail, ACK rewrite failure, and restart.

### P1.3 The MCP transport and credential lifecycle are underspecified

The plan says “streamable HTTP,” short TTL, and static setup injection, but does not define:

- how a long-lived session refreshes an expired credential without restarting the harness;
- how a restarted Hall retains/revokes credentials and signing-key versions;
- the exact audience/URL/TLS identity exposed to a remote sandbox;
- whether credentials are bearer tokens or proof-of-possession;
- how tokens are kept out of adapter logs, process listings, artifacts, crash reports, and model/tool output;
- how archived/revoked sessions terminate existing streamable-HTTP connections, not just reject new requests;
- how rate limits, request body limits, connection limits, and replay/idempotency keys work.

There is also a doctrine conflict: ADR 0002’s worker network policy denies control-plane access (`docs/adrs/0002-olympus-fleet-control-plane.md:1527-1542`), while Hall MCP requires exactly that access. “Undeclared network fails closed” in Task 3.2 (`docs/plans/...:211-224`) does not resolve the contradiction.

**Required correction:** define a narrow Hall-MCP network destination and TLS identity as a declared capability, plus refresh/revocation protocol and connection teardown. Do not broadly enable Hall REST. Prefer an Envoy-mediated channel or narrowly routed MCP endpoint if direct worker-to-Hall HTTP would widen the control-plane surface.

### P1.4 Deployment rollback is not a complete state machine

The ADR’s symlink flip and health rollback protect executable bytes, not state (`docs/adrs/0017...:141-169`). “Backup and run migrations against a copy” does not explain how the activated candidate reaches the migrated state, how writes during backup are fenced, what happens after the candidate writes new-format data, or who restores state after a failed irreversible migration. “State rollback is never assumed safe” is correct but leaves the automatic rollback promise conditional and therefore misleading.

The plan also creates `deploy-journal.jsonl` beside Hall’s event-backed deployment truth without specifying authority, replication, sequence, ACK, or reconciliation (`ADR 0017:61-71`; plan Task 6). Hall restart during `activating` can race an Envoy still flipping units unless attempt/lease fencing is carried on every host effect. A symlink flip without fsync of bundle, link, and parent directories is not a crash-safe activation contract. Caddy route flip and service flip also need a defined order and rollback point.

**Required correction:** define one authoritative deployment attempt in Hall and an Envoy-side effect journal as a replayable subordinate ledger keyed by fenced attempt epoch. Specify crash points for stage, backup, migration, unit/link flip, edge route, health, rollback, and journal ACK. Classify migrations as backward-compatible, forward-only, or restore-required; prohibit automatic binary rollback when schema compatibility is false. Define write quiescence and RPO/RTO.

**Required gates:** destructive/non-backward-compatible migration fixture, writes concurrent with backup, Hall loss after activation, Envoy loss during flip, Caddy loss after route change, disk full during staging/journal, stale-attempt replay, rollback health failure, and restore-required operator flow. “Broken health check rolls back” proves only the easiest binary case.

### P1.5 Edge desired state is volatile, so APP/deployment restart gates cannot pass honestly

`EdgeManager.desired` is an in-memory vector (`crates/control-plane/src/edge/mod.rs:10-21`). `upsert/remove` apply Caddy then mutate that vector (`:44-70`); there is no event-backed edge projection. Hall restart starts with an empty desired set, and `converge()` can only reapply that empty in-memory set (`:73-75`). This contradicts ADR 0014’s restart convergence requirement and the plan’s APP/edge restart gates.

Further, `CaddyDriver` serializes only writers inside one process (`crates/control-plane/src/edge/caddy.rs:15-27,122-131`). Stable Hall and candidate Hall are separate processes, so both can overwrite the same Caddy route subtree. Candidate failure isolation requires stable Hall to be the **only** Caddy writer; candidate edge management must be disabled or target a separate Caddy admin instance.

**Required correction:** make desired routes durable Hall truth, reconcile them on boot, and enforce one external writer (or separate Caddy instances/subtrees with non-overlapping ownership and server-side CAS). Candidate services request routes from stable Hall; candidate Hall must not administer stable Caddy.

### P1.6 APP-1/PKG-1 dependencies are incomplete, while DEPLOY-1 is over-coupled to APP-1

The dependency graph shows `PKG-1 green -> APP-1 -> EDGE-LIVE` and then APP-1 feeding DEPLOY-1 (`docs/plans/...:27-42`). The actual APP-1 card requires **JOBS-1 + PKG-1 + EDGE-1** merged and also depends on CAPS-1 for service principals (`docs/cards/arch/app-1-servicetable.md:15-24`). The graph omits CAPS-1 and treats live edge as downstream of APP-1 even though APP-1’s healthy-route behavior depends on a durable, working edge.

PKG-1’s card defines the ten ADR 0012 plugin extension classes but does not include ADR 0015’s later `contributions.apps` managed-app contribution (`docs/cards/arch/pkg-1-manifest-registry-v2.md:27-33`). The plan notices this with a weak “supports extension” gate (`docs/plans/...:68-80`) but does not require the PKG-1 card/schema/migration/activation rules to be amended and replay-tested before APP-1.

Conversely, ADR 0017 explicitly says a candidate Olympus environment is **not** an APP-1 app (`docs/adrs/0017...:174-181,267-271`). DEPLOY-1 therefore should not be blocked on ServiceTable. It needs durable jobs/artifacts, deployment contracts, fenced Envoy effects, and a durable edge API. Making APP-1 a prerequisite delays the core self-deployment proof and conflates two lifecycle models.

**Correct graph (minimum):**

```text
CAPS hardening + session ACK/recovery foundation
  ├─> JOB protocol identity/inventory -> durable JobService/ACK -> activity sandbox/artifacts -> MCP jobs
  ├─> PKG-1 amended with managed-app contribution (parse/store/activate contract)
  └─> durable EDGE-1 + real Caddy

JOBS plumbing + CAPS + amended PKG-1 + durable EDGE-1 -> APP-1
JOBS activity/artifacts + deployment fencing/journal + durable EDGE-1 -> DEPLOY-1
APP-1 and DEPLOY-1 converge only in the final sandbox product journey, not as an implementation dependency.
```

### P1.7 APP service-principal design grants too much and has no credential lifecycle

APP-1 mints a principal at install as `required_capabilities ∩ granter authority` and passes a bearer through environment (`docs/cards/arch/app-1-servicetable.md:42-44`). Install-time granter authority can be much broader than runtime necessity, and a long-lived environment bearer has no specified audience, TTL, refresh, rotation, revocation, or crash/log hygiene. Restarting the app may resurrect stale authority; removing a package may leave a valid credential.

**Required correction:** service principal identity must be durable, but credentials must be short-lived, audience-bound, and fetched/rotated through an Envoy mediation path. Grants should be explicit resource grants, not an implicit copy of whatever the installer happened to possess. Removal/quarantine must revoke credentials before or atomically with process/route teardown.

---

## Acceptance gates that would not prove readiness

| Proposed gate | Why it is insufficient | Replacement evidence |
|---|---|---|
| “Crash Hall between receiving output and committing it; replay exactly once” (job Task 1.3) | Does not cover session stream; “exactly once” can mean watermark only, as current session code demonstrates. | Crash-point matrix proving payload/reference and watermark commit atomically for both session and job streams; producer/consumer sequence + byte hashes. |
| “No `OnceLock<HashMap>` remains” (Task 1.2) | A structural grep proves neither durability nor correct reconciliation. | SQLite transaction/replay tests, ambiguous-dispatch test, Envoy restart inventory, stale-attempt fencing. |
| Token A cannot inspect/cancel B (Task 2.1) | Can pass while no-envelope A has full authority, resources prefix-match, or A can operate arbitrary node/environment targets. | Default-deny/no-envelope, exact-resource, cross-org, prefix-collision, revocation, archive, expiry, refresh, and connection-teardown tests. |
| Adapter fixtures receive the same MCP set (Task 2.3) | Fixture serialization does not prove remote reachability, TLS identity, token secrecy/refresh, or tool invocation. | Real harness on sandbox calls a harmless MCP tool, rotates token mid-session, survives Hall restart, and loses access on archive/revoke. |
| Forking child leaves no process (Task 1.4) | Covers only one process-tree shape and not daemonization, double-fork, cgroup escape, PID reuse, or Envoy crash. | cgroup populated check after cancel/timeout/Envoy restart; daemon/double-fork fixture; resource counters reconcile. |
| “Escape, symlink, `/proc`, undeclared network…” fail (Task 3.2) | Does not define expected Hall MCP exception or same-uid stable-control access. | Explicit mount/network policy fixture plus hostile candidate/stable-boundary test from P0.5. |
| Screenshots/video + zero duplicate committed messages (Task 4.3) | Visual evidence has no loss oracle and cannot detect ACKed-but-uncommitted bytes. | Deterministic transcript manifest with frame seqs, message IDs, content hashes, tool/reasoning hashes, and restart checkpoints. |
| APP integration may “skip cleanly” if Caddy absent (APP-1 card) | An optional edge test cannot certify the mandatory edge or cutover. | Non-skippable real Caddy test in release/cutover profile; fail build if required binary/version/origin config absent. |
| Broken health triggers rollback (deployment gate) | Proves only binary/link rollback, not data, stale lease, edge, or interrupted activation. | Full deployment crash/migration/fencing matrix in P1.4, including restore-required no-auto-rollback outcome. |
| Candidate failure does not affect stable `/`, Fleet, MCP, or Envoy connectivity | A polite process crash does not test trust/failure isolation. | Hostile candidate resource-exhaustion and control-socket/admin/key-access tests; independent cgroups/UIDs; stable latency/error SLO during failure. |
| Backup/restore from copied DB | Does not prove quiescence, WAL handling, credentials/keys, artifacts, Caddy routes, RPO/RTO, or restored protocol compatibility. | Full restore into isolated home from production-shaped backup, integrity checks, key/identity continuity or explicit rotation, measured RPO/RTO, post-restore Envoy/MCP/edge reconnect. |
| Seven days without “unrecovered message loss” | Without an oracle, silent loss is classified as success; low activity may avoid failure modes. | Scripted daily fault schedule and sequence/hash reconciliation, minimum workload counts, disk-growth limits, and explicit reset criteria. |

## Missing failure modes to add explicitly

1. Hall crashes after durable intent but before dispatch write; after dispatch write but before Envoy receipt; after Envoy spawn but before response.
2. Envoy restarts with live child processes orphaned, or systemd kills children via `KillMode=mixed`; define adopt-versus-kill behavior.
3. Hall ACK write succeeds but Envoy crashes during spool rewrite; ACK rewrite fails with ENOSPC/read-only filesystem.
4. stdout/stderr final bytes race `JobResult`; UTF-8 splits and binary output; output artifact write succeeds but metadata commit fails, and vice versa.
5. Two Hall connections claim the same logical node ID; current iroh allowlist authenticates peer key but hello’s logical `node_id` is caller-supplied (`crates/control-plane/src/node.rs:638-698`). Bind logical node ID to enrolled peer identity and reject duplicates/takeover.
6. Capability signing key loss/rotation and restoration from backup; old MCP/service credentials after restore.
7. Clock skew for TTL/leases, clock rollback, and monotonic-versus-wall-clock semantics.
8. Disk full in Hall DB, Envoy spool, artifact store, release extraction, deployment journal, and app state.
9. Stable Hall unavailable during candidate health verification; Caddy unavailable after activation; health endpoint lies while protocol identity is wrong.
10. Candidate consumes CPU/RAM/PIDs/disk/ports and starves stable Envoy or Caddy despite separate directories.
11. App restart storm survives Hall/Envoy restart; quarantine and route removal reconcile before exposure.
12. Rollback target or artifact is corrupted/missing; last healthy symlink exists but binary/state/protocol are incompatible.
13. MCP request is accepted before revocation and completes after revocation; policy must state cancel, fence, or allow-to-complete.
14. Network partition creates stale Envoy effects while Hall issues a new attempt elsewhere; every mutating effect needs attempt/lease fencing.

## Required plan amendments before approval

1. Add a **Phase 0 migration-safety foundation**: session event atomic ingress+ACK, real runtime inventory/reconciliation, remote control registry, logical-node/peer binding, fail-closed capability semantics.
2. Reorder JOBS-2 so attempt identity, Envoy retained state, and hello inventory precede `JobService` reconciliation.
3. Specify MCP transport, network exception, TLS identity, credential schema, refresh, revocation, and active-connection teardown before implementing tools.
4. Define stable/candidate OS isolation and a single Caddy writer. Separate directories alone must not be called failure domains.
5. Make edge desired routes durable before APP-1 or deployment restart gates.
6. Amend PKG-1 to model `contributions.apps`; include CAPS-1 and durable EDGE-1 as APP-1 prerequisites; remove APP-1 as a prerequisite of DEPLOY-1.
7. Replace happy-path acceptance bullets with non-skippable crash-point, adversarial-auth, hostile-candidate, migration, restore, and sequence/hash oracle tests.
8. State effect semantics precisely: durable intent + idempotent fenced attempt + at-least-once dispatch; durable exactly-once ingestion/projection by sequence. Do not claim exactly-once host execution.

## Final assessment

The proposed north star—Hall owns truth/policy, Envoy owns effects, agents get typed capability-scoped tools instead of SSH—is the correct architecture. The current plan nevertheless builds MCP and remote deployment above an unproven session transport and a fail-open legacy capability seam, then labels same-uid directory separation a candidate failure boundary. That ordering is unsafe. Repair session durability/recovery and host trust boundaries first; after those corrections, JOBS-2, MCP, DEPLOY-1, and APP-1 can be delivered as independently fenced, evidence-backed layers.