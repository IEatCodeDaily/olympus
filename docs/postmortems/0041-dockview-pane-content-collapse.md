# 0041 — Dockview pane content collapsed to top (sessions tiling unusable)

Date: 2026-07-16 · Severity: high (core surface visually broken) · Author: Terminus

## Symptom
Every session pane rendered its content squished to the top of the viewport:
transcript, composer, bottom panel compressed into ~700px while the pane had
1335px. Right-sidebar/bottom-panel resize appeared broken. Reported by the
operator from the live dev UI (screenshot evidence).

## Root cause
`dockview-react` renders each panel inside `.dv-content-container > .dv-react-part`,
both `display: block` with a fixed pixel height. The panel root `.chat-view`
uses `flex: 1; min-height: 0` — which is meaningless inside a block container:
the element collapses to content height. Nothing in the design system gave
panel roots `height: 100%` under the dockview wrappers.

## Fix (ui/src/index.css)
`.olympus-dockview .dv-content-container > *` and
`.olympus-dockview .dv-react-part > *` get `height: 100%; min-height: 0`.
Verified live via CDP geometry probe: chat-view 693px→1335px (= pane height),
composer/terminal at the pane bottom, dv-sash drag resizes groups (843→643).

## Why QA and review missed it
1. The unit suite (jsdom) cannot measure layout at all.
2. The live e2e gate (QA-GATE-1) asserted element PRESENCE and drag deltas,
   not that panel content FILLS the pane — a geometry-class assertion gap.
3. Code review was static: reviewers had no browser tooling requirement, and
   the tiling card shipped with \"screenshots deferred — no browser on fx\".
   The browser existed but system libs were missing; nobody was forced to look.

## Prevention (landed with this fix)
- e2e gate now asserts fill-geometry: chat-view height >= 95% of its dockview
  group height, and the composer is in the bottom quarter of the pane.
- Browser QA toolkit documented in docs/harness/browser-qa.md: headless_shell
  path, CDP screenshot/geometry probes, dev credentials location, dev-e2e.sh.
- Card/review policy: UI-touching branches REQUIRE live screenshots (both
  themes) + geometry probe output as review evidence. \"Build green\" is not
  visual evidence (GREEN GATES ARE NOT A WORKING UI).

## Residual
Layout persistence can restore an empty group (dv-watermark) if its panel
fails to rehydrate — visible as a blank pane. Tracked in TILING-UX-1
(t_4b4687c9): prune empty groups on restore.
