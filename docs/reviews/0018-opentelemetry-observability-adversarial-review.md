# ADR 0018 OpenTelemetry observability — adversarial architecture review

**Review date:** 2026-07-13  
**Verdict:** **NO-GO — 3 BLOCKER, 9 P1, 3 P2 findings.**  
**Scope:** ADR 0018 and its implementation plan, checked against ADRs 0008, 0010,
0017, the approved ADR 0017 implementation ordering, postmortem 0021, and the
current Hall/Envoy source. This reviews a proposal, not an implementation.

## Executive verdict

The separation of permanent product/audit truth from TTL diagnostics is the
right doctrine, and the proposal correctly rejects an unbounded session trace,
TTL-without-quota, metric IDs as labels, and redaction after transport. The plan
is nevertheless unsafe to dispatch.

The blocking defects are architectural rather than editorial:

1. the observability plan can land remote telemetry before ADR 0017 makes session
   payload+watermark ingress durable, despite the roadmap declaring that spine a
   prerequisite;
2. the proposed batch/sequence/dedup schema cannot actually provide the stated
   commit-before-ACK and duplicate-replay semantics for both logs and spans;
3. Hall has no defined authenticated tenancy derivation at telemetry ingest, so
   an Envoy can assert another organization/session's correlation fields.

The current revision added Hall-side physical-file accounting, a free-space
reserve, Hall-observed expiry, and a source epoch while this review was in
progress. Those changes narrow several risks but do not resolve the three
blockers above.

No implementation card touching remote telemetry should be dispatched until the
three blockers are corrected in ADR 0018 and made executable prerequisites in the
plan.

## Findings

### [BLOCKER-1] OBS-3 is ordered ahead of the permanent session-truth prerequisite

**Evidence**

- `docs/plans/2026-07-13-otel-observability-session-diagnostics.md:23-38` orders
  OBS-3 after only the telemetry store and says merely that it “must coordinate”
  with ADR 0017.
- `docs/plans/2026-06-29-olympus-long-horizon-roadmap.md:339-348` is stricter: the
  observability epic depends on ADR 0017's session/runtime durability spine
  before Envoy telemetry ACK semantics land.
- `docs/adrs/0017-session-cutover-and-remote-development-plane.md:169-190`
  requires payload/reference + watermark atomic persistence, durable ingress
  projections, authoritative runtime-attempt inventory, and unified controls
  before primary-session cutover.
- The current source proves this is not already satisfied. `crates/control-plane/src/log.rs:129-156`
  commits only `envoy_watermarks`; `crates/control-plane/src/server/envoy_conn.rs:157-173`
  then forwards the payload into a process-local broadcast channel before ACK.
  The payload is not in that transaction.
- Nevertheless OBS-2's gate claims a “durable transcript append”
  (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:149-153`),
  and OBS-3's gate claims restart diagnostics with no silent gaps (`:190-194`),
  while neither OBS task owns ADR 0017 Task 1.1's durable ingress implementation.

**Failure sequence**

Hall commits sequence `N`, forwards its payload to the volatile accumulator, and
ACKs. Hall crashes before the final product message is appended. Envoy truncates
`N`. After restart, TTL logs and spans can look complete while the permanent
transcript/tool outcome is missing. Telemetry has thereby concealed, not solved,
the source-of-truth loss window.

**Required correction**

- Make ADR 0017 Tasks 1.1–1.3 hard prerequisites of OBS-3 and of every OBS-2/4
  gate that claims session/runtime completeness. Use the exact dependency:
  `atomic session payload+watermark ingress -> authoritative runtime attempts and
  unified controls -> remote telemetry transport/query`.
- Make ADR 0017 Tasks 2.1–2.4 prerequisites for job-output/result diagnostics;
  before then, job instrumentation may report only explicitly non-authoritative
  process observations and must not claim a complete job timeline.
- Change the OBS-2/3 gates to reconcile the permanent producer manifest and
  durable product projection first, then independently reconcile telemetry.
  A trace UI must never turn TTL evidence into an oracle for permanent truth.

### [BLOCKER-2] Batch ACK, source sequence, deduplication, and gap semantics are internally inconsistent

**Evidence**

- ADR 0018 says Hall commits and ACKs a **batch**, while duplicate batches are
  harmless through `(source_id, source_seq)` (`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:227-245`).
- The proposed schema applies `UNIQUE(source_id, source_seq)` only to logs and
  gives spans only `(trace_id, span_id)` identity (`:135-159`). A multi-record
  batch therefore either gives every log the same sequence and violates the
  unique constraint, or gives every record a sequence while leaving the ACK's
  batch range unspecified. Span replay/dedup and partial-batch retry are not
  defined at all.
- OBS-1 asks for batch insertion, deduplication, and one highest-committed
  watermark (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:85-104`)
  without defining the unit sequenced or the atomic relation among records,
  watermark, and ACK.
