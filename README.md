# Olympus

> AI control plane for Hermes Agent — React + self-hosted Convex + a thin Bun host runtime.

**Status:** v0 — docs-first foundation. Greenfield successor concept to Hermes Studio.

Olympus is a clean-room product, not a fork of Hermes Studio (that maintained fork lives at
`IEatCodeDaily/hermes-studio`). Olympus keeps durable truth and orchestration in
**self-hosted Convex**, renders a **React** UI subscribed to Convex, and performs
privileged host actions through a **thin Bun runtime adapter** that talks to Hermes Agent.

## Doctrine

> Convex is the brain and memory. Bun is the hands on the host. Hermes is the current
> execution engine behind a thin, swappable adapter. React is UI only.

## Architecture (at a glance)

```text
React UI
  -> self-hosted Convex (sessions, messages, agents, tool calls, runtime commands/events, authz)
  -> Olympus Bun runtime (claims commands, runs host effects via Hermes adapter, streams events back)
  -> Hermes Agent (existing tool/process/PTY execution engine)
```

See `docs/architecture/architecture.md` for the full model and `docs/adrs/` for decisions.

## Workspace

```text
apps/web/              React frontend (Convex-subscribed UI)
apps/runtime/          Bun host runtime adapter (compiles to a single executable)
convex/                Convex schema + functions (control plane / agent state)
packages/protocol/     Shared runtime command/event/tool schemas
packages/hermes-adapter/  AgentRuntime interface + Hermes implementation
docs/                  architecture, ADRs, plans, product
```

## Toolchain (Bun-first)

```bash
bun install
bun run convex:dev      # self-hosted/local Convex dev deployment
bun run runtime:dev     # Bun host runtime adapter
bun run web:dev         # React dev server (port 5177)
bun run lint            # oxlint
bun run typecheck       # tsc --noEmit
bun run build           # protocol tests + web build + runtime binary
```

## Why not "everything in Convex"

Convex owns app/agent state and orchestration intent extremely well, but it is not an OS
process supervisor. PTYs, long-lived bridge workers, local filesystem authority, and Hermes
process lifecycle stay in the Bun runtime. The Bun runtime is intentionally small.
