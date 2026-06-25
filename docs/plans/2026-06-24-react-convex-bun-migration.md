# React + Convex + Bun Migration Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Migrate Olympus from the current Vue/Koa/SQLite-style web UI into a separated React + self-hosted Convex control plane with a thin Bun host runtime adapter over Hermes Agent.

**Architecture:** Convex owns durable truth, realtime subscriptions, agent/session/message state, tool-call lifecycle, and runtime command/event orchestration. A small Bun runtime runs on the host, claims runtime commands from Convex, executes privileged host effects through Hermes, and writes structured events back. React is a Convex-subscribed UI, not a second backend.

**Tech Stack:** React, TypeScript, Convex, Bun, Vite-compatible frontend build via Bun commands, Bun single-file executable for the host runtime, Hermes Agent adapter boundary.

---

## Architecture Doctrine

> Olympus is the product/control plane. Convex is the brain and memory. Bun is the host-side hands. Hermes remains the current execution engine behind a thin adapter.

### Ownership Matrix

| Capability | Owner | Notes |
|---|---|---|
| Users/authz | Convex | Centralized checks in queries/mutations/actions. |
| Profiles | Convex | Store metadata and local runtime binding, not raw secrets by default. |
| Sessions | Convex | Canonical session records. |
| Messages | Convex | Canonical message history and live subscription source. |
| Tool calls | Convex | Durable lifecycle: queued/running/succeeded/failed/cancelled. |
| Runtime commands | Convex | Intent records for host actions. |
| Runtime events | Convex | Append-only observable state from Bun runtime. |
| Hermes bridge workers | Bun runtime | Host-side process management. |
| PTYs/terminal | Bun runtime, maybe Rust helper later | Keep Convex out of TTY ownership. |
| Filesystem access | Bun runtime | Guarded host capability. |
| LLM provider calls | Convex first where possible; Hermes path initially | Move gradually after parity. |
| Legacy compatibility API | Temporary adapter | Exists only during migration. |

---

## Dev Environment Doctrine

Development must be fully separated from the current installed Olympus/Hermes environment.

### Directory Layout

```text
/home/rpw/olympus-next/
  apps/
    web/              # React frontend
    runtime/          # Bun host runtime adapter
  convex/             # Convex schema/functions/actions
  packages/
    protocol/         # Shared command/event/tool schemas
    hermes-adapter/   # Bun-facing Hermes adapter library
    ui/               # Optional shared UI components
  docs/
    architecture/
    adrs/
    plans/
  scripts/
```

### Local Ports

| Service | Port | Notes |
|---|---:|---|
| React dev server | 5177 | Avoid current Olympus/Vite defaults. |
| Convex local/self-host | Convex default/local configured | Use isolated deployment name. |
| Bun runtime health | 8791 | Localhost only. |
| Current Olympus legacy | 8787 | Keep running separately for comparison. |

### Environment Files

```text
.env.example
.env.local                    # local-only, gitignored
.env.runtime.local            # host runtime env, gitignored
convex/.env.local             # Convex local dev, gitignored if generated
```

Required environment variables should be explicit:

```text
CONVEX_URL=
OLYMPUS_RUNTIME_ID=local-dev
OLYMPUS_RUNTIME_TOKEN=
OLYMPUS_RUNTIME_HEALTH_PORT=8791
HERMES_HOME=/home/rpw/.hermes
HERMES_BIN=hermes
```

---

## Bun Compatibility Position

Current Convex docs explicitly support Bun for scripts and servers using Convex clients, and even running the Convex CLI via Bun. React + Convex is just frontend TypeScript and the `convex/react` package; it should work under Bun-driven installs/scripts as long as the browser bundle is produced normally.

Use Bun as the project package manager and task runner:

```bash
bun install
bunx convex dev
bun run dev
bun run build
bun run runtime:dev
```

