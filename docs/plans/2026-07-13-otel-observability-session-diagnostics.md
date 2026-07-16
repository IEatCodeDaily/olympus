# OpenTelemetry observability and TTL session diagnostics implementation plan

Status: architecture approved; implementation blocked on ADR 0017 Tasks 1.1–1.4 · Date: 2026-07-13
Architecture: `docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md`
Review chain:
- `docs/reviews/0018-opentelemetry-observability-adversarial-review.md`
- `docs/reviews/0018-opentelemetry-observability-adversarial-rereview.md`
- `docs/reviews/0018-opentelemetry-observability-final-approval.md` — APPROVED / GO

## Outcome

Deliver restart-surviving, session-correlated traces and diagnostic logs with a
30-day default TTL, physical disk quotas/reserves, current low-cardinality metrics,
a same-origin authenticated admin surface, and optional OTLP/Prometheus export.
Product/audit truth remains in `olympus.db`; TTL diagnostics live in disposable
`telemetry.db` and a telemetry-only blob namespace.

## Non-goals

- embedding LangSmith;
- requiring an OTel Collector;
- building a general local time-series database;
- copying transcripts, prompts, reasoning, or tool bodies into telemetry;
- one trace spanning an entire session;
- changing ADR 0013 workflow semantics;
- shipping feedback/evaluation/prompt management in this slice;
- using telemetry to compensate for missing permanent session/job durability.

## Executable dependency graph

```text
ADR 0017 Task 1.1 SESSION-SAFE ingress
atomic payload/reference + watermark commit
  -> ADR 0017 Tasks 1.2–1.4
     authoritative runtime attempts + unified controls + peer/OS identity to
     logical-node binding and takeover rejection
       -> OBS-0 contract/physical/redaction/fairness spike
         -> OBS-1 Hall-local model/store/coverage/instrumentation
           -> OBS-3 authenticated Envoy source protocol + transport
             -> OBS-4 authorized query service
               -> OBS-5 admin/session UI

OBS-0 -> OBS-2 low-cardinality live metrics (parallel; Hall/Envoy local only)
OBS-1 -> OBS-6 optional OTLP round-trip/export (parallel with OBS-3)

ADR 0017 Tasks 2.1–2.4 JOBS-2
Envoy attempt truth -> terminal/spool correctness -> durable JobService
  -> job-output/result telemetry and complete job timeline claims

Artifact ownership/encryption/retention contract
  -> OBS-7 incident-bundle export
```

No remote telemetry card starts before ADR 0017 Tasks 1.1–1.4 are implemented and
verified. No complete job-timeline telemetry starts before ADR 0017 Tasks 2.1–2.4.
Until then, Hall-local/job process spans are explicitly non-authoritative and the
UI cannot label them complete.

## PRE-OBS — Verify permanent-truth prerequisites

### Required evidence

1. ADR 0017 Task 1.1 test proves each remote session payload/reference and
   transport watermark commit atomically before ACK.
2. ADR 0017 Tasks 1.2–1.3 prove authoritative runtime-attempt inventory and one
   unified control service after Hall/Envoy restart.
3. ADR 0017 Task 1.4 proves authenticated peer-key/OS-identity to logical-node
   binding, duplicate/takeover rejection, and durable enrollment lookup.
4. Producer and Hall permanent manifests reconcile session/attempt/sequence and
   payload digest across every crash point.
5. For job telemetry, ADR 0017 Tasks 2.1–2.4 separately prove durable job attempts,
   terminal ordering, output/result persistence before ACK, and reconciliation.

### Gate

Attach the prerequisite test artifact IDs to OBS-0. If any prerequisite is absent,
only paper design may continue; no remote telemetry protocol or query claim lands.

## OBS-0 — Contract, physical-safety, redaction, and scheduler spike

### Goal

Resolve the load-bearing contracts before schema migrations or broad
instrumentation create incompatible data.

### Work

