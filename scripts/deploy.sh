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

OLYMPUS_HOME="${OLYMPUS_HOME:-${HOME}/.olympus}"
BIN_DIR="${OLYMPUS_HOME}/bin"
WHAT="${1:-both}"

cd "$(dirname "$0")/.."

GIT_HASH="$(git rev-parse --short=12 HEAD)"
if [ -z "$GIT_HASH" ]; then
    echo "ERROR: could not determine git hash" >&2
    exit 1
fi

# Cargo may be configured with a shared target directory at the user or host
# level. Ask Cargo for the effective path instead of assuming ./target.
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 \
    | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')"

mkdir -p "$BIN_DIR"

build_hall() {
    echo "→ Building olympus-hall (release)…"
    cargo build --release -p olympus-control-plane
    echo "→ Installing olympus-hall-${GIT_HASH}…"
    cp -f "${TARGET_DIR}/release/olympus-hall" "${BIN_DIR}/olympus-hall-${GIT_HASH}"
    ln -sf "olympus-hall-${GIT_HASH}" "${BIN_DIR}/olympus-hall"
    echo "  ${BIN_DIR}/olympus-hall → olympus-hall-${GIT_HASH}"
}

provision_claude_adapter() {
    command -v node >/dev/null 2>&1 && command -v npm >/dev/null 2>&1 \
        || { echo "ERROR: Node.js >=22 and npm are required for Claude ACP" >&2; exit 2; }
    local node_major target installed
    node_major="$(node -p 'Number(process.versions.node.split(".")[0])')"
    [ "$node_major" -ge 22 ] \
        || { echo "ERROR: Node.js >=22 is required (found $(node --version))" >&2; exit 2; }
    [ -f adapters/claude-agent-acp/package.json ] \
        && [ -f adapters/claude-agent-acp/package-lock.json ] \
        || { echo "ERROR: locked Claude ACP adapter manifest is missing" >&2; exit 2; }
    target="${OLYMPUS_HOME}/adapters/claude-agent-acp"
    echo "→ Provisioning locked Claude ACP adapter…"
    mkdir -p "$target"
    cp -f adapters/claude-agent-acp/package.json adapters/claude-agent-acp/package-lock.json "$target/"
    npm ci --ignore-scripts --omit=dev --no-audit --no-fund --prefix "$target"
    installed="$(node -p "require('$target/node_modules/@agentclientprotocol/claude-agent-acp/package.json').version")"
    [ "$installed" = "0.58.1" ] \
        || { echo "ERROR: Claude ACP adapter version mismatch: $installed" >&2; exit 2; }
    [ -x "$target/node_modules/.bin/claude-agent-acp" ] \
        || { echo "ERROR: Claude ACP adapter executable missing" >&2; exit 2; }
}

build_envoy() {
    provision_claude_adapter
    echo "→ Building olympus-envoy (release)…"
    cargo build --release -p olympus-envoy
    echo "→ Installing olympus-envoy-${GIT_HASH}…"
    cp -f "${TARGET_DIR}/release/olympus-envoy" "${BIN_DIR}/olympus-envoy-${GIT_HASH}"
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
