# ADR 0016 — Single-Hall, durable-Envoy architecture (ratified)

- Status: **Accepted (normative)**
- Date: 2026-07-23
- Ratifies: the single-Hall / durable-Envoy target the fleet has converged on.
  The task brief referenced "ADR 0033"; no such file exists (the ADR series
  runs 0001–0015). This ADR takes the next real number and is the authoritative
  statement of the topology.
- Supersedes / narrows (see §7 for the exact list): ADR 0002 §10 (command
  lease/claim/sweeper multi-claimant machinery), ADR 0006 (omp-as-blueprint
  cross-node *replication* layer), and the "iroh is mandatory / node-identity-
  is-the-transport" framing in ADR 0003 and ADR 0011 §2.
- Reaffirms: ADR 0008 (Hall/Envoy process split), ADR 0009 (SQLite substrate),
  ADR 0010 (Hall auth authority), ADR 0014 (external reverse proxy edge).
- Relates to implementation tasks: `t_b671e437` (Hall storage + identity +
  backup), `t_dd8765f0` (Envoy single-Hall pinning + local takeover),
  `t_785b04c6` (user/system Envoy daemon tiers), `t_18eda180` (integration gate).

## 1. Doctrine (the one sentence)

> **Exactly one unprivileged Hall process per Olympus network is the single
> source of authority; Envoys are durable, SSH-like execution agents that each
> pin exactly one Hall, survive Hall downtime, and are the authority for their
> own active sessions and job runs. No Raft, no leases, no recovery PKI, no
> automatic failover, no Iroh requirement.**

Everything below follows from that sentence.

## 2. What this rules IN

- **One Hall.** A single `olympus-hall` process owns: users, organizations,
  grants/memberships, workflow configuration, the canonical vault, apps/plugins,
  Envoy enrollment, and the global query/search projections. It is the sole
  writer of its own event log and the only authority for that state. It runs
  **unprivileged** (systemd `--user` by default; a root system install is
  permitted but the process still drops to a service account — Hall never needs
  host root to do its job).
- **Storage choice, one API.** SQLite is the default and the only requirement
  for local/single-host operation. PostgreSQL is a supported optional backend
  behind the same application storage seam. In PostgreSQL mode the *same* Hall
  process talks to a networked database — this is a storage swap, **not** a
  second Hall and **not** clustering. PostgreSQL mode must provide native
  full-text (lexical) search via PostgreSQL FTS and semantic/hybrid search via
  `pgvector`; pgvector is the semantic layer, never a replacement for lexical
  FTS (per the design in `docs/design/postgres-rag-memory.md`).
- **Traditional web transport UI→Hall.** The UI reaches Hall over ordinary
  HTTP / WebSocket / RPC. Public exposure is a plain reverse proxy or Cloudflare
  Tunnel in front of Hall (ADR 0014); Hall must behave correctly behind such an
  origin (exact-origin / forwarded-origin handling per ADR 0010). No overlay
  network is required for a browser to reach Hall.
- **Hall identity outside the DB.** Hall has an Ed25519 identity keypair. The
  **private key is a `0600` file (or OS secret) stored outside the database**;
  the public key IS the Hall ID. Backup captures the DB *and* the identity key
  together — restoring a DB without its identity key produces a Hall that every
  Envoy will correctly reject.
- **Envoy pins one Hall.** An Envoy persists exactly one pinned Hall public key
  plus its own identity, both outside the Hall DB. First enrollment uses a
  short-lived single-use token whose hash Hall stores. The same Hall key may
  reconnect and update its endpoint freely. A **different** Hall key is rejected
  over every network protocol — remote takeover is impossible by construction.
- **Durable, SSH-like Envoy.** An Envoy is a long-lived execution agent, one
  installation per host (ADR 0008 spools + resume). It is the **authority for
  its own active sessions and job runs**: while Hall is down, held sessions stay
  live, work continues, and events buffer to disk. Hall relearns runtime
  locations *from the Envoys* on reconnect (ADR 0008 §2). Envoy is to Hall what
  an `sshd` is to an operator: it holds the live process, outlives the
  controller's restarts, and answers only to its pinned authority.
- **Local-only key replacement.** Re-pinning an Envoy to a new Hall is a
  **local** operation over the Unix socket / CLI, authorized by OS peer
  credentials (owning user for a user Envoy, root for a system Envoy), with
  interactive fingerprint confirmation or the `--replace-hall
  --expected-old-hall` automation flags. Replacement drains/stops active work,
  retains old history read-only, clears old credentials/policy, and pins the new
  key atomically.