1. Check in a versioned normalized telemetry contract in `crates/proto` covering
   OTel Resource, InstrumentationScope, SpanData, LogRecord, typed values, links,
   events, trace flags/state, schema URLs, dropped counts, timestamp ranges, and
   exact ID widths.
2. Pin compatible Rust crates in the spike only:
   `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp`,
   `tracing-opentelemetry`, and metrics exposition. Rust logs/metrics are beta;
   adapters do not dictate the internal contract.
3. Round-trip generated normalized records through an in-process exporter. Include
   nested maps/arrays/bytes, links, events, dropped counts, invalid timestamps,
   maximum IDs, and unsupported-field behavior.
4. Prototype a `tracing_subscriber::Layer` that writes through a bounded
   non-blocking channel to a dedicated executor. Prove a blocked/full telemetry
   path cannot block `Log::append`, authorization, heartbeat, or control.
5. Prototype producer-owned safe APIs for Hall and Envoy plus a final default-deny
   serialization validator. Byte-scan serialized spool/wire fixtures containing
   cookies, authorization/MCP tokens, env assignments, credentialed URLs, launch
   codes, identity tickets, PEM/JWT/API keys, prompts, reasoning, and tool bodies.
6. Benchmark SQLite WAL batch inserts, bounded deletes, FTS maintenance,
   checkpoints, incremental vacuum, read pool contention, and reopen generations
   at representative and burst loads.
7. Inject ENOSPC, read-only DB, temp-space exhaustion, WAL/checkpoint failure,
   corrupt WAL, slow reader/writer, failed migration, and shutdown with queued
   batches while permanent `olympus.db` appends continue. Record p99 product-append
   latency and physical DB/WAL/temp growth.
8. Prototype segmented Envoy telemetry spool admission accounting including
   segment files, compaction temp, source epoch, preallocated source journal, and
   filesystem reserve. Prove telemetry stops before product spool/runtime state
   reserve is consumed.
9. Prototype one outbound writer-owner task. Measure byte-quanta scheduling under
   simultaneous product replay, telemetry replay, live output, ACK flood, slow
   peer, and reconnect. Select queue capacities, maximum frame/batch size,
   quanta, and heartbeat/control deadline from evidence.
10. Verify trace context propagation across tokio tasks and Hall→Envoy frames and
    list every seam requiring explicit attach/link.

### Deliverable

`docs/measurements/otel-observability-spike.md` records exact crate versions,
contract version, batch/transaction sizes, channel/queue capacities, frame limit,
byte quanta, p50/p95/p99 overhead, physical growth, reserve values, and rejected
configurations.

### Gate

- normalized records round-trip without type/causality loss;
- serialized producer bytes contain none of the secret fixtures;
- permanent append remains successful within the stated p99 bound under all disk
  and telemetry contention fixtures;
- heartbeat/control p99 remains below the liveness budget and both spools
  eventually drain;
- no remote implementation starts if any oracle fails.

## OBS-1 — Hall-local model, store, coverage, and instrumentation

### Modules

- new `crates/control-plane/src/telemetry/`:
  `config.rs`, `model.rs`, `store.rs`, `writer.rs`, `read_pool.rs`,
  `retention.rs`, `coverage.rs`, `redact.rs`, `metrics.rs`;
- Hall boot/shutdown and health wiring;
- `telemetry.db` migrations and tests.

### Work

1. Implement `telemetry_generations`, authenticated-source metadata placeholders,
   normalized records, and positive coverage schema from ADR 0018.
2. Open `~/.olympus/telemetry.db` independently with incremental auto-vacuum,
   bounded WAL policy, one writer executor, bounded read-only pool, statement
   progress/deadline cancellation, and generation fencing.
3. Implement only Hall-local sources first. Source identity is created internally;
   no producer-supplied organization/session value is trusted.
4. Implement bounded batch insertion and normalized digest verification.
5. Implement time/range/row/response-byte bounded queries. No unbounded search or
   connection-global mutex around long reads.
