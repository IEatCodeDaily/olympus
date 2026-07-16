# Postmortem 0020: Vault sidebar file menu inherited bottom stretch

## Summary

The Vault sidebar file action menu reused the global `.menu` class, whose default dropdown geometry sets `bottom: 32px`. The sidebar context menu then added `position: fixed` and `top: <click-y>` without clearing `bottom`, so the browser stretched the menu from the click point toward the bottom of the viewport.

## Impact

Clicking the per-file ellipsis could render a tall menu that ran to the bottom of the screen instead of a compact action list.

## Root cause

The context-menu variant did not reset inherited positioning constraints from the shared dropdown primitive. There was no geometry assertion covering the fixed-position menu.

## Resolution

- Reset the context menu's inherited `right` and `bottom` constraints.
- Bound the menu height to available viewport space and enable vertical scrolling.
- Add a component geometry regression test that opens the file action menu near the viewport bottom and asserts `top`, `bottom: auto`, bounded `max-height`, and `overflow-y: auto`.

## Verification

Verified on fxcompute-01 with focused component tests, full UI Vitest, TypeScript typecheck, and production build from branch `wt/t_491de79c`.
