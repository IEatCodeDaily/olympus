# Postmortem 0035 — Vault group close bypassed dirty-tab protection

- Date: 2026-07-16
- Status: Fixed
- Affected surface: Vault multi-pane workbench

## Summary

The first nested-group implementation protected individual tab close and bulk tab-menu actions, but the new **Close editor group** button called the parent group-removal callback directly. A group containing an unsaved note could therefore be removed without confirmation, unmounting the draft owner.

## Root cause

Dirty state is owned inside `VaultWorkspace`, while structural mutations are delegated to `VaultWorkspaceView`. The new group-close path crossed that boundary without first applying the same dirty-runtime veto used by tab close.

## Correction

`VaultWorkspace` now enumerates every view in the target group, checks the component-owned dirty-tab set, and fails closed unless the operator explicitly confirms discarding changes. Only then does it invoke structural group removal.

## Verification

`VaultWorkspace.test.tsx` marks a real rendered group tab dirty, rejects the confirmation, and asserts that `onCloseGroup` is not called.

## Prevention

Every structural command that can unmount a Vault runtime—tab close, bulk close, cross-group move, group close, vault switch, rename, delete—must pass through one explicit dirty-runtime policy before mutating the split tree.
