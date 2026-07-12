# ADR 0012 — Olympus as a programmable agent operating environment (extension doctrine)

Status: accepted · Date: 2026-07-12
Source: operator + Zephyr design session (2026-07-12), reviewed and amended by
Terminus. Relates to: ADR 0005 (resource model), ADR 0006 (declarative
replication / registry), ADR 0010 (auth), ADR 0011 (jobs/MCP/capabilities
roadmap). Amended by: ADR 0013 (workflow kernel decision).

## Doctrine

**Olympus is a programmable operating environment for human and agent work.
Packages provide typed capabilities; workflows durably compose those
capabilities; sessions provide interactive execution contexts; Hall governs
state, identity, policy, and orchestration; Envoys perform host effects.**

The fleet control plane remains the substrate; the product above it is
programmable. The OS analogy (Hall≈kernel, envoys≈host agents,
workflows≈services, packages≈drivers/apps, capabilities≈permissions,
sessions≈processes, event log≈journal) is a design compass, not a literal
target — Olympus builds above host operating systems, never replaces them.

## Vocabulary (normative)

- **Package** — the installable, distributable, content-addressed artifact.
  One package may carry many contributions.
- **Plugin** — an executable component supplied by a package.
- **Capability** — a permission or callable operation exposed to a principal
  (e.g. `github.pr.create`, `vault.text.search`).
- **Application** — see ADR 0015: "app" = Olympus-managed service (own
  supervised process + datastore, arm's-length MCP/CLI/API integration);
  "embedded app" = a browser-only UI contribution in a sandboxed frame
  (IDE, Draw.io, database browser). The unqualified term is retired.
- **Workflow template** — a versioned workflow definition installed by a
  package, instantiated with caller parameters.

"Plugin" is never used as a catch-all. One package format, several explicit
**extension classes**: activity provider, trigger provider, resource provider,
session tool provider, runtime adapter, embedded app (UI contribution —
renamed from "workspace application" by ADR 0015), indexer/extractor, policy
provider, view provider, storage provider.

> **Amended by ADR 0015 (2026-07-12):** "app" now denotes an
> Olympus-**managed service** (separate supervised binary/runtime + own
> datastore, integrating only via MCP/CLI/API) — distinct from plugins, which
> interface with Olympus internals through the extension classes above. A
> managed app gains in-Olympus presence (views, vault embeds) only through a
> companion plugin. See ADR 0015 for the full model.

## First principles (locked)

1. Workflows compose capabilities; packages provide capabilities.
2. One package format, multiple explicit extension classes.
3. Definitions are runtime data; effects execute through registered activities.
4. Public extension contracts are schema/protocol-based (JSON Schema, WIT,
   HTTP/WSS), never Rust ABI traits.
5. Third-party code never loads natively into Hall. No dynamic-library tier —
   ever.
6. **Supervised envoy process is the default sandboxed execution tier** (see
   Amendments); WASM components are the optimization tier for pure logic.
7. Browser applications run in sandboxed frames with short-lived, narrow
   resource grants — never the installation/bearer token, on WS channels as
   well as REST.
8. Every package declares its manifest (contributions + required capabilities)
   before any of its code executes.
9. Installation, granting, publishing, and activation are **separate
   authorities** (`package.author/build/sign/install/grant/activate`) — as
   distinct capability IDs from day one, even while one person holds all of
   them.
10. Workflow and package versions are immutable and pinned per run.
11. Plugins never write Olympus's event log or internal database directly;
    plugin state lives in host-managed namespaces
    (`plugin-state://<pkg>/{global,org/<id>,project/<id>,session/<id>}`).
12. **A workflow cannot elevate its caller's authority.**
    `effective = caller ∩ workflow_manifest ∩ scope_policy ∩ provider_grant`.
    Scheduled/system workflows run as dedicated, revocable service principals.
13. Agents can author; only separately authorized principals publish/activate.
14. Core owns invariants; packages add functionality, never alternate kernels.
    Non-replaceable core: identity, capability evaluation, event log, workflow
    semantics, scheduler/fencing, package verification, plugin lifecycle,
    resource addressing, session isolation, Hall/Envoy transport, audit,
    secret mediation, artifact identity, org/context/project boundaries.
15. Resources and semantic capabilities are the integration language between
    packages — never private plugin APIs. Cross-package needs are declared as
    capability dependencies (`capability = "artifact.store"`), not package
    dependencies.
16. Semantic capability binding is resolved by policy at scopes
    (system → org → context → project → run override); workflows request
    `ci.run`, deployment policy selects the provider.

## Trust model

Two tiers:
- **Tier A — sandboxed package** (default; all third-party and agent-generated
  code): supervised envoy process or WASM component or sandboxed iframe;
  capability-mediated host APIs only.
- **Tier B — built-in system component**: compiled into Olympus, reviewed as
  Olympus (event log, workflow kernel, scheduler, transport, auth, session
  management, core vault model).

Signing stance (hybrid, settled): **development contexts permit unsigned local
packages, explicitly marked untrusted with interactive grants; normal contexts
require signed packages.** Promoting a dev package to normal use goes through
the build/sign/publish workflow. Today the entire installation is effectively
one development context; the signing pipeline is deferred until a second
publisher exists (see Amendments §A1) — but the *context marking* and the
authority-separated capability IDs are built from the start so the flip to
signed-only is policy, not surgery.

## Amendments to the source design (Terminus review, accepted by operator)

**A1 — Kernel invariants now, ecosystem machinery later.** The invariants that
cannot be retrofitted (capability intersection, manifest-before-execution,
authority separation, no-native-in-Hall, state namespaces) are built
immediately. The machinery that serves a population we don't have (signing
infra, SBOM scanning, OCI distribution, staged rollout, revocation lists) is
deferred until a real second publisher or first untrusted package exists.
Development mode IS the v1 product.