Do **not** assume Convex backend functions run on Bun. Convex functions run in Convex's own JS runtime or Convex Node.js runtime. Bun is for:

- package management
- scripts
- frontend build orchestration
- host runtime adapter
- executable compilation

---

## Binary Strategy

### Runtime Adapter Binary

Compile the Bun runtime adapter to a single executable:

```bash
bun build apps/runtime/src/main.ts \
  --compile \
  --target=bun-linux-x64 \
  --outfile=dist/olympus-runtime
```

The binary should include only the host adapter, not Convex itself.

### Frontend Distribution

Build React to static assets:

```bash
bun run web:build
```

Then choose one of two serving modes:

1. **Dev/simple mode:** serve static assets through Bun runtime.
2. **Production split mode:** serve assets through Caddy/nginx or Convex-compatible hosting later.

For a self-contained local app, Bun can embed or copy static assets near the executable, but do not block v1 on perfect single-binary asset embedding. Start with:

```text
dist/
  olympus-runtime
  web/
    index.html
    assets/...
```

Then add asset embedding once runtime behavior is stable.

---

## Protocol First

Create shared protocol schemas before implementing runtime behavior.

### Runtime Command

```ts
export type RuntimeCommand =
  | {
      kind: 'agent.run.start'
      commandId: string
      profileId: string
      sessionId: string
      input: string
      provider?: string
      model?: string
    }
  | {
      kind: 'agent.run.abort'
      commandId: string
      runId: string
    }
  | {
      kind: 'terminal.open'
      commandId: string
      profileId: string
      cwd?: string
      shell?: string
    }
  | {
      kind: 'terminal.input'
      commandId: string
      terminalId: string
      data: string
    }
  | {
      kind: 'fs.read'
      commandId: string
      profileId: string
      path: string
    }
```

### Runtime Event

```ts
export type RuntimeEvent =
  | { kind: 'runtime.heartbeat'; runtimeId: string; at: number }
  | { kind: 'command.claimed'; commandId: string; runtimeId: string; at: number }
  | { kind: 'agent.run.started'; commandId: string; runId: string; at: number }
  | { kind: 'agent.run.delta'; runId: string; text: string; at: number }
  | { kind: 'agent.run.completed'; runId: string; at: number }
  | { kind: 'agent.run.failed'; runId: string; error: string; at: number }
  | { kind: 'terminal.output'; terminalId: string; data: string; at: number }
```

Use a schema library that works in Bun and browser code. Prefer `zod` initially unless Convex validators are enough at the boundary.

---

## Phase 0 — Foundation Branch

### Task 0.1: Create isolated workspace

**Objective:** Keep the new architecture isolated from the current maintained Olympus runtime.

**Files:**
- Create: `/home/rpw/olympus-next/`

**Steps:**
1. Create repo/workspace directory.
2. Add `.gitignore` for env, dist, generated Convex artifacts as appropriate.
3. Add README explaining this is the React+Convex+Bun migration workspace.
4. Commit.

**Verification:**

```bash
cd /home/rpw/olympus-next
git status --short
```

Expected: clean after commit.

### Task 0.2: Pin package manager to Bun

**Objective:** Make Bun the only package manager for the migration workspace.

**Files:**
- Create: `package.json`
- Create: `bun.lock`
- Create: `.npmrc` only if needed to block accidental package-lock use

**package.json baseline:**

```json
{
  "name": "@ieatcodedaily/olympus-next",
  "private": true,
  "type": "module",
  "packageManager": "bun@1.3.5",
  "workspaces": [
    "apps/*",
    "packages/*"
  ],
  "scripts": {
    "dev": "bun run --filter @olympus/web dev",
    "build": "bun run protocol:check && bun run web:build && bun run runtime:build",
    "protocol:check": "bun test packages/protocol",
    "web:build": "bun run --filter @olympus/web build",
    "runtime:dev": "bun run apps/runtime/src/main.ts",
    "runtime:build": "bun build apps/runtime/src/main.ts --compile --target=bun-linux-x64 --outfile=dist/olympus-runtime"
  }
}
```

