#!/usr/bin/env bash
# scripts/install-envoy.sh — one-time curl-able installer for an Olympus Envoy.
#
# Builds the olympus-envoy binary from source, installs it with git-hash suffix +
# symlink flip (same choreography as scripts/deploy.sh), installs the systemd
# user unit from the repo (systemd/olympus-envoy@.service), enables + starts an
# instance, and verifies registration with the Hall by polling /api/nodes.
#
# Idempotent: re-running upgrades the binary and restarts the instance. It never
# duplicates systemd units.
#
# Usage:
#   scripts/install-envoy.sh --hall uds:/home/rpw/.olympus/control.sock --instance 1
#   scripts/install-envoy.sh --hall iroh:<hall-node-id> --instance 2
#   scripts/install-envoy.sh --dry-run       # print actions without executing
#
# Environment:
#   HALL_ADDR        same as --hall (flag takes precedence)
#   INSTANCE         same as --instance (default 1)
#   OLYMPUS_HOME     base dir (default $HOME/.olympus)
#   OLYMPUS_HALL_PORT  Hall API port (default 8799)
#
# Exit codes: 0 success, 1 usage/misuse, 2 prerequisite failure,
#             3 build failure, 4 registration timeout.
set -euo pipefail

# ── Globals ─────────────────────────────────────────────────────────────
DRY_RUN=false
HALL_ADDR=""
INSTANCE="${INSTANCE:-1}"
OLYMPUS_HOME="${OLYMPUS_HOME:-$HOME/.olympus}"
BIN_DIR=""
HALL_PORT="${OLYMPUS_HALL_PORT:-8799}"
REPO_DIR=""

# ── Pretty output ───────────────────────────────────────────────────────
log()  { printf '\033[1;32m▸\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m⚠\033[0m %s\n' "$*" >&2; }
err()  { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; }
dry()  { printf '\033[0;90m  [dry-run]\033[0m %s\n' "$*"; }
die()  { err "$*"; exit "${2:-1}"; }

# Execute a command (skipped in dry-run).
run() {
    if $DRY_RUN; then dry "$*"; else "$@"; fi
}

# Execute a command even in dry-run (read-only state checks).
run_force() { "$@"; }

# ── Parse args ──────────────────────────────────────────────────────────
usage() {
    cat <<'EOF'
Usage: install-envoy.sh [OPTIONS]

  --hall ADDR        Hall address: uds:<path> or iroh:<node-id>
                     (env: HALL_ADDR)
  --instance N       Envoy instance number (env: INSTANCE, default 1)
  --dry-run          Print actions without executing
  -h, --help         Show this help

Examples:
  install-envoy.sh --hall uds:$HOME/.olympus/control.sock --instance 1
  install-envoy.sh --hall iroh:abc123def456 --instance 2
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --hall)         HALL_ADDR="${2:-}"; shift 2 ;;
        --instance)     INSTANCE="${2:-}"; shift 2 ;;
        --dry-run)      DRY_RUN=true; shift ;;
        -h|--help)      usage; exit 0 ;;
        *)              err "unknown option: $1"; usage; exit 1 ;;
    esac
done

# Fall back to env, then defaults.
HALL_ADDR="${HALL_ADDR:-${HALL_ADDR:-}}"
INSTANCE="${INSTANCE:-1}"

if [[ -z "$HALL_ADDR" ]]; then
    die "no --hall address provided (or set HALL_ADDR env)" 1
fi

BIN_DIR="$OLYMPUS_HOME/bin"

# ── 1. Platform check ───────────────────────────────────────────────────
check_platform() {
    local arch os
    arch="$(uname -m)"
    os="$(uname -s)"
    if [[ "$os" != "Linux" ]]; then
        die "unsupported OS: $os (Linux only for now)" 2
    fi
    if [[ "$arch" != "x86_64" ]]; then
        die "unsupported arch: $arch (x86_64 only for now)" 2
    fi
    log "Platform OK: $os/$arch"
}

# ── 2. Locate repo + verify build prerequisites ─────────────────────────
locate_repo() {
    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    REPO_DIR="$(cd "$script_dir/.." && pwd)"

    if [[ ! -f "$REPO_DIR/Cargo.toml" ]]; then
        die "could not find Cargo.toml relative to $script_dir — is this inside the olympus repo?" 2
    fi
    if [[ ! -d "$REPO_DIR/crates/envoy" ]]; then
        die "envoy crate not found at $REPO_DIR/crates/envoy — wrong repo?" 2
    fi

    log "Repo: $REPO_DIR"

    if ! command -v cargo &>/dev/null; then
        cat >&2 <<'MSG'
ERROR: cargo is not on PATH. Install Rust to build olympus-envoy:

  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

Pre-built release binaries are not yet available (future CI work).
MSG
        die "cargo not found — cannot build olympus-envoy" 2
    fi

    # tmux: required for persistent operator terminals (ADR 0021 cockpit).
    # Without it the envoy falls back to bare PTY (non-persistent) with a
    # visible badge, so this is a warning, not a hard failure.
    if ! command -v tmux &>/dev/null; then
        log "WARNING: tmux not found — operator terminals will be non-persistent."
        log "  Install tmux for persistent sessions:"
        log "    apt install tmux    # Debian/Ubuntu"
        log "    dnf install tmux    # Fedora/RHEL"
    fi
}

