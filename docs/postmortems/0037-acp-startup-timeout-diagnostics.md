# 0037 — ACP lazy startup timeout hid adapter state

**Date:** 2026-07-16  
**Status:** Fixed on `wt/t_c7e6b3d4`

## Impact

Lazy ACP startup could fail after one 30-second attempt with a generic timeout. The final Hall error did not say whether the adapter exited, remained silent, or was still warming a package-manager cache, and there was no second attempt.

## Fresh DEV evidence

Reproduced against `olympus-dev-hall` and `olympus-dev-envoy` on `fxcompute-01` with `OLYMPUS_HOME=/home/rpw/.olympus-dev`.

The Envoy service PATH was `/home/rpw/.local/bin:/home/rpw/.cargo/bin:/usr/local/bin:/usr/bin:/bin`. `hermes`, the pinned Claude adapter, `claude`, and `codex` were absent. The registry nevertheless advertised the implicit `default` Hermes profile.

Sessions created through the DEV API:

- Hermes/default: `20260716T023157Z-421366cb`
- Claude Code: `20260716T023157Z-8161b3c6`
- Codex: `20260716T023158Z-4505d74d`

Hall journal evidence at 02:32 UTC:

- Hermes failed to spawn `hermes acp` with `ENOENT`; no child existed from which stderr could be read.
- Claude failed to spawn `/home/rpw/.olympus-dev/adapters/claude-agent-acp/node_modules/.bin/claude-agent-acp` with `ENOENT`; DEV provisioning had not installed the locked adapter.
- Codex started through `bunx`, spent about 13 seconds resolving/downloading 18 dependencies, then returned `Authentication required`. Its captured stderr contained `Resolving dependencies`, download/extract progress, and `Saved lockfile`.

This was not a recurrence of the framing bug in postmortem 0024 or the missing production Hermes binary in 0031. The current DEV image was incompletely provisioned, discovery described commands different from runtime, and the timeout path lacked retry/state classification.

## Root cause

1. `HermesAgentRuntime::start` made one timed attempt and discarded the child before reporting actionable process state.
2. Discovery advertised Hermes profiles without checking for a runnable `hermes` executable.
3. Claude discovery checked for bare `claude`, while runtime launches the separately provisioned pinned `claude-agent-acp` executable.
4. Cold `bunx` startup is observable on this host and can consume a material part of the 30-second budget.

## Corrective actions

- Retry a timed-out lazy startup once with a fresh child.
- Preserve and report each attempt's bounded stderr tail.
- Classify timeout state as early exit, alive-but-silent, alive-without-ACP-response, or npm/bun cold-cache activity.
- Replace `(no stderr captured)` with an explicit empty-stderr statement; spawn failures already retain their `ENOENT` context.
- Discover Hermes only when `hermes` is executable.
- Discover Claude from the exact pinned adapter path used by runtime.

## Verification

`cargo test -p olympus-envoy` passes 76 of 77 tests. The unrelated pre-existing `job_table::tests::runs_argv_and_streams_output` test remains red on the DEV host because it receives an empty output string. The new retry, stderr, process-state, process-tree cleanup, and discovery tests pass individually.
