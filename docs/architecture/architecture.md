# Olympus Architecture

## 1. Doctrine

> Convex is the brain and memory. Bun is the hands on the host. Hermes is the
> current execution engine behind a thin, swappable adapter. React is UI only.

Olympus is a clean-room product. It is NOT a fork of Hermes Studio. It reuses
Hermes Agent as an execution engine through an adapter boundary so that engine
can later be swapped or absorbed without rewriting the product.

## 2. System Context

```text
+-----------+      Convex client       +------------------------+
|  React UI | <----------------------> |  Self-hosted Convex     |
+-----------+   queries/mutations/subs |  (control plane + state)|
                                       +-----------+------------+
                                                   | command/event protocol
                                                   v
                                       +------------------------+
                                       |  Olympus Bun runtime    |
                                       |  (host effect executor) |
                                       +-----------+------------+
                                                   | adapter
                                                   v
                                       +------------------------+
                                       |  Hermes Agent           |
                                       |  (tools/process/PTY)    |
                                       +------------------------+
```

## 3. Source-of-Truth Matrix

| Concern | Owner | Notes |
|---|---|---|
| Users / authz decisions | Convex | Centralized in queries/mutations/actions. |
| Profiles | Convex | Metadata + runtime binding; raw secrets stay host-side. |
| Sessions | Convex | Canonical session records. |
| Messages | Convex | Canonical history + live subscription source. |
| Agents / threads | Convex | Agent definitions and thread state. |
| Tool calls | Convex | Durable lifecycle: queued/running/succeeded/failed/cancelled. |
| Runtime commands | Convex | Host action intents (queue). |
| Runtime events | Convex | Append-only observed host state. |
| Process / bridge supervision | Bun runtime | Not Convex's job. |
| PTYs / terminals | Bun runtime | Real TTY control; Rust helper later if needed. |
| Filesystem authority | Bun runtime | Path-guarded host capability. |
| LLM provider calls | Convex (later) / Hermes (initial) | Migrate after parity. |
| UI state | React + Convex | React subscribes; no second backend. |

## 4. The Adapter Boundary

React and Convex MUST NOT import Hermes internals. All host execution goes
through `packages/hermes-adapter`'s `AgentRuntime` interface. Initial impl is
`HermesAgentRuntime`; future impls (`OlympusBunRuntime`, `ConvexNativeAgentRuntime`)
can replace it without touching the product model.

## 5. Command / Event Flow (first E2E slice)

```text
React submit message
  -> Convex mutation: create user message
  -> Convex mutation: enqueue runtime command (agent.run.start)
  -> Bun runtime: claim command atomically (writes command.claimed)
  -> Hermes adapter: start run
  -> Bun runtime: stream agent.run.delta / completed / failed events
  -> Convex: store assistant deltas/messages
  -> React: live updates via useQuery subscription
```

## 6. What Convex Owns vs Does Not

Convex owns bounded, durable, reactive work units: state, authz, orchestration
intent, subscriptions, scheduled functions, agent thread/message persistence.

Convex does NOT own: spawning/holding processes, PTYs, local sockets, long-lived
workers, OS signals, or filesystem authority. Those are the Bun runtime's job.

## 7. Deployment Model

- Self-hosted Convex (open-source backend) — same code as cloud.
- Bun runtime compiles to a single executable (`dist/olympus-runtime`).
- React builds to static assets; served via Bun runtime in dev/local, or a
  static host in production.
- Single Olympus deployment for the owner to test capability (not multi-tenant yet).

## 8. Current State vs Target State

| Aspect | Current | Target |
|---|---|---|
| Backend | Hermes Studio (Koa/SQLite/Vue) maintained separately | Convex + React + Bun |
| Agent state | SQLite via Studio | Convex tables/functions |
| Host execution | Hermes bridge inside Studio | Bun runtime adapter over Hermes |
| UI | Vue 3 | React subscribed to Convex |
| Build tool | npm/vite | Bun-first |