- The current product spool is a warning, not a reusable implementation:
  `crates/envoy/src/spool.rs:43-63` advances an in-memory sequence before append;
  `:65-92` appends the record and then persists the counter as a second durable
  action. Call sites allocate before append (`crates/envoy/src/main.rs:632-642`).
  On cap/ENOSPC/fsync failure, the sequence can be consumed without a durable
  record. ADR 0017 already requires this to be replaced, not copied
  (`docs/plans/2026-07-13-session-cutover-remote-development.md:251-273`).

**Failure sequence**

An Envoy allocates sequence 41 for a batch, appends two of ten records, and loses
power or storage. Depending on interpretation, Hall either cannot distinguish a
partial batch, rejects the other logs as duplicates, or inserts duplicate spans.
If the source skips to 42, Hall cannot tell whether 41 was dropped, partially
committed, or belongs to an earlier Envoy incarnation. A gap marker may itself
be impossible to append because the spool is full.

**Required correction**

Define one complete wire/storage state machine before OBS-1:

- durable identity `(enrolled_source_id, source_epoch, record_seq)` for every
  telemetry record, with `source_epoch` changed only through an authenticated,
  durable reset protocol;
- deterministic record IDs and the same dedup key on logs, spans, span events,
  and gap/coverage records;
- batch envelope `{source, epoch, first_seq, last_seq, records}` with contiguous
  sequences, all-or-nothing Hall transaction, committed contiguous watermark,
  and ACK `{source, epoch, through_seq}`;
- atomic source sequence reservation + spool append (or recovery that rolls back
  an uncommitted reservation), plus a durable dropped-range ledger with reserved
  capacity outside the data cap;
- explicit behavior for partial/corrupt batches, epoch reset, duplicate ACK,
  ACK rewrite failure, cap/ENOSPC/read-only/fsync failure, and sequence exhaustion.

Add crash-point tests at reserve, append, fsync, send, each Hall row insert,
watermark commit, ACK send, and spool rewrite. Compare producer and consumer
sequence/content manifests, not only UI output.

### [BLOCKER-3] Telemetry ingest trusts caller-supplied tenancy and correlation

**Evidence**

- ADR 0018 puts `org_id`, `session_id`, `runtime_attempt_id`, and `node_id` in
  every record (`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:90-107`)
  and specifies organization-scoped **query** authorization (`:291-315`), but it
  never says Hall derives or validates those fields at ingest.
- ADR 0010 requires organization-scoped Envoy registrations and says client
  context is never authority (`docs/adrs/0010-hall-auth-and-client-connections.md:19-35`).
- ADR 0017 requires binding logical node ID to enrolled iroh public key before
  this plane is trusted (`docs/adrs/0017-session-cutover-and-remote-development-plane.md:169-186`).
- Current Hall accepts a hello's arbitrary logical `node_id` after only a global
  iroh-key allowlist check. It registers the caller-provided ID and then inserts
  its connection (`crates/control-plane/src/node.rs:638-701`); the allowlist is
  Hall-wide rather than org/source-scoped (`:891-930`). There is no current
  durable source-to-org/session/runtime-attempt binding.