- **Two Envoy daemon tiers.** *User tier*: `systemd --user`, XDG
  config/state/runtime paths, rootless workloads, no sudo escalation. *System
  tier*: root-installed system service with a host-scoped capability ceiling,
  running under a dedicated service account with narrow helpers; workloads stay
  unprivileged by default. Exactly one installation per host per tier; the
  installer detects healthy/stopped/partial installs, refuses duplicates, and
  supports explicit repair/migrate/uninstall.
- **Bounded recovery.** Hall recovers by restoring its DB backup (+ identity
  key), then reconciling *active execution observations* from the Envoys that
  reconnect. Worst case — backup lost — is an **explicit, limited Envoy salvage
  path** (recover what live Envoys still hold), never magical reconstruction of
  Hall-only authority. There is no automatic failover and no standby Hall.

## 3. What this rules OUT (and why)

| Removed | Why |
|---|---|
| **Raft / consensus / election** | One Hall writer. Nothing to elect, no quorum to maintain. |
| **Command leases, `claimedBy`, claim epochs, sweeper** (ADR 0002 §10) | Leases arbitrate *multiple* claimants competing for work. With one Hall scheduler and Envoys that own their own runs, the coordinator is the single Hall; the claim/lease/sweeper timers solve a problem we no longer have. |
| **Multi-Hall replicas / replication layer** (ADR 0006 core) | Replication existed to fan a declared setup across many control-plane peers. The topology is one Hall + many Envoys; fan-out is Hall→Envoy dispatch, not Hall↔Hall replication. |
| **Recovery PKI / cert hierarchy** | Trust is one pinned Ed25519 key per Envoy. A CA/rotation/revocation hierarchy is unneeded ceremony for a single fixed authority. |
| **Automatic failover / standby Hall** | Recovery is restore-from-backup + Envoy reconciliation, an operator action. No hidden second writer can diverge. |
| **Mandatory Iroh overlay** (ADR 0003, ADR 0011 §2) | Iroh QUIC is now *optional* transport for remote Envoys, not a requirement. UI→Hall is plain HTTP/WS behind a reverse proxy; local Envoy↔Hall is the Unix socket; remote Envoy↔Hall MAY use Iroh or any authenticated tunnel. Identity is the pinned key, not the transport. |

Note: the ADR 0006 *node-agent experience* borrowings (session tree, IRC-bus
semantics, plan/goal modes, structural edits, declared skills/MCP materialized
by the Envoy) are **retained** — only its Hall-to-Hall *replication* framing is
withdrawn. Declared-environment materialization is now strictly a one-Hall →
per-Envoy push, which is exactly what an SSH-like durable Envoy already does.

## 4. Ownership boundary (authority map)

```
HALL (one, unprivileged)                 ENVOY (one per host, durable)
── authority for ─────────────           ── authority for ─────────────
users, orgs, grants, roles               its live agent sessions
workflow / trigger config                its active job runs
canonical vault                          local runtime table + ACP children
apps / plugins registry                  on-disk event spools (buffer Hall down)
Envoy enrollment (token→pin)             its own identity key (0600, off-DB)
global query / search projections        its one pinned Hall public key (off-DB)
event log (sole writer) + identity key   session-space / workspace filesystem I/O
```

Rule of thumb: **Hall owns *what should exist and who may touch it*; each Envoy
owns *what is running right now on its host*.** Neither reconstructs the other's
authority — they reconcile it.

## 5. Backend dataflow

