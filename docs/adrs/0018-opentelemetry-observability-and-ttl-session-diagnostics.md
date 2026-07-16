# ADR 0018 — OpenTelemetry-native observability and TTL session diagnostics

Status: accepted architecture; implementation prerequisite-gated · Date: 2026-07-13

Implementation plan:
[`2026-07-13-otel-observability-session-diagnostics.md`](../plans/2026-07-13-otel-observability-session-diagnostics.md)

Review chain:

- [`0018-opentelemetry-observability-adversarial-review.md`](../reviews/0018-opentelemetry-observability-adversarial-review.md)
  — initial **NO-GO: 3 BLOCKER, 9 P1, 3 P2**.
- [`0018-opentelemetry-observability-adversarial-rereview.md`](../reviews/0018-opentelemetry-observability-adversarial-rereview.md)
  — focused **NO-GO: 1 BLOCKER, 1 P1, 0 P2**; this revision addresses the
  remaining loss-sequence state machine and ADR 0017 Task 1.4 ordering defects.
- [`0018-opentelemetry-observability-final-approval.md`](../reviews/0018-opentelemetry-observability-final-approval.md)
  — **APPROVED / GO: 0 BLOCKER, 0 P1, 1 editorial P2**. The P2 stale terminology
  was removed after approval. Implementation remains blocked on ADR 0017 Tasks
  1.1–1.4 despite architecture approval.

Visual model:
[`observability-dataflow.html`](../diagrams/observability-dataflow.html) ·
[`PNG`](../diagrams/observability-dataflow.png)

Relates to: ADR 0002 (§10B observability), ADR 0008 (Hall/Envoy transport),
ADR 0009 (SQLite substrate), ADR 0010 (Hall auth), ADR 0017 (session cutover and
remote development plane).

## Context

Olympus currently emits structured Rust `tracing` records to process output, and
ADR 0002 reserves a separate operator surface with traces, logs, metrics, an
event tail, and optional OTLP export. Diagnostic telemetry is not durable. A Hall
or Envoy restart therefore destroys the evidence most useful for explaining a
failed session.

Olympus needs LangSmith-class debugging across heterogeneous runtimes without
making LangSmith, an OpenTelemetry Collector, or an external database a runtime
requirement. An operator must be able to determine:

- which turn, tool, job, workflow, node, runtime attempt, and model operation was
  active;
- where time was spent and where an error originated;
- what Hall and Envoy recorded around a failure;
- whether the requested telemetry interval is complete, incomplete, expired, or
  unknown;
- which build and package versions produced the behavior.

The permanent Olympus event log is product and audit truth. Raw logs and detailed
spans are high-volume diagnostic evidence with a different lifecycle. Appending
all telemetry to the permanent journal would couple product history to log
verbosity and make TTL deletion incompatible with append-only truth.

A TTL alone is not a disk bound: a log storm can fill a disk before its oldest
record expires. Retention needs age, physical quotas, and reserved free space.

## Doctrine

**Olympus keeps product and audit truth permanently in its event log; it keeps
correlated diagnostic logs and spans in a separate TTL store; it uses
OpenTelemetry as the telemetry vocabulary and interchange format, not as a
required external service.**

Telemetry may explain permanent truth but never substitute for it. A diagnostics
screen is not an oracle for transcript, job, workflow, deployment, permission, or
capability state.

LangSmith is a product reference for trace exploration, session debugging,
feedback, datasets, and evaluations. Olympus does not depend on the LangSmith
deployment platform.

## Hard prerequisite ordering

Remote telemetry cannot land merely because its own store exists. The executable
dependency is:

```text
ADR 0017 Task 1.1
atomic session payload/reference + transport watermark persistence
  -> ADR 0017 Tasks 1.2–1.4
     authoritative runtime-attempt inventory + unified runtime controls +
     authenticated peer/OS identity to logical-node binding and takeover rejection
       -> OBS local telemetry model/store + Hall-only instrumentation
         -> authenticated telemetry source/epoch/record protocol
           -> Envoy telemetry transport and query claims

ADR 0017 Tasks 2.1–2.4
Envoy job-attempt truth + terminal spool correctness + durable JobService
  -> job-output/result telemetry and complete job timelines
```

Before the first chain completes, Hall-local instrumentation may exist, but no
remote session telemetry gate may claim completeness. Before the second chain
completes, job telemetry is explicitly a non-authoritative process observation;
it cannot claim a complete job timeline.

