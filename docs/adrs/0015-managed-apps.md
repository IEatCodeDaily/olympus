# ADR 0015 — Managed apps: Olympus-supervised services, distinct from plugins

Status: accepted · Date: 2026-07-12
Source: operator directive 2026-07-12 (the invapp model).
Relates to: ADR 0012 (extension doctrine — amended by this ADR's vocabulary),
ADR 0014 (edge — apps are served through it), ADR 0011 (JobRunner/envoy
process management — apps extend it), ADR 0005 (org-scoped layout).

## The distinction (normative)

**Plugins directly interface with Olympus components** — views, vault
embeds, workflow activities, session tools — through the ADR 0012 extension
classes. They have no process of their own (or execute only as sandboxed
activities).

**Apps do not touch Olympus internals.** An app is an **Olympus-managed
service**: a totally separate binary/runtime (Go binary, Python/Node service,
or container) whose process lifecycle and datastore location Olympus manages,
and which integrates at arm's length — via MCP, CLI, or its own HTTP API.

Example (canonical): *invapp*, an inventory app. A Go binary, supervised by
Olympus, with its own SQLite in its app directory. It ships a companion
plugin (*invapp-olympus*) which, when installed, contributes a shell view and
a vault embed rendering invapp in an iframe.

**Composition rule:** the app contributes *function* (a running service with
a URL); its companion plugin contributes *presence* (views/embeds/tools
inside Olympus surfaces). One package may ship both (ADR 0012 packaging).

## Vocabulary amendment to ADR 0012

- **App / managed app** — this ADR's concept: a supervised external service.
- ADR 0012's "workspace application" extension class is RENAMED **embedded
  app (UI contribution)**: browser-only content in a sandboxed frame with no
  managed process (e.g. Excalidraw). An embedded app whose backend is a
  managed app uses the companion-plugin pattern.
- "Application" unqualified is no longer used in design documents.

## Architecture

### Supervision (envoy-owned — host effects belong to envoys)

- Envoy gains a **ServiceTable**, sibling of JOBS-1's JobTable: long-lived
  processes with restart policy, health probes, drain/upgrade — reusing the
  role/dispatch/frame plumbing. Node role `AppHost` (analogous to JobRunner).
- Two runtime backends behind one seam, declared in the app manifest:
  - `runtime = "binary"` → systemd user unit (transient, envoy-written).
  - `runtime = "container"` → **podman** (rootless; docker-compatible OCI
    images; no daemon). Podman is preferred for anything with its own
    dependency tree.
- Envoy renders the unit/container from the manifest; Hall never execs apps.

### App manifest (extends the ADR 0012 package manifest)

```toml
[[contributions.apps]]
id = "invapp"
runtime = "binary"            # binary | container
entrypoint = "bin/invapp"      # or image = "ghcr.io/…@sha256:…"
listen = "dynamic"             # envoy allocates a loopback port
health = "/healthz"
env = { INVAPP_DB = "${app_state}/inv.db" }
resources = { memory_max = "512M" }
required_capabilities = ["mcp.register"]   # what its arm's-length surface may do
```

### State

- App state dir: `~/.olympus/<org>/apps/<app_id>/` (ADR 0005 layout).
  Olympus manages the DIRECTORY lifecycle (quota, backup, GC on removal);
  the app owns its contents (its SQLite schema is its business). Mirrors the
  plugin-state posture: host owns lifecycle, extension owns data.
- Apps never receive Olympus DB paths, event-log access, or Hall internals.

### Reachability & identity

- Apps are served exclusively through the ADR 0014 edge: `/app/<slug>/…`,
  loopback bind, network-non-bypassable. Route registered by the envoy when
  the service reports healthy (the proxy-registration flow that already
  exists, now driven by ServiceTable).
- An app gets a **service principal** with a CAPS-1 capability set scoping
  its MCP/API access. Apps are LESS trusted than plugins by default — the
  arm's-length interface is the trust boundary, and it stays narrow.
- Users/agents reach the app UI via the ADR 0014 launch-code → per-app grant
  cookie flow. The app never sees the primary Hall session.

### Embedding & the superapp property (mobile-critical)

The companion plugin registers view/embed contributions pointing at the
app's edge route. The embed is an iframe (desktop shell) or **webview
(Olympus mobile)** — the SAME launch-code contract serves both; nothing
mobile-specific is invented. This is the superapp mechanism: Olympus surfaces
compose managed apps as embedded, capability-scoped panes. The embed contract
(clean URL after redemption, path-prefix awareness per ADR 0014, no primary
token in the frame) is the compatibility bar an app must meet to be
embeddable.

## Sequencing

ServiceTable rides on JOBS-1's plumbing — build after it merges:
APP-1 (ServiceTable + binary runtime + manifest + state dir + edge
registration) → APP-2 (podman backend) → the invapp-style reference app as
the ADR 0012 forcing-function candidate. Companion-plugin view/embed
contributions need PKG-1's registry v2.

## Non-goals

- Olympus is not a PaaS: no build pipelines, no autoscaling, no app store in
  v1. Install from package; supervise; serve.
- No docker-daemon dependency; podman or systemd only.
- Apps cannot contribute extension-class integrations directly — only their
  companion plugin can (keeps the trust split clean).
