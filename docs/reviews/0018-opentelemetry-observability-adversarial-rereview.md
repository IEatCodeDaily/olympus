# ADR 0018 OpenTelemetry observability — focused adversarial re-review

**Review date:** 2026-07-13  
**Verdict:** **NO-GO — 1 BLOCKER, 1 P1, 0 P2.**  
**Scope:** the current rewritten ADR 0018 and implementation plan, reconciled
against every finding in the initial 3-BLOCKER/9-P1/3-P2 review and checked
against ADR 0017, its implementation plan, and the still-current Hall/Envoy
transport source. This is a proposal review, not an implementation review.

## Executive verdict

The rewrite is a substantial correction. It now clearly separates permanent
product/audit truth from disposable telemetry; makes ADR 0017 durability a hard
predecessor; defines Hall-derived ingest authority; specifies a versioned OTel
contract, bounded attempts, physical disk protection, SQLite ownership,
producer-side redaction, a byte-fair writer, positive coverage, same-origin
admin authentication, low-cardinality fallback budgets, a retention clock,
incident ownership, and exact manifest gates.

One original blocker is nevertheless not fully resolved. The loss-ledger path
has no coherent sequence/ACK state machine: the text simultaneously rolls back a
failed sequence reservation, calls the ledger a ledger of dropped *sequence
ranges*, requires every wire record to carry an exact contiguous `record_seq`,
and permits Hall to advance only a contiguous watermark. It never defines how a
range that did not enter the data spool is represented in that contiguous wire
stream or atomically merged with later data. This is precisely the disk-full
path on which positive completeness depends.

There is also one remaining P1 ordering defect: ADR 0017 Task 1.4 owns the durable
peer-key/logical-node binding required for authenticated ingest, but the ADR
0018 dependency chain and PRE-OBS gate require only Tasks 1.1–1.3. OBS-3 then
tries to complete that prerequisite inside the telemetry card. Safety is stated,
but executable ownership and predecessor status are inconsistent.

No implementation card is dispatchable under the plan's own dispatch rule until
the blocker is corrected and re-reviewed. Independently, the current source
still demonstrates that ADR 0017's prerequisite gates have not landed, so remote
telemetry cannot start even after the document blocker is repaired.

## Remaining findings

### [BLOCKER-1] Dropped-range coverage cannot be reconciled with the contiguous record/ACK protocol

**Evidence**

- ADR 0018 requires every wire record to have an exact contiguous `record_seq`
  and defines a batch range plus a contiguous ACK
  (`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:251-268`).
- It says sequence reservation plus complete-batch append is atomic or the
  reservation is rolled back (`:272-274`). A failed append therefore leaves no
  committed source sequence range to call “dropped.”
- It simultaneously requires a separately capped dropped-range ledger to record
  sequence ranges that could not enter the data spool (`:284-287`). If those
  sequence numbers are retained as consumed, the next data batch contains a
  hole; if they are rolled back, those numbers may be reused and cannot identify
  the dropped observations.
- Hall accepts only all-record contiguous batches and advances only the contiguous
  watermark (`:275-283`). No rule lets a coverage tombstone occupy a missing
  sequence, lets one range record advance over several positions, or merges the
  data spool and loss ledger in source-sequence order.
- The schema gives each normalized coverage record the same single-record primary
  key as logs/spans while separately giving `telemetry_coverage` a
  `first_seq,last_seq` range (`:203-217`). The relationship between a range and
  the source positions it replaces is unspecified.