Every session/job diagnostics query reconciles permanent producer/attempt
manifests and durable product projections first, then independently evaluates
telemetry coverage. Missing permanent truth is shown as a product-integrity
fault, never papered over by available TTL records.

## Decision

### 1. OpenTelemetry is the canonical telemetry model

Olympus uses Rust `tracing` as its native emission interface and maps it to OTel:

- traces/spans for bounded operations;
- structured log records correlated to trace and Olympus resources;
- counters, gauges, and histograms;
- W3C trace context where a text carrier exists;
- OTLP as an optional export adapter.

OpenTelemetry is a seam, not a second kernel. Olympus starts and remains useful
without a Collector or network dependency.

A versioned internal contract is checked into `crates/proto` before telemetry DB
migrations. It can represent, without JSON type loss:

- OTel `Resource` and schema URL;
- `InstrumentationScope` and schema URL;
- `SpanData`: 16-byte trace ID, 8-byte span ID, parent, kind, trace flags/state,
  name, start/end, status, typed attributes, events, links, and dropped
  attribute/event/link counts;
- `LogRecord`: event and observed timestamps, severity number/text, trace/span
  context, trace flags, typed body, typed attributes, resource/scope, and dropped
  counts;
- accepted timestamp ranges and explicit unsupported-field behavior.

Round-trip property tests compare a normalized record before persistence with the
record received by an OTLP test collector after SQLite reopen and replay. Rust
OTel logs and metrics are currently beta; crate maturity may change the adapter,
not this storage contract.

### 2. A session is correlation scope, not one trace

A long-lived session MUST NOT be one trace. Each execution **attempt** is bounded:

- an interactive turn attempt;
- runtime start/resume/stop attempt;
- job attempt;
- workflow-run attempt;
- deployment attempt;
- managed-app lifecycle attempt;
- inbound API/MCP request not already inside another root.

Operation identity and attempt epoch are separate. Retry, reconnect recovery, or
post-crash recovery creates a new root trace linked to the previous attempt;
replay never reopens or reparents an ended trace. Late telemetry is attached to
the original attempt and marked late. Abandoned roots are finalized on recovery
as `UNSET/abandoned` with a coverage boundary, not silently left open forever.

Initial per-trace safety limits are configurable and default to:

```toml
max_spans = 4096
max_events = 8192
max_links = 1024
max_attributes_per_record = 128
max_elapsed = "24h"
max_inline_body = "64KiB"
```

Overflow truncates or starts a linked continuation trace and records positive
coverage loss. It never grows an unbounded in-memory trace.

Every record carries or is joined to Hall-derived correlation:

```text
org_id, project_id, session_id, turn_id
runtime_attempt_id, authenticated node_id
job_id + job_attempt, workflow_run_id, deployment_id + attempt
agent_kind, model, build digest, package digest
```

### 3. Product truth, diagnostics, and metrics are distinct

| Data class | Examples | Authority | Retention |
|---|---|---|---|
| Product/audit truth | transcript, tool outcome, permission/capability decision, session/job/workflow/deployment lifecycle, product-integrity and telemetry-store-reset incident facts | `olympus.db` append-only event log + projections | permanent under owning resource policy |
| Diagnostic telemetry | Hall/Envoy/runtime logs, protocol diagnostics, retries, span timing, stack/error detail, resource observations | `telemetry.db` + telemetry-only blobs | TTL + physical quotas |
| Live metrics | counters, gauges, histograms, health | in-process registry; optional OTLP/Prometheus export | current process lifetime locally in v1 |

A durable transition is never recoverable only from TTL logs. Verbose diagnostics
never enter the permanent journal merely because they are useful in an incident.
A low-volume permanent `TelemetryStoreReset/Disabled/Loss` fact records that
evidence was unavailable without copying telemetry payloads into product truth.

Transcripts remain permanent and are not duplicated into logs. Tool arguments or
results already captured as product records are referenced by ID/digest. Child
stderr and supervisor messages are diagnostics. ACP stdout is protocol, not a log
stream; raw protocol bodies require an explicit diagnostic-capture grant.

### 4. Hall owns a separate disposable SQLite store

V1 uses `~/.olympus/telemetry.db`, independent from `olympus.db`, with its own WAL,
migrations, retention, backup exclusion, and corruption recovery. Deleting it
must degrade debugging without affecting product operation.

