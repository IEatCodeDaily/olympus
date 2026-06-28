# ADR 0001: Olympus is a clean-room product with Convex + React + Bun

- Status: SUPERSEDED (tech stack) by ADR 0003; PARTIALLY SUPERSEDED (product framing) by ADR 0002
- Date: 2026-06-25

> **Superseded notes:**
> - **Product framing (2026-06-27, ADR 0002):** the "thin Bun host-effect
>   runtime" framing was reframed to a multi-node agent fleet control plane.
> - **Tech stack (2026-06-28, ADR 0003):** **Convex is removed entirely.** The
>   Layer-1 control plane is now a **Rust-native single binary** (redb event log +
>   in-memory views + single-writer scheduler + iroh/UDS transport + tantivy +
>   embedded workflow engine). React for the UI and Bun/TS for tooling remain
>   acceptable choices but are no longer load-bearing substrate decisions. This
>   ADR remains the record of the *original* stack choice; ADR 0003 is the record
>   of removing it.

## Context

Hermes Studio (maintained fork `IEatCodeDaily/hermes-studio`) is a Koa/SQLite/Vue
web dashboard wrapping the Hermes Agent CLI. Backend correctness is hard to
guarantee with thin controllers over SQLite, realtime relies on bespoke Socket.IO
plumbing, and state ownership is diffuse.

We want a successor product, "Olympus", that makes backend correctness easier,
is realtime-native, and keeps a clean separation between durable state and host
execution.

## Decision

Build Olympus as a clean-room product (not a fork) with three layers:

1. **Self-hosted Convex** as the control plane and source of truth (sessions,
   messages, agents, tool calls, runtime commands/events, authz). Typed function
   args/returns make backend correctness enforceable.
2. **React** UI subscribed to Convex via `convex/react` hooks. No second backend.
3. **A thin Bun host runtime** that claims runtime commands from Convex, performs
   privileged host effects through a Hermes adapter, and streams events back.

Hermes Agent remains the execution engine behind `packages/hermes-adapter`'s
`AgentRuntime` interface, so it is swappable later.

## Rejected Alternatives

- **Everything in Convex (including runtime):** Convex functions are not OS process
  supervisors. PTYs, long-lived workers, and filesystem authority do not belong in
  Convex actions. Rejected as a layer violation.
- **Node backend service:** The user prefers not to add a Node host runtime; Bun is
  the chosen runtime for the host adapter (fast startup, good process APIs, single-binary
  compile).
- **Fork Hermes Studio and refactor in place:** Too much coupling to Vue/Koa/SQLite;
  a clean-room product with a strangler migration is cleaner and lower-risk.
- **TanStack Start / Next.js full-stack:** Unnecessary; Convex provides the backend
  and realtime, React+Vite(Bun) is enough for the UI.

## Consequences

- Bun is the package manager, script runner, frontend build orchestrator, and the
  runtime executable compiler. Convex functions still run in Convex's own runtime.
- The Hermes adapter boundary must be respected: React/Convex never import Hermes
  internals.
- Migration is a strangler: stand up Convex + React + Bun beside the existing Studio,
  move domains one at a time, retire Studio only after parity.
