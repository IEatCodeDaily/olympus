# ADR 0023 — Post-review remediation status

- Date: 2026-07-16
- ADR status: Proposed
- Source review: `docs/reviews/0023-surface-scoped-workbench-rereview.md`
- Current verdict: **READY FOR CONTINUED ITERATION; NOT YET AN ACCEPTED/SHIP GATE**

The adversarial re-review was captured before the final remediation pass. This note records what changed afterward without rewriting the independent review.

## Finding disposition

| Re-review finding | Disposition | Evidence |
|---|---|---|
| BLOCKER 1 — save clears a newer draft | Fixed in source | `NotePage` captures a submitted snapshot, tracks the live draft through a ref, and clears dirty only when both still match. `NotePage.test.ts` covers equal/different snapshots. Postmortem 0034. |
| BLOCKER 2 — destructive transitions bypass dirty veto | Fixed in source; route-level tests still needed | Dirty resource identity is reported to `VaultWorkspaceView`. Group close, vault switch, rename, and delete fail closed before unmount/domain mutation. Rename/delete detach the writable runtime before the asynchronous mutation, closing the type-during-mutation race. `VaultWorkspace.test.tsx` covers dirty group-close veto. Postmortem 0035. |
| BLOCKER 3 — cross-vault route transition rebinds old tabs | Fixed in source; Back/Forward test still needed | The workbench now records its owning vault ID, renders only when it matches the route vault, and centrally handles every route vault transition. Dirty rejection returns to the prior vault; accepted transitions reset before opening the new target. Postmortem 0036. |
| HIGH 4 — no normalization/invariant seam | Fixed | `normalizeWorkbench` validates versions/shapes/identity, repairs active selection and ratios, and fails closed on malformed/duplicate trees. Two reducer tests cover repair and rejection. |
| HIGH 5 — Cockpit test does not verify socket lifetime | Fixed | `CockpitTerminalLifetime.test.tsx` uses the actual terminal renderer boundary with fake xterm/WebSocket implementations and proves hide does not call socket close or terminal dispose. |
| MEDIUM 6 — incomplete Vault keyboard tabs | Mostly fixed | Horizontal group tabs now implement Left/Right/Home/End with roving focus and activation. Explicit focus-after-close coverage remains a follow-up. |
| MEDIUM 7 — Sessions does not identify pane location | Fixed | `SessionsView` maps visible sessions to stable `P1`, `P2`, `P3` labels. Sidebar rows expose active/open pane labels without adding a horizontal tab bar. |

## Current verification

- `npx tsc --noEmit`: passed.
- `npm run test`: 19 files, 119/119 tests passed.
- Production Vite build: passed; existing large-chunk warning remains.
- Real browser, both themes:
  - Vault: three populated groups, two nested separators, three document tabs.
  - Sessions: three panes, two nested separators, zero transcript tablists, five sidebar vertical-tab rows.
  - Sidebar pane labels `P1`, `P2`, and active `P3` are visible.
- Architecture diagram was rerendered and visually reviewed.

## Remaining acceptance work

1. Add a route-level two-vault Back/Forward test with the same note path and a dirty outgoing draft.
2. Add integrated deferred-save coverage using the real editor rather than only the snapshot predicate.
3. Add explicit focus recovery after closing the selected Vault tab.
4. Resolve the pre-existing light-theme Vault rich-editor canvas mismatch visible in browser evidence; its dark document canvas does not read as daybreak.
5. Keep ADR 0023 Proposed until those gates are closed and a fresh adversarial review is run against the remediated source snapshot.