Minimum logical schema:

```text
telemetry_generations
  generation_id PK, created_at, reason

telemetry_sources
  (enrolled_source_id, source_epoch) PK
  authenticated_node_id, generation_id
  committed_through_seq, first_observed_at, last_observed_at

telemetry_records
  (enrolled_source_id, source_epoch, item_first_seq) PK
  item_last_seq // equal to first for data; range end for coverage
  deterministic_record_id UNIQUE
  record_kind: span | log | coverage
  org_id, project_id, session_id, turn_id, runtime_attempt_id, node_id
  job_id, job_attempt, workflow_run_id, deployment_id, deployment_attempt
  trace_id, span_id, timestamp_ns, observed_at_ns, expires_at_ns
  normalized_payload_or_blob_ref, payload_digest

telemetry_coverage
  (enrolled_source_id, source_epoch, first_seq, last_seq) PK
  generation_id, org_id, session_id, runtime_attempt_id
  status: received | dropped | expired | unknown
  reason, observed_at_ns, expires_at_ns
```

All logs and completed spans (including embedded span events/links) consume one
source position. A range-consuming coverage item consumes every position from
`item_first_seq..item_last_seq`. All items have deterministic IDs. Query
projections/FTS may be derived, but the normalized items and coverage ranges are
the disposable telemetry truth.

The DB uses one bounded writer executor and a bounded read-only connection pool.
Reads have statement progress cancellation, time/range/row/response-byte limits,
and short transactions. Pruning uses short batches. One owner coordinates
checkpoints. Quarantine/reopen increments a store generation; old handles are
fenced and cannot write into the replacement.

Explicit policies cover BUSY, IOERR, FULL, CORRUPT, corrupt WAL, failed migration,
shutdown with queued batches, and cancellation. A long query or prune cannot hold
the sole writer connection as the permanent `Log` currently does.

Producer timestamps describe occurrence but do not control retention. Hall uses a
persisted **retention clock**: monotonic elapsed time within a boot and a
nondecreasing persisted checkpoint across restarts. A wall-clock discontinuity
beyond the configured tolerance pauses expiry and marks retention health degraded
until time is stable or an operator acknowledges it. Backward jumps cannot extend
retention silently; forward jumps cannot mass-expire data immediately.

Organization is part of record identity and every query key. External trace IDs
cannot join or overwrite another organization's trace.

### 5. One ordered source sequence, range-consuming coverage, atomic batches

A telemetry producer has an `enrolled_source_id` derived by Hall from authenticated
peer enrollment. A random `source_epoch` is created and durably registered through
an authenticated reset/start protocol. It is persisted beside the Envoy spool and
changes only when that protocol authorizes a new stream.

Every admitted observation receives a source position. Data consumes one
position; a coverage item may consume a contiguous range of positions that could
not be retained as data. The durable wire unit is:

```text
TelemetryBatch {
  enrolled_source_id,
  source_epoch,
  first_seq,
  last_seq,
  items[] // ordered; each consumes item_first_seq..item_last_seq
}

TelemetryAck {
  enrolled_source_id,
  source_epoch,
  through_seq
}
```

Rules:

1. A preallocated durable **source journal** is the sequence authority. Before
   attempting a data-spool append, Envoy fsyncs a journal reservation containing
   source range, expected item count/digests, and state `pending`. No journal
   reservation means no sequence was consumed.
2. Envoy appends and fsyncs the complete data batch, then finalizes the journal
   range as `received`. If data append/fsync fails, it finalizes the same reserved
   range as `dropped{reason}`. If finalization itself fails, durable `pending`
   means `unknown`, never complete.
3. Recovery merges source journal and segmented data spool deterministically. A
   valid complete data batch matching a pending reservation wins and finalizes
   `received`; pending without valid complete data becomes range coverage
   `unknown`. A finalized dropped range emits one coverage item consuming the
   whole reserved range. The range is never reused.
4. Wire items are ordered and non-overlapping. The first item starts at
   `first_seq`; each next item starts at the prior `item_last_seq + 1`; the last
   item ends at `last_seq`. A data item has equal first/last; a coverage item may
   span many positions. Replay merges data and coverage items in this order.
5. Hall validates authenticated source/ownership, range contiguity, deterministic
   IDs, digests, size, and redaction envelope. It inserts every item, coverage,
   and the new contiguous watermark in one transaction. A coverage item advances
   the watermark through its `item_last_seq`. Partial/corrupt batches are rejected
   and not ACKed.
