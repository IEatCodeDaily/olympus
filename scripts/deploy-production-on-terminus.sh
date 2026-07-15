#!/usr/bin/env bash
set -euo pipefail

[[ $(hostname -s) == terminus* ]] || { echo "deploy must run on Terminus" >&2; exit 1; }
[[ $# -eq 1 ]] || { echo "usage: $0 <git-sha>" >&2; exit 2; }
sha=$1
[[ $sha =~ ^[0-9a-f]{40}$ ]] || { echo "invalid git SHA" >&2; exit 2; }

home=/home/rpw/.olympus
releases=$home/releases
incoming=$releases/.incoming-$sha
release=$releases/$sha
[[ -d $incoming ]] || { echo "incoming release missing" >&2; exit 1; }
envoy_units=()
while read -r unit _; do
  [[ -n $unit ]] && envoy_units+=("$unit")
done < <(systemctl --user list-units --type=service --state=active --plain --no-legend 'olympus-envoy@*.service')
((${#envoy_units[@]} > 0)) || { echo "no active production Envoy instances found" >&2; exit 1; }
(
  cd "$incoming"
  sha256sum -c manifest.sha256
)
[[ ! -e $release ]] || { echo "immutable release already exists: $release" >&2; exit 1; }
mv "$incoming" "$release"
chmod -R a-w "$release"

timestamp=$(date -u +%Y%m%dT%H%M%SZ)
install -d -m 0700 "$home/backups"
if [[ -L $releases/current ]]; then
  previous=$(readlink -f "$releases/current")
else
  previous=$releases/legacy-$timestamp
  install -d -m 0755 "$previous/bin" "$previous/ui"
  cp -L "$home/bin/olympus-hall" "$previous/bin/olympus-hall"
  cp -L "$home/bin/olympus-envoy" "$previous/bin/olympus-envoy"
  cp -a /home/rpw/olympus/ui/dist/. "$previous/ui/"
  ln -s "$previous" "$releases/.current.next"
  mv -Tf "$releases/.current.next" "$releases/current"
fi

backup=$home/backups/olympus-$timestamp.db
sqlite3 "$home/olympus.db" ".backup '$backup'"
chmod 600 "$backup"

activate() {
  local target=$1
  rm -f "$releases/.current.next" "$home/bin/.olympus-hall.next" "$home/bin/.olympus-envoy.next" /home/rpw/olympus/ui/.dist.next
  ln -s "$target" "$releases/.current.next"
  mv -Tf "$releases/.current.next" "$releases/current"
  ln -s "$releases/current/bin/olympus-hall" "$home/bin/.olympus-hall.next"
  mv -Tf "$home/bin/.olympus-hall.next" "$home/bin/olympus-hall"
  ln -s "$releases/current/bin/olympus-envoy" "$home/bin/.olympus-envoy.next"
  mv -Tf "$home/bin/.olympus-envoy.next" "$home/bin/olympus-envoy"
  if [[ -d /home/rpw/olympus/ui/dist && ! -L /home/rpw/olympus/ui/dist ]]; then
    mv /home/rpw/olympus/ui/dist "/home/rpw/olympus/ui/dist.pre-managed-$timestamp"
  fi
  ln -s "$releases/current/ui" /home/rpw/olympus/ui/.dist.next
  mv -Tf /home/rpw/olympus/ui/.dist.next /home/rpw/olympus/ui/dist
}

rollback() {
  local code=$?
  trap - ERR
  set +e
  echo "deployment health gate failed; rolling back" >&2
  systemctl --user stop "${envoy_units[@]}" olympus-hall.service || true
  activate "$previous"
  rm -f "$home/olympus.db-wal" "$home/olympus.db-shm"
  cp "$backup" "$home/olympus.db"
  chmod 600 "$home/olympus.db"
  systemctl --user start olympus-hall.service
  systemctl --user start "${envoy_units[@]}"
  exit "$code"
}
trap 'rollback' ERR
activate "$release"
systemctl --user restart olympus-hall.service
systemctl --user restart "${envoy_units[@]}"
healthy=0
for _ in $(seq 1 60); do
  if curl -fsS http://127.0.0.1:8799/api/health >/dev/null 2>&1 \
    && systemctl --user is-active --quiet olympus-hall.service "${envoy_units[@]}"; then
    healthy=1
    break
  fi
  sleep 1
done
[[ $healthy -eq 1 ]]
trap - ERR
printf 'activated %s; database backup %s
' "$release" "$backup"
