# Operator Cockpit Terminal — build progress (ADR 0021)

Status as of 2026-07-14. Feature: operator-only floating tabbed terminal,
Hall-default + hover node-picker, per-host PTY. Building directly (not swarm).

## DONE — backend data-plane (written, compiles past type/borrow; final
## link/test blocked by host swap-death from a sibling agent)

- proto/frames.rs:
  - NodeRole::TerminalHost
  - Hall→Envoy: TerminalOpen{reqId,terminalId,cols,rows,cwd},
    TerminalInput{terminalId,dataB64}, TerminalResize{terminalId,cols,rows},
    TerminalClose{terminalId}
  - Envoy→Hall: TerminalOutput{terminalId,dataB64}, TerminalExited{terminalId,exitCode}
  - req_id() updated (TerminalOpen expects resp; input/resize/close fire-and-forget)
- envoy/src/pty.rs (NEW): PtyManager over libc::forkpty. Real $SHELL as
  session/process-group leader; async output pump → TerminalOutput; input;
  resize (TIOCSWINSZ); close kills the process GROUP (SIGHUP+SIGKILL).
  Idempotent open. Self-contained base64 (encode/decode) with round-trip tests.
  ChannelSink + TerminalMsg forward to the connection layer.
- envoy/src/lib.rs: `pub mod pty;`
- envoy/src/main.rs: Conn.pty field; ChannelSink+PtyManager built in
  run_connection; pty_fwd_handle drains TerminalMsg → EnvoyFrame and aborts on
  disconnect; dispatch arms for TerminalOpen/Input/Resize/Close; every envoy
  advertises NodeRole::TerminalHost by default (configured_roles).
- control-plane/src/server/envoy_conn.rs: TerminalFrame enum;
  terminal_channels on EnvoyConnection; subscribe_terminal / forward_terminal /
  drop_terminal.
- control-plane/src/node.rs: handle_envoy_frame arms for TerminalOutput
  (forward_terminal) + TerminalExited (forward + drop_terminal). This resolves
  the exhaustive-match compile blocker.

## TODO — remaining

1. Build (running), deploy hall+envoy + UI, live-verify a real shell.

## VERIFIED ✅ (2026-07-14)

- `cargo test -p olympus-control-plane`: **299 passed, 0 failed** — full stack.
- UI typecheck: clean (cockpit + xterm).
- **DEPLOYED + LIVE-VERIFIED** on prod (hall+envoy `olympus-*-0a73a86-cockpit`):
  - `GET /api/terminal/targets` → Hall (default) + terminus (TerminalHost node).
  - Live operator WS to Hall-local shell: `echo` round-tripped, real zsh prompt
    (`rpw@terminus:~`). RESULT=PASS.
  - Live operator WS to terminus NODE shell (via EnvoyConnection PTY): real
    interactive zsh echoed back. RESULT=PASS.
  - Auth fix: extended the WS `?token=` bearer bypass in principal.rs to
    `/ws/operator/terminals/*` (was `/ws`-only); browsers use the cookie path.
    Required a hall-only rebuild+redeploy.
- WIP safety: dangling commit tagged `wip-snapshot-cockpit`; full archive
  `~/.olympus/backups/olympus-cockpit-wip-20260714.tar.gz` (121 files).
- Prod DB backed up pre-deploy: `olympus-predeploy-cockpit-20260714.db` (integrity ok).

## ⚠ COLLISION: feat/fxbuilder-jobrunner (origin, 1 commit fbed8b0, base 0a73a86)

A separate remote-job-runner feature (fxcompute-01, job_runner envoy over iroh,
`scripts/olympus_job.py` CLI) touches the SAME files as cockpit:
`envoy_conn.rs`, `node.rs`, `envoy/main.rs`, `spool.rs`, `routes/jobs.rs`.
Logically independent (jobs vs terminals) but textually adjacent. No PR open yet.
Merge order TBD (asked user; proceeding to protect cockpit tree first). When
integrating, resolve by hand: both add EnvoyConnection methods + envoy main
wiring + node.rs dispatch arms.

## TODO — remaining (was)

## DONE — frontend (xterm cockpit)

- ui/package.json: @xterm/xterm + @xterm/addon-fit
- ui/src/cockpit/store.ts: terminal-only tabs; addTab({node,label}); tab.target.nodeId
- ui/src/cockpit/Cockpit.tsx (real): floating draggable/resizable window; tab bar;
  NewTerminalButton (click=Hall, hover=node picker via GET /api/terminal/targets);
  TerminalTab = live xterm.js bound to /ws/operator/terminals/:id (base64 I/O,
  onData→input, onResize→resize, output→term.write); every tab stays mounted
  (display toggle) so shells survive tab switch + cockpit hide.
- ui/src/api.ts: fetchTerminalTargets() + terminalWsUrl()
- ui/src/AppShell.tsx: <Cockpit/> mounted at app root (outside body switch);
  CockpitToggle button in tb-right (terminal icon)
- ui/src/index.css: .cockpit* styles
- UI typecheck: PASSES clean (note: NODE_ENV=test in shell made `npm install`
  prune devDeps; fixed with `npm install --include=dev`).

## DONE — Hall backend (added after the envoy layer)

- server/terminal_ws.rs (NEW): operator WebSocket
  GET /ws/operator/terminals/:terminalId?node=<id> — dedicated channel (NOT the
  /ws firehose). Relays browser<->PTY: input/resize/close out, output/exited in.
  HallTerminals: Hall-local PtyManager (reuses olympus_envoy::pty) for the
  "hall" target, with per-terminal broadcast fan-out. Node target routes through
  the node's EnvoyConnection.
- server/ws.rs: authorize_operator() (installation token or valid session cookie)
- server/envoy_conn.rs: TerminalFrame + terminal_channels +
  subscribe_terminal/forward_terminal/drop_terminal on EnvoyConnection
- node.rs: EnvoyFrame::TerminalOutput/Exited forwarded to the terminal channel
- routes/nodes.rs: GET /api/terminal/targets (Hall first + online TerminalHost
  nodes) for the picker
- principal.rs: /ws/operator/terminals/* + /api/terminal/targets classified User
  (route does its own operator auth)
- server/mod.rs: AppState.hall_pty; route registered; module declared
- AppState constructed with hall_pty in main.rs + both test sites (tests.rs, ws.rs)

## Verify command (Rust)
flock ~/.cache/olympus-cargo.lock env CARGO_TARGET_DIR=$HOME/.cache/cargo-target/plain \
  CARGO_BUILD_JOBS=1 cargo test -p olympus-proto -p olympus-envoy -p olympus-control-plane -j 1

## Notes
- Known false positive: patch-tool reports E0670 "async fn not permitted in Rust
  2015" — workspace is edition 2021; trust cargo.
- Host blocker: sibling agent workload drove swap to 100%; envoy link stalled.
- Spine (ADR 0020 v2) is DONE + deployed separately (postmortems 0029/0030).