6. Implement positive coverage statuses: complete, incomplete, unknown, expired.
   A new/quarantined DB generation starts unknown. Record a low-volume permanent
   audit fact for store reset/disablement/unrecoverable loss.
7. Implement the persisted retention clock. Large forward/backward wall-clock
   discontinuity pauses expiry and reports degraded health; test restart and
   operator acknowledgement.
8. Implement 30-day default TTL, physical accounting over DB/WAL/SHM/temp and
   telemetry-only blobs, 5 GiB global cap, 256 MiB/session cap, 2 GiB product
   reserve, transaction/checkpoint headroom, bounded prune/checkpoint/vacuum.
9. Implement corruption quarantine/recreate with store-generation fence. Old
   readers/writers cannot mutate the replacement.
10. Instrument Hall-local HTTP/auth/event-log/projection/query operations using
    bounded root attempts. Do not claim remote/session completeness yet.
11. Enforce per-trace span/event/link/attribute/body/duration limits,
    continuation traces, abandoned-root finalization, and late-record policy.

### Tests

- migrations/reopen/generation fencing;
- OTel normalized SQLite reopen round-trip;
- typed-body and link/event preservation;
- bounded writer/read pool and cancellation;
- slow-reader/writer/prune/checkpoint races;
- BUSY/IOERR/FULL/CORRUPT/corrupt-WAL/failed-migration behavior;
- physical quota, WAL/temp headroom, product reserve, reclamation/reuse;
- clock forward/backward/restart/acknowledgement;
- complete/incomplete/unknown/expired coverage including DB reset;
- trace limit/continuation/abandonment/late record;
- delete/recreate leaves product operation intact.

### Gate

Hall-local traces survive Hall restart, coverage is never inferred from absent
gaps, and deleting `telemetry.db` leaves permanent truth intact. Exact source and
Hall record manifests match; screenshots are not a data-integrity oracle.

## OBS-2 — Low-cardinality live metrics

May proceed after OBS-0 independently of remote transport.

### Work

1. Define compile-time instrument/label-key registry.
2. Give every metric family finite normalized label values, maximum series count,
   and `other`/`invalid` fallback. Rejection metrics use fixed labels.
3. Add uptime/build, request/transport latency, active bounded states, queue/spool
   depth/age, DB latency/errors/bytes, accepted/dropped/evicted/unknown telemetry,
   heartbeat/reconnect, and cgroup resource metrics.
4. Reject session/trace IDs, prompts, paths, free-form errors, tool args, arbitrary
   user/package/model/tool names as labels. Test Unicode/confusable and endlessly
   novel values.
5. Expose current metrics to health/admin and optional Prometheus format. No local
   time-series persistence.

### Gate

Hostile novel values cannot increase series past the configured family budget.
No rejected value appears as a dynamic label in the rejection metric itself.

## OBS-3 — Authenticated Envoy source protocol, spool, and fair transport

Requires PRE-OBS, OBS-0, and OBS-1.

### Work

1. Consume and re-verify completed ADR 0017 Tasks 1.2–1.4 artifacts for runtime
   attempts/controls, peer-key/OS-identity → logical node enrollment, and takeover
   rejection. OBS-3 does not reimplement those prerequisites.
2. Add authenticated source start/reset protocol. Hall derives
   `enrolled_source_id`; Envoy persists the Hall-authorized random source epoch.
3. Implement the preallocated durable source journal as sequence authority.
   Fsync a pending range reservation with expected item digests before any data
   append; no reservation means no consumed positions.
4. Implement segmented data append and journal finalization. Complete matching
   data finalizes `received`; append failure finalizes the same range `dropped`;
   unresolved durable pending recovers as range coverage `unknown`.
5. Implement ordered source items: data consumes one position; a coverage item
   consumes `item_first_seq..item_last_seq`. Merge journal and data spool in strict
   source order without reusing a reserved range.
6. Implement batch envelope `{source, epoch, first_seq, last_seq, items}` and ACK
   `{source, epoch, through_seq}` exactly as ADR 0018. Validate that item ranges
   are ordered, adjacent, non-overlapping, and span the whole envelope range.
