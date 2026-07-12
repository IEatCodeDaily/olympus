# EDGE-1 · EdgeDriver seam + CaddyDriver (mandatory external edge, ADR 0014)

## Goal
Implement ADR 0014: a vendor-neutral EdgeDriver seam in Hall, a CaddyDriver
adapter applying desired route state via Caddy's localhost admin API, static
artifact hosting, and forward-auth to Hall as the sole auth authority.
Path-based routing only: /app/<slug>/…, /static/…, /artifacts/….

## Read FIRST
- `docs/adrs/0014-caddy-external-reverse-proxy.md` — the spec. The hardening
  and auth sections are non-negotiable.
- `docs/research/reverse-proxy-edge-landscape.md` §Caddy + §shortlist — admin
  API mechanics, the concurrent-edit warning, the PoC checklist.
- `docs/research/identity-aware-proxy-auth-delegation-research.md` — the
  header contract, strip-then-set, launch-code flow (launch codes are
  EDGE-2, not this card — but design the auth endpoint so they slot in).
- `crates/control-plane/src/proxy.rs` — the route table + auth modes you are
  KEEPING as the API contract; only the axum forwarding path is deprecated
  (leave it functional behind the existing routes in this card).
- `crates/control-plane/src/server/routes/` — module pattern; ARCH-A/CAPS-1
  principal seam for the forward-auth endpoint.

## Build on
Branch from main after CAPS-1 merges (parent: CAPS-1 card id). Coordinate
with PKG-1 only via capability ID strings.

## Deliverables
1. `crates/control-plane/src/edge/{mod,driver,caddy}.rs`:
   `Route {id, path_prefix, upstream, artifact_root, auth_policy, websocket}`;
   `trait EdgeDriver { fn apply(&self, desired: &[Route]) -> Result<()> }`;
   `CaddyDriver` rendering routes → Caddy JSON, applied via admin API
   (127.0.0.1:2019) with ONE serialized writer doing level-based full-config
   apply (render desired, PATCH atomically, idempotent; converge-on-restart).
   Caddy JSON never leaves edge/caddy.rs.
2. Forward-auth endpoint `GET /api/edge/auth` (no org path — Caddy calls it
   with X-Forwarded-Uri/-Method/-Host): resolves the Hall session cookie via
   the principal seam, evaluates the route's auth_policy (+ capability check
   for app routes), 200 + `X-Olympus-User/-Org/-Session` on allow, 401/403
   deny. NEVER echo secrets; no identity material in logs.
3. Route registration: existing POST /api/proxy contract extended with
   `pathPrefix` + `artifactRoot`; writes desired state; EdgeDriver applies.
   Static publish: `static.publish` capability writes files under
   `~/.olympus/<org>/artifacts/` + registers a public-or-scoped static route.
4. Caddy base config template (checked into `deploy/caddy/`): admin API
   localhost + enforce_origin ON, unprivileged, strip-then-set for
   X-Olympus-* headers, forward_auth on non-public matchers, file_server
   with symlinks disabled for artifact roots. systemd unit
   `deploy/systemd/olympus-caddy.service` with version floor >=2.11.1
   documented. Node health: Hall probes admin API; absent → `edge: missing`
   in node status; app/static registration refused (fail closed).
5. Tests: driver render snapshot tests (route table → expected JSON);
   forward-auth table tests (policy × principal × capability); integration
   test IF caddy binary is present on PATH (skip cleanly otherwise, note in
   summary): register route → curl through Caddy → forward-auth allow/deny;
   artifact traversal + symlink rejection; route create/delete churn (100
   cycles) with an open WebSocket surviving.

## Settled decisions — do NOT re-litigate
- Caddy, not Traefik/nginx/Envoy (ADR 0014 records why — read it).
- Path-based routing only; no subdomain mode in v1.
- EdgeDriver seam is mandatory — no Caddy types outside edge/caddy.rs.
- Hall is the only auth authority; no oauth2-proxy/Authelia sidecars.
- Launch codes / per-app grant cookies / WS tickets are EDGE-2. Do not build
  them here, but do not preclude them (auth endpoint takes the route id).
- Do not remove the axum /proxy/:slug forwarding path yet.

## Gates
- `make lint` + `make test` + fmt green; `-j 2`; target under ~/.cache/.
- Do NOT install system packages without checking: `which caddy` first; if
  absent, `sudo apt-get install -y caddy` is PRE-APPROVED for this card (or
  download the static binary to ~/.local/bin — prefer that if apt version
  < 2.11.1). Do NOT touch the live olympus services.
- Do not push to main. Green → `blocked: review-required` with render
  snapshots + integration evidence (or the skip note).
