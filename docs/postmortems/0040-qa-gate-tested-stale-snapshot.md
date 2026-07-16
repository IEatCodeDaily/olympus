# Postmortem 0040 — QA gate tested a stale snapshot

- Date: 2026-07-16
- Severity: medium (dev regressions escaped the nightly browser gate)
- Status: fixed

## Symptom

The nightly Playwright job stayed green while the development UI shipped a
stale-closure resize regression and incorrect sidebar selection highlights.

## Root cause

The job ran from `/home/rpw/olympus` on Terminus, a read-only migration
snapshot. Development, deployment, and the live dev services had moved to
`fxcompute-01:/home/rpw/olympus`, so the gate exercised code nobody was
changing. Its render smoke also did not chain drag gestures or inspect active
selection styles.

## Fix

`ui/scripts/dev-e2e.sh` now runs Playwright beside the authoritative checkout
against the live dev Vite and Hall services. It reads dev credentials only at
runtime and covers login, two chained bottom-panel drags, right-panel resize,
open/focused session washes without inset edge bars, active nav state, and the
theme toggle. The Terminus scheduler invokes it over SSH.

## Prevention

Browser gates must identify both the checkout and service endpoints they test.
Interaction state must be asserted after repeated gestures; a single drag
cannot catch a resize callback that closes over its initial value.
