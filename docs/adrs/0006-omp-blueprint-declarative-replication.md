# ADR 0006: omp-as-blueprint — declarative agent replication + observable orchestration

- Status: Accepted (with five flagged-for-veto open questions in §7)
- Date: 2026-07-02
- Builds on: **ADR 0005** (org-scoped local-first resource model), **ADR 0003**
  (Rust substrate), **ADR 0004** (vaults markdown-first + jj). Does not
  supersede any of them; it *sharpens the product thesis* and commits to
  concrete node-agent design borrowed from omp.
- Relates to: ADR 0002 §2 (layer boundary — reaffirmed), §6 (cards),
  §7 (desired-state), §9 (skills/MCP libraries).

> **Amended by ADR 0027 (2026-07-19).** The session tree remains a Hall-owned
> typed graph, but its session spaces are all flat siblings under
> `~/.olympus/<org>/sessions/`. No subagent directory nests inside its parent.

> **Product thesis (the one sentence):** the *agent setup* is the replicable
> unit; Olympus fans it out across nodes and makes it observable. Everything
> below follows from that.

## 1. Context: omp is the reference for the node-agent layer

`omp` (https://omp.sh — a fork of Mario Zechner's Pi) is a terminal-first
coding agent: single process, any provider, sessions that branch like git, an
in-process IRC bus for subagent-to-subagent messaging, structural (hashline)
edits, in-process LSP/DAP, plan/goal modes, skills/hooks/MCP/plugins. It is a
near-exact reference for **what a single node's agent experience should be.**

It is categorically **not** a control plane: no concept of a node, an org
boundary, an envoy, cross-host orchestration, or an append-only audit log. Its
`task` fan-out spawns child processes *on one machine* — concurrency, not a
fleet.

**Decision: adopt omp as the blueprint for Layer 3 (the node-agent experience)
and build Layers 1–2 (control plane + replication) ourselves.** We steal omp's
*models and semantics*, not necessarily its code, and we diverge where our
substrate demands it (jj not git; cross-node not in-process).

## 2. What we steal from omp (decided)

| omp feature | What we take | Divergence |
|---|---|---|
| **Session tree** (`/branch`, `/fork`, `/tree`, JSONL leaves) | The branch/fork/tree model over sessions. A branch is a new leaf from a prior message; a fork is a new session; the graph is walkable. | Our sessions are event-log-backed and every session space is flat under the org's `sessions/` directory (ADR 0027), not per-process JSONL or nested directories. The graph is a Hall projection, not a local file. |
| **IRC bus** (peers DM each other, idle/parked-wake, `list`/`send`/`wait`/`inbox`, `await`) | The *semantics* wholesale — this is the inter-agent comms model, replacing ADR 0002 §6's "previous-attempt-block" forwarding as the primary channel. | omp's bus is in-process; ours must work **cross-node over iroh** (see §7 footgun 2). |
| **`task` fan-out** (parallel child sessions, isolated workspaces, results as `agent://`) | Parallel subagent dispatch with per-task isolation. | Maps onto typed `agent_spawn` edges plus independent flat session spaces (ADR 0027) and bwrap isolation (ADR 0005 §4.3), not omp's COW-clone overlay. |
| **Plan / goal modes** | Sandboxed planning turn against a planner model; approve → execute/keep/compact. | Straight adoption. |
| **Structural (hashline) edits** | Content-hash line anchors for edits with stale-anchor recovery. | Must be re-verified against jj's conflict model (see §7 footgun 1). |
| **GitHub-as-filesystem** (`read pr://`, `issue://`) | The virtual-read pattern is interesting and adoptable. | We keep **jj** as the VCS layer (ADR 0004); the `read <scheme>://` virtualization is orthogonal to git-vs-jj and can coexist. |
| **Skills / hooks / MCP / plugins / marketplace** | The extensibility surface. | **Re-scoped to org/project-declared, envoy-materialized** — see §3. This is where Olympus adds its core value. |

## 3. The replication layer (Olympus's core differentiator)

This is the part omp has **zero** of, and the reason Olympus exists.

**Plugins, skills, MCP servers, and hooks are declared at org/project scope in a
manifest. The envoy materializes them onto each node and into each session
space.** Declare the dev environment once → Olympus reproduces it on N nodes →
fan out work → every node speaks the same tool vocabulary.

### 3.1 The declaration manifest (the load-bearing artifact)

A project (and/or org) carries a declaration of its required agent setup:

```json
// ~/.olympus/<org>/projects/<project_slug>/project.json  (setup section)
{
  "setup": {
    "skills":  ["code-review", "systematic-debugging"],
    "mcp":     ["gitnexus", "grafana"],
    "plugins": ["lsp-rust", "lsp-typescript", "codegraph"],
    "hooks":   ["pre-commit-verify"]
  }
}
```

- **Org-level** setup applies to every session in the org (baseline: e.g.
  every node needs gitnexus).
- **Project-level** setup layers on top (this project also needs the Rust LSP).
- The union is what the envoy must materialize before a session in that
  project/org can run.

### 3.2 Plugins are first-class, `kind`-discriminated

Per ADR 0005 §3, a plugin declares its lifecycle, because the envoy's duty
differs:

- **`kind: install`** — a host-mutating, idempotent installer (gitnexus, a CLI,
  an LSP server binary). The envoy runs it once and reconciles desired-state
  (ADR 0002 §7): if the node doesn't have it, install it; if present, skip.
- **`kind: service`** — a supervised long-running process (an MCP server, a
  receipt/CRM subsystem). The envoy supervises it: ports, health, restart.

### 3.3 Layer boundary is preserved (non-negotiable)

Plugins/skills/LSP/codegraph are **things the agent uses**, materialized by the
**envoy** into the session space — **never run by the control plane** (ADR 0002
§2). The control plane records *what should be installed*; the envoy *installs
and supervises*. The moment the control plane runs an LSP, the architecture is
broken.

### 3.4 Skills/MCP libraries → per-scope activation (ADR 0002 §9 reaffirmed)

The `~/.olympus/<org>/plugins/`, and the skill/MCP libraries, hold *all* managed
artifacts; the manifest's `setup` block is the *activation* list referencing the
library. Materialization = "make the declared subset present and active in this
session space."

## 4. The orchestration layer: kanban as the observable spine

The kanban board is **kept and load-bearing** — it is what makes a fleet of
omp-like runtimes observable. omp has no kanban; this is control-plane value.

- **A card owns a session tree.** The card is the unit of work; the session tree
  (main + sub-sessions) is how the work is executed. The card→session-tree link
  lives in the **event log** (durable, single-writer), not in any node's local
  session files.
- At a glance the board shows: which card, which node, which agent, which
  state (todo/running/blocked/done), and — drilling in — which sessions
  (and sub-session tree) belong to it.
- This makes "fan out development across nodes" observable: N cards running on
  N nodes, each with its own agent setup materialized from the same manifest.

## 5. How the layers compose (the whole picture)

```
                 ┌───────────────────────────────────────────────┐
   Layer 1       │  OLYMPUS control plane (Rust, single binary)   │
   control plane │  event log · views · scheduler · KANBAN spine  │
                 │  owns: org/project/session/plugin DECLARATIONS  │
                 └───────────────────────────────────────────────┘
                        │ records intent (never runs host effects)
                        ▼   iroh / UDS transport
                 ┌───────────────────────────────────────────────┐
   Layer 2       │  ENVOY (one per org per node)                  │
   replication   │  MATERIALIZES the declared setup:              │
                 │   install-plugins · supervise-services ·       │
                 │   clone repos → jj workspaces · bwrap sandbox   │
                 │  reconciliation sqlite (what's on this disk)    │
                 └───────────────────────────────────────────────┘
                        │ spawns + supervises
                        ▼
                 ┌───────────────────────────────────────────────┐
   Layer 3       │  AGENT (omp-blueprint: Hermes/omp/Claude/…)    │
   node agent    │  session tree · IRC bus · plan/goal · skills · │
                 │  LSP/codegraph (materialized plugins) · edits   │
                 └───────────────────────────────────────────────┘
```

The **declaration manifest** (Layer 1 record) → **materialization** (Layer 2
envoy) → **agent uses the tools** (Layer 3). Replicate the manifest to a new
node and the same agent setup reproduces there. That is the product.

## 6. Consequences

- **Gained:** a clear, honest product identity — "declare an agent setup once,
  replicate + fan out + observe across a fleet." Neither omp (single node) nor a
  generic orchestrator has this.
- **Gained:** we don't rebuild the hard node-agent UX from scratch; omp is the
  proven reference (and possibly an embeddable/derivable runtime).
- **Gained:** the kanban stops being "a board we happen to have" and becomes the
  observability layer over the fleet — the thing that answers "what is running
  where."
- **Cost:** the IRC bus and session tree, trivial in-process for omp, are
  genuine distributed-systems work for us (cross-node transport, partial
  connectivity, durable messaging). See §7.
- **Cost:** the declaration→materialization→reconciliation loop is real desired-
  state engineering (install idempotency, service supervision, failure modes).
- **Cost:** stealing omp's git-shaped edit tooling onto jj needs verification,
  not assumption.

## 7. Flagged-for-veto open questions (real engineering under these)

These are **decided in direction but carry a genuine engineering question.**
Each must be resolved with a spike or design before the corresponding code
lands. Do not let a swarm worker build past one of these on an assumption.

1. **jj vs omp's git-shaped edit engine.** omp's hashline structural edits and
   `read pr://…/diff` assume git semantics. jj has first-class conflict commits
   git cannot represent (ADR 0002 §5.4 hazard: `git status` reads clean while
   `jj log` shows an unresolved conflict). **Question:** do omp's edit/diff
   primitives port cleanly to jj-colocated-with-git, or do we need a
   jj-conflict-detection guard before an agent reads a worktree? **Resolve
   with:** a spike editing a file in a jj workspace via the omp edit model and
   deliberately inducing a jj conflict.

2. **Cross-node IRC.** omp's IRC is in-process (all peers are children of one
   Bun process). We want the same semantics (DM, `await`, idle/parked-wake,
   peer list) **over iroh between nodes.** **Question:** peer discovery, partial
   connectivity, parked-peer revival across hosts, and message durability when a
   peer is offline — what's the minimum viable design? **Resolve with:** a
   design doc + spike of two envoys exchanging IRC messages over iroh, including
   an offline-peer case.

