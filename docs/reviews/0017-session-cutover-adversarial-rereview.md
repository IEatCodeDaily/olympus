# ADR 0017 session cutover — adversarial re-review

**Review date:** 2026-07-13  
**Reviewed worktree HEAD:** `f784b04aa6d04e6759eef999012f48ac3f0f1622` plus the current uncommitted worktree  
**Scope:** amended `docs/adrs/0017-session-cutover-and-remote-development-plane.md`, amended `docs/plans/2026-07-13-session-cutover-remote-development.md`, every required amendment at `docs/reviews/0017-session-cutover-adversarial-review.md:210-219`, and the current Hall/Envoy/protocol/edge source needed to validate the proposed ordering.  
**Question:** whether the amended architecture and implementation order are safe enough to approve as a proposed plan. This review does **not** require the planned implementation to exist.

## Verdict

**NO-GO pending three precise plan corrections.**

The amendment is a substantial correction of the initial draft. It now puts atomic session ingress, authoritative runtime inventory, one runtime-control service, node/capability hardening, job attempt identity, durable edge truth, single-writer Caddy, OS isolation, deployment fencing, hostile fixtures, and sequence/hash oracles into the architecture. Six of the eight required amendment classes are now adequately represented, and the remaining two are mostly represented.

Approval is still blocked because the executable order exposes agent-facing `jobs.run` before the activity-provider and host-sandbox boundary exists; the recovery text permits reusing a runtime attempt on another node despite ambiguous prior-process state; and the job durability task assigns Envoy-side ordering/spool corrections to the wrong files and test surface. These are plan defects, not complaints that implementation is absent.

Do not expose agent job tools or start the cutover soak from this ordering. After the three corrections below, the plan is safe enough to approve as a proposal, subject to the gates it already defines.

## Current-source reality remains as stated by the initial review

The amended documents correctly describe the current unsafe substrate rather than pretending it has already been repaired:

- Session application still commits only `envoy_watermarks`, forwards the payload through an in-memory channel, and then ACKs (`crates/control-plane/src/server/envoy_conn.rs:157-173`; `crates/control-plane/src/log.rs:129-156`). The turn consumer still accumulates assistant text/tool state in memory (`crates/control-plane/src/server/routes/sessions.rs:1302-1307,1349-1487`).
- Remote prompt still constructs a task-local `RemoteRuntime` (`crates/control-plane/src/server/routes/sessions.rs:1167-1185`), while cancel/steer/permission still use the local bridge registry (`:1728-1747,1750-1792,1852-1877`).
- Envoy hello still derives runtime inventory from spool files and reports `hermes_id: None`, `state: "spooled"`, and `resumable: false` (`crates/envoy/src/main.rs:292-303`); the actual runtime table remains in memory (`crates/envoy/src/runtime_table.rs:45-60`), and Hall still discards runtime updates (`crates/control-plane/src/node.rs:795-797`).
- Capabilities still omit subject/session/org/audience/expiry/key version, raw resource matching still uses `starts_with`, and missing records still authorize (`crates/control-plane/src/server/capability.rs:45-66,177-187,285-314`).
- Job attempts are still keyed only by job ID and deleted at completion; output drains are detached from the terminal result (`crates/envoy/src/job_table.rs:46-55,85-133,165-196`). Sequence allocation and spool append remain separate operations (`crates/envoy/src/spool.rs:43-63,65-92`), and the forwarding task still continues after allocation/append failure (`crates/envoy/src/main.rs:539-559`).
- Edge desired state remains an in-memory vector (`crates/control-plane/src/edge/mod.rs:10-21,44-75`).

That is acceptable for a proposed implementation plan only because the plan must order and gate each repair correctly.

## Required-amendment reconciliation

