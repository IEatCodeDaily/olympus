# FxBuilder JobRunner operations

`fxcompute-01` is Olympus's dedicated remote build/test executor. It is an
`olympus-envoy` node advertising only `job_runner`; it does not host agent
runtimes.

## Architecture

```text
Terminus
  olympus-job (REST client + source sync)
       |
       | POST/GET /api/jobs on loopback
       v
Olympus Hall on Terminus
       |
       | iroh QUIC; allowlisted node identity
       v
olympus-envoy on fxcompute-01
  role: job_runner
  root: /srv/olympus/jobs
  shared caches: /var/lib/olympus/{cargo-home,cargo-target}
```

This host is intentionally **outside K3s**. K3s remains the target for
containerized ephemeral compute; FxBuilder is a stable host-level executor for
Bun, Rust, and similar repository jobs. Do not install K3s or a cluster agent on
`fxcompute-01` as part of this service.

The Phase 1 runner executes trusted repository code directly as user `rpw`.
Path confinement keeps `cwd` below `/srv/olympus/jobs`, but this is not an
untrusted-code sandbox. Bubblewrap/microVM isolation remains later work under
ADR 0011.

## Installed components

### fxcompute-01

| Component | Path |
|---|---|
| Envoy binary | `~/.olympus/bin/olympus-envoy` |
| User service | `~/.config/systemd/user/olympus-fxbuilder.service` |
| Envoy state/spool | `~/.olympus/envoy/fxbuilder/` |
| Job root | `/srv/olympus/jobs` |
| Synced workspaces | `/srv/olympus/jobs/workspaces/` |
| Shared Cargo home | `/var/lib/olympus/cargo-home` |
| Shared Cargo target | `/var/lib/olympus/cargo-target` |

The tracked service definition is
`systemd/olympus-fxbuilder.service`. Its role is explicitly `job_runner`; do not
replace it with the agent-runtime `olympus-envoy@.service` template.

### Terminus

| Component | Path |
|---|---|
| CLI source | `scripts/olympus_job.py` |
| Installed CLI | `~/.local/bin/olympus-job` |
| Hall API | `http://127.0.0.1:8799` |
| Hall token | `~/.olympus/token` |
| SSH alias | `fxbuilder` |

Install/update the CLI:

```bash
install -m 0755 scripts/olympus_job.py ~/.local/bin/olympus-job
```

## Submit jobs from Terminus

The CLI preserves argv as an array; it does not execute through a shell.

Run in an existing remote workspace:

```bash
olympus-job run \
  --cwd workspaces/olympus/ui \
  --timeout 600 \
  -- bun run test
```

Synchronize a local checkout and run:

```bash
olympus-job sync-run \
  --source ~/olympus \
  --workspace olympus \
  --timeout 900 \
  -- cargo test --workspace
```

`sync-run` excludes `.git`, `target`, `node_modules`, build output, coverage,
and caches. It uses `--delete-excluded`; use separate workspace names when a
later command must retain generated dependency state.

The command waits on Hall's current job record, prints the bounded combined
stdout/stderr result, and exits with the remote process exit code. An abrupt
loss of the submitting SSH/client process does not terminate the remote child;
retrieve its record with authenticated `GET /api/jobs/{job_id}`. `Ctrl-C`
explicitly requests cancellation. Phase 1 job records are in-memory and do not
survive a Hall restart.

## Service operations

```bash
# fxcompute-01
systemctl --user status olympus-fxbuilder.service
journalctl --user -u olympus-fxbuilder.service -f
systemctl --user restart olympus-fxbuilder.service

# Terminus
systemctl --user status olympus-hall.service
journalctl --user -u olympus-hall.service -f
```

Verify actual dispatch rather than relying only on node visibility:

```bash
olympus-job run --timeout 30 -- /bin/true
```

Expected: Hall returns `202`, the job reaches `completed`, and the CLI exits
zero.

## Deployment and rollback

Build release artifacts on FxBuilder using the shared target directory, verify
the SHA-256 after copying, preserve the previous binary, then restart only the
changed service. Hall and Envoy roll independently.

```bash
# Envoy rollback on fxcompute-01
cp ~/.olympus/bin/olympus-envoy.rollback-fxbuilder \
   ~/.olympus/bin/olympus-envoy
systemctl --user restart olympus-fxbuilder.service

# Hall rollback on Terminus
cp ~/.olympus/bin/olympus-hall.rollback-fxbuilder \
   ~/.olympus/bin/olympus-hall
systemctl --user restart olympus-hall.service
```

After any transport deployment, test a quick Envoy restart, wait at least 40
seconds, and submit `/bin/true`. This catches stale-connection cleanup races.
For a high-output job, run the full Cargo workspace suite to exercise spool
append/ACK compaction and terminal-result delivery.

## Operational pitfalls

- Non-login SSH commands on both hosts omit user tool directories. Use an
  explicit absolute binary or export the full service `PATH` for the entire
  command chain.
- Hall removes a node's role registration when its active Envoy connection
  closes. Connection removal is generation-safe: an old delayed connection
  cannot remove its replacement.
- Envoy spool append, replay reads, and cumulative ACK compaction must remain
  serialized. Concurrent file mutation can lose a frame and permanently stall
  Hall's strict sequence watermark.
- Keep corrupted spools for forensics under `/srv/olympus/jobs/quarantine/`;
  stop Envoy before moving any spool files.
- The `fxbuilder` SSH alias is an operations path for sync/deployment only.
  Job execution itself must travel through Hall and iroh.