- The plan tests store/query organization isolation
  (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:110-120,216-220`)
  but has no hostile ingest test.

**Impact**

A compromised but allowlisted Envoy can label telemetry as another organization
or session, poison incident evidence, consume another tenant's per-session quota,
or place attacker-controlled log bodies where a victim with
`session.diagnostics.read` will retrieve them. Query filtering does not repair
untrusted ownership metadata.

**Required correction**

- Complete ADR 0017 node-key/logical-ID binding and durable runtime-attempt
  inventory before OBS-3.
- Derive `source_id`, `node_id`, allowed organization, session, turn, and runtime
  attempt from the authenticated transport peer plus Hall's permanent resource
  truth. Do not trust those envelope fields as assertions.
- Reject telemetry for unknown, stale, archived/revoked, cross-org, or mismatched
  attempts. Put node-wide logs in a separate operator-only scope; do not invent a
  session/org for them.
- Bind source sequence state to the enrolled source identity/epoch, not a
  self-declared string.
- Add wrong-org, wrong-session, stale-attempt, duplicate-node takeover,
  source-reset, revoked-node, and concurrent-revocation ingest tests.

### [P1-1] Hall-side physical quotas were added, but Envoy spool safety and admission headroom remain unspecified

**Evidence**

- ADR 0018 now defines Hall's budget over the DB, WAL/SHM, and telemetry blobs,
  adds `min_free_bytes`, incremental auto-vacuum, and bounded checkpoints
  (`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:177-225`).
  OBS-1 repeats those Hall-side requirements
  (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:85-108`).
  This is a material correction to the initial draft.
- The Envoy side still says only that source spool limits are mandatory
  (`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:241-245`)
  and OBS-3 asks for caps (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:155-188`)
  without a physical budget, filesystem reserve, or isolation from the product
  event spool and runtime state.
- Rewriting an acknowledged spool can temporarily require the retained file plus
  a second temporary file (`crates/envoy/src/spool.rs:119-142`). A nominal cap
  therefore is not sufficient admission headroom.
- SQLite deletion does not shrink the database automatically; WAL and checkpoint
  work can transiently consume substantial additional space. The permanent log
  already runs another WAL database under the same home and uses
  `synchronous=NORMAL` (`crates/control-plane/src/log.rs:1092-1114`). A separate
  connection/mutex prevents lock contention but not filesystem exhaustion.
- Disk-full and slow-SQLite injection is deferred to OBS-7
  (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:267-286`),
  after the store and transport would already be considered complete.

**Impact**

Hall's new reserve addresses its most direct failure mode, but an Envoy log storm
can still exhaust the filesystem holding the product-event spool, source epoch,
runtime state, and telemetry rewrite temporary files. It can also consume the
space needed to write the mandatory gap marker. The plan has moved from a
blocker to an incomplete P1 operational contract.

**Required correction**

- Keep Hall's new DB/WAL/blob accounting and reserve, but include SQLite temp
  space and worst-case transaction/checkpoint headroom in its measured budget.
- Give every Envoy a separate telemetry physical budget and
  high-water/critical-water reserve on the filesystem containing its product
  spool/runtime state. Account for live file + ACK-rewrite temp file + pending
  gap ledger; telemetry must stop before entering the product reserve.
- Use bounded WAL/checkpoint policy and prove post-prune physical reclamation or
  bounded file reuse. Do not call logical `SUM(length(...))` a disk quota.
- Move real ENOSPC, WAL-growth, checkpoint-failure, read-only, temp-space, and
  simultaneous product-append tests into OBS-0/1 gates. The oracle is that
  permanent `Log::append` remains successful and within a stated latency bound.

### [P1-2] “Weighted priority” is not a scheduler and can repeat postmortem 0021

**Evidence**

- Postmortem 0021 attributes the outage to replay writes, per-frame ACKs, and a
  shared writer path (`docs/postmortems/0021-envoy-replay-starved-heartbeats.md:11-25`).
- Current Envoy still has one mutexed writer (`crates/envoy/src/main.rs:204-225`).
  Heartbeats and replay contend for it (`:317-357`); `ResumeFrom` replay also
  loops every frame (`:594-601`). The postmortem fix keeps the read loop free but
  does not provide traffic-class fairness.
- OBS-3 says “use weighted priority” and lists classes
  (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:173-177`)
  without defining ownership, queue bounds, frame/byte quanta, or a heartbeat
  deadline.

**Required correction**

Replace direct writer-lock acquisition with one outbound scheduler task. Give
control/heartbeat/ACK a reserved bounded queue and deadline; schedule lower
classes by byte quanta (not frame count), cap replay batches, and force a yield
after each quantum. Bound every producer queue and specify drop/backpressure by
class. Test simultaneous product replay, telemetry replay, live output, ACK
flood, slow peer, and reconnect with payloads large enough to exceed socket
buffers; assert p99 heartbeat/control latency and eventual spool drain.

### [P1-3] Absence of a gap row cannot establish completeness

**Evidence**

- ADR 0018 explicitly requires operators to know whether telemetry is complete
  (`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:25-33`)
  but creates a gap row only “when storage permits” (`:209-211`).
- OBS-1 may quarantine a corrupt DB and start empty
  (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:105-108`).
  After that reset, all old gap rows are gone.
