# 0037 — Cockpit Terminal Session Loss on WebSocket Disconnect

**Date:** 2026-07-16
**Severity:** Medium (data loss — operator shell state lost on transient disconnect)
**Component:** `crates/envoy/src/pty.rs`, `crates/control-plane/src/server/terminal_ws.rs`, `ui/src/cockpit/tabs.tsx`
**Discovered by:** Task t_fb28bf17 (persistent tmux-backed terminals)

## Summary

Operator terminals in the Olympus Cockpit were backed by bare `forkpty` shells
with no persistence layer. When the operator's WebSocket dropped — network
blip, page reload, cockpit hide/show in some edge cases — the relay's cleanup
path unconditionally called `PtyManager::close()`, which killed the shell
process group. The client-side `TerminalPane` printed `[disconnected]` and made
no attempt to reconnect. The operator lost all shell state (running commands,
scrollback, environment variables, background processes) on every transient
disconnect.

## Root Cause

Three layers conspired:

1. **pty.rs**: `PtyManager::open()` spawned `$SHELL` directly via `forkpty` —
   no process-group or session manager stood between the PTY and the WebSocket.
   When the relay closed the PTY, the shell died.

2. **terminal_ws.rs**: The `relay()` function's post-loop cleanup called
   `close_terminal()` unconditionally — whether the socket closed from a
   network drop (code 1006) or an explicit tab close (user action). There was
   no distinction between "I lost my connection" and "I'm done with this
   terminal."

3. **tabs.tsx**: `ws.onclose` wrote `[disconnected]` to the terminal and did
   nothing else. No reconnect logic, no backoff, no status indicator.

## Impact

- Any WebSocket disruption (common behind Cloudflare, mobile networks, or
  laptop sleep) killed the operator's shell mid-command.
- Page reload lost all open terminals.
- The cockpit window resize did not propagate to the PTY (fit addon only
  listened to `window.resize`, not container resize from cockpit drag).

## Fix

Four changes, one per requirement:

1. **tmux-backed sessions** (`pty.rs`): `PtyManager::open()` now spawns shells
   inside `tmux new-session -s olympus-term-<id>` (or `attach-session` if the
   session exists). A new `detach()` method drops the PTY client but leaves the
   tmux session alive. Falls back to bare `forkpty` if tmux is absent, reporting
   `persistent=false` so the UI shows a "non-persistent" badge.

2. **WS close distinguishes detach from kill** (`terminal_ws.rs`): Normal close
   (1000/1001/1006) calls `detach_terminal()` — the tmux session survives.
   Explicit close (custom code 4000, sent by the client on tab close) calls
   `close_terminal()` — kills the session permanently. The server sends an
   `attached` frame with `persistent: bool` after attach.

3. **Auto-reconnect with backoff** (`tabs.tsx`): On abnormal socket close (not
   4000, not after `exited`), the client schedules a reconnect with exponential
   backoff + jitter (1s → 2s → 4s → … → 10s cap). A slim "Reconnecting…"
   status line shows the state. A stable `terminalId` is persisted in
   `tab.state.terminalId` so a page reload reattaches to the same tmux session.

4. **ResizeObserver** (`tabs.tsx`): A `ResizeObserver` on the terminal
   container catches cockpit drag-resize that `window.resize` misses, calling
   `fit.fit()` and propagating the new cols/rows to the server.

## Lessons

- A terminal relay must distinguish connection lifecycle from session
  lifecycle. The WebSocket is ephemeral; the shell does not have to be.
- `window.resize` is insufficient for panels inside resizable containers —
  `ResizeObserver` is the correct primitive.
- The cleanup function of a React component runs on unmount, not on
  visibility change — but WS cleanup must also signal intent (close 4000)
  so the server knows whether to persist.
