# ARCH-B · Split server/mod.rs into per-resource route modules

## Goal
Mechanical decomposition of `crates/control-plane/src/server/mod.rs` (~6.2k loc,
~89 routes) into per-resource modules. `mod.rs` shrinks to AppState + router
assembly + middleware stack. ZERO behavior change — this is a pure move.

## Read FIRST
- `crates/control-plane/src/server/mod.rs` — inventory every route + handler +
  shared helper before moving anything. Build a route→module map first and put
  it in your worktree as `docs/cards/arch/arch-b-route-map.md`.
- The ARCH-A result (already merged into your base): the `Principal`/`OrgScope`
  seam in `server/principal.rs`. Every resource module mounts through it.
- `crates/control-plane/src/server/{dto,ws,bridge_mgr,envoy_conn,identity}.rs`
  — these stay where they are.

## Build on
Your worktree branches from main AFTER ARCH-A merges. Confirm
`server/principal.rs` exists in your base before starting; if it doesn't, STOP
and signal blocked.

## Deliverables
1. `crates/control-plane/src/server/routes/` with one module per resource:
   `sessions.rs`, `messages.rs` (or fold into sessions), `vaults.rs`,
   `fleet.rs` (nodes/envoys/agents), `projects.rs` (boards/cards),
   `registry.rs`, `setup.rs`, `proxy.rs`, `triggers.rs`, `irc.rs`, `auth.rs`
   (login/logout/orgs), `search.rs`, `misc.rs` (health/metrics/events tail).
   Adjust granularity to the actual route inventory — the map you build is the
   authority; don't force this exact list.
2. Each module exposes `pub fn router(state: AppState) -> Router<...>` (match
   the axum 0.8 state pattern already in use) + its handlers + handler-local
   helpers. Genuinely shared helpers move to a `server/routes/support.rs` (keep
   it small; prefer duplicating a 3-liner over widening a shared surface).
3. `mod.rs` = AppState, middleware stack, `Router::new().merge(...)` assembly,
   and nothing else. Target < 600 loc.
4. Route-registration order is preserved EXACTLY where it matters: local API
   routes before proxy catch-alls (`/proxy/{slug}`), ws route intact.
5. Proof of zero behavior change: `cargo test --workspace` green + a
   route-inventory diff — dump the route table before and after (a small test
   that asserts the same (method, path) set) OR include the before/after grep
   inventory in your summary.

## Settled decisions — do NOT re-litigate
- This is a MOVE, not a redesign. No handler logic changes, no DTO changes, no
  route renames. Resist every temptation.
- dto.rs remains the ONLY view-row→wire-JSON seam.
- Keep the existing tests passing without rewriting them; add the route-set
  assertion test.
- axum 0.8: `delete` must be imported explicitly or fully qualified (known
  E0425 pitfall).

## Gates
- `cargo test --workspace` + clippy `-D warnings` + fmt green.
- The patch-tool E0670 async-fn lint is a false positive; trust cargo.
- Do NOT start/restart the olympus server.
- Do not push to main. Green → `blocked: review-required` with the route map.
