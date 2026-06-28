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

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **olympus** (175 symbols, 213 relationships, 1 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> Index stale? Run `node .gitnexus/run.cjs analyze` from the project root — it auto-selects an available runner. No `.gitnexus/run.cjs` yet? `npx gitnexus analyze` (npm 11 crash → `npm i -g gitnexus`; #1939).

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows. For regression review, compare against the default branch: `detect_changes({scope: "compare", base_ref: "main"})`.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `query({search_query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `context({name: "symbolName"})`.
- For security review, `explain({target: "fileOrSymbol"})` lists taint findings (source→sink flows; needs `analyze --pdg`).

## Never Do

- NEVER edit a function, class, or method without first running `impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `rename` which understands the call graph.
- NEVER commit changes without running `detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/olympus/context` | Codebase overview, check index freshness |
| `gitnexus://repo/olympus/clusters` | All functional areas |
| `gitnexus://repo/olympus/processes` | All execution flows |
| `gitnexus://repo/olympus/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
