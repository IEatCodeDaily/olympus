#!/usr/bin/env bash
set -euo pipefail

BASE="${1:-${OLYMPUS_PROD_BASE:-http://127.0.0.1:8799}}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

request() {
  local name="$1" expected="$2" path="$3"
  shift 3
  local status
  status="$(curl -sS -o "$TMP_DIR/$name.body" -w '%{http_code}' "$@" "$BASE$path")"
  if [[ "$status" != "$expected" ]]; then
    echo "ERROR: $path returned $status, expected $expected" >&2
    sed -n '1,20p' "$TMP_DIR/$name.body" >&2
    exit 1
  fi
}

request index 200 /
grep -qi '<!doctype html>' "$TMP_DIR/index.body"
grep -q '<div id="root">' "$TMP_DIR/index.body"

request fallback 200 /vaults/some-vault-id
grep -qi '<!doctype html>' "$TMP_DIR/fallback.body"

request health 200 /api/health
grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"' "$TMP_DIR/health.body"

request unauthenticated 401 /api/sessions
request foreign-origin 403 /api/sessions -H 'Origin: https://evil.example.com'
request tunnel-origin 200 /api/health -H 'Origin: https://olympus.entelechia.cloud'

asset="$(grep -oE 'src="/assets/[^"]+\.js"' "$TMP_DIR/index.body" | head -1 | cut -d'"' -f2)"
[[ -n "$asset" ]] || { echo "ERROR: index.html has no JavaScript asset" >&2; exit 1; }
request asset 200 "$asset"
content_type="$(curl -sS -o /dev/null -w '%{content_type}' "$BASE$asset")"
[[ "$content_type" == *javascript* ]] || {
  echo "ERROR: $asset returned unexpected content type: $content_type" >&2
  exit 1
}

echo "Production HTTP smoke passed: $BASE"