3. **Card ↔ session-tree consistency.** Sessions branch/fork/sub-session into
   trees. **Question:** does a card own the whole tree or a leaf? When a worker
   forks mid-card, does the fork attach to the card or start a new one? When a
   node dies mid-card, where does the orphaned tree land? **Resolve with:** an
   event-schema design for the card↔session-tree link (which events, which
   invariants) before wiring it.

4. **Declaration → materialization failure modes.** A project declares
   gitnexus + an LSP; node X lacks them. **Question:** fail-closed before the
   session runs (block), or degrade (run without the tool + warn)? How is a
   half-materialized setup reconciled after a crash mid-install? **Resolve
   with:** the desired-state state machine (declared → materializing → ready →
   failed) and its transitions.

5. **Plugin trust / supply chain.** Plugins are `kind: install` host-mutating
   scripts and `kind: service` long-running processes, replicated to every node.
   **Question:** what stops a malicious or broken plugin declaration from
   compromising a node? Signing? An allowlist? Sandbox-by-default for services?
   **Resolve with:** a trust model before the marketplace/authoring path opens.

## 8. Immediate build implications

We already have the substrate (event log + scheduler, ADR 0003), the org-scoped
session model (ADR 0005 §4), and the kanban board. This ADR **adds**, in
dependency order:

1. **The declaration manifest** at org/project scope (the `setup` block) +
   its event-log records. *(No footgun blocks this — it's pure control-plane
   record-keeping.)*
2. **Envoy materialization** of the declared setup (install/supervise) — gated
   on footgun 4's state machine.
3. **Session-tree events** in the log (branch/fork/sub-session) + the card link
   — gated on footgun 3.
4. **IRC bus** — in-process first (single-node), then cross-node — gated on
   footgun 2 for the cross-node step.
5. **omp edit/diff model on jj** — gated on footgun 1's spike.

Start at (1): it's unblocked, it's the load-bearing new artifact, and everything
else references it.

## 9. Harness adapter design (grounded — verified 2026-07-02)

Increment 1 (the declaration manifest) shipped (commit `0245a03`): a scope
declares slug lists, org+project merge to an effective set, durable across
restart. It is currently **inert** — nothing consumes it. Increment 2
(materialization) is what makes it real, and it forced the real design question,
which is bigger than footgun 4's state machine:

> **The materialization *target* is harness-specific.** The declaration is
> harness-AGNOSTIC (slugs); each agent (Hermes / Claude Code / Codex) reads its
> config from a different place in a different format. So the envoy's
> materialization is a **per-harness adapter**: given the effective declared
> setup and the session's locked agent kind, render the config the way THAT
> harness expects — *into the session space*, never into a shared profile.

### 9.1 Decisions (operator-ratified)

- **Harness-specific adapter is the real design.** Footgun 4's state machine is
  a sub-concern inside it.
- **Agent kind is locked at session creation.** Branch/fork/tree inherit the
  original agent. Switching agents happens ONLY via a **handover** (its own
  increment). So the adapter runs at session creation (locked kind) and at
  handover (target kind) — never on branch/fork.
- **Design the adapter interface up front.** First targets: Hermes, Claude
  Code, Codex.
- **Merge mode:** Hermes = union (Olympus adds on top of profile defaults).
  Claude Code / Codex = configurable (override vs union) where the harness
  allows it.

### 9.2 Verified config surfaces (the load-bearing facts)