7. Implement Hall all-or-nothing item/coverage/watermark transaction. Coverage
   advances through its range end. Reject partial overlap/corruption; ACK a wholly
   duplicate batch only when range/ID/digest equals committed state.
8. Derive organization/session/turn/runtime attempt/node from authenticated peer
   plus permanent Hall projections. Reject wrong-org/session, stale attempt,
   duplicate-node takeover, unknown/reset epoch, revoked source, and unauthorized
   capture. Keep node-wide records in operator-only scope.
9. Add producer-safe typed emission and final serialization firewall in Envoy.
   Stream-scrub child stderr; never spool raw stderr or ACP protocol stdout.
10. Keep source-journal capacity and filesystem reserve outside the telemetry data
    cap. At high water stop admission before exhaustion and mark source health
    `coverage_unknown`; unadmitted observations consume no sequence positions.
11. Implement measured Envoy physical budget/reserve and ACK cleanup across both
    journal and data segments. Cleanup failure retains replayable state.
    Avoid full-cap rewrite temporary files.
12. Replace all direct connection-writer acquisition with the single measured
    outbound scheduler. Reserve control/heartbeat/ACK capacity; schedule lower
    classes by bounded byte quantum and yield.
13. Flush best-effort during graceful shutdown within a fixed bound and finalize
    coverage/clean-shutdown boundary.

### Crash and hostile-ingest matrix

Inject failure at source-journal reservation, data append/fsync, journal
finalization, merged replay, send, each Hall item/coverage insert, watermark
commit, ACK send, journal/data cleanup, epoch reset, and journal exhaustion. Test
later successful data after a dropped range, duplicate range replay, partial
overlap, pending recovery, malformed batch, duplicate ACK, sequence exhaustion,
source reset, wrong tenant/session, stale/late attempt, concurrent
revocation, duplicate node takeover, spool ENOSPC/read-only, slow peer, and large
simultaneous product+telemetry replay.

### Gate

1. Producer and Hall manifests match source/epoch, consumed data/coverage ranges,
   normalized digests, trace/span IDs, coverage, and ACK watermark exactly.
2. Hall ACK never precedes all-or-nothing commit.
3. Restart replay has no duplicate records and no invented continuity.
4. Heartbeat/control p99 remains below the OBS-0 bound and both spools drain.
5. Secret fixtures are absent from serialized Envoy spool/wire bytes.
6. Permanent session manifest/projection reconciles independently before UI may
   call the telemetry interval complete.

## JOB-OBS — Job telemetry branch

Starts only after ADR 0017 Tasks 2.1–2.4.

1. Bind job records to Hall-derived `(job_id, attempt_epoch, authenticated node)`.
2. Instrument dispatch/process/output/result without duplicating durable output.
3. Reconcile durable JobService attempt/output/result manifests before evaluating
   telemetry coverage.
4. Retry/recovery creates linked new trace; late stale-attempt records are marked
   or rejected by policy.
5. Until this gate passes, existing job spans are labeled non-authoritative
   process observations and no UI says complete.

## OBS-4 — Authorized query service

Requires OBS-3 for remote/session data.

### Work

1. One organization-scoped telemetry query service backs all surfaces.
2. Resolve expected source/attempt set from permanent Hall truth; return permanent
   integrity state separately from telemetry coverage.
3. Reserve/enforce `session.diagnostics.read`; ordinary session read is
   insufficient. Node/system logs require Admin/operator.
4. Add bounded session timeline, trace detail, correlated logs, expected/observed
   sources, coverage, expiry, and current metrics endpoints.
5. Add DTOs only through `server/dto.rs`; update API contract and UI types in the
   same change.
6. Enforce pagination, time-range, row, scan, and response-byte limits.
7. Test wrong org/member/session, stale/revoked principal, revoked live stream,
   broad query cancellation, and hostile trace IDs.

### Gate

No caller-supplied organization controls scope. Permanent-integrity fault and
telemetry incomplete/unknown/expired are distinct response states.