- OBS-4 exposes a gap/completeness status (`:196-214`) without a coverage model.

**Impact**

ENOSPC, writer-channel overflow, process death before flush, DB quarantine, or
expired gap rows can leave no gap record. A query that equates “no gap found”
with “complete” gives a false assurance—the precise behavior the ADR forbids.

**Required correction**

Model completeness as positive coverage, not absence of errors. Persist
source/epoch coverage intervals and clean-shutdown/flush boundaries; return
`complete`, `incomplete`, or `unknown` for a requested interval and source set.
A new/quarantined DB generation starts `unknown`. Hall-local bounded-channel
loss and Envoy source loss must advance explicit dropped ranges. Preserve a
low-volume authoritative operator incident fact for telemetry-store reset,
disablement, or unrecoverable loss; it may describe loss without copying
telemetry payloads into product truth.

### [P1-4] The redaction boundary is aspirational, while current seams emit raw data

**Evidence**

- ADR 0018 correctly requires producer-side allowlisting and fail-closed sensitive
  fields (`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:273-289`).
- OBS-1 creates a Hall `redact.rs`, while OBS-3 captures Envoy child stderr
  (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:76-108,155-188`),
  but no Envoy redaction/typed-emission module or serialization firewall is
  assigned.
- Current wire responses carry arbitrary error strings
  (`crates/proto/src/frames.rs:255-264`). Envoy emits full chained errors and
  sends them in responses (`crates/envoy/src/main.rs:146-154,449-468`), while
  existing `tracing` fields are captured before a later store layer could make
  them safe.

**Required correction**

Introduce producer-owned typed safe telemetry APIs in both Hall and Envoy:
stable error code, allowlisted safe fields, bounded scrubbed message, and an
opaque diagnostic digest. Put a final default-deny validator immediately before
telemetry wire serialization/spool append. Child stderr must pass a streaming,
bounded scrubber before framing and never be retained raw. Define secret
fixtures (authorization/cookie/MCP token, env assignments, URLs, launch codes,
identity tickets, PEM/JWT/API keys, prompts/tool bodies) and byte-scan serialized
spool/wire fixtures. Diagnostic capture needs a separate capability, duration,
size, audit event, and conspicuous UI state; it cannot be a boolean escape hatch.

### [P1-5] The minimum SQLite schema cannot round-trip the promised OTel model

**Evidence**

ADR 0018 requires preserving IDs, timestamps, severity, scope, resources, span
links, status, and typed attributes for later OTLP export
(`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:56-70`),
but the minimum schema has no span-links field, trace flags/state, dropped
attribute/event/link counts, schema URLs, or typed log-body representation
(`:135-159`). OBS-6's integration test cannot recover information never stored
(`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:246-265`).

**Required correction**

Check in a versioned internal telemetry contract before store migrations. It must
represent OTel `Resource`, `InstrumentationScope`, `SpanData` (including links,
events, flags/state and dropped counts), and typed `LogRecord` body/attributes
without JSON type loss. Define accepted timestamp ranges and ID byte widths.
Round-trip property tests must compare the normalized internal record before
insert with records received by an OTLP test collector after reopen/replay. If a
field is intentionally unsupported, narrow the ADR's compatibility claim.

### [P1-6] SQLite concurrency/failure policy is not strong enough for a shared diagnostic service

**Evidence**

- OBS-0 proves only that a blocked writer does not block the instrumented
  operation (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:47-74`).
- OBS-1 adds writer, sweeper, broad indexed queries, FTS, checkpointing,
  quarantine/recreate, and concurrent query/write tests (`:85-120`) but does not
  define connection ownership, read transaction deadlines, cancellation,
  checkpoint coordination, migration locking, or safe replacement while tasks
  hold old handles.
- Current authoritative `Log` serializes each connection behind a process mutex
  (`crates/control-plane/src/log.rs:17-26,86-105`). Copying that shape would let a
  long query/delete monopolize the telemetry connection even under WAL.

**Required correction**

Specify one bounded writer executor and a bounded read-only connection pool;
statement progress/deadline cancellation; maximum rows, scanned time range, and
response bytes; short batched prune transactions; checkpoint ownership; and a
store-generation fence for quarantine/reopen. Define behavior for BUSY,
IOERR/FULL/CORRUPT, failed migration, corrupt WAL, and shutdown with queued
batches. Gate with deterministic slow-reader/slow-writer/prune/checkpoint races,
not only a generic concurrent test.

