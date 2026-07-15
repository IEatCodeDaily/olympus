#!/usr/bin/env bash
set -euo pipefail

repo=$(git rev-parse --show-toplevel)
cd "$repo"
exec 9>/tmp/olympus-production-promotion.lock
flock -n 9 || { echo "another Olympus promotion is running" >&2; exit 1; }

fail() { echo "promotion refused: $*" >&2; exit 1; }
[[ $(hostname) == fxcompute-01 ]] || fail "run this on fxcompute-01"
[[ $(git branch --show-current) == main ]] || fail "current branch is not main"
[[ -z $(git status --porcelain) ]] || fail "main worktree is dirty"
git fetch --quiet origin main
head=$(git rev-parse HEAD)
origin_main=$(git rev-parse origin/main)
[[ $head == "$origin_main" ]] || fail "HEAD $head does not equal origin/main $origin_main"

export PATH="/home/rpw/.local/bin:/home/rpw/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
export CARGO_HOME=/var/lib/olympus/cargo-home
export CARGO_TARGET_DIR=/var/lib/olympus/cargo-target-prod
export RUSTUP_HOME=/home/rpw/.rustup
export SCCACHE_DIR=/var/lib/olympus/sccache
export SCCACHE_CACHE_SIZE=10G

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
(
  cd ui
  bun install --frozen-lockfile
  bun test --run
  bun run build
)
cargo build --release -p olympus-control-plane -p olympus-envoy

stage="$HOME/.cache/olympus-releases/$head"
rm -rf "$stage"
install -d -m 0755 "$stage/bin" "$stage/ui"
install -m 0755 "$CARGO_TARGET_DIR/release/olympus-hall" "$stage/bin/olympus-hall"
install -m 0755 "$CARGO_TARGET_DIR/release/olympus-envoy" "$stage/bin/olympus-envoy"
cp -a ui/dist/. "$stage/ui/"
install -m 0755 scripts/deploy-production-on-terminus.sh "$stage/deploy-production-on-terminus.sh"
(
  cd "$stage"
  find bin ui -type f -print0 | sort -z | xargs -0 sha256sum > manifest.sha256
)
python3 - "$stage" "$head" <<'PY'
import json, os, platform, subprocess, sys, time
from pathlib import Path
stage=Path(sys.argv[1]); sha=sys.argv[2]
manifest={
  "schema":1,
  "git_sha":sha,
  "built_at_unix":int(time.time()),
  "builder":platform.node(),
  "rustc":subprocess.check_output(["rustc","-V"],text=True).strip(),
  "cargo":subprocess.check_output(["cargo","-V"],text=True).strip(),
  "bun":subprocess.check_output(["bun","--version"],text=True).strip(),
}
(stage/"manifest.json").write_text(json.dumps(manifest,indent=2,sort_keys=True)+"\n")
PY

remote_incoming="/home/rpw/.olympus/releases/.incoming-$head"
ssh terminus "rm -rf '$remote_incoming' && install -d -m 0755 '$remote_incoming'"
rsync -a --delete "$stage/" "terminus:$remote_incoming/"
ssh terminus "'$remote_incoming/deploy-production-on-terminus.sh' '$head'"
ssh terminus /home/rpw/.local/bin/olympus-job run --timeout 60 -- /usr/bin/true
public_status=$(curl -sS -o /dev/null -w '%{http_code}' https://olympus.entelechia.cloud/)
[[ $public_status == 200 || $public_status == 302 || $public_status == 403 ]] \
  || { echo "production public route returned HTTP $public_status" >&2; exit 1; }
printf 'promoted Olympus %s to Terminus (public route HTTP %s)\n' "$head" "$public_status"
