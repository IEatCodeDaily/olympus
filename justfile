set shell := ["/usr/bin/bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# Show all development and production-runner services on fxcompute-01.
dev-status:
    systemctl --user --no-pager --full status olympus-dev-hall.service olympus-dev-envoy.service olympus-dev-ui.service olympus-prod-job-runner.service fxcompute-01-tunnel.service

# Restart the isolated development stack only.
dev-restart:
    systemctl --user restart olympus-dev-hall.service olympus-dev-envoy.service olympus-dev-ui.service

# Fast local verification loop.
check-fast:
    CARGO_HOME=/var/lib/olympus/cargo-home CARGO_TARGET_DIR=/var/lib/olympus/cargo-target-dev RUSTUP_HOME=/home/rpw/.rustup cargo check --workspace
    CARGO_HOME=/var/lib/olympus/cargo-home CARGO_TARGET_DIR=/var/lib/olympus/cargo-target-dev RUSTUP_HOME=/home/rpw/.rustup cargo nextest run --workspace
    cd ui && bun run build

# Promote a clean origin/main release to Terminus. Terminus never builds.
promote:
    ./scripts/promote-production.sh