### [P1-7] Blob/FTS retention and incident preservation can leak or destroy the wrong data

**Evidence**

- ADR 0018 permits telemetry bodies in the content-addressed blob store and an
  external-content FTS index (`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:161-166`).
- It then copies selected diagnostics into an “immutable” incident bundle that
  follows artifact retention (`:317-326`).
- OBS-7 names manifest/hash export but no ownership, capability, reference-count,
  encryption, purge, or failure-atomicity rules
  (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:267-286`).

**Required correction**

Use a telemetry-only blob namespace or class-aware references so TTL deletion
cannot remove an authoritative artifact and shared content cannot silently keep
expired telemetry alive. Delete row, FTS token data, blob reference, and expiry
metadata transactionally (with an idempotent blob-GC journal). Add a dedicated
`incident.export` capability separate from diagnostics read, derive bundle org
ownership, require explicit retention/classification, encrypt at rest where the
artifact policy requires it, and record export as a permanent audit event without
copying bodies into the event log. Test shared blobs, partial export, concurrent
expiry, session/org deletion, and cross-org digest collisions.

### [P1-8] “Bounded operation” lacks attempt, size, and lifecycle semantics

**Evidence**

- ADR 0018 rejects one trace per session and names bounded roots
  (`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:75-107`).
- The plan tests context through tokio and Hall→Envoy metadata
  (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:49-66`)
  but does not define retry/reconnect/resume relationships, trace closure after
  crash, or hard span/event/duration limits.
- ADR 0017 explicitly models runtime/job attempt epochs and new attempts on
  recovery (`docs/adrs/0017-session-cutover-and-remote-development-plane.md:188-219,280-292`).

**Required correction**

Make each dispatch/runtime/job/workflow/deployment **attempt** the bounded unit.
A retry or post-crash recovery gets a new root trace/attempt and links to the
prior attempt; replay does not reopen or re-parent an ended trace. Store operation
ID and attempt epoch separately. Define maximum spans, events, links, attributes,
body bytes, and elapsed duration per trace, with truncation/segmentation and
coverage markers. Define how abandoned roots are finalized on restart and how
late telemetry is represented. Add retry, reconnect, resumed runtime, stale
attempt, and late-span tests.

### [P1-9] The separate `:8788` authentication contract is unresolved

**Evidence**

- ADR 0018 says `:8788` is system-wide, loopback by default, Hall-authenticated,
  organization-authorized, and requires `session.diagnostics.read`
  (`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:291-315`).
- ADR 0010 says the Web UI is permanently bound to its serving Hall origin and
  requires exact-origin browser checks (`docs/adrs/0010-hall-auth-and-client-connections.md:77-100`).
- The current principal model distinguishes Operator/Admin/organization scope
  but has no diagnostics capability (`crates/control-plane/src/server/principal.rs:9-56`).
- OBS-5 says “implement authenticated routes” without defining login/cookie,
  CSRF/origin, org selection, system-wide operator versus member visibility, or
  reverse-proxy identity (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:222-244`).

**Required correction**

Define whether `:8788` is a distinct browser origin, a reverse-proxied path on the
Hall origin, or operator-token only. Specify cookie scope, login/logout, exact
allowed origins/Fetch Metadata, CSRF on export/mutations, TLS/forwarded-header
trust, organization selection, and capability issuance/revocation. The
organization must come from validated membership, never a query field. Add
cross-origin, cross-org, non-member, ordinary session-reader, revoked login,
installation-token, and reverse-proxy tests before UI work.

### [P2-1] Metric label rejection needs stable fallback semantics and budgets

ADR 0018 forbids high-cardinality labels
(`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:247-268`),
and OBS-6 adds an allowlist (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:246-265`).
However, “model/tool counts using bounded labels” is ambiguous because model and
tool names are producer/user-extensible. Define compile-time label keys,
finite normalized value vocabularies, `other`/`invalid` buckets, maximum series
per family, and behavior at the budget. Never dynamically register a rejected
value, including in the rejection metric. Test Unicode/confusable and ever-new
names as well as obvious IDs.

### [P2-2] Hall clock discontinuity semantics remain unspecified

