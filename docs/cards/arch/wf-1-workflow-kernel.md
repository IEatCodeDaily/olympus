# WF-1 · Workflow kernel: bounded declarative activity chains (ADR 0013)

## Goal
Implement ADR 0013 exactly: versioned YAML DAG definitions, event-sourced
runs, single-scheduler step dispatch over JOBS-1/frames, capability
intersection at run start + step dispatch. NO general control flow — read the
ADR's scope ceiling twice before starting; it is enforced at review.

## Read FIRST
- `docs/adrs/0013-workflow-kernel-bounded-chains.md` — the whole thing; it IS
  the spec. The scope ceiling section is non-negotiable.
- `docs/adrs/0012-programmable-operating-environment.md` principles 10/12/16.
- JOBS-1's merged result: job dispatch API + JobDispatched/JobCompleted
  events — your local-activity execution substrate.
- CAPS-1's merged result: `authorize_capability` — call it at run start and
  before EVERY step dispatch; revoked → StepFailed{cause:"revoked"}, run
  aborts (this is the ADR's revocation semantics).
- PKG-1's merged result: workflow templates + activity providers in registry
  v2; runs pin definition digest + provider bindings.
- `crates/control-plane/src/server/routes/` — add workflows.rs following the
  established module pattern.

## Build on
Branch from main after JOBS-1, CAPS-1, and PKG-1 all merge (three parents).

## Deliverables
1. Definition schema (serde YAML + JSON-Schema validation): steps[] with
   {id, uses (capability ID), with (templated inputs), needs[], when
   (single field comparison only), on_failure (retry N|continue|abort),
   timeout_secs}. Inputs declared with types. REJECT: loops, undeclared
   template functions, expression grammar beyond `{{ inputs.x }}` /
   `{{ steps.<id>.output.y }}`.
2. Draft/publish lifecycle as events, gated on the CAPS-1-reserved
   `workflow.draft.create` / `workflow.publish` capability IDs. Published
   definitions immutable; digest recorded.
3. Run engine: `WorkflowRunStarted/StepDispatched/StepCompleted/StepFailed/
   RunSignaled/RunCompleted/RunCancelled` events; run projection; scheduler
   loop in Hall dispatching ready steps (needs satisfied) with idempotency
   key run_id:step_id:attempt. Parallel fan-out via concurrent dispatch,
   join via needs. Hall restart resumes from projection (test this).
4. Activity resolution: `uses: job.run` → JOBS-1 dispatch; provider bindings
   resolved at run start and pinned. Unknown capability → validation error
   at publish, not at run.
5. REST: POST /api/workflows (draft), POST /:id/publish, POST /:id/runs
   (non-blocking, returns run id), GET runs/:id, POST runs/:id/signal,
   DELETE runs/:id (cancel). Three-file contract rule.
6. Timers: fireAt steps ride the existing trigger scheduler (ADR 0008 §4) —
   do not build a second timer wheel.
7. Tests: DAG execution order, parallel join, retry, cancel, restart-resume,
   capability-revocation mid-run, template resolution, schema rejection table
   (each forbidden construct → specific error).

## Settled decisions — do NOT re-litigate
- ADR 0013's scope ceiling. If you find yourself wanting an expression
  evaluator, stop and re-read it. Logic goes in activities; generality lives
  in agent sessions.
- WorkflowComplete AgentEvent push into session streams: SKIP in this card if
  it requires proto changes — note it as follow-up instead (proto is
  contended by other workers).
- No MCP tools in this card (that's the Hall MCP server card, ADR 0011
  Phase 2).

## Gates
- `make lint` + `make test` + fmt green; `-j 2`, target under ~/.cache/.
- Do not push to main. Green → `blocked: review-required` with a worked
  example definition + its run event trace as evidence.
