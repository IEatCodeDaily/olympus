# ADR 0007 — Per-node agent discovery (envoy owns host facts)

Status: accepted · Date: 2026-07-04

## Context

Agent discovery (which Hermes profiles + CLI harnesses like claude/codex are
available) was performed by the **control plane** probing its **own host's**
PATH and `~/.hermes`, then presented as fleet-wide truth via `GET /api/agents`.

This violates the ADR 0002 §2.1 layer boundary: *host-level facts belong to
Layer 2 (the envoy), never the control plane*. Concretely it was wrong because:

- The agent list was identical for every node — it always reflected the control
  plane host, not the node you were looking at. A remote node without codex
  would still show codex.
- Discovery was cached for the process lifetime (`OnceLock`), so installing a
  new agent never showed up without a restart.
- `NodeInfo` carried no agent list; the envoy hello/heartbeat reported no
  capabilities.

## Decision

**Each node's envoy discovers its own agents and reports them; the control
plane aggregates per-node. Refresh is manual.**

1. **`NodeInfo.agents: Vec<AgentInfo>`** — every node carries its own
   envoy-discovered agent list. This is the per-node source of truth.

2. **The local node runs its envoy in-process.** At boot the control plane
   calls `agents::discover_local_agents()` (fresh probe of this host's
   `~/.hermes` profiles + PATH CLI harnesses) and registers the `local` node
   with that list. There is no separate discovery path for "the control plane";
   the control plane simply hosts the local node's envoy.

3. **Remote envoys report their agents** in the registration handshake
   (`NodeRegistry::register(..., agents)`) and can re-report via
   `set_agents`. Until a remote envoy reports, its agent list is empty (honest —
   no phantom agents).

4. **Discovery is NOT cached and NOT automatic.** The `OnceLock` cache is gone.
   Re-detection is an explicit operator action:
   - `POST /api/nodes/:id/agents/refresh` — local node re-probes in-process;
     remote nodes return `501` until the standalone envoy binary exists.
   - Surfaced in the UI as **Fleet › Agents › "Detect agents"** per node.

5. **API shape**
   - `GET /api/nodes` — each node includes its `agents[]`.
   - `GET /api/nodes/:id/agents` — one node's agents.
   - `POST /api/nodes/:id/agents/refresh` — manual re-detect.
   - `GET /api/agents` — flat union across all nodes (dedup by id), kept for
     backward compat; sourced from the registry, not a live probe.
   - `GET /api/agents/:id/models` — models scoped to an agent's provider
     (unchanged; keeps the composer model selector agent-specific).

## Consequences

- Agent availability is now honest per node. A node without codex won't show it.
- `list_agents()` still exists but is documented as the local-host fallback /
  model-list source, not fleet truth. The registry is the truth.
- **Follow-up (not in this ADR):** a standalone `olympus-envoy` binary that
  runs on remote hosts, discovers agents there, and reports over the UDS/iroh
  transport — plus remote agent *installation* ("install agent" in Fleet). The
  in-process local envoy already implements the discovery contract the remote
  binary will speak, so the split is mechanical.
