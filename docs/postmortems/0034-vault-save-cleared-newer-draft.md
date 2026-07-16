# Postmortem 0034 — Vault save completion cleared a newer unsaved draft

- Date: 2026-07-16
- Status: Fixed
- Affected surface: Vault note editor

## Summary

`NotePage.handleSave` submitted the current draft, awaited the API call and query invalidation, then unconditionally marked the editor clean. The editor remained writable while the request was in flight. If an operator typed after clicking Save but before completion, the later text stayed visible in component state but the dirty marker was cleared. Closing the tab could then discard that newer text without warning.

## Failure path

```text
operator clicks Save with draft A
  → PUT markdown=A starts
operator types draft B while request is pending
  → local draft becomes B and dirty=true
PUT/invalidation completes
  → old code unconditionally sets dirty=false
operator closes tab
  → dirty-close guard does not run
  → draft B is lost
```

## Root cause

The save handler treated request completion as proof that the live editor state had been persisted. It did not distinguish the submitted snapshot from subsequent edits.

## Correction

- Capture the exact submitted snapshot at save start.
- Track the current draft in a ref so the async continuation reads current state rather than a stale closure.
- Clear dirty state only when the live draft still equals the submitted snapshot.
- If the operator typed during the request, leave the editor dirty and preserve the close veto.

## Verification

`ui/src/views/vaults/pages/NotePage.test.ts` covers both outcomes:

- unchanged live draft equals submitted snapshot → clean;
- newer live draft differs from submitted snapshot → remains dirty.

The full UI typecheck and test suite are part of ADR 0023's final gate.

## Follow-up

The Vault API still lacks a revision/ETag compare-and-swap precondition. ADR 0023 therefore prohibits duplicate writable note views. Independent writable projections must remain blocked until shared-draft ownership or backend conflict detection is implemented.
