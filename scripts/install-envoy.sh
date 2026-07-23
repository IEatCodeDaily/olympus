#!/usr/bin/env bash
# scripts/install-envoy.sh — explicit Olympus Envoy installer tiers.
set -euo pipefail

DRY_RUN=false
TIER="user"
ACTION="install"
HALL_ADDR="${HALL_ADDR:-}"
INSTANCE="${INSTANCE:-1}"
HALL_PORT="${OLYMPUS_HALL_PORT:-8799}"
REPO_DIR=""

log()  { printf '\033[1;32m▸\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m⚠\033[0m %s\n' "$*" >&2; }
err()  { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; }
dry()  { printf '\033[0;90m  [dry-run]\033[0m %s\n' "$*"; }
die()  { err "$*"; exit "${2:-1}"; }
run()  { if $DRY_RUN; then dry "$*"; else "$@"; fi; }

usage() {
    cat <<'EOF'
Usage: install-envoy.sh [OPTIONS]

  --tier user|system        Installation tier (default: user)
  --hall ADDR              Hall address: uds:<path> or iroh:<node-id>
  --instance N             User-tier instance number (default: 1)
  --repair                 Reinstall the selected tier over an existing install
  --migrate                Remove the other tier, then install the selected tier
  --uninstall              Remove the selected tier
  --print-capabilities     Print the selected tier's advertised capabilities
  --dry-run                Print actions without executing
  -h, --help               Show this help

One Envoy tier may exist per host. Cross-tier duplicates are refused unless
--migrate or --uninstall is explicit.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --tier)               TIER="${2:-}"; shift 2 ;;
        --hall)               HALL_ADDR="${2:-}"; shift 2 ;;
        --instance)           INSTANCE="${2:-}"; shift 2 ;;
        --repair)             ACTION="repair"; shift ;;
        --migrate)            ACTION="migrate"; shift ;;
        --uninstall)          ACTION="uninstall"; shift ;;
        --print-capabilities) ACTION="capabilities"; shift ;;
        --dry-run)            DRY_RUN=true; shift ;;
        -h|--help)            usage; exit 0 ;;
        *)                    err "unknown option: $1"; usage; exit 1 ;;
    esac
done

case "$TIER" in user|system) ;; *) die "unknown --tier '$TIER' (expected user or system)" 1 ;; esac

xdg_config_home() { printf '%s\n' "${XDG_CONFIG_HOME:-$HOME/.config}"; }
xdg_state_home() { printf '%s\n' "${XDG_STATE_HOME:-$HOME/.local/state}"; }
xdg_runtime_dir() { printf '%s\n' "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"; }

user_unit() { printf '%s/systemd/user/olympus-envoy@.service\n' "$(xdg_config_home)"; }
user_dropin() { printf '%s/systemd/user/olympus-envoy@.service.d/instance-%s.conf\n' "$(xdg_config_home)" "$INSTANCE"; }
user_bin_dir() { printf '%s/olympus/bin\n' "$(xdg_state_home)"; }
user_state_dir() { printf '%s/olympus/envoy/envoy-%s\n' "$(xdg_state_home)" "$INSTANCE"; }
user_runtime_dir() { printf '%s/olympus\n' "$(xdg_runtime_dir)"; }

system_unit() { printf '/etc/systemd/system/olympus-envoy.service\n'; }
system_bin_dir() { printf '/usr/local/lib/olympus/bin\n'; }
system_state_dir() { printf '/var/lib/olympus/envoy\n'; }
system_config_dir() { printf '/etc/olympus/envoy\n'; }

capabilities() {
    if [[ "$TIER" == "system" ]]; then
        printf '%s\n' agent_runtime job_runner rootless_workloads system_envoy host_telemetry host_scoped_helpers
    else
        printf '%s\n' agent_runtime rootless_workloads basic_telemetry SYSTEM_ENVOY_REQUIRED
    fi
}

if [[ "$ACTION" == "capabilities" ]]; then
    capabilities
    exit 0
fi

check_platform() {
    [[ "$(uname -s)" == "Linux" ]] || die "unsupported OS: $(uname -s) (Linux only for now)" 2
    [[ "$(uname -m)" == "x86_64" ]] || die "unsupported arch: $(uname -m) (x86_64 only for now)" 2
}

locate_repo() {
    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    REPO_DIR="$(cd "$script_dir/.." && pwd)"
    [[ -f "$REPO_DIR/Cargo.toml" && -d "$REPO_DIR/crates/envoy" ]] \
        || die "could not find olympus repo relative to $script_dir" 2
}

