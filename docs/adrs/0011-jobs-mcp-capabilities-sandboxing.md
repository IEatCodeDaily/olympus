# ADR 0011 — Jobs, MCP agent interface, capabilities, and sandboxing (evolution roadmap)

Status: accepted · Date: 2026-07-11
Source: operator + Zephyr session decisions, relayed to Terminus 2026-07-11.
Relates to: ADR 0002 (layer boundary), ADR 0008 (Hall/Envoy split, wire
protocol), ADR 0010 (Hall auth), ADR 0006 (setup adapter).

## Decisions

### 1. JobRunner is a role of olympus-envoy, not a separate binary

`Hello` gains `NodeRole` — `AgentRuntime`, `JobRunner`, or both. Hall routes
work by role. Same binary, same iroh transport, same frame protocol.

- `HallFrame` += `DispatchJob`, `CancelJob`.
- `EnvoyFrame` += `JobOutput`, `JobResult`.
- Envoy gains a `JobTable` paralleling `RuntimeTable`. The idle-reaper pattern
  (shipped 2026-07-11, `3fd7d2f`) applies to both tables.

### 2. iroh is the overlay network

All inter-node Olympus traffic rides iroh QUIC (e2e-encrypted, NAT-traversing,
keyed by node id). No WireGuard/Netbird/Tailscale for Olympus traffic. A new
build node (e.g. LXC) needs only: envoy binary + toolchains + Hall's iroh node
ID. Allowlist enrollment stays fail-closed per ADR 0008.

### 3. The agent-facing interface is MCP, not a CLI

Hall exposes an MCP server endpoint. The setup adapter injects it into each
session's `.mcp.json` (native path per ADR 0006 §9). Tools: `list_nodes`,
`run_job`, `get_job`; later `run_workflow`. The human-facing `olympus` CLI is
a separate concern and speaks the REST API — the two surfaces do not share an
implementation contract beyond the underlying services.

### 4. Per-session capability model

Replaces single-token authorization for agent sessions. A capability set is:

- inherited from the parent session; **narrowed only on fork, never expanded**;
- controls: readable/writable paths, allowed tools, linked repos/vaults,
  resource limits, `can_fork`;
- signed by Hall, validated on every call — fail closed.

Builds on the `Principal`/`OrgScope` seam (ARCH-A): a session capability is a
principal payload, not a parallel auth mechanism.

### 5. File isolation via bubblewrap mount namespaces

The symlink-based repo/skill attach has no enforcement. Each agent session
runs inside a bwrap mount namespace: only capability-approved paths are
bind-mounted (RO/RW per the capability set); the process cannot see outside
its namespace. No root required. Firecracker/microVM is the future tier for
untrusted code — out of scope here.

### 6. Workflow engine (prerequisite for `run_workflow`)

DAGs of steps, templates, typed I/O, exposed as MCP tools. Non-blocking:
returns `workflow_id` immediately; Hall pushes a `WorkflowComplete` event as a
new `AgentEvent` variant into the session stream (async push, not polling).

## Build order (each phase independently shippable)

> **Amended by ADR 0012/0013 (2026-07-12):** the phase list below stands, but
> Phase 3 (capability model) is pulled forward ahead of Phase 2, the workflow
> engine of Phase 5 is bound to ADR 0013's bounded-chain decision, and each
> phase now lands under the ADR 0012 extension doctrine (JOBS-1 becomes the
> first activity provider; MCP fulfills the session-tool class).

1. Job dispatch — proto frames + envoy JobTable + Hello roles. Alone unblocks
   remote compilation offload.
2. MCP server on Hall.
3. Capability model.
4. File isolation (bwrap).
5. Workflow engine.

## Sequencing constraint vs the architecture-refactor wave (2026-07-11)

Phase 1 touches `crates/proto/src/frames.rs` and envoy internals — the same
surfaces as in-flight cards ARCH-E (envoy ingestion, extends frames) and
ARCH-F (bridge seams). Phase 1 is therefore gated on ARCH-E + ARCH-F merging.
Phase 3 builds on ARCH-A's Principal seam. Phase 2 builds on ARCH-B's route
modules.