| Harness | Skills path | MCP config | Settings/Hooks | Context file | Session-scope lever |
|---|---|---|---|---|---|
| **Hermes** | `~/.hermes/skills/` + profile (union) | ACP `session/new` `mcpServers` param (bridge sends `[]` today — just populate it) | profile | injected | ACP params + env; no profile mutation |
| **Claude Code** | `.claude/skills/<name>/SKILL.md` (**reads from cwd**) | `.mcp.json` (**cwd**) | `.claude/settings.json` (cwd) | `CLAUDE.md` (cwd) | **the session-space cwd, natively** |
| **Codex** | skills dir / `AGENTS.md` (cwd) | `[mcp_servers.*]` in `config.toml` (**STDIO only**) | Codex hooks | `AGENTS.md` (cwd) | **`CODEX_HOME` env → session-local config dir** |

**The convergent win:** all three scope by the **session space**. Claude Code
and Codex read their config from the cwd / a `CODEX_HOME` dir; Hermes takes
per-session ACP params. Writing config INTO the session space is the native
path for two of three and the safe path for all three. This avoids the Hermes
Studio cross-contamination original sin (ADR 0002 §1.1) **by construction** —
materialization never mutates a shared profile.

### 9.3 Skills are portable as-is (footgun-2-of-materialization dissolved)

Verified: all three harnesses use the **same skill shape** — a per-skill
directory with `SKILL.md` (YAML frontmatter `name`/`description` + markdown
body). Claude Code: `.claude/skills/<name>/SKILL.md`; Hermes: same; Codex: same.
So a skill is portable content; the only per-harness difference is the *install
path*. The "renderer" for skills is therefore **copy the skill dir to the
harness's path**, not a schema translation. No canonical-form + heavy-renderer
layer is needed. (Hooks remain harness-specific and do NOT port; MCP is a
protocol standard `{command,args,env}` and ports cleanly to all three.)

### 9.4 The registry dependency (new unblocked increment, before the adapter)

Increment 1 stores slug *lists* (`mcp: ["gitnexus"]`). A slug alone can't be
materialized — nothing yet resolves `"gitnexus"` → its MCP `{command,args,env}`,
or a skill slug → its `SKILL.md` directory. So the chain is:

```
registry (resolve slug → definition)  →  adapter (render def into harness config)  →  spawn (point runtime at it)
```

The **registry** is the unblocked foundational piece — same character as
increment 1 (pure control-plane record-keeping, references the skill/MCP/plugin
libraries of ADR 0002 §9 and the resource dirs of ADR 0005 §4). The **adapter**
is the next layer and is where footgun 4's state machine (declared →
materializing → ready → failed) actually lives.

### 9.5 The adapter trait (interface, first cut)

```rust
enum AgentKind { Hermes, ClaudeCode, Codex }
enum MergeMode { Union, Override }   // Hermes forces Union

struct SetupCapabilities {  // per harness — drives drop-with-warning vs fallback
    skills: Support, mcp: Support, hooks: Support, plugins: Support,
}
enum Support { Native, FallbackToContext, Unsupported }

/// Rendered result the runtime factory applies when spawning.
struct SpawnOverlay { env: Vec<(String,String)>, args: Vec<String> }

trait SetupAdapter {
    fn agent_kind(&self) -> AgentKind;
    fn capabilities(&self) -> SetupCapabilities;
    /// Render the resolved (registry-looked-up), org+project-merged setup into
    /// the session space; return the env/args the runtime needs. Never mutates
    /// a shared profile.
    fn materialize(&self, resolved: &ResolvedSetup, space: &Path, mode: MergeMode)
        -> Result<SpawnOverlay>;
}
```

Capability fallback: skills/context that a harness can't natively activate
degrade to prose appended to its context file (`CLAUDE.md`/`AGENTS.md`);
things that can't degrade (e.g. an LSP plugin on a harness without LSP) are
**dropped with a surfaced warning** so the operator sees "declared X, harness Y
can't do it." MCP maps cleanly everywhere (protocol standard).

### 9.6 Resequenced build order (supersedes §8's list from increment 2 on)

1. ✅ Declaration manifest (done, `0245a03`).
2. **Registry**: resolve slug → definition for skills / MCP / plugins. Unblocked,
   pure control-plane. *(next)*
3. **Adapter trait + Hermes adapter** (first concrete impl; populate ACP
   `mcpServers`, session-scoped skills). Includes footgun 4's state machine.
4. **Claude Code + Codex adapters** (render into session-space cwd / `CODEX_HOME`).
5. Session-tree events + card link (footgun 3).
6. Handover (the sole cross-harness transition — its own increment; translates
   history to the target harness's transcript format + re-materializes setup).
7. IRC bus (in-process, then cross-node — footgun 2).
8. omp edit/diff model on jj (footgun 1 spike).

