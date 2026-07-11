# Postmortem 0001 — "Failed to start agent: starting agent runtime (lazy)"

- **Date:** 2026-07-08 / resolved 2026-07-09
- **Severity:** User-blocking (Olympus could not start any Hermes session)
- **Status:** Resolved — verified end-to-end on production service
- **Affected code:** `crates/control-plane/src/bridge/hermes.rs`,
  `crates/control-plane/src/server/mod.rs`, `~/.config/systemd/user/olympus.service`

## Summary

Starting any session in the Olympus UI failed with a generic error:

```
⚠ Failed to start agent: starting agent runtime (lazy)
Turn ended with error: error: failed to start agent: starting agent runtime (lazy)
```

The message was useless because the error rendering discarded the cause chain.
The real root cause was two compounding environment bugs:

1. The systemd service's `PATH` omitted `/home/rpw/.local/bin` (where `hermes`
   lives), so the `hermes acp` subprocess spawn failed with
   `No such file or directory (os error 2)`.
2. The `default` agent id (the base Hermes install) was mis-routed to
   `hermes -p default acp` instead of plain `hermes acp`. (`-p default` happened
   to work, but it's semantically wrong — `default` is not a profile.)

An earlier diagnosis blamed missing `[acp]` Python deps (`agent-client-protocol`).
That package was indeed missing and was installed, but it was **not** the cause
of this failure — the ACP adapter's `--check` passed once installed, yet the
error persisted. The real cause was hidden by the error-rendering bug below.

## Timeline

1. User opens Olympus, starts a new Hermes session.
2. `post_message` → `tokio::spawn` → `BridgeManager::ensure_runtime` →
   `HermesAgentRuntime::start` (`bridge/hermes.rs`).
3. `start()` builds `Command::new("hermes")` and spawns it. Under the systemd
   service, `hermes` is not on `PATH` → spawn returns
   `No such file or directory (os error 2)`.
4. `ensure_runtime` wraps that with `.context("starting agent runtime (lazy)")`.
5. The error handler in `server/mod.rs:post_message` renders the error with
   `format!("⚠ Failed to start agent: {e}")`. anyhow's `Display` (`{e}`) prints
   **only the outermost context** — the inner "No such file or directory" is
   discarded. The user sees only "starting agent runtime (lazy)".
6. Because `stderr` was `Stdio::inherit()` and the tracing layer defaulted to
   ERROR (RUST_LOG ignored — `init()` without env filter), the adapter's own
   diagnostics never surfaced either.

## Root cause

Three layered flaws, each of which alone would have made diagnosis hard:

1. **Service PATH** (`olympus.service`): `hermes` is installed at
   `/home/rpw/.local/bin/hermes`; the unit's `PATH` started at
   `/home/rpw/.cargo/bin`. Subprocess spawn failed.
2. **Agent-id routing** (`acp_command_for_agent`): the canonical base-install
   agent id `"default"` was treated as a profile name, producing
   `hermes -p default acp`. Harmless in effect (Hermes falls back), but wrong.
3. **Error-rendering** (`server/mod.rs`): `{e}` (Display) on an anyhow Error
   shows only the top-level context, hiding the cause chain. This is why the
   first diagnosis went to the wrong layer.

## Fix

### Immediate (environment)

- Added `/home/rpw/.local/bin` to the service unit `PATH` and ran
  `systemctl --user daemon-reload && systemctl --user restart olympus`.
- (Earlier, still valid:) installed the ACP extra into the Hermes venv:
  `uv pip install -e '.[acp]' --python venv/bin/python`.

### Durable (code)

1. **`server/mod.rs` — render the full error chain.** The `ensure_runtime` error
   handler now uses `format!("{e:#}")` (alternate Display), which prints the
   whole chain joined by `": "`. This alone would have surfaced the real cause
   ("starting agent runtime (lazy): spawning …: No such file or directory") in
   the UI from the first failure.
2. **`bridge/hermes.rs` — `acp_command_for_agent`: `default` is not a profile.**
   The base Hermes install (id `"default"`, and the empty/None case) now maps to
   plain `hermes acp`. Any other id names a profile and gets `-p <id>`. Test
   updated to cover the `default` case.
3. **`bridge/hermes.rs` — capture child stderr + detect early exit.** Both
   `start()` and `fork_session()` now pipe stderr into a bounded 8 KiB buffer
   (`spawn_stderr_capture`) instead of `Stdio::inherit()`, and the handshake
   wait loop checks `child.try_wait()` — if the child dies before `session/new`
   completes, the error includes the exit status and the stderr tail instead of
   running out a 30 s timeout. Added `tail_or_empty` helper + tests.

### Verification

End-to-end through the real systemd service (port 8799):

```
POST /api/sessions {"agent":"default"} → session created
POST /api/sessions/<id>/messages {"text":"say ok"} → 202
GET  /api/sessions/<id>/messages → user "say ok" / assistant "ok"
```

`cargo test --lib bridge::hermes` → 13 passed, 0 failed.

## How to reproduce the original failure

```bash
# 1. PATH bug (pre-fix unit):
sudo -u rpw env -i PATH=/home/rpw/.cargo/bin:/usr/bin hermes acp --check
# → command not found
# 2. Agent routing (pre-fix binary):
# agent "default" produced ["hermes","-p","default","acp"]
```

## Lessons

- **Render the full anyhow chain, not just Display.** `{e}` prints only the
  outermost context; `{e:#}` prints the whole chain. Any error that crosses a
  `.context()` boundary is invisible to the user (and to logs) under plain
  Display. This is now the rule for all user-facing error messages.
- **The systemd unit's PATH must include every directory the binary spawns
  from.** `hermes` lives in `~/.local/bin`, not `~/.cargo/bin`. When a service
  spawns subprocesses by bare name, the unit PATH is the only search path — the
  user shell's PATH is irrelevant.
- **Canonical ids are not profile names.** `"default"` identifies the base
  install; treating it as a profile name (`-p default`) is a latent bug that
  happened to be masked by Hermes' fallback behavior. Reserve `-p <id>` for ids
  that correspond to a real `~/.hermes/profiles/<id>/` directory.
- **Don't ship the first diagnosis.** The initial writeup blamed missing ACP
  deps. Installing them was necessary (the adapter wouldn't run without them)
  but not sufficient — the real cause was two layers deeper. Each layer of
  `.context()` that swallows its inner cause makes this trap worse.

## Required CLIs (envoy responsibility)

The Olympus envoy must ensure every CLI it routes to is installed and on the
service PATH. Current mandatory set:

- **`hermes`** — base agent runtime (`~/.local/bin/hermes`). Installed by the
  Hermes setup; must be on `olympus.service`'s PATH.
- **`jj`** — used by the edit model (`edit_model.rs`); the test
  `jj_conflict_detection_on_clean_dir` fails without it.
- **`gh`** — GitHub CLI, for repo/maintenance flows.
- **`bunx`** (Bun) — required for the Claude Code / Codex ACP adapters
  (`bunx @zed-industries/claude-code-acp@…`). Olympus no longer requires Node.js.

See "Envoy must install required CLIs" in AGENTS.md.

## Related

- Code: `crates/control-plane/src/bridge/hermes.rs` (`acp_command_for_agent`,
  `start`, `fork_session`, `spawn_stderr_capture`, `tail_or_empty`),
  `crates/control-plane/src/server/mod.rs:post_message` (error rendering).
- Service: `~/.config/systemd/user/olympus.service` (PATH).
- Caller: `crates/control-plane/src/server/bridge_mgr.rs:ensure_runtime`.
- AGENTS.md — postmortem rule + required-CLIs rule added alongside this doc.
