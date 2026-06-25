# Agent Map — Olympus

Short map for coding agents working on Olympus. Detailed guidance lives in `docs/`.

## What Olympus is

A clean-room AI control plane for Hermes Agent: **React + self-hosted Convex + a
thin Bun host runtime**. NOT a fork of Hermes Studio. See `docs/architecture/architecture.md`
and `docs/adrs/0001-clean-room-convex-react-bun.md`.

## First reads

- `docs/architecture/architecture.md` — doctrine, ownership matrix, command/event flow.
- `docs/adrs/` — accepted decisions.
- `docs/plans/` — the migration plan (bite-sized tasks).

## Workspace

- `apps/web` — React UI (Convex-subscribed). No business logic here beyond view state.
- `apps/runtime` — Bun host runtime adapter. Claims Convex commands, runs host effects via the Hermes adapter, streams events back. Keep it small.
- `convex/` — Convex schema + functions. Source of truth + orchestration intent.
- `packages/protocol` — shared command/event schemas (dependency-free).
- `packages/hermes-adapter` — `AgentRuntime` interface + `HermesAgentRuntime` impl.

## Commands (Bun-first)

```bash
bun install
bun run convex:dev      # Convex dev deployment
bun run runtime:dev     # Bun host runtime (health on :8791)
bun run web:dev         # React dev server (:5177)
bun run lint            # oxlint (0 warnings policy)
bun run typecheck       # tsc --noEmit
bun test                # bun test (protocol + units)
bun run build           # protocol tests + web build + runtime binary
```

## Hard rules

- React and Convex MUST NOT import Hermes internals. All host execution goes through `packages/hermes-adapter`'s `AgentRuntime`.
- Convex functions MUST declare explicit `args` AND `returns` validators.
- Convex is for state/orchestration; the Bun runtime owns processes/PTY/filesystem. Do not put OS-process supervision in Convex actions.
- Keep the Bun runtime small and boring; no business logic that belongs in Convex.
- oxlint must pass with 0 warnings before a PR. Run `react-doctor --diff` on web changes.
- Every PR must pass code-reviewer AND tester (Maestro web e2e) before merge.