- OBS-3 repeats per-record contiguous sequencing, atomic complete-batch append,
  the exact batch/ACK shape, and the dropped-range ledger
  (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:226-243`),
  but its crash matrix and exact-manifest gate do not choose either representation
  (`:250-268`).

**Failure sequence**

Envoy prepares ten admitted records for source positions 41–50. Appending the
complete data batch fails with ENOSPC. Under the rollback rule, the next durable
record may reuse 41, so the loss ledger cannot truthfully report dropped source
range 41–50. If Envoy instead commits 41–50 to the loss ledger and later sends a
coverage record at 51, Hall rejects the batch because its committed watermark is
40 and records 41–50 are absent. If one coverage record is assigned 41 and says
it covers 41–50, the current batch contract still requires exact records for
42–50 and gives no rule for advancing/ACKing through 50. In all cases, either
transport stalls or completeness is invented/lost on the exact storage-failure
path the design claims to cover.

**Required correction**

Choose and specify one state machine end to end. For example:

1. **Per-position tombstones:** allocation commits each admitted position to
   exactly one of the data spool or the preallocated loss journal; the loss
   journal durably stores a coverage tombstone for every consumed position;
   replay merges both stores in strict sequence order; Hall inserts tombstones
   like any other record and advances/ACKs normally; or
2. **Range-consuming coverage records:** define a coverage wire record with
   explicit `covers_first_seq..covers_last_seq` semantics that occupies that
   whole source range, update batch contiguity and watermark rules accordingly,
   and atomically validate/store the range and advance through its end.

In either design, specify atomic allocation across the data spool and loss
journal, ordering when both contain adjacent records, overlap/dedup rules,
manifest representation, ACK cleanup of both stores, ledger exhaustion, and
what happens when the ledger itself cannot fsync. Add crash tests for failure of
a multi-record append after reservation, successful later data, merged replay,
duplicate range replay, partial overlap, and ACK cleanup. The producer/Hall
manifests must prove both received payload positions and explicitly lost
positions.

### [P1-1] ADR 0017 Task 1.4 is required by ingest authority but is not a hard predecessor

**Evidence**

- ADR 0018's hard ordering names Task 1.1 and Tasks 1.2–1.3 before the authenticated
  telemetry protocol, but omits Task 1.4
  (`docs/adrs/0018-opentelemetry-observability-and-ttl-session-diagnostics.md:65-82`).
- The same ADR later makes durable enrollment binding peer identity to logical
  node/source a basis of ingest authority and explicitly says the design depends
  on ADR 0017's node-key/logical-ID binding (`:293-316`).
- That binding is owned by ADR 0017 **Task 1.4**, not Tasks 1.1–1.3
  (`docs/plans/2026-07-13-session-cutover-remote-development.md:152-171`).
- The OBS dependency graph and PRE-OBS evidence/gate still require only Tasks
  1.1–1.3 (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:26-35,50-71`).
- OBS-3 work item 1 then says to complete peer-key/OS-identity enrollment before
  the telemetry card branches (`:216-225`). This protects the intended runtime
  behavior if followed serially, but duplicates/moves an ADR 0017 prerequisite
  into OBS-3 and contradicts PRE-OBS's definition of the verified predecessor.
- Current Hall still accepts the hello's logical `node_id` and registers it with
  the optional peer ID rather than resolving the logical ID from a durable
  enrollment binding (`crates/control-plane/src/node.rs:623-701`), confirming
  this is a real open prerequisite rather than an editorial task number.

**Required correction**

Add ADR 0017 Task 1.4's peer-key/OS-identity → logical-node binding and takeover
rejection to the ADR 0018 hard dependency chain, the executable graph, PRE-OBS
evidence, and the “no remote telemetry” rule. OBS-3 should consume and verify
that completed prerequisite rather than own a second implementation of it. If
only the identity subset of Task 1.4 is required, give that subset a separately
named ADR 0017 gate and artifact ID so dispatch cannot interpret the dependency
differently.

## Reconciliation of all initial findings