build_and_install() {
    log "Building olympus-envoy (release)…"
    local target_dir
    target_dir="$(cd "$REPO_DIR" && cargo metadata --no-deps --format-version 1 \
        | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')"
    if $DRY_RUN; then
        dry "cd '$REPO_DIR' && cargo build --release -p olympus-envoy"
    else
        (cd "$REPO_DIR" && cargo build --release -p olympus-envoy) || die "cargo build failed" 3
    fi

    local git_hash
    if $DRY_RUN; then
        git_hash="dryrun0000000"
        dry "git rev-parse --short=12 HEAD → $git_hash"
    else
        git_hash="$(cd "$REPO_DIR" && git rev-parse --short=12 HEAD)"
    fi

    local target="$BIN_DIR/olympus-envoy-$git_hash"
    local symlink="$BIN_DIR/olympus-envoy"

    log "Installing $target"
    run mkdir -p "$BIN_DIR"
    run cp -f "$target_dir/release/olympus-envoy" "$target"
    run ln -sfn "olympus-envoy-$git_hash" "$symlink"
    log "  $symlink → olympus-envoy-$git_hash"
}

# ── 3. Required CLI check ───────────────────────────────────────────────
check_required_clis() {
    # hermes — fail closed (envoy needs it to spawn agent sessions).
    if ! command -v hermes &>/dev/null; then
        err "hermes is not on PATH — envoy requires it to spawn agent sessions."
        err "  Install Hermes Agent: https://hermes-agent.nousresearch.com/docs"
        die "required CLI 'hermes' not found" 2
    fi
    log "Required CLI OK: hermes"

    # jj / gh / bunx — warn only.
    local cli
    for cli in jj gh bunx; do
        if ! command -v "$cli" &>/dev/null; then
            warn "optional CLI '$cli' not found (some agent features may be limited)"
        fi
    done
}

install_agent_adapters() {
    if ! command -v node &>/dev/null || ! command -v npm &>/dev/null; then
        die "Node.js >=22 and npm are required to provision the Claude ACP adapter" 2
    fi
    local node_major
    node_major="$(node -p 'Number(process.versions.node.split(".")[0])')"
    if [[ "$node_major" -lt 22 ]]; then
        die "Node.js >=22 is required for Claude ACP (found $(node --version))" 2
    fi

    local source="$REPO_DIR/adapters/claude-agent-acp"
    local target="$OLYMPUS_HOME/adapters/claude-agent-acp"
    [[ -f "$source/package.json" && -f "$source/package-lock.json" ]] \
        || die "locked Claude ACP adapter manifest is missing" 2
    log "Provisioning locked Claude ACP adapter…"
    run mkdir -p "$target"
    run cp -f "$source/package.json" "$source/package-lock.json" "$target/"
    if $DRY_RUN; then
        dry "npm ci --ignore-scripts --omit=dev --no-audit --no-fund --prefix '$target'"
    else
        npm ci --ignore-scripts --omit=dev --no-audit --no-fund --prefix "$target" \
            || die "Claude ACP adapter install failed" 2
        local installed
        installed="$(node -p "require('$target/node_modules/@agentclientprotocol/claude-agent-acp/package.json').version")"
        [[ "$installed" == "0.58.1" ]] \
            || die "Claude ACP adapter version mismatch: expected 0.58.1, got $installed" 2
        [[ -x "$target/node_modules/.bin/claude-agent-acp" ]] \
            || die "Claude ACP adapter executable missing after npm ci" 2
    fi
    log "  Claude ACP adapter: 0.58.1 (locked, install-time provisioned)"
}

# ── 4. Transport config validation ──────────────────────────────────────
validate_hall_addr() {
    case "$HALL_ADDR" in
        uds:*)
            local path="${HALL_ADDR#uds:}"
            if [[ ! -e "$path" ]]; then
                warn "UDS socket $path does not exist yet — hall may not be running"
            fi
            log "Transport: UDS ($path)"
            ;;
        iroh:*)
            local node_id="${HALL_ADDR#iroh:}"
            if [[ -z "$node_id" ]]; then
                die "iroh transport selected but node-id is empty" 1
            fi
            log "Transport: iroh ($node_id)"
            ;;
        *)
            die "unrecognized --hall format: $HALL_ADDR (expected uds:<path> or iroh:<node-id>)" 1
            ;;
    esac
}

