# 0042 — Tiling e2e failed after proving two visible group headers

Date: 2026-07-17 · Severity: medium (merge gate false negative) · Author: Terminus

## Symptom

The live dev tiling gate created two panes successfully, rendered both session
tab bars, and then failed on `toBeVisible()` with a Playwright strict-mode
violation. The locator matched the two expected tab bars.

## Root cause

The assertion treated a plural invariant as a singular element:
`.sessions-dockview.multi-group .dv-tabs-and-actions-container`. After the
Open Right action, one header exists per dockview group. Playwright correctly
refused to pick one of the two matches implicitly.

This was a test bug, not a rendering regression. The failure snapshot showed
both tab lists and both full-height session panes.

## Fix

The gate now asserts exactly two group tab bars, then asserts each one is
visible. This preserves strict locators and checks the intended product
invariant rather than weakening the assertion with an arbitrary `.first()`.

## Prevention

- Assertions for per-pane UI must state the expected pane count first.
- Never use a singular visibility assertion against a locator intentionally
  matching every dockview group.
- Keep Playwright strict mode enabled; it exposed the ambiguous assertion.
