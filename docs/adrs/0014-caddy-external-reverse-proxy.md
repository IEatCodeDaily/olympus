# ADR 0014 — Caddy as the mandatory external edge (app gateway + static hosting), behind an EdgeDriver seam

Status: accepted · Date: 2026-07-12
Supersedes: the "built-in reverse proxy, not an external Caddy/Traefik"
operator directive recorded with the `proxy.rs` design (shipped `3b5f065`).
Operator reversal 2026-07-12: external proxy so upstream owns CVE response.
Evidence: `docs/research/reverse-proxy-edge-landscape.md` (25-candidate
survey, 3-year CVE analysis, 39 sources) and
`docs/research/identity-aware-proxy-auth-delegation-research.md` (SSO/auth
delegation deep-dive, 26 sources). Relates to: ADR 0012 (workspace
applications), ADR 0011 (§2 iroh — unchanged as the inter-node transport;
Caddy is the HTTP edge), ADR 0010 (Hall auth authority).

## Context

The plugin/application system (ADR 0012) makes HTTP apps first-class:
workspace applications (IDE, diagram editors, DB browsers), dev environments,
per-session proxied apps, and static content (agent HTML, artifacts). All
need stable URLs, edge authentication (SSO), and static file serving. The
built-in axum proxy buffers whole responses, is HTTP/1-only, has no static
story, and makes Olympus the security-response owner for an edge component.

A full landscape survey (25 candidates: HAProxy, Envoy, nginx/OpenResty/Unit,
Apache httpd/ATS, Caddy, Traefik, YARP, Pingora/River, Sozu, rpxy, lighttpd,
h2o, Varnish, APISIX, Kong, Skipper, Zoraxy, NPM, and identity-aware proxies)
was run against nine axes. Key eliminations:

- **Traefik**: no static file serving (confirmed); recurring path-
  normalization/auth-bypass advisory class — disqualifying when forward-auth
  is the security boundary.
- **nginx OSS**: no first-class route CRUD API (config rewrite + reload).
- **HAProxy**: runtime API adds servers to existing backends; new routes
  still require config/reload. No static serving.
- **Envoy**: strongest fleet protocol (xDS) but fails both mandatory axes
  (no file server, no ACME) + 18 memory-safety-class CVEs in 3 years (C++).
- **NGINX Unit**: the one dynamic-API+static contender — **archived 2025**,
  fails the external-CVE-owner requirement outright.
- **Pingora/River**: Pingora is a library not a server (CVE triage would be
  ours — e.g. CVE-2026-2833, 9.1-critical request smuggling); River stalled
  at v0.5.0 (bus factor).
- **Sozu**: genuinely runtime-configurable and alive (2.0, May 2026), but no
  static serving, no forward-auth ecosystem, give-back license clause.
- **Pomerium/Authelia/oauth2-proxy/Teleport**: rejected — each duplicates
  identity/session authority that Hall already owns (ADR 0010).

## Decision

**Caddy 2 (≥ 2.11.1, pinned) is the mandatory HTTP edge**, one instance per
app-exposing node, supervised (systemd v1). It is the only mature, actively
maintained option satisfying live route API + native static files + native
forward-auth + WebSocket/H2/H3 in one memory-safe binary with upstream
security response.

**Routing is path-based (operator decision 2026-07-12):** hosted apps at
`/app/<slug>/…`, static at `/static/…` and `/artifacts/…`. No subdomain mode
in v1 (drops the on-demand-TLS axis; Caddy still wins without it). Known
cost: apps that assume root-serving need path-prefix awareness — the
ADR 0012 workspace-application contract requires it.

### The EdgeDriver seam (vendor neutrality)

Hall models desired edge state vendor-neutrally and never leaks Caddy's JSON
shape into the domain model:

```rust
Route { id, path_prefix, upstream: Option<HostPort>,
        artifact_root: Option<PathBuf>, auth_policy: AuthPolicy /* public |
        session_scoped | app_grant */, websocket: bool }
trait EdgeDriver { fn apply(&self, desired: &[Route]) -> Result<()>; … }
```