6. Hall ACKs only committed contiguous `through_seq`. A wholly duplicate batch is
   ACKed without reinsertion only when every item range/ID/digest equals committed
   state. Partial overlap or same range with different identity is rejected.
7. ACK cleanup advances both the source journal and data segments through the
   acknowledged position. Duplicate ACK is idempotent. Cleanup failure leaves
   replayable state and cannot advance local deletion metadata.
8. The journal has capacity and filesystem reserve outside telemetry data. At
   journal high water Envoy stops admitting observations before exhaustion and
   enters `coverage_unknown`; unadmitted observations consume no positions and
   cannot later be called dropped by sequence. Hall learns the unknown interval
   from source health/reconnect and the permanent low-volume loss fact. Absence of
   a coverage item never means complete.
9. Sequence exhaustion, epoch reset, corrupt journal/data, ACK failure,
   read-only/ENOSPC/fsync failure, and producer crash have explicit outcomes and
   never invent continuity.

Crash-point tests cover journal reservation, data append/fsync, journal
finalization, merged replay, send, every Hall insert stage, watermark commit, ACK
send, and cleanup of both stores. They include later successful data after a
dropped range, duplicate range replay, partial overlap, journal exhaustion, and
pending recovery. Producer and Hall manifests compare source/epoch, consumed data
and coverage ranges, normalized digests, trace/span IDs, and watermark.

### 6. Hall derives tenancy and runtime ownership at ingest

No Envoy-provided `org_id`, `node_id`, `session_id`, `turn_id`, or
`runtime_attempt_id` is authority.

Hall derives/validates ownership from:

1. authenticated iroh peer/UDS credentials;
2. durable enrollment binding peer key/OS identity to logical node and source;
3. permanent session/job/runtime-attempt projections;
4. current revocation and organization policy.

Hall rewrites correlation fields to canonical values or rejects the record. It
rejects unknown source epochs, duplicate-node takeover, wrong-org/session,
stale/mismatched attempt, revoked node/source, and unauthorized diagnostic
capture. Terminal attempts may accept late telemetry only during a bounded grace
period while source and attempt ownership still validate; records are marked
late. Archived/revoked resources do not accept new session diagnostics.

Node-wide Hall/Envoy logs live in an operator-only system scope. Olympus never
invents a session or organization for them.

This depends specifically on ADR 0017 Tasks 1.2–1.4: authoritative runtime
attempts/controls plus peer-key or OS-identity to logical-node binding and
takeover rejection. Query authorization cannot repair poisoned ingest metadata.

### 7. Positive coverage defines completeness

Completeness is not “no gap row found.” For a requested interval Hall identifies
the expected authenticated producer/attempt set from permanent truth, then
compares positive source manifests and coverage intervals.

The result is one of:

- `complete`: every expected source proves contiguous received coverage;
- `incomplete`: a known dropped/expired/truncated range intersects the request;
- `unknown`: expected coverage cannot be proven, including DB generation reset,
  process death before flush, missing source manifest, or unrecoverable ledger
  failure;
- `expired`: the requested interval is wholly outside configured retention and no
  incident bundle was exported.

A new/quarantined telemetry DB generation starts `unknown`. Hall-local channel
loss and Envoy loss advance explicit dropped ranges where possible. Coverage is
itself TTL metadata, but the generation boundary and permanent low-volume reset/
loss incident fact prevent false completeness after it disappears.

The UI always displays coverage state and expected/observed sources. It never
presents an incomplete or unknown trace/log range as complete.

### 8. Retention is TTL plus physical quotas and reserves

Initial Hall defaults:

```toml
[telemetry]
enabled = true
retention = "30d"
max_bytes = "5GiB"
max_session_bytes = "256MiB"
min_free_bytes = "2GiB"
transaction_headroom = "512MiB"
prune_interval = "15m"
clock_step_tolerance = "5m"
```

`max_bytes` accounts for DB, WAL/SHM, SQLite temp/headroom, and telemetry-only
blobs—not logical payload sums. `min_free_bytes` is reserved for `olympus.db` and
normal Hall operation. Hall stops telemetry admission before entering the reserve.

