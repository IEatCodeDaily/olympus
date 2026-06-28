# Olympus Architecture

> **Condensed reference. The authoritative specification is
> [`docs/adrs/0002-olympus-fleet-control-plane.md`](adrs/0002-olympus-fleet-control-plane.md),
> with the substrate decision in
> [`docs/adrs/0003-remove-convex-rust-native-substrate.md`](adrs/0003-remove-convex-rust-native-substrate.md).**
> Where this file and ADR 0002 disagree, ADR 0002 wins; where ADR 0002 and ADR
> 0003 disagree on the substrate, ADR 0003 wins. This file is a map; the ADRs are
> the contracts. Section numbers point into ADR 0002.

## 1. Doctrine

> Olympus is a multi-node agent fleet control plane: a **Rust-native single-binary
> control plane** owns all durable truth as an **append-only event log** in redb,
> orchestration intent, workflows, and reactive views; one **node agent ("envoy")**
> per host owns every host effect — process supervision, workspace, skills/MCP,
> credentials, desired-state, vault sync; and heterogeneous agents (Hermes, Claude
> Code, Codex, Droid, OpenClaw) run as supervised processes behind a stable
> `AgentRuntime` boundary.

Olympus is a **control plane**, NOT an agent runtime and NOT a Hermes Studio UI
replacement. Agents remain external programs; Olympus manages, routes, observes,
steers, and records them — and owns their working directories so they operate in
a scoped jj worktree instead of grepping the whole host.

**Convex is removed (ADR 0003).** The control plane is a self-contained Rust
binary; no external backend, no DB server, no SPOF connection terminus.

## 2. Three layers

```text
LAYER 1  Control plane     single Rust binary + React UI over WSS
                           sole source of truth: redb EVENT LOG
                           in-memory VIEWS → delta broadcast
                           single-writer SCHEDULER (contended state; fencing)
                           durable WORKFLOW engine (embedded, checkpoint-based)
                           tokio timers; axum HTTP/webhooks
                           does NOT perform host effects
   │ command/event protocol over a Transport:
   │   • iroh (remote nodes; NodeId = Ed25519 = node identity)
   │   • Unix domain socket (local nodes; peer = OS creds)
LAYER 2  Envoy             one Bun-or-Rust node agent per host; the ONLY host-level
                           process; the single trust boundary
                           PID supervision, port reservation, sandboxing, PTY
                           bridge, artifact serving, workdir lifecycle, skills/MCP/
                           cred materialization, vault sync, desired-state reconcil
   │ spawns + supervises
LAYER 3  Agents            many per node; orchestrators (Hermes/OpenClaw) +
                           workers (Claude Code/Codex/Droid) behind AgentRuntime
```

**Hard boundary:** orchestration/state → control plane; any host effect (process,
file, PTY, port, install) → that host's envoy. React/UI never import agent
internals. **All agents — orchestrators included — are Layer-3 host processes; no
agent loop runs inside the control-plane process** (§2.3). **Node identity is the
transport** (iroh `NodeId` / UDS peer creds) — no application-layer node token
(§2.5, §10.7).

## 3. Differentiators (vs Paperclip / Fusion)

1. **Correct, owned substrate** — single-writer scheduler over an append-only log
   gives the coordination correctness Fusion/Paperclip patch by hand — from
   topology, not distributed-ACID dependency. Shippable; all deps MIT/Apache.
