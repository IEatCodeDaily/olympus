# Olympus Development/Production Split Implementation Plan

> **For Hermes:** Execute this plan task-by-task and verify every external side effect.

**Goal:** Make fxcompute-01 the authoritative Olympus development host, expose an isolated dev plane at olympus-dev.entelechia.cloud, retain Terminus as build-free production, and establish a measured fast development and promotion workflow.

**Architecture:** Preserve the complete live repository at the same absolute path on fxcompute-01. Run isolated dev Hall, Envoy, and Vite services there; relay the dev frontend through the existing reverse SSH and Terminus Cloudflare connector. Keep a separate production JobRunner on fxcompute-01 and promote immutable clean-main bundles to Terminus.

**Tech Stack:** Rust/Cargo, React/Vite/Bun, systemd user services/slices, reverse SSH, Cloudflare Tunnel/Access, Git/rsync, SQLite.

---

### Task 1: Capture migration manifest and baseline

**Files:**
- Create: `/home/rpw/olympus-migration-20260715/terminus-manifest.txt`
- Create: `/home/rpw/olympus-migration-20260715/performance-before.txt`

**Steps:**
1. Record Terminus Git HEAD, refs, worktrees, status-v2, file counts, and checksums for Git metadata and representative modified/untracked files.
2. Record active repository writers and compiler processes.
3. Record fxcompute-01 CPU, memory, storage, current services, and tool versions.
4. Measure an existing warm Cargo check/build through the JobRunner.
5. Verify manifests contain no secret contents.

### Task 2: Pre-copy and finalize repository migration

**Files:**
- Copy: Terminus `/home/rpw/olympus/` -> fxcompute-01 `/home/rpw/olympus/`
- Create: Terminus `/home/rpw/olympus-source-moved-to-fxcompute-01.md`

**Steps:**
1. Pre-copy with rsync archive/hardlink/ACL/xattr support, excluding generated `target` and `node_modules` trees.
2. Check for active writers; stop only Olympus development agents/processes that hold the repository open, never production Hall/Envoy.
3. Run final checksummed rsync without deleting source.
4. Validate `git fsck`, refs, worktree registrations, status counts, local commit count, and representative hashes on both hosts.
5. Retain Terminus source as the rollback snapshot and mark it non-authoritative in a notice; do not delete it.

### Task 3: Normalize fxcompute-01 naming

**Files:**
- Modify: Terminus `~/.ssh/config`
- Modify: Terminus `~/.local/bin/olympus-job`
- Create: fxcompute-01 `~/.config/systemd/user/fxcompute-01-tunnel.service`
- Create: fxcompute-01 `~/.config/systemd/user/olympus-prod-job-runner.service`
- Retire after canary: `fxbuilder-tunnel.service`, `olympus-fxbuilder.service`

**Steps:**
1. Make `fxcompute-01` the first canonical SSH alias and retain `fxbuilder` second for compatibility.
2. Change CLI default node and rsync destination to `fxcompute-01`.
3. Create renamed services with node ID `fxcompute-01` and production job root `/srv/olympus-prod/jobs`.
4. Add reverse forward `127.0.0.1:8800 -> fxcompute-01:127.0.0.1:5177`.
5. Start renamed tunnel, then renamed JobRunner; verify production Hall sees `fxcompute-01`.
6. Submit a production canary job and only then disable old units.

### Task 4: Install pinned development toolchain

**Files:**
- Create/modify: fxcompute-01 host-local tool installations and PATH configuration

**Steps:**
1. Install pinned Node 22/npm and Bun from official release channels.
2. Install prebuilt or cargo-binstall releases of `sccache`, `cargo-nextest`, `cargo-watch`, and `just` with checksums where published.
3. Verify versions in a non-login systemd-compatible PATH.
4. Do not source interactive shell startup files from services.

### Task 5: Configure isolated compiler caches and resource controls

**Files:**
- Create: `/var/lib/olympus/cargo-home/config.toml`
- Create: fxcompute-01 `~/.config/systemd/user/olympus-build.slice`
- Modify: production and development build service units

**Steps:**
1. Configure clang+mold and `RUSTC_WRAPPER=sccache` host-locally.
2. Create separate `/var/lib/olympus/cargo-target/dev` and `/var/lib/olympus/cargo-target/prod` roots.
3. Configure a bounded persistent sccache directory.
4. Put compiler-bearing services in an aggregate memory-bounded build slice.
5. Verify a clean test compile and `sccache --show-stats`.

### Task 6: Create isolated development Hall

**Files:**
- Create: fxcompute-01 `~/.config/olympus-dev/hall.env` mode 0600
- Create: fxcompute-01 `~/.config/systemd/user/olympus-dev-hall.service`