The Envoy telemetry spool has a separate physical budget and reserve on the
filesystem containing product spool/runtime state. V1 uses segmented append-only
files so ACK cleanup deletes complete segments and bounded tail compaction never
needs a full-cap duplicate rewrite. Admission accounts for current segments,
compaction temp, source epoch, and the preallocated source journal. At high
water telemetry sheds TRACE/DEBUG; at critical water it stops telemetry before
entering the product reserve.

Expired rows are excluded immediately and physically deleted in bounded batches.
Incremental auto-vacuum and bounded WAL checkpoints make pages reusable and
reclaim space opportunistically. Physical-file measurements, not SQL length sums,
drive quota state.

Eviction order is expired → oldest TRACE/DEBUG → INFO → WARN/ERROR. Forced loss
updates metrics and coverage when possible. Telemetry never blocks or exhausts
storage required for permanent product append. This is verified with ENOSPC,
read-only, temp-space, WAL growth, checkpoint failure, slow SQLite, and
simultaneous product-append tests in the store/transport gates—not deferred.

TTL is not cryptographic erasure. Secrets and unnecessary sensitive bodies must
not be recorded in the first place.

### 9. Producer-side redaction is a serialization firewall

Hall and Envoy each own typed safe telemetry emission:

- stable error code;
- allowlisted typed fields;
- bounded scrubbed message;
- optional opaque diagnostic digest.

A final default-deny validator runs immediately before local persistence, Envoy
spool append, or telemetry wire serialization. Child stderr passes through a
streaming bounded scrubber and is never retained raw. Existing arbitrary response
errors are not automatically promoted into diagnostic telemetry.

Fixtures cover authorization/cookies/MCP tokens, env assignments, credentialed
URLs, launch codes, identity tickets, PEM/JWT/API keys, prompts, reasoning, and
tool bodies. Tests byte-scan serialized spool and wire fixtures.

Explicit raw diagnostic capture requires a separate capability, scope, duration,
size budget, permanent audit event, and conspicuous UI indicator. It is not a
boolean escape hatch and never bypasses known-secret filtering.

Redaction failure drops the sensitive field and increments a bounded metric; it
does not fail the product operation.

### 10. One fair outbound scheduler owns each connection writer

Direct lock acquisition by heartbeat, response, live events, or replay is
forbidden. One outbound scheduler task owns the connection writer.

Traffic classes:

1. control, heartbeat, and ACK — reserved bounded queue and deadline;
2. live permanent product events;
3. permanent product replay;
4. live telemetry;
5. telemetry replay.

Scheduling is by byte quantum, not frame count. Frames/batches have hard byte
limits; replay sends one bounded quantum then yields. Every producer queue is
bounded with explicit backpressure/drop policy. Product/control queues never
share telemetry capacity.

The implementation spike selects measured queue sizes and quanta. The acceptance
oracle is a stated p99 heartbeat/control latency below the Hall liveness budget
under simultaneous product replay, telemetry replay, live output, ACK flood, slow
peer, and reconnect, while both spools eventually drain. Freeing the read loop,
as postmortem 0021 did, is necessary but not sufficient fairness.

### 11. Metrics remain low-cardinality

V1 metrics live in process and are optionally exported. Olympus does not build a
local time-series database.

Metric label keys are compile-time allowlisted. Each family has a finite
normalized value vocabulary, maximum series budget, and `other`/`invalid` bucket.
Producer/user-extensible model, tool, agent, or package names never dynamically
register new series. Rejection metrics also use fixed labels.

Forbidden labels include session/trace IDs, prompts, paths, free-form errors,
tool arguments, and arbitrary user/package values. Unicode/confusable and
continually novel values are tested. Exemplars may link a sample to a trace
without becoming labels.

Required families cover uptime/build, latency, active bounded states, queue/spool
depth/age, DB writes/errors/bytes, telemetry accepted/dropped/evicted/unknown,
heartbeat/reconnect, cgroup resources, and normalized model/tool/job/workflow
terminal classes.

Long-term metric history belongs in an optional backend. Session historical timing
comes from TTL spans.

### 12. Telemetry blobs, FTS, and incident bundles have explicit ownership

Telemetry bodies use a telemetry-only, organization-scoped blob namespace; they
do not share lifecycle references with authoritative artifacts. TTL deletion
transactionally removes the telemetry row, FTS entry, blob reference, and expiry
metadata, with an idempotent blob-GC journal for physical deletion.