```text
                          ┌─────────────────────────────────────────────┐
   Browser / Desktop UI   │  HALL  (unprivileged, single writer)         │
   ───────────────────►   │                                             │
     HTTP / WebSocket     │   auth.sqlite ──── users / orgs / sessions   │
   (via reverse proxy or  │        │            (ADR 0010, security truth)│
    Cloudflare Tunnel)    │        ▼                                     │
                          │   event log (SQLite default | PostgreSQL)    │
                          │        │   append + projection = 1 txn       │
                          │        ▼                                     │
                          │   projections ── FTS (SQLite FTS5 |          │
                          │        │          PostgreSQL FTS + pgvector) │
                          │        ▼                                     │
                          │   trigger scheduler (dispatch, NOT executor) │
                          │   identity key  ◄── 0600 file, OFF-DB        │
                          └───────┬──────────────────────────────┬───────┘
                                  │ dispatch (HTTP/WS/RPC;        │ enrollment
                                  │ UDS local, Iroh optional      │ token→pin
                                  │ for remote) — Envoy pins      │
                                  │ Hall pubkey; wrong key = deny │
              ┌───────────────────▼──────────┐        ┌───────────▼──────────────┐
              │ ENVOY (user tier)            │        │ ENVOY (system tier)       │
              │ systemd --user, XDG paths    │        │ root svc, dedicated acct  │
              │ rootless workloads           │        │ host-scoped ceiling       │
              │                              │        │                           │
              │ pinned Hall pubkey (off-DB)  │        │ pinned Hall pubkey (off-DB)│
              │ own identity key (0600)      │        │ own identity key (0600)   │
              │ per-session disk spool ──────┼── buffers events across Hall down  │
              │ ACP children (hermes/claude/ │        │  ...                      │
              │  codex) = live sessions      │        │                           │
              └──────────────────────────────┘        └───────────────────────────┘

Recovery:  restore Hall DB + identity key ► Envoys reconnect (pin still valid)
           ► Hall reconciles live runtime tables from Envoys (ADR 0008 §2)
           ► worst case (backup lost): explicit limited Envoy salvage, never
             silent reconstruction of Hall-only authority.

Takeover:  remote "become your Hall" with a different key  ─► REJECTED (all nets)
           local re-pin over UDS w/ OS peer creds + fingerprint ─► ACCEPTED
             (drain old work ► retain old history read-only ► clear old creds
              ► pin new key atomically)
```

## 6. Consequences

- The distributed-systems surface collapses: no consensus, no lease timers, no
  election, no split-brain. The only "cluster" fact is "one Hall, N Envoys, each
  Envoy trusts one Hall."
- Availability of *running work* comes from Envoy durability (spools + resume),
  not from Hall redundancy. Hall can restart or be restored without dropping
  live sessions.
- Security is a single pinned Ed25519 relationship per Envoy plus OS peer-cred
  authority for the one privileged action (local re-pin). Remote compromise
  cannot re-home an Envoy.
- PostgreSQL becomes a *storage/RAG* option for one Hall, decoupled from any
  clustering promise. The dual-backend test burden (SQLite ⇔ PostgreSQL FTS
  parity + pgvector) is the accepted cost, gated by `t_18eda180`.
- The reverse-proxy edge (ADR 0014) is the only public-exposure story; Hall
  never terminates its own TLS for public ingress.

## 7. Migration / deletion list

Documentation (this ADR authoritative on conflict):

1. **ADR 0002** — add a header note: **§10 (command lease / `claimedBy` /
   claimEpoch / heartbeat-lease-sweeper three-timer machinery) is SUPERSEDED by
   ADR 0016.** The single-writer event log, envoy layer boundary, and scheduler-
   as-dispatcher remain in force.
2. **ADR 0006** — add a header note: its **cross-node replication layer (§3
   framing as Hall↔peer replication) is SUPERSEDED by ADR 0016**; the node-agent
   experience borrowings (session tree, IRC-bus semantics, plan/goal, structural
   edits, declared-env materialization) are retained as one-Hall→per-Envoy push.
3. **ADR 0003 / ADR 0011 §2** — amend "iroh is mandatory / node identity is the
   transport" to "Iroh is an *optional* remote-Envoy transport; UI→Hall is
   HTTP/WS behind a reverse proxy; identity is the pinned Ed25519 key, not the
   transport."
4. `docs/design/postgres-rag-memory.md` — reclassify from "exploration" to
   "accepted optional backend per ADR 0016" once `t_b671e437` lands the real
   PostgreSQL + pgvector path.
5. `docs/architecture/architecture.md` and the roadmap Status Ledger — update
   any multi-Hall / Raft / failover / lease prose to the single-Hall model.

Code (executed by the implementation tasks, deletion verified in `t_18eda180`):

6. Remove command-lease / claim-epoch / sweeper scheduler code paths (ADR 0002
   §10 machinery) — no multi-claimant arbitration remains.
7. Remove any multi-Hall replication / Hall-peer code and config surfaces.
8. Make Iroh transport optional behind the existing `Transport` seam; ensure
   local UDS and HTTP/WS paths do not require an Iroh endpoint to exist.
9. Land: Hall identity key (off-DB, `0600`), SQLite-safe backup (backup API /
   WAL snapshot, not live file copy) + identity in the backup set, PostgreSQL
   storage + FTS + pgvector behind the storage seam (`t_b671e437`); Envoy single-
   Hall pin + local-only re-pin (`t_dd8765f0`); user/system daemon installer
   tiers with duplicate refusal (`t_785b04c6`).

Deletions are staged so `make verify` is green at each step (ADR 0008 §6
discipline). No new architecture is introduced by the integration task — it only
merges, deletes stale machinery, and proves the two modes (local + full).
