# ADR 0023 — Surface-Scoped Workbench Re-review

**Verdict: NOT READY**

The revised ADR is materially better and now states the correct surface policy. The current implementation also has the requested gross shape: Cockpit is single-pane, Vault has nested split groups with per-group horizontal document tabs, and Sessions has nested panes with the left sidebar as its only session tab list. It is still not ready because Vault can silently lose edits during save and destructive transitions, and browser navigation between vaults can bind tabs from one vault to another vault's editor runtime.

Review snapshot: ADR SHA-256 `9b719896d2daf6517b503a2953fdf2604ee4071681d20fd7daf93567d76acba0`; repository `HEAD` `70f6c564c1c69afd66184dd6668d50e3b70e1b31`. The shared worktree was heavily dirty. Findings refer to the source as read at this snapshot, not to a clean-HEAD diff.

## Policy verification

| Surface | Required policy | Current implementation | Result |
|---|---|---|---|
| Cockpit | Single pane; one horizontal tool tab strip; no split tree; hide/show preserves mounted live renderers (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:12`, `:80-90`, `:302-306`) | `Cockpit` renders one flat tab strip and one visible pane, imports no workbench split model, keeps every renderer mounted, and hides only the shell (`ui/src/cockpit/Cockpit.tsx:57-66`, `:71-97`, `:110-128`; `ui/src/index.css:1121`). | **PASS structurally**; real socket-survival verification is still missing. |
| Vault | Nested multi-pane; each group has a horizontal document tab strip; splits start empty (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:13`, `:92-113`) | Recursive `SplitLayout` renders the tree (`ui/src/views/vaults/components/VaultWorkspace.tsx:120-136`, `:228-230`); `splitGroup` inserts an empty group (`ui/src/workbench/model.ts:155-177`); every group renders its own `role="tablist"` (`ui/src/views/vaults/components/VaultWorkspace.tsx:129-182`). | **PASS structurally; NOT READY lifecycle.** |
| Sessions | Nested multi-pane; sidebar-only vertical session tabs; no horizontal session tab bar (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:14`, `:136-150`) | `SessionsView` renders the recursive split tree and one `SessionChatLayout` per group (`ui/src/views/SessionsView.tsx:176-207`); `sessionWorkspace` enforces one session view per group (`ui/src/views/sessions/sessionWorkspace.ts:39-75`); session rows and placement actions live only in `SessionSidebar` (`ui/src/views/sessions/components/SessionSidebar.tsx:150-184`, `:299-359`). No transcript `tablist` or horizontal session tab strip exists. | **PASS structurally.** |

## Findings, ordered by severity

### BLOCKER 1 — Save still clears dirty state for text that was never saved

The revised ADR explicitly requires snapshot-safe save semantics: only clear dirty when the live draft still equals the submitted draft (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:132-134`; acceptance gate `:284-286`). The implementation sends the current `draft`, awaits the request and two invalidations, then unconditionally calls `setDirty(false)` and `onDirtyChange(false)` (`ui/src/views/vaults/pages/NotePage.tsx:73-86`). The editor remains writable and updates `draft` while that await is in flight (`ui/src/views/vaults/pages/NotePage.tsx:109-126`).

Concrete loss path: submit draft A, type B before the response, response completes, tab becomes clean although only A reached the server, then close/switch without a warning and lose B. The prior review's principal data-loss defect remains unfixed.

**Required fix:** capture `submittedDraft`; clear dirty only if the live draft still equals it (use a ref or equivalent current-value check), otherwise keep the tab dirty. Add a deferred-request real-editor test that types during save and then attempts close.

### BLOCKER 2 — Group close, vault switch, rename, and delete bypass the ADR's dirty-runtime veto

Dirty ownership is component-local in `VaultWorkspace` (`ui/src/views/vaults/components/VaultWorkspace.tsx:54-57`). Only individual tab close and bulk tab-menu actions consult it (`:79-97`). The editor-group close button calls `onCloseGroup` directly with no dirty check (`:183-189`), and the parent removes the whole group (`ui/src/views/VaultWorkspaceView.tsx:219-223`; `ui/src/views/vaults/vaultWorkspace.ts:158-160`). Every dirty editor in that group is unmounted without confirmation.

The other destructive paths bypass dirty state too:

- explicit vault selection resets the entire manifest immediately (`ui/src/views/VaultWorkspaceView.tsx:169-176`);
- rename performs the domain mutation and then closes the old resource (`:131-141`);
- delete performs the domain deletion and then closes matching views (`:149-159`).

This directly violates the ADR's switch/rename/delete requirements (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:123-130`) and acceptance gate (`:286`). Rename/delete are worse than a discarded presentation: the durable resource is changed or deleted before the UI has asked what to do with its unsaved draft.

**Required fix:** lift dirty-runtime enumeration to `VaultWorkspaceView` (or expose a single surface-owned guard) and run every destructive command through it before any state or domain mutation. A dirty group close must veto/cancel; vault switch must ask once for all outgoing dirty views; rename/delete must fail closed until the matching draft is saved or explicitly discarded. Add component tests for all four paths.

### BLOCKER 3 — Browser navigation between vaults can render and save Vault A tabs against Vault B

