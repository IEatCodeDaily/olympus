#!/usr/bin/env bash
set -euo pipefail

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
cat >"$tmp_profile" <<'PROFILE'
# Permit the Selenium-managed Chrome for Testing used by Maestro web flows to
# create the user namespace required by Chromium's sandbox. Keep the global
# Ubuntu restriction enabled and scope the exception to this binary path.
abi <abi/5.0>,
include <tunables/global>

profile maestro-chrome /home/*/.cache/selenium/chrome/**/chrome flags=(unconfined) {
  userns,
  @{exec_path} mr,
  include if exists <local/maestro-chrome>
}
PROFILE

install -m 0644 "$tmp_profile" "$profile"
apparmor_parser -r "$profile"
echo "Installed $profile"