require_user_prereqs() {
    command -v systemctl >/dev/null || die "systemd is required" 2
    systemctl --user show-environment >/dev/null 2>&1 \
        || warn "no active systemd user session; start one before enabling the user service"
    if command -v loginctl >/dev/null 2>&1 \
        && ! loginctl show-user "$(whoami)" 2>/dev/null | grep -q 'Linger=yes'; then
        warn "linger is OFF; the user Envoy stops when this user logs out."
        warn "ask an operator to run: loginctl enable-linger $(whoami)"
    fi
}

require_build_prereqs() {
    command -v cargo >/dev/null || die "cargo not found — cannot build olympus-envoy" 2
    command -v hermes >/dev/null || die "required CLI 'hermes' not found" 2
}

validate_hall_addr() {
    [[ -n "$HALL_ADDR" ]] || die "no --hall address provided (or set HALL_ADDR env)" 1
    case "$HALL_ADDR" in
        uds:*) log "Transport: UDS (${HALL_ADDR#uds:})" ;;
        iroh:*) [[ -n "${HALL_ADDR#iroh:}" ]] || die "iroh node id is empty" 1; log "Transport: iroh (${HALL_ADDR#iroh:})" ;;
        *) die "unrecognized --hall format: $HALL_ADDR (expected uds:<path> or iroh:<node-id>)" 1 ;;
    esac
}

installed_tiers() {
    [[ -e "$(user_unit)" || -d "$(xdg_state_home)/olympus/envoy" ]] && printf 'user\n'
    [[ -e "$(system_unit)" || -d "$(system_state_dir)" ]] && printf 'system\n'
}

refuse_duplicates() {
    local found other
    found="$(installed_tiers || true)"
    other="$(printf '%s\n' "$found" | grep -vx "$TIER" || true)"
    if [[ "$ACTION" == "install" && -n "$found" ]]; then
        die "duplicate envoy install detected ($found). Use --repair, --migrate, or --uninstall explicitly." 1
    fi
    if [[ "$ACTION" == "repair" && -n "$other" ]]; then
        die "duplicate envoy install detected ($other). Use --migrate or --uninstall explicitly." 1
    fi
}

target_dir() {
    cargo metadata --no-deps --format-version 1 \
        | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])'
}

build_and_install_binary() {
    local bin_dir="$1" target hash cargo_target
    log "Building olympus-envoy (release)…"
    if $DRY_RUN; then
        dry "cd '$REPO_DIR' && cargo build --release -p olympus-envoy"
        hash="dryrun000000"
        cargo_target="$REPO_DIR/target"
    else
        (cd "$REPO_DIR" && cargo build --release -p olympus-envoy) || die "cargo build failed" 3
        hash="$(cd "$REPO_DIR" && git rev-parse --short=12 HEAD)"
        cargo_target="$(cd "$REPO_DIR" && target_dir)"
    fi
    target="$bin_dir/olympus-envoy-$hash"
    run mkdir -p "$bin_dir"
    run cp -f "$cargo_target/release/olympus-envoy" "$target"
    run ln -sfn "olympus-envoy-$hash" "$bin_dir/olympus-envoy"
    log "  $bin_dir/olympus-envoy → olympus-envoy-$hash"
}

unit_exec_args() {
    local node_id="$1"
    case "$HALL_ADDR" in
        uds:*) printf -- '--socket %s --node-id %s' "${HALL_ADDR#uds:}" "$node_id" ;;
        iroh:*) printf -- '--hall %s --node-id %s' "$HALL_ADDR" "$node_id" ;;
    esac
}

install_user() {
    local unit dropin bin_dir node_id args
    require_user_prereqs
    unit="$(user_unit)"; dropin="$(user_dropin)"; bin_dir="$(user_bin_dir)"; node_id="envoy-$INSTANCE"
    build_and_install_binary "$bin_dir"
    run mkdir -p "$(dirname "$unit")" "$(dirname "$dropin")" "$(user_state_dir)" "$(user_runtime_dir)"
    args="$(unit_exec_args "$node_id") --state-dir $(user_state_dir) --roles agent_runtime"
    if $DRY_RUN; then
        dry "write user unit: $unit"
        dry "write drop-in: $dropin"
        dry "ExecStart=$bin_dir/olympus-envoy $args"
    else
        cat > "$unit" <<UNIT
[Unit]
Description=Olympus Envoy %i — user agent runtime holder
After=network-online.target

[Service]
Type=simple
Environment="PATH=%h/.local/bin:%h/.bun/bin:%h/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
Environment="OLYMPUS_ENVOY_TIER=user"
Environment="OLYMPUS_ENVOY_ROLES=agent_runtime"
ExecStart=$bin_dir/olympus-envoy
Restart=on-failure
RestartSec=5
KillMode=mixed
KillSignal=SIGTERM
TimeoutStopSec=10

[Install]
WantedBy=default.target
UNIT
        cat > "$dropin" <<DROPIN
[Service]
ExecStart=
ExecStart=$bin_dir/olympus-envoy $args
Environment="OLYMPUS_NODE_ID=$node_id"
DROPIN
    fi
    run systemctl --user daemon-reload
    run systemctl --user enable "olympus-envoy@$INSTANCE.service"
    run systemctl --user restart "olympus-envoy@$INSTANCE.service"
}

