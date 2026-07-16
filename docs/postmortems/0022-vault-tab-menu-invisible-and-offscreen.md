# 0022 — Vault tab menu rendered invisibly and outside narrow viewports

## Status

Resolved before release.

## Impact

The first Vault tab-management implementation mounted the expected menu DOM, so component queries found all actions, but the menu was not visible in the browser. After making it visible, a context click near the right edge could position the fixed menu entirely outside a narrow viewport.

## Root cause

`vault-tab-menu` composed the shared `.menu` class without satisfying its visibility contract. `.menu` defaults to `display: none` and only `.menu.on` becomes visible. It also inherits absolute-menu positioning constraints, including `bottom`, which conflicted with the tab menu's fixed `top` coordinate.

The initial implementation also copied raw pointer coordinates into `left` and `top` without accounting for menu dimensions or viewport boundaries.

## Detection

Real Chromium visual review showed no menu despite the DOM assertion reporting all four menu actions. A 390×844 capture then showed that a menu opened at `clientX = 420` was fully outside the viewport.

## Resolution

- Added the shared `on` visibility class.
- Reset inherited `right` and `bottom` positioning constraints.
- Clamp menu coordinates to an 8 px viewport inset using the menu's bounded dimensions.
- Added a component regression test that asserts both visibility class and bounded `left`/`top` coordinates.
- Re-ran desktop and 390×844 Chromium captures; the menu rectangle is now `left=202`, `right=372`, inside a 390 px viewport.

## Prevention

A DOM-presence assertion is insufficient for composed popup primitives. Popup tests must assert the shared visibility state and viewport bounds, and every popup change must receive real-browser geometry plus visual review.
