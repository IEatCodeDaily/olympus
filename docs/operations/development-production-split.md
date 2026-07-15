# Olympus development and production operations

## Authority and boundaries

- Source authority: `fxcompute-01:/home/rpw/olympus`
- Development URL: `https://olympus-dev.entelechia.cloud`
- Production URL: `https://olympus.entelechia.cloud`
- Production runtime/state: Terminus under `/home/rpw/.olympus`
- The Terminus repository is a retained runtime/migration snapshot. Do not edit or build there.
- `fxbuilder` is only an SSH compatibility alias. Active services and fleet records use `fxcompute-01`.

## Development services

```bash
cd /home/rpw/olympus
just dev-status
just dev-restart
just check-fast
```

| Service | Purpose |
|---|---|
| `olympus-dev-hall.service` | Cargo Watch-managed development Hall on `127.0.0.1:8799` |
| `olympus-dev-envoy.service` | Isolated AgentRuntime + JobRunner using `/srv/olympus-dev/jobs` |
| `olympus-dev-ui.service` | Vite/HMR on `127.0.0.1:5177` |
| `fxcompute-01-tunnel.service` | Restricted reverse forwards to Terminus ports 2223 and 8800 |
| `olympus-prod-job-runner.service` | Production Hall JobRunner using `/srv/olympus-prod/jobs` |

Development state is `/home/rpw/.olympus-dev`; its browser credential is in the mode-0600 file `/home/rpw/.config/olympus-dev/admin-credentials`. The development operator token is `/home/rpw/.olympus-dev/token`. Use `olympus-dev-job` for isolated dev jobs.

Cargo targets are separated:

- `/var/lib/olympus/cargo-target-dev`
- `/var/lib/olympus/cargo-target-prod`

Both are bounded by `olympus-build.slice` and use mold plus the local sccache directory `/var/lib/olympus/sccache`.

## Production promotion

```bash
cd /home/rpw/olympus
just promote
```

Promotion refuses unless the branch is `main`, the worktree is clean, and `HEAD` exactly equals freshly fetched `origin/main`. It runs Rust formatting, clippy, nextest, UI install/tests/build, and release builds on fxcompute-01. It transfers an immutable checksummed bundle to Terminus; Terminus only verifies, backs up SQLite, switches symlinks, restarts services, and runs health gates. Failed health rolls back the release and database backup.

Do not build with Cargo, rustc, Bun, npm, or Vite on Terminus.

## Recovery

```bash
systemctl --user restart olympus-dev-hall olympus-dev-envoy olympus-dev-ui
systemctl --user restart fxcompute-01-tunnel
```

If the reverse tunnel is down, inspect `fxcompute-01-tunnel.service` on fxcompute-01. Terminus sshd detects vanished clients in 30 seconds, allowing the reverse ports to rebind after a VM restart.