**Verification:**

```bash
bun install
bun pm ls
```

Expected: succeeds with `bun.lock`, no `package-lock.json`.

---

## Phase 1 — Convex Skeleton

### Task 1.1: Add Convex project

**Objective:** Create local Convex backend structure without replacing existing Olympus APIs.

**Files:**
- Create: `convex/schema.ts`
- Create: `convex/auth.ts` or placeholder auth module
- Create: `convex/runtimeCommands.ts`
- Create: `convex/runtimeEvents.ts`
- Create: `convex/sessions.ts`
- Create: `convex/messages.ts`

**Commands:**

```bash
bun add convex
bunx convex dev
```

**Verification:**

```bash
bunx convex codegen
```

Expected: generated API types are created and TypeScript imports resolve.

### Task 1.2: Define initial schema

**Objective:** Store enough state to drive a chat run through the runtime adapter.

**Schema tables:**

- `profiles`
- `sessions`
- `messages`
- `runtimeCommands`
- `runtimeEvents`
- `runtimeHeartbeats`
- `toolCalls`

**Acceptance:**
- Every public mutation validates args.
- Every public query returns explicitly shaped data.
- Runtime-only functions are internal or token-guarded.

---

## Phase 2 — Shared Protocol Package

### Task 2.1: Create `packages/protocol`

**Objective:** Share command/event schemas between React, Convex, and Bun runtime.

**Files:**
- Create: `packages/protocol/package.json`
- Create: `packages/protocol/src/runtime.ts`
- Create: `packages/protocol/src/index.ts`
- Create: `packages/protocol/runtime.test.ts`

**Verification:**

```bash
bun test packages/protocol
```

Expected: protocol parsing tests pass.

---

## Phase 3 — Bun Runtime Adapter

### Task 3.1: Runtime heartbeat

**Objective:** Prove Bun runtime can authenticate/connect to Convex and publish heartbeat events.

**Files:**
- Create: `apps/runtime/package.json`
- Create: `apps/runtime/src/main.ts`
- Create: `apps/runtime/src/convexClient.ts`
- Create: `apps/runtime/src/heartbeat.ts`

**Behavior:**
- Reads `CONVEX_URL`, `OLYMPUS_RUNTIME_ID`, `OLYMPUS_RUNTIME_TOKEN`.
- Writes heartbeat every 10s.
- Exposes `GET /healthz` on localhost.

**Verification:**

```bash
bun run runtime:dev
curl -fsS http://127.0.0.1:8791/healthz
```

Expected: JSON health response and Convex heartbeat row updates.

### Task 3.2: Command claim loop

**Objective:** Runtime claims pending commands without double execution.

**Files:**
- Modify: `convex/runtimeCommands.ts`
- Modify: `apps/runtime/src/commandLoop.ts`

**Behavior:**
- Runtime polls or subscribes for pending commands.
- Claims command atomically through Convex mutation.
- Writes `command.claimed` event.

**Verification:**
- Insert a test command through Convex dashboard or CLI.
- Runtime claims it exactly once.

### Task 3.3: Hermes adapter interface

**Objective:** Define the stable boundary that lets Hermes be replaced later.

**Files:**
- Create: `packages/hermes-adapter/src/types.ts`
- Create: `packages/hermes-adapter/src/hermesAgentRuntime.ts`

**Interface:**

```ts
export interface AgentRuntime {
  startRun(command: AgentRunStartCommand): Promise<{ runId: string }>
  abortRun(runId: string): Promise<void>
  listTools(profileId: string): Promise<ToolDescriptor[]>
  callTool(call: ToolCallRequest): Promise<ToolCallResult>
}
```

**Rule:** React and Convex never import Hermes internals directly.

---