| Initial required amendment | Re-review | Evidence in amended ADR/plan | Remaining concern |
|---|---|---|---|
| 1. Phase-0 migration-safety foundation: atomic session ingress/ACK, real runtime inventory/reconciliation, unified remote controls, node/peer binding, fail-closed capabilities | **Substantively satisfied** | ADR §§3-4 (`:128-186`); plan Phase 1 Tasks 1.1-1.5 (`:92-183`) | It is named Phase 1 rather than Phase 0, which is harmless. Runtime recovery attempt semantics still need correction under blocker 2. |
| 2. Job attempt identity, retained Envoy state, and hello inventory before Hall reconciliation | **Satisfied** | ADR §5 (`:188-215`); plan Task 2.1 precedes durable projection and `JobService` (`:187-243`) | The implementation file/test ownership for terminal/spool correctness is incomplete under blocker 3. |
| 3. Specify MCP transport/network/TLS/credential/refresh/revocation/connection teardown before tools | **Satisfied by architectural substitution** | ADR §3 chooses an Envoy-mediated local stdio/UDS bridge over authenticated Hall↔Envoy transport, with no Hall network access or bearer refresh (`:128-141`), binds peer+attempt+session and checks durable authority (`:134-163`); plan Task 3.1 closes the bridge on archive/revoke and fences in-flight effects (`:280-300`) | Direct HTTP/TLS/token refresh requirements no longer apply to the cutover path. A future direct HTTP path is explicitly out of scope. |
| 4. Define stable/candidate OS isolation and one Caddy writer | **Architecture satisfied; ordering partial** | ADR §1 gives distinct identities/cgroups and socket/filesystem restrictions (`:71-107`); ADR §7 makes stable Hall sole Caddy writer (`:269-274`); plan Tasks 4.2, 6.1, and 7.4 contain enforcement/gates | Agent job tools are exposed one phase before Task 4.2 establishes the host boundary; see blocker 1. |
| 5. Make edge desired routes durable before APP-1 or deployment restart gates | **Satisfied** | Dependency graph (`plan:46-52`); Task 6.1 precedes APP-1 and deployment (`:420-457`); ADR §7 (`:269-271`) | None blocking. |
| 6. Amend PKG-1 for `contributions.apps`; CAPS and durable EDGE prerequisite to APP; APP not prerequisite to DEPLOY | **Satisfied, with one clarity edit recommended** | Graph explicitly gives APP `JOBS + CAPS + PKG-1 + durable EDGE`, while DEPLOY uses jobs/artifacts+journal+edge (`plan:48-52`); Task 6.2 requires hardened CAPS, amended/replay-tested PKG-1, jobs, and Task 6.1 (`:443-457`) | Because numbered Phase 7 follows a Phase 6 that also contains APP-1, state explicitly that Task 7.1 may begin after Task 6.1 without waiting for Task 6.2. The graph already carries the correct dependency, so this is not an independent blocker. |
| 7. Replace happy-path bullets with non-skippable crash/auth/hostile/migration/restore/hash-oracle evidence | **Mostly satisfied** | Session crash matrix (`plan:172-183`), job ambiguity and spool gates (`:187-203,245-276`), real Caddy non-skippable gate (`:422-441`), deployment crash matrix (`:481-495`), hostile candidate gate (`:508-519`), restore/soak evidence (`:555-568`); ADR cutover gates (`:295-364`) | Envoy-process crash/adopt-versus-kill and same-attempt ambiguity remain underspecified; see blocker 2. Job spool test ownership is incomplete; see blocker 3. |
| 8. State durable intent + idempotent fenced attempt + at-least-once dispatch; exactly-once ingestion/projection only by sequence | **Satisfied** | ADR §4 states exactly those semantics and rejects exactly-once process effects (`:184-186`); job and deployment sections carry attempt epochs/fencing (`:210-215,235-253`); plan Tasks 2.1 and 7.1 repeat them (`:198-203,472-479`) | Task 5.2 currently contradicts this rule by allowing a runtime attempt to be restarted on another node; see blocker 2. |

## Unresolved approval blockers

### BLOCKER 1 — Agent-facing job execution precedes the provider and sandbox boundary

**Plan conflict:** Task 3.2 exposes `nodes.list`, `jobs.run`, `jobs.get`, `jobs.logs`, and `jobs.cancel` to sessions and says the client chooses a registered activity rather than argv (`docs/plans/2026-07-13-session-cutover-remote-development.md:302-314`). The activity-provider seam does not exist until Task 4.1 (`:330-344`), and bubblewrap, distinct OS identity, mount/network policy, cgroups, and hostile-boundary tests do not land until Task 4.2 (`:346-360`).

The current Envoy source makes this ordering security-relevant: `DispatchJob` still accepts argv and inherited allowlisted environment values (`crates/envoy/src/main.rs:530-574`; `crates/envoy/src/job_table.rs:20-27,46-71`). Therefore Task 3.2 cannot honestly satisfy its own “registered activity” rule, and an agent-callable job path would exist before the host enforcement that makes capability decisions real.

**Required correction:**