ADR 0018 now correctly derives expiry from Hall-observed ingest time rather than
producer time (`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:171-175`).
It still does not define backward Hall-clock jumps (retention extension), large
forward jumps (mass immediate expiry), or restart with a corrected clock, while
the plan defers clock-jump injection to OBS-7
(`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:267-278`).
Specify those policies and gate them in OBS-1 before retention is accepted.

### [P2-3] Verification over-relies on visual evidence and weak “no duplicates” claims

OBS-3's restart gate asks to “show” a tree with no duplicates
(`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:190-194`), and
final evidence emphasizes screenshots/video (`:299-306`). Visual output cannot
prove byte completeness, exact dedup, redaction, or causality. Require source and
Hall manifests containing epoch/sequence, normalized-record digest, trace/span
IDs, and committed ACK watermark for every crash point. Compare them exactly,
then use browser evidence only for presentation, expiry, authorization, and gap
warnings.

## Required plan rewrite before dispatch

The corrected executable dependency should be at least:

```text
ADR 0017 SESSION-SAFE atomic payload+watermark ingress
  -> authoritative runtime-attempt inventory + unified controls
  -> OBS-0 OTel/schema/physical-quota/redaction spike
  -> OBS-1 local model/store/coverage + Hall-only instrumentation
  -> authenticated source/epoch/record sequence contract
  -> OBS-3 Envoy transport + fair outbound scheduler
  -> OBS-4 authorized query service
  -> OBS-5 admin UI

ADR 0017 JOBS-2 attempt inventory
  -> Envoy terminal/spool correctness
  -> durable JobService reconciliation
  -> job telemetry instrumentation
```

OBS-6 low-cardinality in-process metrics can proceed after OBS-0 independently of
remote telemetry. Incident export remains after the artifact ownership/retention
contract exists.

## Minimum approval gates

Approval of the revised proposal requires all of the following to be explicit in
the ADR/plan, not left to implementation judgment:

1. permanent session/job payload truth is durable before corresponding telemetry
   ACK/query claims;
2. authenticated source/epoch/per-record sequencing, atomic batch commit and ACK,
   replay dedup for every record type, and durable dropped-range coverage;
3. Hall-derived organization/session/runtime-attempt ownership at ingest;
4. physical disk accounting plus product-database free-space reserve and real
   ENOSPC/WAL/checkpoint tests;
5. one fair, byte-quantized outbound scheduler with heartbeat/control latency
   oracles under simultaneous replay;
6. positive completeness/unknown semantics across channel loss, DB reset, TTL,
   and corruption;
7. producer-side default-deny redaction proven on serialized spool/wire bytes;
8. a versioned internal OTel contract that round-trips through SQLite and OTLP;
9. explicit `:8788` identity/origin/org/capability semantics; and
10. exact sequence/content manifests for crash/replay verification.

Until those are present, ADR 0018 is directionally sound but not an implementable
or safely orderable architecture.

## Reviewed snapshot

Repository HEAD at review start: `f784b04aa6d04e6759eef999012f48ac3f0f1622`.
The ADR/plan/review-chain documents were untracked in a dirty worktree; findings
therefore reference the following content snapshots rather than claiming a clean
commit:

- ADR 0018: `b7f0386021ebc606c8a6643b104ee182ba72ef650d30e3cccec1719fca8d94ef`
- OBS plan: `a1fe1b12ca08311c08f60ea5033052a5f003db0eb57551807b3e7d5d185fbd1c`
- ADR 0017: `02d6cdfbd5516831e8131c442725b125f2216ab8440f570362d95290e987842d`
- ADR 0017 plan: `22577cf643520bb6fc4aaf92d0c1f4eeb274260518fe31dff84ce0012437b4e2`
- `frames.rs`: `b1ed01764c8065fdbf2652ac28031338b9439706e13c9996162cb27478a30015`
- `spool.rs`: `42152fbc8d29539210ebcba489687af097758461b4f764037c176222fcc2dcd9`
- Envoy `main.rs`: `010589a2445f13d20c9d97958ceacee90e4f41bb342f62694a3fa0b5ec588d97`
- Hall `log.rs`: `b6c87e0e8622081c5cf6db2a7a36545499dd7e9ca50ad13eeca5a1cb888dd84b`
- Hall `node.rs`: `63c7ac94c74ea4af019cd936073a13cf5601a75fcde2272928d26a7f7d3f056d`
- Hall `envoy_conn.rs`: `3f15c50254987094848d9f1aa67c846902e439324fda4e271dd825f6bddadcb1`