`CaddyDriver` is the first adapter: renders routes to Caddy JSON, applies via
the **localhost admin API** with serialized, atomic subtree updates (docs
warn about concurrent-edit collisions — one writer, level-based sync: render
full desired config, apply idempotently; a restarted Caddy converges on next
sync). A future xDS driver stays possible; the seam earns its keep the day a
second adapter exists. The existing `POST/GET/DELETE /api/proxy` API and auth
modes are kept as the contract; the axum forwarding path is deprecated and
removed after Caddy paths are verified.

### Hardening (non-negotiable, from the survey + CVE record)

- Admin API on localhost only, **`enforce_origin` enabled**
  (CVE-2026-27589: CSRF config-replacement on the admin API, fixed 2.11.1 —
  the reason both the version floor and origin enforcement are mandatory).
- Caddy runs unprivileged, same user as envoy; no admin API route is ever
  exposed through Caddy itself.
- Artifact roots are **symlink-free by construction** (amended 2026-07-12:
  stock Caddy has no symlink-disable option in file_server — the guarantee
  moves to the writer, which Olympus fully controls): the `static.publish`
  write path rejects symlinks (lstat every created entry; refuse symlink
  sources), route registration validates the root contains no symlinks
  before EdgeDriver applies it, and path-traversal tests stay in the
  acceptance suite. Caddy stays stock — no custom modules.

### Auth architecture (Hall is the sole authority)

Per the identity research — dumb proxy + forward-auth; no second identity
product:

1. Every non-public route: Caddy `forward_auth` → Hall `/api/edge/auth`.
   Hall answers allow/deny from the session-cookie/capability seam (ARCH-A,
   CAPS-1) and returns namespaced identity headers. **Strip-then-set**: the
   edge strips all `X-Olympus-*` headers from client requests before setting
   its own; upstreams are network-non-bypassable (loopback binds).
2. Primary Hall session stays in a `__Host-` Secure HttpOnly cookie; never
   forwarded to apps; no `Domain=` cookies.
3. **Sandboxed iframe apps get single-use launch codes**: the shell asks Hall
   for a launch code → redeemed at the edge → opaque host-only per-app grant
   cookie `{user, org, app instance, allowed actions, expiry}` → redirect to
   clean URL. The app never holds the primary session and cannot call Hall
   generally. (Path-based routing keeps this same-origin — simpler.)
4. Apps that verify assertions: Hall-signed short-lived JWT with exact
   `aud = app-instance/route`; gateway strips any client-supplied copy.
5. WebSockets: authenticated at upgrade; 30–60s single-use audience-bound
   tickets for cookie-less sockets; connection TTL + revocation hooks;
   Origin allowlist (cross-site WS hijacking).
6. Forward-auth is not CSRF protection — apps keep SameSite/token discipline.
7. Identity headers, launch codes, and tickets never appear in logs.
8. Fail closed: Hall unreachable → protected routes deny; only explicit
   `public` static routes continue serving.

## Acceptance proof (before the axum path is removed)

From the survey's PoC list: 1,000 route create/delete cycles with live
WebSockets and streaming open; crash/restart convergence; forward-auth
allow/deny/header-propagation + fail-closed timeout; artifact
traversal/symlink tests; launch-code redemption + replay rejection.

## Doctrine fit (ADR 0012)

Caddy is **kernel-adjacent infrastructure** like SQLite — mandatory,
externally maintained, supervised; not a package. What is doctrine-shaped:
the capabilities riding it (`proxy.route.register`, `static.publish`, app
grants) and the workspace-application contract. Packages contribute apps;
the edge is system-owned. Nodes without Caddy report `edge: missing` and
refuse app/static registration (fail closed).

## Consequences

- proxy.rs forwarding deprecated; route table + API live on behind
  EdgeDriver. Net Rust code shrinks; HTTP-edge features (H2/H3, streaming,
  compression, TLS, range requests) stop accreting in Olympus.
- New supervised dependency + version pinning in installers. Accepted.
- SSO becomes one Hall endpoint + header contract; per-app auth work
  collapses into the launch-code/grant-cookie pattern.
- Multi-node later: one Caddy per node, Hall broadcasts the same desired
  state through EdgeDriver per node — fan-out/convergence/rollback is
  explicitly Olympus's job (the survey's strongest argument against Caddy,
  accepted consciously).
