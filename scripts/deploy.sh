#!/usr/bin/env bash
# scripts/deploy.sh — build + install olympus-hall and/or olympus-envoy binaries
# with git-hash suffixes + symlink flip (ADR 0008 §5 deploy choreography).
#
# Usage:
#   scripts/deploy.sh hall          # build + install hall only
#   scripts/deploy.sh envoy         # build + install envoy only
#   scripts/deploy.sh both          # build + install both (default)
#
# Binaries are installed as:
#   ~/.olympus/bin/olympus-hall-<gitHash>
#   ~/.olympus/bin/olympus-envoy-<gitHash>
# with stable symlinks:
#   ~/.olympus/bin/olympus-hall → olympus-hall-<gitHash>
#   ~/.olympus/bin/olympus-envoy → olympus-envoy-<gitHash>
#
# Does NOT restart any services — use `make deploy-hall` / `make deploy-envoy`
# for the full choreography (symlink flip + systemd restart + health gate).
set -euo pipefail

BIN_DIR="${HOME}/.olympus/bin"
WHAT="${1:-both}"

cd "$(dirname "$0")/.."

GIT_HASH="$(git rev-parse --short=12 HEAD)"
if [ -z "$GIT_HASH" ]; then
    echo "ERROR: could not determine git hash" >&2
    exit 1
fi

mkdir -p "$BIN_DIR"

build_hall() {
    echo "→ Building olympus-hall (release)…"
    cargo build --release -p olympus-control-plane
    echo "→ Installing olympus-hall-${GIT_HASH}…"
    cp -f target/release/olympus-hall "${BIN_DIR}/olympus-hall-${GIT_HASH}"
    ln -sf "olympus-hall-${GIT_HASH}" "${BIN_DIR}/olympus-hall"
    echo "  ${BIN_DIR}/olympus-hall → olympus-hall-${GIT_HASH}"
}

build_envoy() {
    echo "→ Building olympus-envoy (release)…"
    cargo build --release -p olympus-envoy
    echo "→ Installing olympus-envoy-${GIT_HASH}…"
    cp -f target/release/olympus-envoy "${BIN_DIR}/olympus-envoy-${GIT_HASH}"
    ln -sf "olympus-envoy-${GIT_HASH}" "${BIN_DIR}/olympus-envoy"
    echo "  ${BIN_DIR}/olympus-envoy → olympus-envoy-${GIT_HASH}"
}

case "$WHAT" in
    hall)  build_hall ;;
    envoy) build_envoy ;;
    both)  build_hall; build_envoy ;;
    *) echo "Usage: $0 {hall|envoy|both}" >&2; exit 1 ;;
esac

echo "✓ Deploy install complete: ${WHAT} @ ${GIT_HASH}"
