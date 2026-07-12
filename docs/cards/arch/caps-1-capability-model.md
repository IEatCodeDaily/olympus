# CAPS-1 · Per-session capability model (Principal payload + intersection evaluation)

## Goal
ADR 0011 §4 + ADR 0012 build-order step 1. Replace ambient session authority
with explicit per-session capability sets: inherited from parent, narrowed
only on fork (never expanded), evaluated at the ARCH-A Principal/OrgScope seam.

## Read FIRST
- `docs/adrs/0012-programmable-operating-environment.md` — doctrine, esp.
  principles 9/12 and the capability-ID vocabulary reservation.
- `docs/adrs/0011-jobs-mcp-capabilities-sandboxing.md` §4 — the spec: paths
  (readable/writable), allowed tools, linked repos/vaults, resource limits,
  can_fork; signed by Hall; validated on every call.
- `docs/adrs/0013-workflow-kernel-bounded-chains.md` — capability re-check at
  step dispatch is a future consumer of your evaluation function; design the
  seam so it can be called per-operation, not just per-request.
- `crates/control-plane/src/server/principal.rs` — the merged ARCH-A seam you
  extend. Read its authorize() matrix and tests fully.
- `crates/control-plane/src/server/routes/sessions.rs` — fork/subsession
  creation paths (post-ARCH-B layout).
- `crates/control-plane/src/event.rs` + `log.rs` — events are JSON+zstd now
  (ARCH-D); additive fields use #[serde(default)]; NEVER positional reshaping.

## Build on
Current main. ARCH-E/ARCH-F run concurrently in crates/envoy + sync/import —
DO NOT touch crates/envoy, sync.rs, import.rs, or crates/proto. Your surface
is control-plane server/auth/views/event only. If you believe you need a proto
change, STOP and signal blocked with the reason instead.

## Deliverables
1. `CapabilitySet` type (server-side): capability IDs (dotted strings, e.g.
   `session.fork`, `vault.read:<vault_id>`, `tool.terminal`), plus structured
   limits (paths RO/RW, resource limits, can_fork). Serde, versioned envelope.
2. Session capability lifecycle as EVENTS: `SessionCapabilitiesAssigned
   { session_id, capabilities, assigned_by, parent_session_id }` — additive
   Event variant (JSON codec makes this safe). Projection onto SessionRow.
   Fork path: child capabilities = requested ∩ parent (compute the
   intersection server-side; reject expansion attempts with 403 + a specific
   error body, and TEST that).
3. Evaluation: extend the principal seam with
   `authorize_capability(principal, session_id, capability) -> Allow/Deny`
   as ONE function — the single place ADR 0012 principle 12's intersection
   happens. Sessions with no capability record = full legacy grant (negative-
   polarity migration per operator convention: new enforcement ON for
   capability-carrying sessions, absent record = legacy behavior).
4. Signing: capability sets are stamped by Hall with an HMAC (key material in
   `~/.olympus/`, 0600, same pattern as the installation token) so envoy-side
   validation is possible later WITHOUT a Hall round-trip (ADR 0011 says
   "signed by Hall, validated on every call" — v1 validates Hall-side; the
   signature makes envoy-side validation additive later). Do NOT log key or
   signature material.
5. Reserve (constants + doc comment, no enforcement yet) the ADR 0012
   authority IDs: `workflow.{list,execute,draft.create,publish,...}`,
   `package.{author,build,sign,install,grant,activate}`.
6. REST: capabilities readable on session DTO (GET), assignable at create +
   PATCH before first message (the optimistic-create pattern), immutable
   after agent lock except narrowing. camelCase DTO in dto.rs; update
   docs/api-contract.md + ui/src/types.ts together (three-file rule).
7. Table-driven tests at the seam: inherit / narrow-on-fork / expansion-
   rejected / legacy-absent / revoked-parent chains. Property test: for any
   parent P and request R, effective(child) ⊆ P.

## Settled decisions — do NOT re-litigate
- Narrowing-only inheritance (ADR 0011 §4). No expansion path exists, even
  for operators — an operator mints a NEW session with wider caps instead.
- Capability IDs are dotted strings with optional `:<resource>` suffix; no
  regex/glob grammar in v1 (exact match + prefix match on the resource part
  only). Keep the grammar boring.
- bwrap/filesystem ENFORCEMENT is Phase 4 (separate card) — you record and
  evaluate path capabilities; you do not enforce them at the OS level.
- No UI work beyond types.ts sync.

## Gates
- `make lint` + `make test` + `cargo fmt --check` green. CARGO_TARGET_DIR
  under ~/.cache/, cargo with `-j 2` — the box is shared with two other
  workers; a cargo exit 124 means contention, rerun with -j 1, not a failure.
- Do not push to main. Green → `blocked: review-required` with files, the
  event shape, and test evidence.