The ADR chose one in-memory manifest per active vault and requires a guarded discard on vault switch (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:115-130`). The implementation resets the workspace only in the sidebar's `onSelectVault` callback (`ui/src/views/VaultWorkspaceView.tsx:169-176`). A browser Back/Forward transition or direct route change updates `activeVaultId`, but the route effect merely opens the new vault target into the existing tree (`:43-64`). It does not reset or partition the old manifest.

`VaultWorkspace` then receives only the newly active outer `vaultId` (`ui/src/views/VaultWorkspaceView.tsx:185-189`) and passes that same value to every tab renderer (`ui/src/views/vaults/components/VaultWorkspace.tsx:193-217`, `:245-248`). A retained tab whose resource key says Vault A is therefore mounted as `NotePage(vaultId = Vault B, notePath = A's path)`. If that path exists in B, the operator can view or save the wrong document; if it does not, an unsaved A draft is replaced by a missing/loading state. Scoped `resourceKey` construction (`ui/src/views/vaults/vaultWorkspace.ts:180-208`) does not protect rendering because the renderer ignores the key's vault scope.

**Required fix:** make the manifest actually per-vault (keyed by vault ID), or atomically clear it on every `activeVaultId` transition after the dirty guard succeeds. Render each tab from validated scoped payload rather than blindly injecting the current route vault. Add Back/Forward tests across two vaults with the same note path and with a dirty outgoing draft.

### HIGH 4 — The shared core does not implement normalization or invariant validation promised by the ADR

The ADR defines normalization and invariant validation as core operations (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:66-76`) and makes malformed-tree normalization an acceptance gate (`:269-277`). `ui/src/workbench/model.ts:1-283` exports constructors and mutations only; there is no normalizer, validator, malformed-version handling, active-group repair, duplicate-ID repair/rejection, invalid-ratio repair, or active-view repair. The model tests cover generated happy-path trees and invalid operation IDs, not malformed manifests (`ui/src/workbench/model.test.ts:27-114`).

This is not immediately reached while state remains generated and memory-only, but it makes the claimed shared model incomplete and unsafe to hydrate or migrate. The ADR itself says unknown versions fail closed (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:192`).

**Required fix:** either implement and test one small `normalizeWorkbench`/validation seam now, or explicitly remove hydration/malformed-tree claims from this implementation milestone. Do not add persistence before this gate exists.

### HIGH 5 — Cockpit lifecycle is fixed structurally, but the acceptance test does not verify the terminal socket contract

The hide implementation now keeps panes mounted (`ui/src/cockpit/Cockpit.tsx:59-66`, `:110-128`), which fixes the React ancestry defect. The actual terminal cleanup still closes the WebSocket on unmount (`ui/src/cockpit/tabs.tsx:126-135`), so socket survival is the load-bearing behavior. The only Cockpit test mocks the renderer and asserts that a marker DOM node survives (`ui/src/cockpit/Cockpit.test.tsx:6-10`, `:24-33`). It cannot detect a terminal renderer reconnect, `WebSocket.close()`, or xterm disposal and therefore does not satisfy the ADR's explicit socket-survival gate (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:302-306`).

**Required fix:** add one test with the actual terminal renderer boundary and a fake `WebSocket`, toggle hide/show and inactive/active tabs, and assert the original socket remains open and `close` is not called. Keep Cockpit out of the split tree.

### MEDIUM 6 — Vault tabs do not implement the ADR's required keyboard tab model

The ADR requires Left/Right, Home/End, Enter/Space, roving focus, and focus recovery (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:194-211`). Current tabs have correct IDs, relationships, selected state, and roving `tabIndex`, but only an `onClick` activation handler (`ui/src/views/vaults/components/VaultWorkspace.tsx:166-178`); there is no keyboard navigation or post-close focus transfer. Separators do implement keyboard resizing correctly (`ui/src/workbench/SplitLayout.tsx:82-110`).

**Required fix:** add the standard tablist key handler and deterministic focus after close. Cover it with a component test; do not invent a second tab component abstraction unless an existing one already exists.

### MEDIUM 7 — Session rows show that a session is open, but not which pane contains it

The revised ADR requires a visible row to indicate both which pane contains the session and which pane is active (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:140-148`). The sidebar receives only a `Set<string>` (`ui/src/views/SessionsView.tsx:90-93`, `:145-159`), so it has discarded the session-to-group mapping. Rows can render only `ACTIVE` or `OPEN` (`ui/src/views/sessions/components/SessionSidebar.tsx:299-324`). With three or more nested panes, `OPEN` does not identify left/right/top/bottom or a pane label.

**Required fix:** pass a small session-to-pane descriptor map instead of only a set, then render and expose a stable pane label (for example “Left pane”, “Top-right pane”, or a numbered pane label). Keep the sidebar as the only tab list; do not add a horizontal tab bar.

## Readiness decision

**NOT READY.** The surface-scoping decision is now correct, but the implementation must not ship while BLOCKER 1–3 can discard or misdirect Vault edits. After those are fixed, the minimal readiness bar is: implement/limit the core normalization claim, add real Cockpit socket-lifetime coverage, complete Vault keyboard behavior, and expose Session pane location without introducing horizontal session tabs.

## Verification run

- Focused Vitest: **28/28 passed** across workbench, Vault workspace, Sessions workspace, SplitLayout, and Cockpit tests.
- Full Vitest: **114/114 passed** (existing React `act(...)` warnings in `VaultMarkdownEditor.test.tsx`; no failures).
- `npx tsc --noEmit`: **passed**.
- `npm run build`: **passed** (existing large-chunk warning).

Passing tests do not cover the three Vault loss/cross-scope paths above; the focused search found no deferred-save, dirty vault-switch/group-close, or Back/Forward cross-vault behavioral tests.