install_system() {
    local unit bin_dir args
    unit="$(system_unit)"; bin_dir="$(system_bin_dir)"
    build_and_install_binary "$bin_dir"
    args="$(unit_exec_args "envoy-system") --state-dir $(system_state_dir) --roles agent_runtime,job_runner,system-envoy"
    run install -d -m 0755 "$(system_config_dir)" "$(system_state_dir)" "$bin_dir"
    if $DRY_RUN; then
        dry "write system unit: $unit"
        dry "User=olympus-envoy"
        dry "CapabilityBoundingSet="
        dry "NoNewPrivileges=yes"
        dry "ExecStart=$bin_dir/olympus-envoy $args"
    else
        if ! id -u olympus-envoy >/dev/null 2>&1; then
            useradd --system --home-dir /var/lib/olympus --shell /usr/sbin/nologin olympus-envoy
        fi
        chown -R olympus-envoy:olympus-envoy "$(system_state_dir)"
        cat > "$unit" <<UNIT
[Unit]
Description=Olympus Envoy — system host capability holder
After=network-online.target

[Service]
Type=simple
User=olympus-envoy
Group=olympus-envoy
Environment="OLYMPUS_ENVOY_TIER=system"
Environment="OLYMPUS_ENVOY_ROLES=agent_runtime,job_runner,system_envoy"
ExecStart=$bin_dir/olympus-envoy $args
Restart=on-failure
RestartSec=5
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=$(system_state_dir)
CapabilityBoundingSet=
AmbientCapabilities=
LockPersonality=yes
RestrictSUIDSGID=yes

[Install]
WantedBy=multi-user.target
UNIT
    fi
    run systemctl daemon-reload
    run systemctl enable olympus-envoy.service
    run systemctl restart olympus-envoy.service
}

uninstall_tier() {
    local tier="$1"
    if [[ "$tier" == "user" ]]; then
        run systemctl --user disable --now "olympus-envoy@$INSTANCE.service"
        run rm -f "$(user_dropin)" "$(user_unit)"
        run systemctl --user daemon-reload
    else
        run systemctl disable --now olympus-envoy.service
        run rm -f "$(system_unit)"
        run systemctl daemon-reload
    fi
}

verify_registration() {
    $DRY_RUN && { dry "poll /api/nodes for tier '$TIER' (up to 30s)"; return 0; }
    local token_file="${XDG_STATE_HOME:-$HOME/.local/state}/olympus/token"
    [[ -f "$token_file" ]] || { warn "token file $token_file not found — cannot verify registration via API"; return 0; }
    for _ in $(seq 1 30); do
        curl -sf -H "Authorization: Bearer ***" "http://127.0.0.1:$HALL_PORT/api/nodes" | grep -q 'envoy' && return 0
        sleep 1
    done
    die "registration timeout" 4
}

main() {
    local state_path
    check_platform
    locate_repo
    refuse_duplicates
    if [[ "$TIER" == user ]]; then
        state_path="$(user_state_dir)"
    else
        state_path="$(system_state_dir)"
    fi
    log "Olympus Envoy installer"
    log "  tier:  $TIER"
    log "  state: $state_path"
    $DRY_RUN && log "  mode:  DRY-RUN"
    if [[ "$ACTION" == "uninstall" ]]; then
        uninstall_tier "$TIER"
        return 0
    fi
    if [[ "$ACTION" == "migrate" ]]; then
        if [[ "$TIER" == user ]]; then
            uninstall_tier system
        else
            uninstall_tier user
        fi
    fi
    require_build_prereqs
    validate_hall_addr
    if [[ "$TIER" == user ]]; then
        install_user
    else
        install_system
    fi
    verify_registration
}

main "$@"