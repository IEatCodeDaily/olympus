# 0037 — Cockpit titlebar menu hit-testing

## Summary

The Cockpit titlebar treated the whole bar as a drag handle. Interactive children worked only if each one remembered to stop pointer propagation, and the tab strip clipped the new-tab menu when regular tabs were present.

## Impact

Operators could open a new tab from the empty Cockpit state, but the `+` control in the populated titlebar did not reliably expose its menu. Nested node pickers also required a click instead of standard hover navigation.

## Root cause

Drag behavior lived on `.cockpit-titlebar` and depended on per-control `onPointerDown(stopPropagation)` patches. The tab strip also used overflow clipping, so menus rendered inside it could be hidden. The node picker was modeled as a click-only second screen instead of a nested menu.

## Fix

- Titlebar drag now ignores interactive descendants in one shared guard.
- The populated tab strip no longer clips the new-tab popup.
- Node pickers render as hover-open submenus with a small close delay and a CSS hover bridge.
- Keyboard access stays intact: Enter/ArrowRight opens node submenus and Escape closes the menu.

## Regression coverage

`ui/src/cockpit/Cockpit.test.tsx` covers opening the titlebar `+` menu while tabs exist and opening the node submenu on hover. The existing hide-without-unmount test remains in place.
