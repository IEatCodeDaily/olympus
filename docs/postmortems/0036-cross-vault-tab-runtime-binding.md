# Postmortem 0036 — Route vault changes could render old tabs against the new vault

- Date: 2026-07-16
- Status: Fixed
- Affected surface: Vault workbench

## Summary

The first split-tree migration reset workbench state only from the Vault sidebar's explicit selection callback. Browser Back/Forward or any direct route transition could change `activeVaultId` without clearing the old manifest. `VaultWorkspace` then supplied the new outer vault ID to every retained tab renderer, including tabs whose resource key belonged to the prior vault.

A matching path in both vaults could therefore show or save the wrong document. A missing path could replace an outgoing unsaved editor with a missing-resource state.

## Root cause

The route and workbench had separate vault identity variables, but rendering assumed they were always synchronized. The transition guard lived in one click handler rather than at the route/workbench boundary.

## Correction

- Track the vault ID that owns the current in-memory workbench manifest.
- Render the workbench only when that ID equals the route's active vault ID.
- Handle every active-vault transition centrally, including Back/Forward and direct navigation.
- If outgoing resources are dirty, fail closed and return to the prior vault unless the operator explicitly confirms discard.
- After confirmation, atomically reset the manifest and dirty registry before opening the new route target.
- Dirty resource identity is now lifted to `VaultWorkspaceView`, allowing vault switch, rename, and delete to guard before domain mutation.

## Verification

- Vault resource keys remain scoped as `vault:<vaultId>:note:<path>`.
- Component tests verify dirty resources are reported from editor runtimes and group close fails closed.
- Full typecheck, Vitest, production build, and real-browser nested-pane evidence are required by ADR 0023.

## Follow-up

Add a route-level test with two vaults containing the same path and a dirty outgoing draft. The long-term model may retain one manifest per vault, but unsaved draft runtime ownership must be lifted before switching can preserve drafts without confirmation.
