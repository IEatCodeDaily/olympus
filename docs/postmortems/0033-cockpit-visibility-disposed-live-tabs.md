# Postmortem 0033 — Cockpit visibility disposed live tab runtimes

- Date: 2026-07-16
- Status: Open; implementation fix is part of ADR 0023 migration
- Affected surface: Operator Cockpit

## Summary

The cockpit store and ADR promise that hiding the floating cockpit preserves terminal/browser/editor runtimes. The current React component violates that contract: `ui/src/cockpit/Cockpit.tsx` returns `null` when `open` is false. React therefore unmounts every tab renderer. The terminal renderer cleanup in `ui/src/cockpit/tabs.tsx` closes its WebSocket and disposes xterm.

The persisted tab manifest survives, but the live process attachment does not. This made the visibility toggle a hidden lifecycle command.

## User impact

- Hiding the cockpit disconnects every mounted terminal tab.
- Reopening shows tabs from the manifest, but each terminal renderer creates a new WebSocket/PTY attachment rather than continuing the prior mounted client runtime.
- Browser and future editor component-local state is also discarded unless explicitly persisted.
- The UI claim “Hide cockpit (tabs stay open)” is false at runtime.

## Evidence and failure path

```text
TopBar cockpit toggle
  → useCockpit.open = false
  → Cockpit(): `if (!open) return null`
  → React unmounts every Cockpit tab pane
  → TerminalPane effect cleanup
      → ws.close()
      → term.dispose()
  → reopening remounts a new TerminalPane
```

Source evidence:

- `ui/src/cockpit/Cockpit.tsx`: the component returns `null` when hidden.
- `ui/src/cockpit/tabs.tsx`: `TerminalPane` cleanup closes the socket and disposes xterm.
- `ui/src/cockpit/store.ts`: comments and public state contract explicitly say hiding never disposes tabs or sockets.
- ADR 0021 §2/§3 requires hide/show to be presentation-only.

## Root cause

Visibility and lifecycle were represented by the same React condition. The implementation treated “not visible” as “does not exist,” while the product contract treats a cockpit tab as a long-lived runtime whose presentation can be hidden independently.

The flat cockpit store persists only a serializable tab manifest. It does not own React renderer instances, so it cannot preserve runtime state after the component tree is removed.

## Corrective action

1. Keep the Cockpit and all tab renderer components mounted while hidden.
2. Hide the floating shell with `visibility`, `pointer-events`, and accessibility state rather than conditional rendering.
3. Keep inactive tab panels mounted and toggle their presentation only.
4. Under ADR 0023, separate the serializable workbench manifest from runtime instances keyed by tab ID.
5. Veto cross-group movement for live terminal tabs until renderer hosting is lifted above split-tree ancestry; otherwise moving a tab could reproduce the same unmount/dispose failure.

## Verification required before closure

- Mount a terminal renderer with a stable instance marker.
- Toggle cockpit hidden, then visible.
- Assert the same DOM/runtime instance remains mounted and the WebSocket was not closed.
- Switch to another cockpit tab and back; assert the same condition.
- Verify `aria-hidden` and pointer behavior while the shell is invisible.
- Capture a browser run showing hide/reopen while terminal output continues.

## Prevention

For every UI object backed by a live process, socket, unsaved draft, or non-serializable handle, review these separately:

- presentation visibility;
- route/view visibility;
- tab activation;
- renderer mount lifetime;
- backend resource lifetime.

No presentation toggle may implicitly cross a lifecycle boundary unless its label and confirmation explicitly say so.