| Initial finding | Re-review status | Current evidence / disposition |
|---|---|---|
| BLOCKER-1: telemetry ordered before permanent session truth | **Resolved** | ADR hard ordering and permanent-first reconciliation (`ADR 0018:65-92`); executable graph and PRE-OBS (`plan:26-71`); independent product reconciliation in OBS-3 gate (`plan:261-268`). Current ADR 0017 implementation remains a predecessor, so implementation cannot start yet. |
| BLOCKER-2: batch/sequence/dedup/gap inconsistency | **Partially resolved; still BLOCKER** | Successful-record identity, all-or-nothing transaction, duplicate replay, and ACK are now explicit (`ADR:192-223,244-291`; `plan:226-268`). The dropped-range branch remains internally incoherent as detailed in BLOCKER-1 above. |
| BLOCKER-3: producer-supplied tenancy authority | **Resolved architecturally, with P1 ordering defect** | Hall derives/validates source, node, org, session, turn, and attempt and rejects hostile ingest (`ADR:293-316`; `plan:222-239,250-257`). Task 1.4 must be made a hard predecessor as detailed in P1-1. |
| P1-1: Hall/Envoy physical disk safety | **Resolved** | Hall accounts DB/WAL/SHM/temp/headroom/blobs and reserves product space; Envoy has segmented physical accounting, compaction headroom, separate reserve, and early shedding/stop (`ADR:342-379`; `plan:100-114,147-185,242-243`). Disk faults are in OBS-0/1/3 gates, not deferred. |
| P1-2: informal weighted writer priority | **Resolved** | One writer-owner, reserved control capacity, bounded queues/frames, byte quanta, forced yield, p99 liveness oracle, and simultaneous replay/drain test (`ADR:409-431`; `plan:111-114,244-265`). |
| P1-3: absence of gap treated as completeness | **Concept resolved; loss encoding still blocked** | Expected producer set, positive coverage, complete/incomplete/unknown/expired, generation reset, and permanent reset/loss facts are explicit (`ADR:318-340`; `plan:147-191,289-305`). The remaining blocker is how disk-loss coverage enters the contiguous sequence protocol, not the query-state vocabulary. |
| P1-4: redaction boundary aspirational | **Resolved** | Typed producer APIs and default-deny firewall exist for both Hall and Envoy; stderr is stream-scrubbed; raw protocol is excluded; serialized spool/wire bytes are scanned with hostile fixtures (`ADR:384-407`; `plan:93-99,238-239,250-266`). |
| P1-5: schema cannot round-trip OTel | **Resolved** | Versioned internal contract covers resources/scopes/schema URLs, IDs, flags/state, typed values/body, events/links/status/dropped counts/timestamps; SQLite reopen → OTLP receiver comparison is explicit (`ADR:109-125`; `plan:82-92,175-177,332-344`). |
| P1-6: SQLite concurrency/failure undefined | **Resolved** | Bounded writer executor/read-only pool, cancellation and query limits, short prune transactions, checkpoint owner, generation fence, and BUSY/FULL/IOERR/CORRUPT/WAL/migration/shutdown races are specified and gated (`ADR:224-239`; `plan:100-106,147-185`). |
| P1-7: blob/FTS/incident ownership | **Resolved** | Org-scoped telemetry-only blobs, transactional row/FTS/ref/expiry removal, idempotent GC journal, separate export capability, Hall-derived ownership, classification/retention/encryption, failure atomicity, and audit manifest are explicit (`ADR:456-471`; `plan:346-366`). |
| P1-8: bounded operation lacks attempt lifecycle | **Resolved** | Attempts are roots; retries/recovery link new roots; replay cannot reopen; late/abandoned behavior and concrete span/event/link/attribute/body/duration limits are defined and tested (`ADR:127-157`; `plan:168-184,270-281`). |
| P1-9: `:8788` authentication unresolved | **Resolved** | Browser UI is same-origin under Hall/Caddy with cookie/membership/origin/Fetch Metadata/CSRF; direct loopback 8788 is operator-credential-only and rejects cookies/installation tokens; forwarded-header and revocation tests are explicit (`ADR:473-502`; `plan:307-330`). |
| P2-1: metric fallback/budget undefined | **Resolved** | Compile-time keys, finite vocabularies, per-family series budgets, fixed `other`/`invalid`, fixed-label rejection metrics, and Unicode/novel-name tests (`ADR:433-451`; `plan:193-214`). |
| P2-2: Hall clock discontinuity undefined | **Resolved** | Persisted nondecreasing retention clock, monotonic in-boot elapsed time, pause/degraded state on large steps, operator acknowledgement, and forward/backward/restart tests (`ADR:234-239`; `plan:160-162,175-184`). |
| P2-3: visual/weak dedup verification | **Resolved** | Exact source/Hall manifests include epoch, sequence, normalized digest, trace/span IDs, coverage, and watermark at every crash point; screenshots are presentation-only (`ADR:289-291`; `plan:250-268,368-392`). |

## Internal consistency checks beyond keyword presence

- **Permanent truth vs telemetry:** consistent. Queries resolve permanent
  integrity first and expose it separately from disposable coverage
  (`ADR:89-92,168-184`; `plan:289-305`).
- **Successful all-or-nothing batch/ACK path:** coherent for logs, completed spans
  with embedded events/links, and ordinary coverage records. All share the same
  source/epoch/record key and deterministic ID (`ADR:192-223,244-283`).
- **Hall-derived ingest authority:** the validation/rejection contract is strong;
  only predecessor ownership needs repair (`ADR:293-316`).
