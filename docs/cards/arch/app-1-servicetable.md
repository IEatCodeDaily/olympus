# APP-1 · Managed apps: ServiceTable + binary runtime + app manifest (ADR 0015)

## Goal
Implement ADR 0015's core: envoy-supervised long-lived app services. An app
declared in a package manifest gets: a state dir, a supervised process
(systemd user unit v1), health probes, an envoy-registered edge route on
healthy, and a service principal. Podman/container backend is APP-2 — NOT
this card.

## Read FIRST
- `docs/adrs/0015-managed-apps.md` — the spec, esp. the plugin/app
  distinction and non-goals.
- `docs/adrs/0014-caddy-external-reverse-proxy.md` — edge routes apps are
  served through; EDGE-1's Route/auth_policy model.
- JOBS-1's merged JobTable + NodeRole work — ServiceTable is its long-lived
  sibling; reuse the frame/role/dispatch plumbing, don't fork it.
- PKG-1's merged manifest model — `[[contributions.apps]]` extends it.
- CAPS-1's merged capability seam — service principals.
- `systemd-run --user` transient unit pattern OR written unit files under
  `~/.config/systemd/user/` — pick one, justify in summary (transient
  preferred: no file lifecycle to manage).

## Build on
Branch from main after JOBS-1 + PKG-1 + EDGE-1 all merge (three parents).

## Deliverables
1. Proto: `NodeRole::AppHost`; frames `EnsureService/StopService/DrainService`
   (Hall→Envoy) and `ServiceStatus{app_id, state, health, port}` (Envoy→Hall,
   in hello + on change) — mirroring the runtimes-table pattern.
2. Envoy `ServiceTable`: spawn from manifest (binary runtime only), dynamic
   loopback port allocation, env templating (`${app_state}`), health probe
   loop (HTTP GET, backoff), restart policy (always, rate-limited 3/5min then
   quarantine + report), drain on command, reap on stop. State dir
   `~/.olympus/<org>/apps/<app_id>/` created on first ensure, never deleted
   implicitly (GC is an explicit Hall command).
3. Hall: app lifecycle events (`AppInstalled/Started/Healthy/Unhealthy/
   Stopped/Removed`) + projection; REST under the routes module pattern
   (install-from-package, start, stop, status, remove). On Healthy: register
   edge route `/app/<slug>/` → allocated port via EdgeDriver, auth_policy
   from manifest (default session_scoped). On Unhealthy/Stopped: route
   removed (fail closed, 502 from edge is acceptable UX).
4. Service principal: minted at install with the manifest's
   required_capabilities ∩ granter's authority (CAPS-1 semantics); passed to
   the app as a bearer env var scoped to its MCP/API surface only.
5. Reference fixture app for tests: a tiny static Go-or-rust binary (check
   into `fixtures/apps/echoapp/` prebuilt or built by the test) serving
   /healthz + an echo endpoint. Integration test: install → healthy → route
   live → curl through edge (if caddy present; skip cleanly) → kill process
   → auto-restart → drain → stopped → route gone.

## Settled decisions — do NOT re-litigate
- Apps never touch Olympus internals; no event-log/DB paths in app env.
- Envoy owns supervision; Hall never execs apps.
- Companion-plugin views/embeds are separate (need the plugin classes) —
  out of scope here.
- Podman backend = APP-2. Design the runtime seam (`trait AppRuntime`) so it
  slots in, but implement binary only.
- No autoscaling, no build pipeline, no app store.

## Gates
- `make lint` + `make test` + fmt green; `-j 2`; target under ~/.cache/.
- Do NOT touch live olympus services or live ~/.olympus data; temp dirs.
- Do not push to main. Green → `blocked: review-required` with the frame
  shapes, unit strategy chosen, and integration evidence.
