# PKG-1 · Package manifest + registry v2 (extension classes, install validation)

## Goal
ADR 0012 build-order step 3. Replace the flat ADR 0006 registry
(kind ∈ skill|mcp|plugin|hook, slug → JSON definition) with the package model:
a declarative TOML manifest carrying typed contributions + required
capabilities, validated before anything executes. Registry v2 is the
projection of installed packages.

## Read FIRST
- `docs/adrs/0012-programmable-operating-environment.md` — vocabulary,
  extension classes, principles 2/8/9/11, migration map, amendment A3 (MCP IS
  the session-tool contract — do not invent a parallel one).
- `docs/adrs/0006-omp-blueprint-declarative-replication.md` §9.4 — the
  registry you are superseding. `crates/control-plane/src/views/registry.rs`
  + its routes module + `EntryRegistered` event.
- CAPS-1's merged result — `package.{author,build,sign,install,grant,activate}`
  capability IDs are reserved there; installation paths check
  `package.install` via the capability seam.
- The manifest shape sketch in the Zephyr design (mirrored in ADR 0012): id,
  version, publisher, compatibility, [[contributions.<class>]] tables.

## Build on
Branch from main AFTER CAPS-1 merges (parent). JOBS-1 may land before or
after you — coordinate via capability IDs only, no shared files expected.

## Deliverables
1. `PackageManifest` (TOML, serde): package{id,name,version,publisher,license},
   compatibility{olympus_api,platforms}, and contribution tables for the ten
   ADR 0012 extension classes. v1 executes/activates ONLY: session tool
   (MCP declaration), skill dir, activity provider (JOBS-1-backed), workflow
   template (inert until WF-1). Other classes parse + validate + store but
   report `unsupported_yet` on activation (fail closed, explicit).
2. Events: `PackageInstalled{manifest, digest, source, installed_by}`,
   `PackageActivated/Deactivated/Removed`. Digest = BLAKE3 of the package dir
   (content-addressed identity from day one). Additive Event variants.
3. Install validation pipeline (pure functions, table-tested): manifest schema
   → olympus_api compat → capability review (requested caps listed; grant is
   a SEPARATE call gated on `package.grant`) → collision check (two active
   packages contributing the same capability ID → reject unless bindings
   disambiguate). Signing VERIFICATION is stubbed to `dev-unsigned` marking
   per ADR 0012's hybrid stance — structure the pipeline so a signature stage
   slots in without reshaping.
4. Migration: existing registry entries auto-wrap as synthetic packages
   (`legacy.<kind>.<slug>`, publisher=legacy, dev-unsigned) at boot,
   idempotently — the setup adapter path keeps working unchanged. The old
   PUT /api/registry stays as a legacy route writing synthetic packages.
5. REST: install (path or inline manifest for v1 — no remote fetch yet),
   list, show, activate/deactivate, remove. Grant endpoint separate.
   dto.rs + api-contract.md + types.ts together.
6. Setup adapter consumes registry v2: resolve slugs → active package
   contributions. `ResolvedSetup::from_registry` gets a v2 source with the
   same output shape (adapters unchanged).

## Settled decisions — do NOT re-litigate
- One package format, typed extension classes (no generic "plugin" blob).
- Manifest-before-execution; nothing gets ambient authority by installation.
- install ≠ grant ≠ activate — three calls, three capability checks.
- No signing infra, no SBOM, no OCI in this card (ADR 0012 A1). Dev-mode
  marking only.
- Plugin state namespaces (`plugin-state://…`) are declared in the manifest
  but the state API itself is a later card.

## Gates
- `make lint` + `make test` + fmt green; `-j 2`, target dir under ~/.cache/.
- Boot-migration idempotency test (double boot, same synthetic packages).
- Do not push to main. Green → `blocked: review-required`.
