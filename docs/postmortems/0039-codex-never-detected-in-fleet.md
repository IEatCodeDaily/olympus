# 0039 — Codex agent never detected in the fleet

**Status:** fixed
**Area:** envoy discovery, control-plane refresh route, Fleet UI
**Severity:** medium — codex was drivable but invisible; "Detect agents" a no-op on real nodes

## Symptom

"Detect agents" on a node never surfaced a `codex` agent, even on hosts where
codex sessions could be started successfully if you already knew the id. The
agents list stayed empty or claude-only. On the dev node (`fxcompute-01`),
clicking Detect appeared to do nothing at all.

## Root cause (two independent bugs, same feature)

### 1. Detection checked the wrong artifact

The runtime spawns codex via the locked bunx adapter:

```
command_for_agent(Some("codex")) → ["bunx", "@zed-industries/codex-acp@0.16.0"]
```

But discovery gated codex on a **bare `codex` binary** on PATH:

```rust
if let Some(codex) = which_in_path("codex", path_env) { … }
```

No such binary exists — codex is an npm adapter resolved by bunx at spawn time,
not a standalone CLI. So detection and the spawn path disagreed: a codex that
would *run* fine never *showed up*. (Compare claude-code, whose discovery
correctly checks the adapter binary it spawns — this was the codex-side
oversight of that same design.)

This is the same class of PATH-gating mismatch as postmortem 0031 (hermes
discovery), applied to the wrong probe target.

### 2. Remote refresh was a blanket 501

`POST /api/nodes/:id/agents/refresh` only handled `id == "local"`; everything
else returned 501 Not Implemented with "only 'local' is supported in-process".
But the local dev node registers under its **hostname** (`fxcompute-01`,
`local: false`), not the literal string `local`. So Detect on the only real
node in the dev fleet always 501'd — and the UI swallowed the error silently
(`catch {}` with a "returns 501" comment), so the operator saw nothing.

The `HallFrame::Probe` request frame — which makes an envoy re-run discovery and
report back over its live connection — already existed (envoy handles it,
`send_request` routes it), but the refresh route predated it and never used it.

## Fix

- **Detection tracks the runtime.** Derive the codex launcher from
  `command_for_agent(codex)` (the spawn table) and check *that* is resolvable on
  PATH, instead of hardcoding `which("codex")`. Change how codex is spawned and
  detection follows automatically. A stray `codex` binary no longer satisfies
  detection; the bunx launcher does.
- **Real refresh for connected nodes.** The route now: `local` → in-process
  discovery (unchanged); any node with a live envoy connection → send `Probe`
  (bounded 5s) and store what it reports; known-but-disconnected → honest 503;
  unknown → 404. The BAD_GATEWAY/timeout paths are surfaced.
- **UI surfaces the error.** `handleDetect` no longer swallows failures — a
  failed detect shows an inline alert with the server's message.

## Tests

- `discover_cli_harnesses_finds_runtime_adapter_and_codex` rewritten: stubs a
  `bunx` launcher (not a `codex` binary) and asserts codex is detected via it.
- `codex_not_detected_without_its_launcher` (new): a bare `codex` binary with no
  bunx must NOT satisfy detection — locks in the corrected gate.

## Provisioning note

The codex adapter is bunx-resolved at spawn (fetched over the network on first
use). Unlike the claude adapter, `install-envoy.sh` does not pre-provision it —
detection only requires the `bunx` launcher, which the installer already
warns-if-missing. Pre-fetching the codex-acp package at install time is a
possible future hardening (offline-first), tracked separately; not required for
detection to work.

## Prevention

Detection of any harness should be *derived from its spawn command*, never a
separately-guessed binary name. The two must not be allowed to drift.
