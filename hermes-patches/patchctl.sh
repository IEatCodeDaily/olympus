#!/usr/bin/env bash
# patchctl.sh — manage Olympus's patches to Hermes Agent.
# We patch, never fork. See README.md + manifest.toml.
#
# Usage:
#   ./patchctl.sh status                  # registry + per-patch apply state
#   ./patchctl.sh check                   # dry-run: do all patches apply cleanly?
#   ./patchctl.sh apply                   # apply all (idempotent; skips applied)
#   ./patchctl.sh save <slug> [paths...]  # capture working-tree diff as a new patch
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PATCH_DIR="$HERE/patches"
MANIFEST="$HERE/manifest.toml"

# Resolve the Hermes checkout: env override > manifest > default.
hermes_dir() {
  if [[ -n "${HERMES_AGENT_DIR:-}" ]]; then echo "$HERMES_AGENT_DIR"; return; fi
  local m
  m="$(python3 - "$MANIFEST" <<'PY'
import sys,tomllib,os
with open(sys.argv[1],'rb') as f: d=tomllib.load(f)
print(os.path.expanduser(d.get("target",{}).get("checkout","~/.hermes/hermes-agent")))
PY
)"
  echo "$m"
}

# Ordered list of patch files from the manifest.
patch_files() {
  python3 - "$MANIFEST" <<'PY'
import sys,tomllib
with open(sys.argv[1],'rb') as f: d=tomllib.load(f)
for p in d.get("patch",[]):
    print(p["file"])
PY
}

HDIR="$(hermes_dir)"

require_checkout() {
  [[ -d "$HDIR/.git" ]] || { echo "ERROR: Hermes checkout not a git repo: $HDIR" >&2; exit 1; }
}

cmd_status() {
  require_checkout
  echo "Hermes checkout: $HDIR"
  echo "HEAD:           $(git -C "$HDIR" log --oneline -1 2>/dev/null)"
  local base; base="$(python3 -c "import tomllib;print(tomllib.load(open('$MANIFEST','rb'))['target'].get('base_commit','?'))")"
  echo "manifest base:  $base"
  echo
  local files; files="$(patch_files)"
  if [[ -z "$files" ]]; then echo "(no patches registered yet)"; return; fi
  printf '%-40s %s\n' "PATCH" "STATE"
  while read -r f; do
    [[ -z "$f" ]] && continue
    local pf="$PATCH_DIR/$f" state
    if [[ ! -f "$pf" ]]; then state="MISSING-FILE"
    elif git -C "$HDIR" apply --reverse --check "$pf" >/dev/null 2>&1; then state="APPLIED"
    elif git -C "$HDIR" apply --check "$pf" >/dev/null 2>&1; then state="APPLIES-CLEAN (not yet applied)"
    elif git -C "$HDIR" apply --3way --check "$pf" >/dev/null 2>&1; then state="NEEDS-3WAY (upstream drift)"
    else state="CONFLICT"
    fi
    printf '%-40s %s\n' "$f" "$state"
  done <<< "$files"
}

cmd_check() {
  require_checkout
  local files rc=0; files="$(patch_files)"
  [[ -z "$files" ]] && { echo "(no patches to check)"; return 0; }
  while read -r f; do
    [[ -z "$f" ]] && continue
    local pf="$PATCH_DIR/$f"
    if git -C "$HDIR" apply --reverse --check "$pf" >/dev/null 2>&1; then
      echo "OK (already applied): $f"
    elif git -C "$HDIR" apply --check "$pf" >/dev/null 2>&1; then
      echo "OK (will apply):      $f"
    else
      echo "FAIL:                 $f"; rc=1
    fi
  done <<< "$files"
  return $rc
}

cmd_apply() {
  require_checkout
  local files; files="$(patch_files)"
  [[ -z "$files" ]] && { echo "(no patches to apply)"; return 0; }
  while read -r f; do
    [[ -z "$f" ]] && continue
    local pf="$PATCH_DIR/$f"
    if git -C "$HDIR" apply --reverse --check "$pf" >/dev/null 2>&1; then
      echo "skip (applied): $f"
    elif git -C "$HDIR" apply "$pf" >/dev/null 2>&1; then
      echo "applied:        $f"
    elif git -C "$HDIR" apply --3way "$pf" >/dev/null 2>&1; then
      echo "applied (3way): $f"
    else
      echo "ERROR applying:  $f — resolve manually (git -C $HDIR apply --3way $pf)" >&2; exit 1
    fi
  done <<< "$files"
}

cmd_save() {
  require_checkout
  local slug="${1:-}"; shift || true
  [[ -n "$slug" ]] || { echo "usage: patchctl save <slug> [paths...]" >&2; exit 1; }
  mkdir -p "$PATCH_DIR"
  local out="$PATCH_DIR/${slug}.patch"
  # Diff the working tree (staged+unstaged) for the given paths, git-format.
  git -C "$HDIR" diff HEAD -- "$@" > "$out"
  [[ -s "$out" ]] || { echo "ERROR: empty diff — nothing to save for: $*" >&2; rm -f "$out"; exit 1; }
  echo "wrote $out"
  echo "NEXT: add an entry to manifest.toml [[patch]], then commit to the Olympus repo."
}

case "${1:-status}" in
  status) cmd_status ;;
  check)  cmd_check ;;
  apply)  cmd_apply ;;
  save)   shift; cmd_save "$@" ;;
  *) echo "usage: $0 {status|check|apply|save}" >&2; exit 1 ;;
esac
