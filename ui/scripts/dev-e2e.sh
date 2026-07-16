#!/usr/bin/env bash
# Nightly from Terminus:
# ssh fxcompute-01 'cd /home/rpw/olympus && ui/scripts/dev-e2e.sh'
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
UI="$ROOT/ui"
CREDS="${OLYMPUS_DEV_CREDENTIALS:-$HOME/.config/olympus-dev/admin-credentials}"
export OLYMPUS_DEV_BASE_URL="${OLYMPUS_DEV_BASE_URL:-http://127.0.0.1:5177}"

[[ -r "$CREDS" ]] || { echo "ERROR: unreadable credentials: $CREDS" >&2; exit 1; }
while IFS='=' read -r key value; do
  case "$key" in
    username) export OLYMPUS_DEV_USERNAME="$value" ;;
    password) export OLYMPUS_DEV_PASSWORD="$value" ;;
  esac
done < "$CREDS"
[[ -n "${OLYMPUS_DEV_USERNAME:-}" && -n "${OLYMPUS_DEV_PASSWORD:-}" ]] || {
  echo "ERROR: credentials must contain username= and password=" >&2
  exit 1
}

curl -fsS "$OLYMPUS_DEV_BASE_URL/" >/dev/null || {
  echo "ERROR: dev UI unavailable at $OLYMPUS_DEV_BASE_URL" >&2
  exit 1
}
curl -fsS "${OLYMPUS_DEV_HALL_URL:-http://127.0.0.1:8799}/health" >/dev/null || {
  echo "ERROR: dev Hall unavailable" >&2
  exit 1
}

cd "$UI"
rm -rf test-results/dev-e2e
exec timeout --signal=TERM --kill-after=10s 9m bunx playwright test e2e/dev.spec.ts \
  --workers=1 --reporter=line --output=test-results/dev-e2e