## OBS-5 — Same-origin admin and session diagnostics UI

Requires OBS-4.

### Work

1. Serve server-rendered admin handlers independently of React.
2. Public browser path is `/admin/observability/*` on the Hall/Caddy origin. Use
   Hall secure cookie, validated membership, exact Origin/Fetch Metadata, and
   CSRF for incident export/mutations.
3. Direct loopback `:8788` is explicit operator-credential break-glass/API only;
   reject ambient cookies and installation tokens. Trust forwarded headers only
   from configured edge.
4. Build health/metrics, session timeline, trace waterfall/tree, correlated logs,
   event-tail, coverage/expiry/source warnings, and live tail with bounded queues.
5. Add authorized session Diagnostics deep link in React; it does not become a
   dependency of the server-rendered surface.
6. Escape log bodies and never render known-sensitive fields.

### Gate

Tests cover cross-origin, CSRF, cross-org, non-member, ordinary session reader,
revoked login/stream, installation token, direct-port cookie, and reverse-proxy
identity. Browser evidence covers presentation only, not byte completeness.

## OBS-6 — Optional OTLP and Prometheus interoperability

Requires OBS-1; remote records additionally require OBS-3.

1. Export spans/logs/metrics through bounded queues, timeout, retry, and circuit
   breaker. No exporter is contacted by default.
2. Export failures cannot fail product operations or recursively emit unbounded
   telemetry.
3. Use an in-process/ephemeral OTLP receiver and compare normalized pre-insert
   records with post-reopen exported records including IDs, links, events,
   resources/scopes, typed values, flags/state, status, severity, timestamps, and
   dropped counts.
4. Prove dead exporter memory and log rate remain bounded.

## OBS-7 — Telemetry-only blobs, FTS, and incident bundles

Requires artifact ownership/encryption/retention contract and OBS-4.

1. Add organization-scoped telemetry-only blob namespace; no lifecycle-sharing
   reference with authoritative artifacts.
2. TTL deletion transactionally removes row, FTS entry, blob ref, and expiry
   metadata; idempotent GC journal removes bytes.
3. Add separate `incident.export` capability.
4. Require explicit bundle classification/retention/encryption policy and derive
   org ownership from Hall.
5. Make export failure-atomic and append a permanent manifest/digest audit event,
   never bodies.
6. Test shared telemetry content, cross-class reference rejection, partial export,
   concurrent expiry, session/org deletion, and cross-org digest collisions.

### Gate

An authorized operator exports a failing session, telemetry expires, and the
bundle remains verifiable under artifact policy without keeping hidden telemetry
references alive.

## Final verification

Run repository gates after each card and at the final gate:

```bash
CARGO_TARGET_DIR="$HOME/.cache/olympus-target" cargo test --workspace
CARGO_TARGET_DIR="$HOME/.cache/olympus-target" cargo clippy --all-targets -- -D warnings
cargo fmt --check
cd ui && bun run typecheck && bun run build && bun run test:e2e
```

Data-integrity evidence is mandatory:

- producer and Hall sequence/content manifests for every crash point;
- normalized record digest, trace/span IDs, source epoch, coverage, committed ACK;
- measured idle/normal/burst overhead and p99 product-append/heartbeat latency;
- DB/WAL/temp/spool physical growth and reclamation chart;
- serialized redaction fixture scan;
- OTLP round-trip comparison.

Visual evidence is also required, but only for presentation:

- architecture/dataflow diagram;
- desktop/mobile session diagnostics screenshots/video;
- failed trace, unknown/incomplete/expired states, and authorization denial.

## Dispatch rule

The architecture review is GO with no blocker/P1. Implementation remains
non-dispatchable until PRE-OBS proves ADR 0017 Tasks 1.1–1.4 with the required
artifacts. Any future architecture correction requires focused adversarial
re-review; any new blocker/P1 re-blocks dispatch until resolved or explicitly
accepted by the operator with rationale.
