#!/usr/bin/env bash
set -euo pipefail

TIER="${1:-mock}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
UI_DIR="$ROOT/ui"
MAESTRO="${MAESTRO_BIN:-${MAESTRO_DIR:-$HOME/.maestro}/bin/maestro}"
SERVER_PID=""
LOCAL_TIER=0
SCREEN_SIZE="1280x800"

export MAESTRO_CLI_NO_ANALYTICS=1
export MAESTRO_CLI_ANALYSIS_NOTIFICATION_DISABLED=true

cleanup() {
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill -TERM -- "-$SERVER_PID" 2>/dev/null || kill -TERM "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

[[ -x "$MAESTRO" ]] || {
  echo "ERROR: Maestro CLI not found at $MAESTRO. Run ui/scripts/install-maestro.sh." >&2
  exit 1
}
command -v java >/dev/null || { echo "ERROR: Java 17+ is required" >&2; exit 1; }

case "$TIER" in
  mock|mobile)
    LOCAL_TIER=1
    if [[ "$TIER" == "mobile" ]]; then
      CONFIG="$ROOT/.maestro/config.mobile.yaml"
      FLOW_DIR="$ROOT/.maestro/flows/mobile"
      OUTPUT_DIR="$UI_DIR/test-results/maestro-mobile"
      SCREEN_SIZE="412x915"
    else
      CONFIG="$ROOT/.maestro/config.yaml"
      FLOW_DIR="$ROOT/.maestro/flows/mock"
      OUTPUT_DIR="$UI_DIR/test-results/maestro"
    fi
    rm -rf "$OUTPUT_DIR"
    mkdir -p "$OUTPUT_DIR"
    if [[ -z "${MAESTRO_BASE_URL:-}" ]]; then
      checksum="$(printf '%s' "$ROOT" | cksum | cut -d' ' -f1)"
      PORT="${OLYMPUS_E2E_PORT:-$((5200 + checksum % 1000))}"
      if (echo >/dev/tcp/127.0.0.1/"$PORT") >/dev/null 2>&1; then
        echo "ERROR: E2E port $PORT is already in use; refusing to test another checkout's server." >&2
        exit 1
      fi
      MAESTRO_BASE_URL="http://127.0.0.1:$PORT"
      (
        cd "$UI_DIR"
        exec setsid env NODE_ENV=development VITE_USE_MOCKS=true \
          VITE_API_BASE=http://127.0.0.1:8787 \
          ./node_modules/.bin/vite --host 127.0.0.1 --port "$PORT" --strictPort
      ) >"$OUTPUT_DIR/vite.log" 2>&1 &
      SERVER_PID=$!
      for _ in $(seq 1 60); do
        if curl -fsS "$MAESTRO_BASE_URL/" >/dev/null 2>&1; then break; fi
        if ! kill -0 "$SERVER_PID" 2>/dev/null; then
          echo "ERROR: Vite exited before becoming ready" >&2
          sed -n '1,120p' "$OUTPUT_DIR/vite.log" >&2
          exit 1
        fi
        sleep 0.25
      done
      curl -fsS "$MAESTRO_BASE_URL/" >/dev/null || { echo "ERROR: Vite did not become ready" >&2; exit 1; }
    fi
    ;;
  live)
    CONFIG="$ROOT/.maestro/config.live.yaml"
    FLOW_DIR="$ROOT/.maestro/flows/live"
    OUTPUT_DIR="$UI_DIR/test-results/maestro-live"
    MAESTRO_BASE_URL="${MAESTRO_BASE_URL:-http://127.0.0.1:5177}"
    ;;
  prod)
    CONFIG="$ROOT/.maestro/config.prod.yaml"
    FLOW_DIR="$ROOT/.maestro/flows/prod"
    OUTPUT_DIR="$UI_DIR/test-results/maestro-prod"
    MAESTRO_BASE_URL="${MAESTRO_BASE_URL:-${OLYMPUS_PROD_BASE:-http://127.0.0.1:8799}}"
    "$UI_DIR/scripts/prod-http-smoke.sh" "$MAESTRO_BASE_URL"
    ;;
  *)
    echo "Usage: $0 [mock|mobile|live|prod] [flow-file-or-directory]" >&2
    exit 2
    ;;
esac

FLOW_TARGET="${2:-$FLOW_DIR}"

if [[ "$LOCAL_TIER" -eq 0 ]]; then
  curl -fsS "$MAESTRO_BASE_URL/" >/dev/null || {
    echo "ERROR: $TIER target is unavailable at $MAESTRO_BASE_URL" >&2
    exit 1
  }
fi

if [[ "$LOCAL_TIER" -eq 0 ]]; then
  rm -rf "$OUTPUT_DIR"
  mkdir -p "$OUTPUT_DIR"
fi

timeout --signal=TERM --kill-after=10s "${MAESTRO_TIMEOUT:-10m}" "$MAESTRO" test \
  --headless \
  --screen-size "$SCREEN_SIZE" \
  --no-ansi \
  --config "$CONFIG" \
  --test-output-dir "$OUTPUT_DIR" \
  --debug-output "$OUTPUT_DIR" \
  --flatten-debug-output \
  --format JUNIT \
  --output "$OUTPUT_DIR/report.xml" \
  -e "MAESTRO_BASE_URL=$MAESTRO_BASE_URL" \
  "$FLOW_TARGET"
