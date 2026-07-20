#!/usr/bin/env bash
set -euo pipefail

render_profile() {
  local abi_dir="${APPARMOR_ABI_DIR:-/etc/apparmor.d/abi}"
  local abi_name=""
  local candidate
  local candidate_name
  local -a abi_names=()

  if [[ -d "$abi_dir" ]]; then
    for candidate in "$abi_dir"/*; do
      [[ -f "$candidate" ]] || continue
      candidate_name="${candidate##*/}"
      [[ "$candidate_name" =~ ^[0-9]+([.][0-9]+)*$ ]] || continue
      abi_names+=("$candidate_name")
    done
  fi
  if ((${#abi_names[@]} > 0)); then
    abi_name="$(printf '%s\n' "${abi_names[@]}" | sort -V | tail -n 1)"
  fi

  cat <<'PROFILE_HEADER'
# Permit the Selenium-managed Chrome for Testing used by Maestro web flows to
# create the user namespace required by Chromium's sandbox. Keep the global
# Ubuntu restriction enabled and scope the exception to this binary path.
PROFILE_HEADER
  if [[ -n "$abi_name" ]]; then
    printf 'abi <abi/%s>,\n' "$abi_name"
  fi
  cat <<'PROFILE_BODY'
include <tunables/global>

profile maestro-chrome /home/*/.cache/selenium/chrome/**/chrome flags=(unconfined) {
  userns,
  include if exists <local/maestro-chrome>
}
PROFILE_BODY
}

if [[ "${1:-}" == "--render-profile" ]]; then
  render_profile
  exit 0
fi

if [[ "${EUID}" -ne 0 ]]; then
  echo "ERROR: run this script with sudo" >&2
  exit 1
fi

sysctl -n kernel.apparmor_restrict_unprivileged_userns 2>/dev/null | grep -qx '1' || {
  echo "AppArmor does not restrict unprivileged user namespaces; nothing to install."
  exit 0
}
command -v apparmor_parser >/dev/null || { echo "ERROR: apparmor_parser is required" >&2; exit 1; }

profile=/etc/apparmor.d/maestro-chrome
tmp_profile="$(mktemp)"
trap 'rm -f "$tmp_profile"' EXIT
render_profile >"$tmp_profile"

install -m 0644 "$tmp_profile" "$profile"
apparmor_parser -r "$profile"
echo "Installed $profile"
