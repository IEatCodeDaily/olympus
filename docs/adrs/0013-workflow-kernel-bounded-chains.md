# ADR 0013 — Workflow kernel: bounded activity chains over the event log, not a general durable-execution engine

Status: accepted · Date: 2026-07-12
Relates to: ADR 0012 (extension doctrine), ADR 0011 (§6 workflow engine,
§7 build order), ADR 0009 (SQLite event log), ADR 0027 (workflow triggers are
typed graph invocations, not subagent calls; workflow agent sessions preserve
the initiating agent-spawn depth and remain separately workflow-bounded).

## Context

ADR 0012 makes workflows the composition fabric: "workflows durably compose
capabilities." That phrase hides the hardest engineering object in the whole
design. A general durable-execution engine (Temporal-class) requires:

- deterministic replay (code-as-workflow must re-execute identically against
  recorded history);
- event-sourced timers, signals, and child workflows that survive restarts;
- versioning of in-flight definitions (a run started on v1 must finish on v1
  semantics while v2 runs concurrently);
- idempotent activity dispatch with exactly-once *effects* (or explicit
  at-least-once contracts);
- backpressure, fencing, and multi-writer arbitration across nodes.

Temporal is a decade into this. Hand-rolling it "because we already have an
event log" is the classic trap: the event log gives durability of *records*,
not correctness of *replay*.

Three candidate paths were considered:

**Option 1 — Embed an existing durable-execution core.** Temporal(ite) is a
Go server + heavy operational surface — against the local-first single-binary
posture, and its code-as-workflow model wants worker SDKs per language.
Rust-native durable-execution cores are young/unproven. Embedding buys replay
correctness at the cost of a second kernel with its own operational model,
inside a product whose kernel discipline (ADR 0012 §14) says exactly one.

**Option 2 — Hand-roll general durable execution over our event log.**
Highest risk. The subtle failures (non-deterministic replay, timer skew,
version drift mid-run) ship as data corruption of *intent*, discovered months
later. Nothing in the current roadmap needs general replay.

**Option 3 — Bounded declarative activity chains.** Workflows are DATA (YAML
DAGs of activity invocations, per ADR 0012 principle 3 — "definitions are
runtime data"), not code. The scheduler is a state machine over the event log:
each run is a sequence of `WorkflowRun*` events; each step is an activity
dispatch with recorded input/output. No user code executes inside the kernel;
therefore **no deterministic-replay problem exists** — recovery is "read the
run's events, find the last completed step, dispatch the next one." Timers are
`fireAt` rows (the ADR 0008 trigger scheduler already exists). Signals are
events appended to the run.

## Decision

**Option 3. The workflow kernel is a bounded, declarative, event-logged
activity chain executor — and its scope ceiling is enforced by doctrine.**

- A workflow definition is a versioned, immutable YAML document: a DAG of
  steps, each `uses:` a semantic capability (activity), `with:` templated
  inputs, plus `on_failure:` (retry N / continue / abort), `timeout`, and
  step-level `needs:` edges. JSON-Schema-validated at draft time.
- Definitions carry NO general control flow: no loops, no recursion, no
  arbitrary expressions beyond input templating and declared conditionals
  (`when: <field comparison>`). If a workflow needs logic, the logic belongs
  in an ACTIVITY (a package contribution running in the sandboxed tier) — the
  workflow only sequences effects.
- Run state is event-sourced: `WorkflowRunStarted / StepDispatched /
  StepCompleted / StepFailed / RunSignaled / RunCompleted / RunCancelled` —
  appended to the existing SQLite event log, projected like every other view.
  Hall restarts resume runs by projection, not replay.
- Activity dispatch requires the hardened JOBS-2 substrate from ADR 0017, not
  the original volatile JOBS-1 route. Hall atomically appends
  `StepDispatchPlanned` plus durable job intent before sending a fenced
  `(job_id, attempt_epoch)` dispatch. Restart recovery reconciles/attaches to
  that exact attempt; absence of completion never means "dispatch another."
  Durable output ingestion is exactly-once by sequence, while host effects are
  explicitly at-least-once with idempotency keys
  (`run_id:step_id:attempt_epoch`). Providers declare idempotency in their
  manifest. A non-idempotent ambiguous effect enters `StepIndeterminate` and
  pauses for operator reconciliation; `max_retries: 0` alone is not considered
  fail-closed.
- Runs pin their definition digest + provider bindings + package versions at
  start (ADR 0012 principle 10). Referenced package versions are retained
  until no live run depends on them.
- Capability evaluation per ADR 0012 §12 at RUN START and re-checked at each
  step dispatch (this gives revocation a natural boundary: revoked authority
  stops the run at the next step, recorded as `StepFailed{cause: revoked}` —
  resolving ADR 0012's open question for the v1 semantics).
- Concurrency: single scheduler in Hall (the ADR 0002 single-writer rule);
  parallel steps fan out as concurrent activity dispatches, joined by `needs:`.

### The scope ceiling (doctrine, enforced in review)

The kernel NEVER grows: loops/recursion, in-definition expression languages,
code-as-workflow SDKs, sub-DAG generation at runtime, or replay-based
recovery. If a use case appears to need one of these, the answer is one of:
(a) push the logic into an activity; (b) have an agent session author the
next workflow run (agents are the general-purpose layer — Olympus already has
them; the workflow kernel does not need to become one); (c) if genuinely
neither, write the ADR that supersedes this one and adopt an existing engine
rather than growing ours. **Options (a) and (b) are expected to cover
everything.** The point of owning agents is that the workflow engine can stay
dumb.

## Consequences

- Workflow kernel remains bounded: definition schema + run projection +
  scheduler loop + step dispatch over JOBS-2's durable service/frames. It does
  not ship until JOBS-2 attempt, ACK, terminal-order, reconciliation, provider,
  and sandbox gates are green.
- WorkflowComplete/StepCompleted push into session streams as new AgentEvent
  variants (ADR 0011 §6) — additive to proto.
- MCP tools (`run_workflow`, `get_run`, `signal_run`) and the schema-aware
  `olympus workflow ...` CLI expose the same typed run operations through the
  Hall operation seam (ADR 0019). MCP starts non-blocking; CLI waits by default
  for Unix pipeline semantics and supports explicit `--detach`.
- What we give up: long-lived code-shaped orchestrations with complex
  branching living INSIDE the engine. Accepted: those live in agents or
  activities by design.
- Risk accepted: if Olympus someday needs true general durable execution,
  this ADR is superseded and an engine is adopted — the event-sourced run
  format migrates (it's data), the YAML definitions migrate (they're data);
  only the scheduler is discarded. The bounded design keeps that exit cheap.
