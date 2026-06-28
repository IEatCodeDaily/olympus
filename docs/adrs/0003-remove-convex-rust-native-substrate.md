# ADR 0003: Remove Convex — Rust-native control-plane substrate

- Status: Accepted
- Date: 2026-06-28
- Amends: ADR 0002 (which is rewritten to its Rust-native form in the same change)
- Relates to: ADR 0001 (the original Convex+React+Bun stack choice is now
  superseded on the backend substrate; React for the UI and Bun/TS for tooling
  remain acceptable but are not load-bearing decisions here)

## Context

ADR 0002 specified self-hosted **Convex** as Layer 1: the source of truth,
reactive subscriptions, the Agent component (sessions/messages/streaming),
the Workflow component (durable execution), transactional state, scheduled
functions, and HTTP actions. Across the design conversation two problems with
that choice became decisive:

1. **Networking & identity friction.** Olympus wants transport-native, secure
   node↔control-plane connectivity with cryptographic node identity (iroh /
   WireGuard-style direct tunnels for remote nodes; local sockets for co-located
   nodes). Convex wants to be the connection terminus with its own application
   -layer auth (JWT to a central websocket). Running Convex *over* a secure
   tunnel leaves you operating **two identity systems** (transport keys + Convex
   auth), and the node-identity/fingerprint model has to be hand-rolled at the
   app layer anyway (ADR 0002 §10.7's `nodeToken` dance existed only because the
   transport couldn't provide identity).

2. **The substrate solves a problem we don't have, and adds ones we do.**
   Convex's headline feature is distributed ACID across many independent
   writers. Olympus's control plane is a **single authority**: nodes *report*
   and *propose*, only the control plane *assigns*. With one writer, the
   correctness we need (no double-claim, no slot leak, fencing) comes from
   **single-writer serialization** (topology), not distributed transactions
   (substrate). Meanwhile Convex brings: a critical-dependency connection
   terminus (SPOF), GC/runtime characteristics we don't control, and — for a
   product we intend to **ship** — an external backend to operate.

Additionally, several otherwise-attractive turnkey substrates were rejected on
**licensing** (we must ship a closed product without litigation risk):
SpacetimeDB reintroduces the same DB-as-server identity friction; Restate and
n8n and Windmill are BSL / fair-code / AGPL respectively (see ADR 0002 §23
Rejected alternatives).

## Decision

**Remove Convex entirely. Build the Layer-1 control plane as a Rust-native,
single-binary process** with these parts:

- **Truth:** an append-only **event log** in **redb** (pure-Rust embedded ACID
  KV; no server). Event-sourced because the log *is* the auditable history the
  product requires.
- **Reactive views:** in-memory **materialized views** projected from the log
  (the cache + reactivity layer); changes diffed and pushed as **deltas** to
  subscribers. "User-view correctness," not distributed-transaction correctness.
- **Serialization / scheduler:** a **single-writer** scheduler owns the
  contended state (slots, assignment, fencing epochs). Group-commit fsync for
  durable throughput. Partition-by-scope (org/project/session writers) is the
  documented forward path; single-writer is sufficient for v1 (>100k mutations/s
  headroom; the real ceiling is fsync, solved by group commit).
- **Transport & identity:** **iroh** for remote nodes (endpoint = Ed25519
  keypair; `NodeId` = the public key → node identity is the transport, no
  separate token), **Unix domain socket** for local/co-located nodes (peer
  authenticated by filesystem permission / `SO_PEERCRED`). One wire protocol
  behind a `Transport` trait, two byte-pipes. Browser↔control-plane uses WSS
  (browsers cannot be raw iroh peers) — the one unavoidable second transport.
- **Durable workflows:** an embedded, single-process, **checkpoint-based**
  engine backed by redb. **Decision: adopt Sayiir** (MIT, continuation-based, no
  deterministic-replay constraint) via a custom redb `PersistentBackend` — per the
  completed source review (ADR 0002 §15.2). The n8n-like visual builder is ours.
  (Workflows are post-MVP; this records the engine choice.)
- **Storage / compression / search:** redb values **zstd-compressed with a
  trained dictionary** (messages are highly compressible); **tantivy** (MIT) for
  full-text/BM25 search as a derived, rebuildable index; a **content-addressed
  blob store** (blake3-keyed, fs cache + object storage) for artifacts and
  non-text files, with text extraction at ingest for searchability; **vector/
  semantic search deferred** to a later additive index (it needs an embedding
  pipeline — a model dependency + cost) and is purely additive because indices
  are derived projections of the log.
- **File & repo sync:** **jj** (everywhere) for workspaces and vaults; git-over-
  tunnel for remote sync. (Removes the earlier iroh-docs CRDT layer — jj is the
  sync substrate.)
- **Scheduled work / webhooks:** tokio timers and an axum HTTP listener inside
  the control plane.

The three-layer model is unchanged in shape: **Layer 1 control plane (now a
Rust binary, not Convex) → Layer 2 node agent ("envoy", Bun or Rust) → Layer 3
agents.** The host-effect boundary is unchanged: the control plane orchestrates
many hosts and never performs host effects directly; each host's envoy does.
(Rationale shifts from "Convex container physically cannot reach the host" to
"the control plane is one logical authority over many hosts; host effects are
inherently per-host and belong to that host's envoy.")

## Consequences

- **Gained:** transport-native unspoofable node identity (the `nodeToken`
  subsystem largely dissolves); no SPOF connection terminus; one identity system
  per transport, not two; fully shippable (all chosen libs are MIT/Apache);
  predictable, GC-free memory; a simpler correctness model (single-writer
  serialization for the small contended core, deterministic views for reads).
- **Cost (owned now):** we build the **reactive-view + delta-broadcast** layer
  and the **message/streaming model** ourselves — these were Convex's batteries
  and are the real engineering work. We run **3–4 stores** (redb truth + tantivy
  + blob store + later vector) instead of one — each the right tool for its
  access pattern, all derived ones rebuildable from the log.
- **Correctness caveat unchanged:** the substrate guarantees state/view
  correctness, not behavioral correctness. The verification layer (verifiable
  artifact, not agent self-report) is still required.
- **Resolved inputs:** (a) Sayiir **adopted** after the source review (ADR 0002
  §15.2); (b) event-sourced log over state-tables (decided: log, for audit). Both
  captured in ADR 0002.

## Rejected alternatives (substrate)

- **Keep Convex, run it over the tunnel.** Two identity systems; SPOF terminus;
  external backend to operate; distributed-ACID we don't need. Rejected.
- **SpacetimeDB (Rust, reactive).** Closest turnkey "Convex but Rust," but it is
  also a DB-as-server with its own websocket protocol and identity model —
  reintroduces the exact transport/identity friction we're removing. Rejected.
- **Restate (Rust durable execution).** BSL — production use restricted until a
  Change Date. Unshippable. Rejected.
- **n8n / Windmill (workflow engines).** Fair-code / AGPL — commercial-shipping
  risk. Rejected; the n8n-like surface is built in-house over our own log.