Incident export requires `incident.export`, separate from
`session.diagnostics.read`. The bundle derives organization ownership from Hall,
requires explicit classification and retention, follows encryption-at-rest policy,
and records a permanent audit event containing manifest/digests—not bodies.
Export is failure-atomic. Shared content, partial export, concurrent expiry,
org/session deletion, and cross-org digest collision are tested.

There is no hidden telemetry `pinned` flag. Preservation is an explicit copy into
an authoritative artifact.

### 13. Query and admin authentication are one Hall policy

The public browser route is same-origin under Hall/Caddy:

```text
/admin/observability/
/admin/observability/traces
/admin/observability/traces/:id
/admin/observability/logs
/admin/observability/events
/admin/observability/sessions/:id
```

Caddy routes that path to the server-rendered admin handler, which remains
independent of the React build. It uses the normal Hall secure session cookie,
validated organization membership, exact public Origin/Fetch Metadata rules, and
CSRF protection for incident export or other mutations. Organization never comes
from an untrusted query parameter.

Direct loopback `:8788` is an operator break-glass/API surface only. It does not
accept ambient browser cookies or provide a second login origin; it requires an
explicit operator credential and denies installation tokens for diagnostics.
Forwarded headers are trusted only from the configured edge. Remote exposure
requires TLS through Caddy.

System-wide node/Hall diagnostics require Admin/operator authority. Session
records additionally require `session.diagnostics.read`; incident export requires
`incident.export`. Revocation terminates live streams and denies subsequent reads.
Tests cover cross-origin, CSRF, cross-org, non-member, ordinary session reader,
revoked login, installation token, direct-port cookie, and reverse-proxy identity.

### 14. LangSmith-class features build above this substrate

Future feedback, datasets, evaluators, regression runs, prompt/package
comparisons, and failure clustering are product resources with event-backed
lifecycles. They consume telemetry but do not make raw telemetry permanent truth.

LangChain/LangGraph or other managed applications may ingest/export OTLP. Their
agent loops and internal graph checkpoints remain external and do not become
Olympus workflow semantics.

## Rejected alternatives

- **Embed or require LangSmith:** vendor and operational topology mismatch.
- **Require an OTel Collector:** supported adapter, not local-debug prerequisite.
- **In-memory ring as retention:** loses post-crash evidence; remains live cache.
- **Append telemetry to permanent journal:** wrong lifecycle and volume.
- **One trace per session:** unbounded lifecycle and query cost.
- **TTL without physical quotas/reserves:** cannot prevent pre-expiry disk fill.
- **Trust producer tenancy fields:** query filtering cannot repair poisoned data.
- **Absence of gaps means complete:** failures can erase the gap evidence itself.
- **Shared writer mutex with informal priority:** repeats replay starvation risk.

## Consequences

- Olympus gains restart-surviving session diagnostics without changing product
  truth semantics.
- ADR 0017 session/job durability is a hard predecessor, not adjacent work.
- Hall adds a disposable SQLite database; Envoy adds a separately sequenced,
  segmented telemetry spool and fair outbound scheduler.
- The protocol and coverage model are more work than an in-memory ring, but they
  make ACK, replay, completeness, and loss claims testable.
- OTLP permits external backends without making one canonical.
- Local historical metrics remain deliberately limited.
- Incident preservation is explicit and separately authorized.

## Implementation gates

Architecture review confirms that the ADR and plan enforce:

1. permanent session/job truth before corresponding telemetry claims;
2. authenticated source/epoch ordered sequencing, range-consuming coverage,
   all-or-nothing batch commit, contiguous ACK, and durable source journal;
3. Hall-derived tenant/session/runtime-attempt ownership at ingest;
4. physical Hall/Envoy accounting, reserves, and real disk-failure tests;
5. one byte-quantized fair writer scheduler with heartbeat latency oracle;
6. positive complete/incomplete/unknown/expired coverage semantics;
7. producer-side default-deny redaction proven on serialized bytes;
8. versioned OTel contract round-tripping through SQLite and OTLP;
9. explicit same-origin admin and break-glass `:8788` policy;
10. exact source/Hall manifests at every crash point.

Implementation dispatch nevertheless remains blocked until PRE-OBS proves ADR
0017 Tasks 1.1–1.4 are implemented and supplies the named permanent-ingress,
runtime-attempt/control, and peer-identity binding artifacts. Architecture GO is
not implementation GO.