- **Hall and Envoy disk safety:** physical rather than logical accounting,
  filesystem reserve, compaction/temp headroom, and product-append latency oracle
  are now in the earliest spike/store/transport gates (`ADR:342-379`; `plan:100-133`).
- **Writer fairness:** scheduling is byte-quantized and owned by one task; the
  test load exceeds a mere class-name assertion (`ADR:409-431`; `plan:111-114`).
- **Producer redaction and OTel fidelity:** both are tested at serialized/reopened
  boundaries, not merely at API objects (`plan:90-99,127-133,175-177,340-344`).
- **SQLite and retention:** writer/read/checkpoint/generation ownership and clock
  discontinuity outcomes are implementation-grade (`ADR:224-239`).
- **Blobs/FTS/export:** TTL and authoritative artifact lifecycles cannot share a
  hidden reference; export has separate authority and audit (`ADR:456-471`).
- **Bounded attempts:** root/continuation/recovery/late/abandoned semantics agree
  across ADR and plan (`ADR:127-157`; `plan:168-184,270-281`).
- **Admin/direct-port auth:** there is one Hall policy, one browser origin, and a
  non-cookie break-glass direct port (`ADR:473-502`).
- **Metrics, exact manifests, and visual evidence:** fallback series are bounded;
  byte/data integrity uses exact manifests; visual evidence is correctly limited
  to presentation (`plan:193-214,250-268,379-392`).

## Implementation-start decision

**Implementation may not start.** The plan's dispatch rule blocks every card
while a review blocker exists (`docs/plans/2026-07-13-otel-observability-session-diagnostics.md:394-399`). After BLOCKER-1 is repaired, P1-1 must also be resolved or explicitly accepted with rationale under the same rule.

Even with those document corrections, current source does not satisfy ADR 0017:

- Hall still persists only the session watermark before forwarding a payload to a
  process-local channel and ACKing (`crates/control-plane/src/log.rs:129-157`;
  `crates/control-plane/src/server/envoy_conn.rs:157-173`), so Task 1.1 remains
  open.
- Hall still registers the hello-provided logical node ID rather than deriving it
  from a durable peer enrollment (`crates/control-plane/src/node.rs:623-701`), so
  the required Task 1.4 identity subset remains open.
- The current product spool still allocates in memory before append and persists
  its counter separately, and ACK cleanup still rewrites a temporary full retained
  file (`crates/envoy/src/spool.rs:43-92,119-141`), reinforcing that ADR 0017's
  job/session spool gates are prerequisites, not reusable telemetry machinery.

Paper design may continue exactly as PRE-OBS permits, but no OBS implementation
card or remote telemetry/query claim should be dispatched.

## Reviewed snapshot

Repository HEAD at review start: `f784b04aa6d04e6759eef999012f48ac3f0f1622`.
The worktree was dirty and the ADR/plan/review documents were untracked. This
report therefore identifies immutable content hashes rather than claiming a
clean commit:

- ADR 0018: `2f74b81c922f4644aaa01eb95fe0f30445d134ef3041a667af6d55a13d8081af`
- OBS plan: `5e84ac09f4830f3e5cb094461afc4c772c13124d9020626972a5b28b3e1fedaf`
- Initial ADR 0018 review: `ed6341af6c88877275f49d80d655500ed7d6705d2187a46c8072d79723f5156d`
- ADR 0017: `02d6cdfbd5516831e8131c442725b125f2216ab8440f570362d95290e987842d`
- ADR 0017 plan: `22577cf643520bb6fc4aaf92d0c1f4eeb274260518fe31dff84ce0012437b4e2`
- Hall `log.rs`: `b6c87e0e8622081c5cf6db2a7a36545499dd7e9ca50ad13eeca5a1cb888dd84b`
- Hall `envoy_conn.rs`: `3f15c50254987094848d9f1aa67c846902e439324fda4e271dd825f6bddadcb1`
- Hall `node.rs`: `63c7ac94c74ea4af019cd936073a13cf5601a75fcde2272928d26a7f7d3f056d`
- Envoy `spool.rs`: `42152fbc8d29539210ebcba489687af097758461b4f764037c176222fcc2dcd9`

Post-write verification re-hashed the ADR and plan as
`2f74b81c922f4644aaa01eb95fe0f30445d134ef3041a667af6d55a13d8081af` and
`5e84ac09f4830f3e5cb094461afc4c772c13124d9020626972a5b28b3e1fedaf`
respectively, unchanged from the reviewed snapshot.