2. **Interactive agency** — every session is a live, steerable, interruptible
   conversation (not Paperclip's heartbeat/ticket black boxes).
3. **Heterogeneous complementary runtimes** — provider harnesses for directive
   tasks, intelligent agents for orchestration; routed by task shape.
4. **Identity/context/session isolation** — one operator identity across many
   isolated contexts (personal vs corporate).
5. **Subscription-aware budgeting** — tracks finite subscription quotas (Claude
   Code Max, Codex, multiple API keys), not just token cost.
6. **Transport-native identity** — iroh NodeId / UDS creds = node identity; no
   second identity system; NAT traversal + encryption native.

## 4. Source-of-truth matrix (summary)

All "owned by control plane" rows are an append-only event type in the redb log
with a derived in-memory view.

| Concern | Owner |
|---|---|
| Operator auth, identities, contexts, projects | Control plane |
| Sessions, messages, tool-calls, streaming, utility inference | Control plane (log + views) |
| Cards / board | Control plane (`cards`) |
| Workflows, cron, webhooks | Control plane (embedded engine + tokio + axum) |
| Node registry, desired-state, host-command queue, leases, budgets, chat rooms, artifact/vault/skill-MCP index, FTS | Control plane |
| Process spawn/supervision, PTY, filesystem, workdir lifecycle, ports, sandbox, artifact bytes + blob store | Envoy |
| Skills/MCP/cred materialization to disk, vault sync, desired-state reconciliation (installs) | Envoy |
| Agent loop, context management, tools | The agent (Hermes etc.) — NOT Olympus |

## 5. Key models (→ ADR 0002 sections)

- **Identity / Context / Session / Project** (§3): identity crosses contexts;
  context is the boundary (credentials/workspace/network hard-isolated, memory
  convention-isolated); **a project belongs to one context, a session belongs to
  one context + optionally one project** (§3.1.1); session pinned to one node.
- **Transport & identity** (§2.5): iroh (remote, NodeId = identity) + UDS (local,
  peer creds); one wire protocol behind a `Transport` trait; browser↔control plane
  over WSS (the one unavoidable second transport).
- **Truth model** (§2.4): **event-sourced** — redb append-only log is truth;
  in-memory materialized views + search indices are derived, rebuildable
  projections.
- **Filesystem hierarchy** (§5): `~/olympus/{sessions,projects,skills,mcp,creds,
  system}/`. Main sessions flat; cards nest under their main session keyed by
  stable `card_id`; projects hold shared assets (vault, config, default
  activation). jj git-colocate + mandatory conflict guard. Orchestrator-only `gh`.
- **Cards / board** (§6): card = durable unit of work; 1:1 with a worker session;
  reassignment only on node-unreachable or agent-changed, forwarding prior trace
  as a "previous attempt" block; per-attempt jj bookmarks; task-based directory
  survives reassignment. Workflows operate on cards.
- **Node desired-state** (§7): control plane declares required programs; envoy
  reconciles (installs) + reports drift; skill/MCP library refresh is part of the
  same loop.
- **Knowledge vaults** (§8): project-scoped llm-wiki (incl. non-text refs); jj
  repo per vault; write = commit+push, read = pull on `vault.updated` webhook +
  cron backstop.
- **Skills / MCP** (§9): library (read-all for discovery) vs activation (scoped
  config, Claude-Code/Codex style). MCP = a skill-shaped "how to use a CLI/API".
- **Command/event protocol** (§10): `hostCommands` with atomic claim,
  `claimEpoch` fencing, lease expiry. The **single-writer scheduler** (§10.5) owns
  all capacity/lease/fencing transitions (`availableSlots` decremented on assign,
  released exactly once on terminal/sweep); two liveness timers — node heartbeat +
  command lease — with one `reap` sweeper (§10.6); node identity is the transport,
  allowlist + per-context grants authorize (§10.7). Host output streamed via
  `hostCommandOutput` → message view. **Reactive views + delta broadcast**
  (§10.3.1) replace Convex subscriptions.
- **Storage / compression / search** (§10A): redb truth + **zstd with a trained
  dictionary** + **tantivy FTS** (v1, derived index) + **content-addressed blob
  store** (blake3) for artifacts/non-text; **vector/semantic search deferred**
  (additive — needs embedding pipeline); message lifecycle (hot→warm→cold) bounds
  bloat while keeping sessions forever searchable.
- **Memory & observability** (§11): views are bounded caches (bulk is paged from
  redb/blob); **cgroups v2** unifies per-agent memory monitoring + limits
  (`memory.current` = observability, `memory.max` = cap); WSL cgroup-v2 caveat to
  verify early.
- **Inter-agent comms** (§13): local-first — SDK handles parent↔child, filesystem
  reads sibling↔sibling, control plane mirrors for observability. Control-plane-
  routed only as multi-node fallback.
- **Chat rooms** (§14): operator-created, observable, access-controlled channels
  between peer agents of differing access (capability bridges).
- **Workflows** (§15): generic n8n-like node/edge/template engine, embedded +
  checkpoint-based (Sayiir-MIT-or-built-equivalent over redb). Workflows orchestrate
  card sessions, never run agent code. GitHub CI/PR loops are one instance.
- **Budget/subscriptions** (§16): subscription-aware routing; scheduler reserves
  budget + selects a subscription with remaining quota atomically.
- **Artifacts** (§17): index of generated files, PRs, builds; content-addressed;
  workers produce, orchestrator publishes; ephemeral served by envoy, durable to
  object storage; lifecycle designed in.
- **SSH/terminal** (§18): envoy PTY ↔ xterm.js (over WSS); operator capability,
  audited.
- **AgentRuntime** (§19): an envoy-owned command queue (`start`/`send`/`events`/
  `stop`) — `send` takes `prompt`/`steer`/`cancel`/`stop`/`switchModel`/`slash`
  and each per-harness adapter maps them to that harness's native stdio protocol
  (Hermes = ACP over stdio). Orchestrator = same impl + control-plane tools. Prove
  the seam with a real 2nd impl early.
- **Node death** (§20): accept; card reset + recover-as-new-attempt seeded with
  trace; base case = `reapOrphanedMainSessions`. State correctness + execution
  durability guaranteed; execution continuity NOT.

## 6. Sandboxing and limits (§12)

- `Sandbox` interface: **HostDirect (default)**, **Bubblewrap** (isolation; `rm
  -rf` via `--ro-bind`, ports via `--unshare-net`), **Docker (deferred)** for
  reproducible toolchains / mutable env / GPU.
- Port reservation owned by the envoy. Worker egress default-deny with a
  per-context netns allowlist; orchestrator has full network.
- Three resource layers, all required: OS (cgroup v2, per session), harness
  (wall-clock/tokens, per run), control plane (cost/concurrency, per context).

## 7. Current state vs target

| Aspect | Current | Target |
|---|---|---|
| Product framing | "thin host-effect runtime" (ADR 0001) | fleet control plane (ADR 0002) |
| Substrate | ADR 0001/0002-draft: Convex | **Rust-native single binary** (ADR 0003) |
| Scaffold | ~613 LOC TS: apps/{web,runtime}, convex/{...}, packages/{...} | Rust control plane + envoy + AgentRuntime impls |
| Node model | single host implied | multi-node fleet, session-node affinity, desired-state |
| Workdir | none | Olympus-owned `~/olympus/` hierarchy with cards |

See ADR 0002 §23 for the 16-phase build order.