# ── 5. Install systemd unit + start + verify ────────────────────────────
install_systemd_unit() {
    local unit_src="$REPO_DIR/systemd/olympus-envoy@.service"
    local unit_dest_dir="$HOME/.config/systemd/user"
    local unit_dest="$unit_dest_dir/olympus-envoy@.service"

    if [[ ! -f "$unit_src" ]]; then
        die "systemd unit template not found: $unit_src" 2
    fi

    log "Installing systemd user unit (from repo — not duplicated)"
    run mkdir -p "$unit_dest_dir"
    run cp -f "$unit_src" "$unit_dest"
    run systemctl --user daemon-reload
}

# Write a drop-in override for this instance number that sets the correct
# ExecStart args (transport-specific) + node id.
write_instance_override() {
    local node_id="envoy-$INSTANCE"
    local dropin_dir="$HOME/.config/systemd/user/olympus-envoy@.service.d"
    local dropin_file="$dropin_dir/instance-$INSTANCE.conf"

    local exec_args=""
    case "$HALL_ADDR" in
        uds:*)
            local socket_path="${HALL_ADDR#uds:}"
            exec_args="--socket $socket_path --node-id $node_id"
            ;;
        iroh:*)
            exec_args="--hall $HALL_ADDR --node-id $node_id"
            ;;
    esac

    if $DRY_RUN; then
        dry "write drop-in: $dropin_file"
        dry "  [Service]"
        dry "  ExecStart="
        dry "  ExecStart=$BIN_DIR/olympus-envoy $exec_args"
        dry "  Environment=OLYMPUS_NODE_ID=$node_id"
    else
        mkdir -p "$dropin_dir"
        cat > "$dropin_file" <<EOF
# Auto-generated by install-envoy.sh for instance $INSTANCE.
# Do not edit — re-run install-envoy.sh to update.
[Service]
ExecStart=
ExecStart=$BIN_DIR/olympus-envoy $exec_args
Environment="OLYMPUS_NODE_ID=$node_id"
EOF
        log "  drop-in: $dropin_file"
    fi
}

enable_and_start() {
    local unit="olympus-envoy@$INSTANCE.service"
    log "Enabling + starting $unit"
    run systemctl --user enable "$unit"

    # If already running, restart to pick up the new binary/override.
    if run_force systemctl --user is-active --quiet "$unit" 2>/dev/null; then
        log "  already running — restarting to pick up upgrade"
        run systemctl --user restart "$unit"
    else
        run systemctl --user start "$unit"
    fi
}

# Poll Hall /api/nodes for the new envoy node-id (envoy-<INSTANCE>).
verify_registration() {
    local expected_node="envoy-$INSTANCE"
    local token_file="$OLYMPUS_HOME/token"
    local token=""

    if $DRY_RUN; then
        dry "poll /api/nodes for '$expected_node' (up to 30s)"
        return 0
    fi

    if [[ -f "$token_file" ]]; then
        token="$(cat "$token_file")"
    else
        warn "token file $token_file not found — cannot verify registration via API"
        warn "check manually: curl -H 'Authorization: Bearer <token>' http://127.0.0.1:$HALL_PORT/api/nodes"
        return 0
    fi

    log "Polling /api/nodes for '$expected_node' (up to 30s)…"

    local i online
    for ((i = 0; i < 30; i++)); do
        online="$(curl -sf -H "Authorization: Bearer $token" \
            "http://127.0.0.1:$HALL_PORT/api/nodes" 2>/dev/null \
            | grep -c "\"nodeId\":\"$expected_node\"" || true)"

        if [[ "$online" -gt 0 ]]; then
            log "✓ Registered: $expected_node is online"
            return 0
        fi
        sleep 1
    done

    err "envoy '$expected_node' did not register within 30s."
    err "Diagnose with:"
    err "  systemctl --user status olympus-envoy@$INSTANCE"
    err "  journalctl --user -u olympus-envoy@$INSTANCE --no-pager -n 50"
    die "registration timeout" 4
}

# ── Main ────────────────────────────────────────────────────────────────
main() {
    log "Olympus Envoy installer — instance $INSTANCE"
    log "  hall:  $HALL_ADDR"
    log "  home:  $OLYMPUS_HOME"
    if $DRY_RUN; then
        log "  mode:  DRY-RUN (no changes will be made)"
    fi

    check_platform
    locate_repo
    check_required_clis
    install_agent_adapters
    validate_hall_addr
    build_and_install
    install_systemd_unit
    write_instance_override
    enable_and_start
    verify_registration

    log "Done. Envoy '$INSTANCE' is installed and registered."
    if $DRY_RUN; then
        warn "dry-run complete — no services were started."
    fi
}

main "$@"