## Phase 4 — React Frontend Skeleton

### Task 4.1: Create React app using Bun commands

**Objective:** Build a clean React client that talks to Convex.

**Files:**
- Create: `apps/web/package.json`
- Create: `apps/web/index.html`
- Create: `apps/web/src/main.tsx`
- Create: `apps/web/src/App.tsx`
- Create: `apps/web/src/convex.ts`

**Commands:**

```bash
bun add -d vite typescript @vitejs/plugin-react
bun add react react-dom convex
bun run --filter @olympus/web dev --host 127.0.0.1 --port 5177
```

**Verification:**
- Page loads on `127.0.0.1:5177`.
- React can call a Convex query.

### Task 4.2: Runtime status page

**Objective:** First useful UI: show runtime online/offline from Convex heartbeat.

**Files:**
- Create: `apps/web/src/features/runtime/RuntimeStatus.tsx`
- Modify: `apps/web/src/App.tsx`

**Verification:**
- Stop runtime: UI shows offline after timeout.
- Start runtime: UI shows online.

---

## Phase 5 — First End-to-End Slice

### Task 5.1: Start simple agent run command

**Objective:** From React, create a session/message and enqueue an `agent.run.start` command.

**Flow:**

```text
React submit message
  -> Convex mutation creates user message
  -> Convex mutation enqueues runtime command
  -> Bun runtime claims command
  -> Hermes adapter starts run
  -> Bun runtime streams events
  -> Convex stores assistant deltas/messages
  -> React updates live
```

**Verification:**
- Submit `Return exactly PONG` from React.
- See command row, runtime events, assistant message.

---

## Phase 6 — Packaging

### Task 6.1: Build runtime binary

**Objective:** Produce a Bun single-file runtime executable.

**Command:**

```bash
bun run runtime:build
./dist/olympus-runtime --version
```

**Expected:** version output and no Bun install required for runtime execution.

### Task 6.2: Build frontend assets

**Objective:** Produce static React assets with Bun command runner.

**Command:**

```bash
bun run web:build
```

**Expected:** `apps/web/dist/index.html` and assets created.

### Task 6.3: Serve static assets from runtime

**Objective:** Allow one process to serve the local UI and runtime health in dev/local mode.

**Rule:** Static serving is allowed; Convex remains the durable backend.

---

## Phase 7 — Migration Gates

Do not retire current Olympus until these pass:

- [ ] Runtime heartbeat visible in React.
- [ ] One chat run works end-to-end.
- [ ] Abort works.
- [ ] Tool-call event lifecycle works.
- [ ] Terminal prototype works or is explicitly deferred.
- [ ] File read prototype works with path guardrails.
- [ ] Existing Olympus remains runnable on `8787` during migration.
- [ ] New stack runs on separate ports and separate Convex deployment.
- [ ] No production profile/secrets are copied into Convex unintentionally.

---

## Open Decisions

1. Should `/home/rpw/olympus-next` be a separate repo or a branch/subdirectory inside `IEatCodeDaily/olympus`?
   - Recommendation: separate workspace/repo until first E2E slice works, then merge or replace.
2. Should runtime command delivery use polling first or Convex subscriptions?
   - Recommendation: polling first for simplicity; subscription later if latency matters.
3. Should static frontend assets be embedded in the Bun binary for v1?
   - Recommendation: no. Ship `dist/olympus-runtime` + `dist/web/` first. Embed later.
4. Should Hermes bridge access be via CLI, Python bridge socket, or direct module call?
   - Recommendation: CLI/socket first; direct module only after adapter tests exist.

---

## First Implementation Milestone

Milestone 1 is complete when this command sequence works from a clean dev checkout:

```bash
bun install
bunx convex dev
bun run runtime:dev
bun run --filter @olympus/web dev --port 5177
```

And the React app shows:

```text
Olympus Runtime: online
Convex: connected
Hermes adapter: detected
```
