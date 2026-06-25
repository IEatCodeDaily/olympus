# ADR 0001: Olympus is a clean-room product with Convex + React + Bun

- Status: Accepted
- Date: 2026-06-25

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