1. Task 3.1 may land early as a harmless runtime-bound bridge with no effectful tools.
2. Move Task 4.1 and Task 4.2 before Task 3.2, or keep `jobs.run` compile-time/runtime disabled until both gates pass.
3. Make the dependency explicit: `job attempt/durability -> provider registry -> sandbox/OS/cgroup boundary -> agent-facing jobs.run`.
4. Require the first real MCP `jobs.run` integration test to prove the selected provider—not caller data—constructs argv/env/cwd and that the hostile mount/network/process fixtures are non-skippable.

### BLOCKER 2 — Recovery may reuse an attempt after ambiguous process state

**Plan conflict:** The ADR correctly says node failure recovers “as a new attempt” (`docs/adrs/0017-session-cutover-and-remote-development-plane.md:285-288`) and states host effects are at-least-once with idempotent fenced attempts (`:184-186`). Task 5.2 instead says: “If harness resume provenance is valid, restart the runtime attempt on an eligible node” (`docs/plans/2026-07-13-session-cutover-remote-development.md:390-404`).

That wording permits the same attempt identity to spawn on another node after a partition or Envoy crash without proof that the previous process is dead. The current `RuntimeTable` is in memory only (`crates/envoy/src/runtime_table.rs:45-60`), Envoy disconnect aborts support tasks but does not establish a durable child disposition (`crates/envoy/src/main.rs:422-428`), and the units use `KillMode=mixed` (`systemd/olympus-envoy@.service:14-19`). A Hall-visible disconnect is therefore not a proof that no old prompter/effect holder remains.

**Required correction:** define one of two fenced outcomes for Envoy restart/node loss:

- **Attach:** the same Envoy proves, from durable local attempt/cgroup/process identity, that the original child is still the unique owner; Hall reattaches to the same attempt without spawning; or
- **Recover:** Envoy/host supervision proves or forces the old cgroup empty, Hall terminalizes/orphans the old attempt, and any resumed/trace-seeded harness is a **new attempt epoch**. A new node always means a new attempt.

Add explicit crash gates for Envoy loss (idle, in-flight output, pending permission, and immediately after child spawn), partition with stale Envoy effects, daemon/double-fork fixtures, and a single-prompter/cgroup oracle. “Resume provenance” may seed a new attempt; it must not authorize ambiguous same-attempt respawn.

### BLOCKER 3 — The job ACK/terminal task omits the Envoy source that must be changed

Task 2.4 requires output-drain join, terminal-last sequencing, atomic sequence reservation+append, and fail-closed ENOSPC/cap/fsync/ACK-rewrite behavior (`docs/plans/2026-07-13-session-cutover-remote-development.md:245-262`), but its file list names only Hall files (`:249-253`). The required defects live primarily in:

- `crates/envoy/src/job_table.rs:85-133,165-196` — detached stdout/stderr readers and result emission;
- `crates/envoy/src/main.rs:539-559` — sequence allocation, append, send, and silent continuation; and
- `crates/envoy/src/spool.rs:43-92` — separate sequence reservation, append, counter persistence, and fsync boundaries.

Task 2.5 mentions `job_table.rs` and `main.rs` but still omits `spool.rs`, and its single forked-child gate does not prove the required crash/disk behavior (`plan:264-276`). As written, an executor can complete the listed Task 2.4 files without making its gate achievable.

**Required correction:** add those three Envoy files to Task 2.4 (or split a preceding Envoy spool/terminal task), specify the durable record/counter transaction or append protocol, and locate ENOSPC/cap/corrupt-tail/ACK-rewrite tests against the real Envoy spool. Include final-byte ordering and daemon/double-fork process-tree fixtures. Task 2.3 reconciliation and all MCP exposure remain gated on this Envoy-side task.

## Non-blocking precision edits

1. State beside Task 7.1 that DEPLOY-1 depends on Task 6.1 durable edge but **not** Task 6.2 APP-1; this removes any ambiguity created by sequential phase numbering.
2. In the dependency graph, replace “fenced deployment journal” as an input to `DEPLOY-1` with “JOBS activities/artifacts + durable EDGE -> DEPLOY contract/journal/provider,” because the journal is itself part of DEPLOY-1.
3. Make the Task 4.2 and runtime/process hostile fixtures explicitly non-skippable in the cutover profile, matching the clear language already used for real Caddy.

## Approval condition

Approve ADR 0017 and the plan as **proposed architecture** once the three blockers are corrected in the documents. No current implementation is required for that approval. Implementation and cutover remain separately gated by the amended plan’s real-substrate, crash-point, hostile-boundary, migration/restore, and sequence/hash evidence.