**A2 — Envoy process is the default tier, WASM the optimization.** The design's
own placement table shows nearly all real work (language servers, DBs, CLI
tools, IDE backends, CI) landing in envoy processes. Olympus already owns the
transport, spool, and (post-JOBS-1) dispatch for supervised processes. WASM
components (WIT, wasmtime) are adopted when cheap pure-logic activities
justify the toolchain — not as an entry requirement.

**A3 — MCP is the session-tool contract.** The "session tool provider"
extension class is fulfilled by MCP servers, which Olympus already declares in
the ADR 0006 registry and injects via setup adapters. Olympus does not invent
a parallel session-tool protocol. A package contributes session tools BY
shipping an MCP server declaration.

**A4 — Workflow kernel is decided separately** (ADR 0013). This ADR locks the
capability/package doctrine only.

## Migration map (current → target)

| Today | Becomes |
|---|---|
| ADR 0006 registry entries (skill/mcp/plugin/hook slugs) | package contributions with manifests |
| Setup adapter (Hermes/ClaudeCode/Codex) | the **runtime adapter** extension class consuming session-tool + skill contributions |
| JOBS-1 job dispatch | the first **activity provider** (`job.run` on JobRunner envoys) |
| MCP server declarations | **session tool provider** contributions (A3) |
| ARCH-A Principal/OrgScope seam | the capability evaluation point; per-session capabilities (ADR 0011 §4) become a principal payload |
| Vault storage (SQLite/FTS5) | first **storage provider** behind the vault service contracts (`vault.document.*`, `vault.text.search`, …) with advertised capability flags |
| `~/.olympus/<org>/…` layout (ADR 0005) | unchanged; plugin state namespaces slot under it |

## Build order

1. **Capability model** (ADR 0011 Phase 3, pulled forward) — per-session
   capabilities as Principal payload, intersection evaluation, the
   `workflow.*`/`package.*` capability ID vocabulary reserved.
2. **JOBS-1 as first activity provider** — already in flight; retrofit its
   `job.run` under the activity-provider contract when the manifest lands.
3. **Package manifest + registry v2** — declarative manifest (TOML), extension
   classes, install validation (schema, compat, capability review), the
   ADR 0006 registry migrates in.
4. **One real package end-to-end** as the forcing function (candidates:
   GitHub activities+triggers, or the diagram workspace application) — this
   teaches more than further paper design.
5. Workflow kernel per ADR 0013's decision, once activities exist to compose.

## Open questions (tracked, not blocking)

- Capability **revocation semantics for in-flight runs** (versions are pinned;
  authority is not) — decide before the first long-running workflow ships.
- Resource-type registry (MIME-handler analog) shape — design with the first
  workspace application.
- Provider capability flags vocabulary for storage providers (transactions,
  vectorSearch, multiWriter, …) — freeze with the second vault backend.
