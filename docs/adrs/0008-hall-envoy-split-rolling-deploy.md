# ADR 0008 — Hall/Envoy process split with rolling deploy

Status: accepted · Date: 2026-07-09
Wayfinder map: [#1](https://github.com/IEatCodeDaily/olympus/issues/1) (tickets #2–#10 hold decision detail)
Evidence: `docs/wayfinder/resume-semantics.md`, `docs/wayfinder/resume-semantics-claude-codex.md`

## Context

Olympus ships as one binary (`olympus-control-plane`) that embeds the "local
envoy" in-process: the same process owns the redb event log, views, search,
REST/WS API, UI hosting, **and** the ACP child processes (`hermes acp`,
claude-code-acp, codex-acp) that hold live agent sessions. Consequences:

- Restarting the control plane kills every agent runtime. A session that is
  itself developing Olympus cannot survive — let alone observe — a deploy of
  the thing it is building.
- There is no way to roll out a new runtime-holder without dropping sessions.
- ADR 0002 §2.1 already drew the layer boundary (host facts and runtimes are
  Layer 2/envoy concerns); ADR 0005 §3 and ADR 0007 kept the boundary "so
  multi-node is additive" — but no envoy binary exists and the UDS node
  protocol carries only hello/heartbeat/bye.

Empirical grounding (tickets #2, #10): all three supported harnesses implement
cross-process session resume backed by on-disk state — hermes (~2.5 s flat,
lockless, silent-new-session hazard), claude-code-acp (verified 1.8 s
cross-process resume), codex-acp (source-verified; fails closed on unknown
ids). Hermes persists completed turns only (in-flight turn output dies with
the child) and resets mode on resume; model choice persists.

## Decision

Split into two independently-deployable binaries:

- **Olympus Hall** (`crates/hall`, bin `olympus-hall`) — the control plane:
  event log (sole source of truth), views, search, import/sync, REST/WS, UI,
  auth, NodeRegistry + listeners, trigger scheduler.
- **Olympus Envoy** (`crates/envoy`, bin `olympus-envoy`) — the runtime
  holder: agent discovery, the runtime table, ACP bridge + children, setup
  adapter materialization, session spaces I/O.
- **`crates/proto`** — the only shared crate: wire frame types, AgentEvent/
  AgentCommand, RuntimeSpec. Serde only, no heavy deps. Hall and Envoy depend
  on proto, never on each other.

There is **no in-process envoy in production**. Hall's tests keep the
MockAgentRuntime seam behind the existing factory trait; one integration test
boots a real envoy against a temp hall over a temp socket.

### 1. Wire protocol (#3)

JSON-lines frames (one compact JSON object per line), multiplexed over one
persistent connection per envoy. Transport: **UDS** locally
(`~/.olympus/control.sock`), **iroh** for remote envoys (QUIC, e2e-encrypted,
keyed by node id; Hall rejects node ids not on its allowlist — fail closed).

Frame families:
- Hall→Envoy: `ensure_runtime`, `prompt`, `steer`, `cancel`, `stop`,
  `respond_permission`, `drain`, `probe` — each with Hall-assigned `reqId`;
  envoy replies `resp {reqId, ok|error}`.
- Envoy→Hall: `event {sessionId, turnId, seq, payload}` — `seq` is a
  per-session monotonic counter assigned by the envoy (the ordering/idempotency
  key); `runtimes {[{sessionId, hermesId, state, resumable, lastSeq}]}` in
  hello and on change.
- Hall→Envoy: `ack {sessionId, seq}` (spool truncation watermark),
  `resume_from {sessionId, seq}` (replay cursor at reconnect),
  `heartbeat_ack`, and `re_register`. Hall acknowledges every known-node
  heartbeat and requests a fresh hello when the authenticated connection is
  alive but its registry entry is missing. Envoy also re-sends hello after
  three consecutive unacknowledged heartbeats.

Hello carries **two version fields with distinct jobs**:
- `protocolVersion: u32` — frame-schema compat gate. Unparseable version →
  registration rejected (fail closed). Changes rarely.
- `version: {semver, gitHash, builtAt}` — envoy **build identity**. This is
  what drain/evict decisions key on and what the Nodes UI shows. Protocol
  version does not solve "which envoy is outdated"; build version does.

The current Hall↔Envoy frame set is protocol version 1. Olympus is pre-release,
so Hall requires an exact version match; there is no partial feature negotiation
or rolling wire compatibility. `heartbeat_ack`, `re_register`, node fencing, and
durable jobs are unconditional after a successful hello.

### 2. Envoy autonomy across Hall downtime (#5)

Envoys buffer through Hall restarts — sessions stay fully live:
- Every outbound `event` is appended to a per-session disk spool
  (`~/.olympus/envoy/<id>/spool/<sessionId>.jsonl`) and sent when connected.
  Spools truncate at Hall's ack watermark; 512 MB/session cap with a
  `SPOOL_OVERFLOW` marker event on breach.
- Reconnect: envoy hello carries its runtimes table (+ lastSeq); Hall answers
  `resume_from` per session; envoy replays spool > seq, then streams live.
  Seq gate makes replay exactly-once and ordered; Hall derives its applied
  watermark from the event log itself on boot.
- A restarted Hall relearns who holds what **from the envoys** (the runtimes
  table), then reconciles against its log: sessions the log says are running
  that no envoy claims → marked detached, lazily resumable.

### 3. Drain and handover (#4, amended by #10)

Hall orchestrates; handover is **resume-then-flip**, not kill-then-resume
(research: resume is lockless; nothing arbitrates writers, so Hall enforces
single-prompter).

Per session (E1 → E2): quiesce (prompts queue in Hall) → turn boundary
(bounded, `drain_turn_timeout` 10 min, then cancel; in-flight turn recorded
lost — its streamed chunks already live in Hall's log even though the harness
store drops them) → `ensure_runtime` on E2 with resume id → **verify
provenance** (returned session id must match; hermes silently creates new
sessions on unknown ids) → re-apply mode / reconcile from resume response →
flip (`SessionRuntimeMoved` event) → release queued prompts → reap E1's child.

- Handover requires the runtime's **`resumable` capability flag** (from the
  adapter's initialize response: `loadSession` + `sessionCapabilities.resume`)
  — capability-driven, never harness-name-driven. All three current harnesses
  pass. Non-resumable → degraded drain: turn boundary → stop → session
  runtime-less; lazy revival on next prompt.
- Node drain = registry status Draining (no new sessions) + handover of every
  held session (parallel, bounded 4); complete when the envoy's runtime table
  is empty. Runtime location is **event-logged** (`SessionRuntimeMoved`), not
  registry-only — Hall restarts mid-drain must not forget locations.
- Every failure fails closed to "session stays where it was": E2 death,
  resume failure, provenance mismatch → abort, surface, drain pauses.
- Budget ~5 s/session (2.5 s resume + spawn/init).

### 4. Triggers — Hall-initiated turns (#8)

Minimal, event-logged trigger object; Hall gains a scheduler, **not** an
executor (the session's agent already has bash):
- `POST /api/triggers {sessionId, prompt, fireAt|intervalSecs, maxFires=1,
  ttlSecs}`; `GET`/`DELETE`. Defaults fail closed: one-shot, TTL 24 h;
  recurring requires explicit `maxFires`.
- Firing = the existing post-message path (lazy ensure_runtime revives the
  session if needed). State events: TriggerCreated/Fired/Exhausted/Cancelled —
  triggers survive Hall restarts; a one-shot whose fireAt passed during
  downtime fires once on boot (fire-or-expire, never a burst).

### 5. Deploy choreography (#7)

systemd user units: `olympus-hall.service`, `olympus-envoy@.service`
(templated; `%i` = node-id suffix, own spool dir; `Restart=on-failure` so a
drained envoy that exits 0 stays down). Binaries at
`~/.olympus/bin/olympus-{hall,envoy}-<gitHash>` with a symlink as the deploy;
unit PATH includes `~/.local/bin` (postmortem 0001).

**Envoy rolling deploy** (Session A story): build → symlink flip →
`systemctl --user start olympus-envoy@2` → Hall health-gates E2 (hello +
protocolVersion parses + `probe` round-trip returning agent discovery; result
event-logged) → `POST /api/nodes/envoy-1/drain {toNode: envoy-2}` → E1 empties
and exits. Session A hands over mid-drain and continues on E2. Gate failure →
drain never starts; nothing moved.

**Hall deploy** (Session B story): no rolling pair (one redb writer). Arm
one-shot trigger ("Hall restarted — verify and report") → symlink flip →
`systemctl --user restart olympus-hall`. Envoys buffer; hello/runtimes
re-attach; spools drain; trigger fires the verification turn into Session B.
Rollback = flip symlink back.

Hall owns the drain state machine; the deploy *sequence* is scripted outside
Hall (`make deploy-envoy`, `make deploy-hall`, agent-callable bash) using
Hall's primitives. Hall exposes primitives, not a pipeline.

### 6. Migration (green tree at every step) (#6)

1. Extract `crates/proto` (types only; monolith builds against it).
2. Extract envoy-side modules into `crates/envoy` as a lib the monolith still
   links.
3. Add UDS session-RPC to hall + envoy `main.rs` — both binaries exist; the
   monolith still works.
4. Cutover: units for hall + envoy@, delete the monolith and in-process
   registration. `olympus-control-plane` retires here.

`make verify` gates each step.

## Consequences

- Sessions survive and observe Hall deploys; envoys roll without dropping
  sessions. Olympus can develop Olympus.
- New machinery to own: spool files + seq bookkeeping, drain state machine,
  trigger scheduler, iroh endpoint + allowlist. Each is bounded and
  independently testable; all frames stay jq/socat-debuggable.
- The event log gains event kinds: `SessionRuntimeMoved`, Trigger*, gate
  results. Views project runtime location; Nodes UI shows build versions.
- Two binaries to version and deploy instead of one; the symlink-flip scheme
  and build-version-in-hello keep that manageable.
- Hermes-specific hazards are contained at the envoy edge (provenance check,
  mode re-apply) behind capability flags — no harness names in Hall logic.
- Open refinements (do not block build): re-run claude probe after
  `claude /login` (replay fidelity, mid-turn); runtime-verify codex-acp when
  the CLI lands on a node.
