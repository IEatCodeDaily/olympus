# Postmortem 0003: Playwright reused another worktree's Vite server

## Summary

The Hall-auth E2E run targeted the fixed port `5188` with Playwright's
`reuseExistingServer` enabled. A Vite server from another Olympus worktree was
already listening there, so Playwright tested that checkout instead of the
Hall-auth worktree. This produced misleading passes for Fleet and Projects and
a failure whose rendered UI did not match the source under test.

## Impact

The first `make verify-ui` result was not valid evidence for the current
worktree. In particular, tests for resource surfaces without organization
ownership exercised another checkout's globally scoped UI.

## Root cause

`ui/playwright.config.ts` used a fixed origin and allowed any healthy server on
that origin to satisfy the `webServer` prerequisite. A listening Vite process
carries no worktree identity, so port availability was incorrectly treated as
proof that the right application was running.

## Resolution

- Added `OLYMPUS_E2E_PORT` so isolated worktrees can select an unused port.
- Replaced Fleet, Projects, and Vaults E2E expectations with explicit
  fail-closed assertions while those resource classes lack durable
  organization ownership.
- Final verification uses an isolated port and therefore starts this
  worktree's own Vite server.

## Prevention

Agents working in parallel worktrees must use distinct E2E ports. A passing
Playwright run is only valid when the web server belongs to the worktree being
verified; reusing a fixed port across worktrees is not acceptable evidence.
