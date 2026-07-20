#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
installer="$script_dir/install-maestro-apparmor.sh"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

mkdir -p "$tmp_dir/abi"
touch "$tmp_dir/abi/3.0" "$tmp_dir/abi/4.0" "$tmp_dir/abi/10.0" \
  "$tmp_dir/abi/kernel-99.0-vanilla"
APPARMOR_ABI_DIR="$tmp_dir/abi" bash "$installer" --render-profile >"$tmp_dir/with-abi.profile"
grep -qx 'abi <abi/10.0>,' "$tmp_dir/with-abi.profile"
if grep -q 'abi <abi/kernel-' "$tmp_dir/with-abi.profile"; then
  echo "ERROR: selected a kernel feature ABI instead of a policy ABI" >&2
  exit 1
fi

mkdir -p "$tmp_dir/empty-abi"
APPARMOR_ABI_DIR="$tmp_dir/empty-abi" bash "$installer" --render-profile >"$tmp_dir/without-abi.profile"
if grep -q '^abi <' "$tmp_dir/without-abi.profile"; then
  echo "ERROR: rendered an ABI declaration without a host ABI file" >&2
  exit 1
fi

if command -v apparmor_parser >/dev/null; then
  bash "$installer" --render-profile >"$tmp_dir/host.profile"
  apparmor_parser -Q -T "$tmp_dir/host.profile"
fi

echo "AppArmor profile rendering checks passed"
