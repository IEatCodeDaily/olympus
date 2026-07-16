# CLI-1 · Agent and human `olympus` interface (ADR 0019)

## Goal

Ship a Rust `olympus` binary as a schema-aware, pipeable adapter over the same
typed Hall operation seam used by MCP/REST. Agent mode uses the Envoy-owned
runtime UDS and never receives Hall credentials or endpoint selection.

## Read first

- `docs/adrs/0019-agent-and-human-cli-interface.md`
- `docs/adrs/0017-session-cutover-and-remote-development-plane.md` §3
- `docs/adrs/0013-workflow-kernel-bounded-chains.md`
- Phase 3/4 and the workflow task in
  `docs/plans/2026-07-13-session-cutover-remote-development.md`

## Dependencies

- SESSION-SAFE Tasks 1.1-1.5, including runtime-attempt identity and fail-closed
  capabilities.
- AGENT-IFACE Tasks 3.1-3.3: canonical operation registry, durable operation
  reservation, UDS gateway, adapters, and CLI core.
- A command group may land only after its backing durable operation is green.
- `workflow run` depends on JOBS-2 Tasks 2.1-2.5, Tasks 4.1-4.4, canonical
  PKG-1, and WF-1 Task 4.5; effectful workflows require the hostile sandbox
  gate. Mirror these as dispatch-blocking Kanban edges before starting work.

## Deliverables

1. `crates/cli` binary named `olympus`, with `clap` static grammar.
2. Versioned operation request/result/error types in `olympus-proto`.
3. Exhaustive operation descriptors covering types, scope, capability,
   idempotency, revocation, availability, audit, and adapter mapping.
4. Runtime UDS client with no token/endpoint override in agent mode and ADR
   0019's exact path/UID/GID/cgroup/peer/generation/revocation contract.
5. `session info` as the first non-effectful vertical slice.
6. `operation get` as the generic scoped recovery surface when effect acceptance
   is unknown and only the stable client operation ID is available.
7. Job/workflow/artifact/deployment/app command groups gated by their backing
   services; no raw API/argv/SSH command.
8. Two-phase workflow input parser driven by the pinned digest and closed
   `olympus.workflow-input/v1` publication artifact.
9. Human, JSON, JSONL, result-only, durable non-expiring cursor wait/detach,
   Ctrl-C, and
   stable exit classes.
10. Generated man pages and Bash/Zsh/Fish/PowerShell completions from the static
   command definition; dynamic workflow help from Hall.
11. Black-box runtime UDS→Envoy→iroh→Hall, operation retry/revocation, parser
    conformance, pipeline, and CLI/MCP equivalence tests.

## Gates

- Agent runtime cannot switch to operator transport or address another session.
- Copied/open FD, wrong-cgroup/PID/generation, accepted-connection revoke, and
  ambiguous response retry gates pass against the real gateway.
- Commit+lost-response with Hall/gateway unavailable returns
  `acceptance_unknown` with the operation ID; old v1 watch cursors remain
  resumable and cannot silently reset.
- CLI and MCP produce equivalent durable operation events and capability
  outcomes.
- Invalid/colliding/oversized workflow schemas fail at publish; invalid flags or
  truncated stdin create no workflow run; republish races cannot substitute a
  digest.
- stdout/stderr and JSON/JSONL contracts are byte-tested and pipe-tested.
- Help, man pages, completions, and operation schemas cannot drift in CI.
- Real Hermes sandbox evidence includes a piped workflow invocation.