**Steps:**
1. Set `OLYMPUS_HOME=/home/rpw/.olympus-dev`, bind `127.0.0.1:8799`, origin `https://olympus-dev.entelechia.cloud`, and an intentionally nonexistent/isolated Hermes state path.
2. Generate development-only bootstrap credentials without printing them.
3. Run Hall from the authoritative checkout under cargo-watch with the dev target/cache.
4. Verify fresh DB/auth/token/capability/Iroh files and local `/api/health`.
5. Verify no production state path appears in the service environment or open files.

### Task 7: Create isolated development Envoy

**Files:**
- Create: fxcompute-01 `~/.config/systemd/user/olympus-dev-envoy.service`
- Create: `/srv/olympus-dev/jobs`

**Steps:**
1. Use `OLYMPUS_HOME=/home/rpw/.olympus-dev`, node ID `fxcompute-01`, roles `agent_runtime,job_runner`, and isolated job root.
2. Connect over the local `~/.olympus-dev/control.sock`.
3. Start after Hall and verify registration in dev `/api/nodes`.
4. Submit a dev canary job and verify its workspace remains under `/srv/olympus-dev/jobs`.

### Task 8: Create Vite/HMR development frontend

**Files:**
- Create: fxcompute-01 `~/.config/systemd/user/olympus-dev-ui.service`
- Modify only if required: `ui/vite.config.ts`

**Steps:**
1. Install locked UI dependencies with Bun.
2. Start Vite on `127.0.0.1:5177`, proxying to `127.0.0.1:8799`, with mocks disabled.
3. Verify HTML, API proxy, WebSocket upgrade path, and host-header handling for the dev hostname.
4. Change a harmless CSS source line and verify Vite emits HMR, then revert it.

### Task 9: Publish protected external development ingress

**Files:**
- Modify through API: Cloudflare Access application/policy, tunnel ingress, and DNS route

**Steps:**
1. Clone the existing Olympus Access application session settings and allow policy to the dev hostname.
2. Verify the Access application exists before tunnel ingress.
3. Add `olympus-dev.entelechia.cloud -> http://127.0.0.1:8800` before the 404 catch-all through the Cloudflare API.
4. Create the tunnel DNS route.
5. Verify unauthenticated external requests redirect to Cloudflare Access and local backing service remains loopback-only.

### Task 10: Add operations and promotion tooling

**Files:**
- Create: `docs/operations/dev-prod-environments.md`
- Create: `scripts/promote-production.sh`
- Create: `scripts/rollback-production.sh`
- Modify: `Makefile` or create `justfile`

**Steps:**
1. Document source authority, services, logs, recovery, promotion, rollback, and the no-build-on-Terminus invariant.
2. Implement fail-closed checks for branch `main`, clean tree, fetched remote, and `HEAD == origin/main`.
3. Build Hall, Envoy, and UI into a versioned staging bundle on fxcompute-01.
4. Generate a checksum manifest and verify it before transfer.
5. Transfer to a versioned Terminus release directory, verify again, create a consistent DB backup, atomically activate, restart, and health-gate.
6. Implement rollback to the prior release and database backup.
7. Add script tests for dirty tree, wrong branch, divergent main, bad checksum, and failed health gate without touching production.

### Task 11: Measure and verify the finished system

**Files:**
- Create: `/home/rpw/olympus-migration-20260715/performance-after.txt`
- Update: `docs/operations/dev-prod-environments.md` with measured timings

**Steps:**
1. Measure cold/warm Rust build, representative nextest run, UI startup, and HMR latency.
2. Compare to baseline and remove optimizations that regress the relevant loop.
3. Hash dev/prod token and key files and prove they differ without exposing contents.
4. Mutate a dev-only record and verify production database metadata/hash is unchanged.
5. Verify production and development fleet nodes use `fxcompute-01`.
6. Verify no active config or process uses `fxbuilder` except the documented SSH compatibility alias.
7. Monitor Terminus during a build/canary and prove no Cargo/rustc/linker/Bun/Vite process runs there.
8. Run service, local HTTP, external Access, tunnel, and canary-job checks.

### Task 12: Commit and hand off

**Files:**
- Commit only intentional repository documentation/tooling changes.

**Steps:**
1. Review diffs without absorbing pre-existing dirty work into new commits.
2. Run syntax/static checks for scripts and unit verification for service files.
3. Commit implementation files in focused commits on fxcompute-01.
4. Record rollback locations and remaining compatibility aliases in the operations document.
5. Report verified outcomes and any blocked acceptance criteria with exact evidence.
