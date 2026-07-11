# ARCH-A · One authorization seam — collapse the dual router stacks

## Goal
Deepen authorization into ONE module: a single `Principal` extractor + a single
`OrgScope` middleware applied to every `/api/*` route. The legacy
installation-token path becomes a *principal kind*, not a parallel router stack.

## Read FIRST (do not start coding before these)
- `docs/adrs/0010-hall-auth-and-client-connections.md` — the governing ADR. This
  card implements its end state; do NOT contradict it.
- `crates/control-plane/src/auth.rs` — bearer/origin checks.
- `crates/control-plane/src/auth_store.rs` — auth.sqlite (users/orgs/sessions).
- `crates/control-plane/src/server/identity.rs` — current identity extraction.
- `crates/control-plane/src/server/mod.rs` — how the org-scoped router vs the
  legacy router are assembled today (search for route registration + the
  installation-token guard).

## Build on
Commit `3fd7d2f` (main). Base your worktree there.

## Deliverables
1. `crates/control-plane/src/server/principal.rs` (new): a `Principal` enum —
   `User { user_id, memberships }` (cookie session) and
   `Operator` (installation bearer token). One axum extractor produces it.
   Every use of the Operator principal is logged (tracing, NO token material in
   logs — see commit `03cc613` for the prior token-logging bug; do not regress it).
2. An `OrgScope` layer/middleware: given a Principal + the request's explicit
   organization, answers allow/deny in ONE place. Operator maps to an explicit
   admin principal. Fail closed: no membership → 403; unknown org → 404/403;
   missing org on an org-scoped route → 400.
3. Rewire existing routes through the single seam. The separate "legacy router
   stack" disappears as a *structure*; legacy token access still works but flows
   through Principal::Operator. Gate it behind a config flag
   (`OLYMPUS_ALLOW_INSTALLATION_TOKEN`, default ON for now) so deletion later is
   one line.
4. Table-driven authz tests at the seam: (principal kind × org membership ×
   route class) → expected status. Include: non-member org selection fails
   closed; expired/revoked login session fails closed; operator token reaches
   admin surface; absent Origin + cookie fails per ADR 0010 rules.

## Settled decisions — do NOT re-litigate
- ADR 0010 doctrine stands: Hall owns users/orgs/memberships; client-selected
  org is context, never authority.
- auth.sqlite stays separate from the event log. No secrets into events/search.
- Do NOT redesign the login flow, cookies, or Argon2id/BLAKE3 choices.
- Do NOT split server/mod.rs into resource modules — that is card ARCH-B,
  running after you. Keep your diff scoped to auth/identity/middleware + the
  minimal route-wiring changes.

## Gates
- `cargo test --workspace` + `cargo clippy --all-targets -- -D warnings` +
  `cargo fmt --check` green.
- The patch-tool linter's E0670 "async fn" complaint is a FALSE POSITIVE —
  trust cargo.
- Do NOT start/restart the olympus server or systemd units. The controller owns
  the single running instance.
- Do not push to main. When your worktree is green, signal
  `blocked: review-required` with a summary of files + tests.
