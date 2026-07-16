# ADR 0023 — Shared Workbench Adversarial Review

**Verdict: NOT READY**

The binary split-tree direction is sound, and Sessions should remain sidebar navigation for now. The ADR is not implementation-ready because its Vault clone policy permits two unsynchronized editors for one note, its resource identity omits the vault, and it does not define save/close/rename/delete transitions that protect drafts and URL state. The generic plugin lifecycle contract also promises behavior its proposed interface cannot implement.

Review snapshot: ADR SHA-256 `b6364eb6e6ba2422f663cea29208e704c1766c3a11219943c034d05e40072c28`; repository `HEAD` `70f6c564c1c69afd66184dd6668d50e3b70e1b31`. The worktree was already heavily modified; findings are against the cited files as read, not a clean-HEAD diff.

## Findings, ordered by severity

### BLOCKER 1 — “Clone-safe” Vault notes are not clone-safe; duplicate panes can silently overwrite each other

ADR 0023 explicitly allows the same note resource in two groups with separate tab/runtime IDs and says Vault may clone an active note when splitting (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:64-76`). Each mounted `NotePage` owns an independent `draft` and `dirty` flag (`ui/src/views/vaults/pages/NotePage.tsx:26-34`) and saves with an unconditional last-write-wins `putVaultNote(..., { markdown: draft })` (`ui/src/views/vaults/pages/NotePage.tsx:73-81`). The wire type carries no revision, ETag, expected content hash, or conflict precondition (`ui/src/types.ts:470-482`). Two panes can therefore edit the same note, save A, then save stale B and silently erase A.

There is a second loss window in one pane: the editor stays writable while `handleSave` awaits the request and query invalidations, then unconditionally clears dirty state (`ui/src/views/vaults/pages/NotePage.tsx:73-86`, `:121-126`). If the operator types after clicking Save but before completion, the later text remains in memory but the tab is marked clean; close/switch can discard it without warning.

**Required ADR correction:** notes are **not clone-safe today**. Before allowing duplicate note views, choose and specify one of:

1. one shared draft owner keyed by `{vaultId, path}` with multiple projections; or
2. independent drafts plus backend revision/ETag compare-and-swap and an explicit conflict UI.

Also define Save as a snapshot boundary: only clear dirty state if the live draft still equals the submitted snapshot (or make the editor read-only while saving). Add deferred-save and two-pane stale-save acceptance tests using the real editor.

### BLOCKER 2 — Vault resource identity is not globally valid and the ADR omits destructive vault/path lifecycle transitions

The proposed note `resourceKey` is only `note:<path>` (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:64-69`), but `VaultWorkspace` receives a separate `vaultId` and renders every tab against that current vault (`ui/src/views/vaults/components/VaultWorkspace.tsx:25-46`, `:285-288`). The current screen resets the whole workspace on vault selection (`ui/src/views/VaultWorkspaceView.tsx:154-165`), which also unmounts dirty editors without confirmation. A persisted/shared workbench that survives vault changes would instead reuse `note:docs/index.md` from Vault A while rendering it under Vault B. Either outcome is wrong: draft loss on reset or cross-vault aliasing if retained.

Rename and delete are likewise unresolved. Rename opens the new path but does not reconcile the old open tab (`ui/src/views/VaultWorkspaceView.tsx:120-129`). Delete closes tabs by the current tab ID shape (`ui/src/views/VaultWorkspaceView.tsx:136-145`, `:230-232`), but ADR 0023 deliberately makes tab ID different from resource identity (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:62-71`). That helper will no longer find all instances after migration. A rename/delete can leave stale or dirty mounted views for a path that no longer exists.

**Required ADR correction:** define Vault identity as at least `vault:<vaultId>:note:<canonical-path>` (and equivalently scope every Vault kind). State whether there is one manifest per vault or one Vault-surface manifest; do not leave both possible. Specify atomic policies for vault switch, vault deletion, note rename, note deletion, and remote disappearance, including all duplicate instances and dirty veto/confirmation. Acceptance tests must cover dirty vault switch, rename with two open instances, and delete with dirty/clean instances.

### HIGH 3 — Route synchronization is underspecified and closing an active tab can be undone by the URL

The ADR only says a deep link “opens or activates” a resource (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:146`) and asks to preserve URL deep links (`:220`). In the current source, route state opens a tab (`ui/src/views/VaultWorkspaceView.tsx:53-56`), but close only mutates workspace state and does not navigate to the replacement tab (`ui/src/views/VaultWorkspaceView.tsx:172-189`). The URL therefore continues naming the closed note. Refresh reopens it; a later note-query change can also rerun the route effect and resurrect it. Browser Back/Forward, close-last-tab, group collapse, and cross-group activation have no source-of-truth rule.

**Required ADR correction:** define an atomic route/workbench transition table. Closing the route-selected tab must replace the URL with the selected successor (or clear it) in the same user action. Define URL-absence, Back/Forward, invalid/deleted routes, active-group changes, and duplicate-resource instances. Add route assertions to reducer/component/browser gates; screenshots are not an oracle.

### HIGH 4 — The kind interface cannot satisfy the ADR’s payload, unload, suspension, and runtime-lifecycle promises

The proposed `WorkbenchTabKind` only has metadata, `singleton`, `canClose`, and `render` (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:98-110`). The failure rules nevertheless require kind-owned payload validation/recovery, plugin serialization before suspension, unload blocking, and portable/non-portable runtime handling (`:197-206`). No interface operation validates or normalizes `TPayload`, constructs/disposes runtime, serializes/suspends/resumes it, reports portability, or migrates payload versions. “Invalid payload” cannot safely reach a kind-owned error without a parser contract.

This is not an existing plugin-loader seam. The source registry is a module-global `Map` overwritten by `registry.set` with no collision, ownership, unload, or capability checks (`ui/src/cockpit/tabs.tsx:20-43`). The package schema has `view_provider`, but the backend explicitly rejects it as unsupported (`crates/control-plane/src/package.rs:179-193`); the current TS manifest merely exposes package data (`ui/src/types.ts:376-415`). ADR 0023 now acknowledges this at `docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:113` and defers the loader at `:222`, which is correct, but the generic interface still claims the deferred lifecycle behavior.

**Required ADR correction:** keep Phase 1 boring: a framework-free tree/tab manifest plus built-in surface adapters. Remove plugin lifecycle promises from the initial workbench interface. Put dynamic `view_provider` loading, trust/isolation, capability intersection, collision ownership, versioned payload schemas, unload/revocation, and suspend/resume into its own reviewed ADR. If lifecycle stays in scope, specify the complete contract and acceptance tests before implementation.

### HIGH 5 — Cockpit terminal lifetime and migration remain tied to tab IDs and React mount lifetime

Today the browser uses `tab.id` as the server terminal ID (`ui/src/cockpit/tabs.tsx:88-92`). Unmount closes the WebSocket (`ui/src/cockpit/tabs.tsx:126-135`), and the server explicitly closes the shell when the socket closes (`crates/control-plane/src/server/terminal_ws.rs:17-22`, `:262-264`). The current hide path unmounts the entire Cockpit (`ui/src/cockpit/Cockpit.tsx:22-24`, `:57-58`), contradicting both source comments and ADR 0021’s persistence contract. ADR 0023 correctly calls this out and vetoes moving non-portable terminals (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:32`, `:203-206`), but implementation step 4 still says “enable optional splits” without a hard gate on terminal movement/group removal (`:221`).

The legacy migration says “read once,” preserve old IDs, and write a new payload (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:187-195`) but does not name the new key, source precedence, migration marker, old-key deletion, or behavior when the new write fails. The existing loader accepts weakly shaped tabs and geometry (`ui/src/cockpit/store.ts:71-100`) and silently ignores persistence failures (`:103-113`). A partial migration can repeatedly re-import stale v1 state or claim success without durable new state.

**Required ADR correction:** make terminal split/move/group-collapse unavailable until a lifted runtime host demonstrably preserves one renderer/socket and terminal identity. Separate `tabInstanceId` from durable `terminalId`; never derive process identity from React view identity. Define the exact migration algorithm: old/new keys, validated shape, precedence, successful-write verification, migration marker/removal, retry behavior, and recovery copy. Test close/hide/move/collapse against actual WebSocket open/close events, not only component identity.

### HIGH 6 — Persistence security and operator scoping are too vague

The ADR forbids serializing credentials but explicitly permits URL payloads (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:79-96`). URLs routinely carry query tokens, signed links, userinfo, internal hostnames, and sensitive paths. The current browser tab persists the full normalized URL in tab state (`ui/src/cockpit/tabs.tsx:176-189`), and the store writes that state as-is to localStorage (`ui/src/cockpit/store.ts:33-35`, `:103-110`). localStorage is origin-wide, not “per operator”; changing users in the same browser profile can expose the prior operator’s layout, targets, and URLs unless keys and cleanup are principal-scoped.

The embedded browser uses an unrestricted iframe (`ui/src/cockpit/tabs.tsx:191-216`). A future plugin view loaded as arbitrary same-origin React would inherit all Hall UI authority. ADR 0023’s instruction not to load arbitrary package JavaScript is good (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:222`) but there are no browser-tab constraints.

**Required ADR correction:** define per-kind persisted payload schemas and redaction/allowlist policy; reject URL credentials and define treatment of query/fragment secrets. Namespace local persistence by stable operator/org identity and clear or switch it on logout/principal change. Specify iframe `sandbox`, `allow`, referrer policy, navigation/download/pop-up policy, and same-origin restrictions. Dynamic plugin UI remains out of scope until an isolation/capability design exists.

### MEDIUM 7 — Accessibility requirements are incomplete for both the workbench and the Sessions sidebar

ADR 0023 asks for `role="tablist"`, associated panels, separators, keyboard reachability, and visual distinction (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:208-214`) but omits the actual ARIA tab keyboard model: roving `tabIndex`, Arrow/Home/End, stable `id`/`aria-controls`/`aria-labelledby`, focus behavior after close/move, and `aria-pressed` on layout toggles. The current Vault tabs have `role="tab"` and `aria-selected` but none of those relationships/keyboard behaviors (`ui/src/views/vaults/components/VaultWorkspace.tsx:167-203`, `:217-235`). Current separators expose only `aria-valuenow`, not min/max (`:244-268`). Acceptance gates do not test these semantics (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:248-255`).

Sessions has more immediate failures. Each clickable row is a non-focusable `div` (`ui/src/views/sessions/components/SessionSidebar.tsx:217-224`); metadata exists only in an unassociated hover tooltip (`:225-230`); pin/archive actions are revealed only by CSS `:hover` (`ui/src/index.css:117-131`); and the hovercard is disabled on touch (`ui/src/index.css:929-955`). The sidebar resize handle is an unlabeled mouse-only `div` (`ui/src/views/sessions/components/SessionSidebar.tsx:125-127`). Detailed metadata fixes discoverability but not keyboard/touch operation.

**Required ADR correction:** make the ARIA tabs pattern and keyboard focus transitions explicit. Sessions acceptance must include keyboard activation, `aria-current`/selected state, focus-visible pin/archive controls, touch access, programmatic status text, and a labeled keyboard-resizable separator.

### MEDIUM 8 — Sessions should remain sidebar navigation, but the stated rationale and preference design should be tightened

**Decision: keep Sessions in the sidebar.** The source already provides route-backed switching, pinned and recent sections, and a History escape hatch (`ui/src/views/sessions/components/SessionSidebar.tsx:26-27`, `:58-62`, `:96-124`). There is no demonstrated side-by-side session workflow, and a second tab strip would duplicate navigation and reduce transcript space. Reconsider only with a concrete compare/monitor workflow, as ADR 0023 says (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:148-163`).

However, “sessions are durable domain objects” is not a valid distinction: Vault notes are durable domain objects and still benefit from view tabs. The defensible distinction is current task shape and navigation duplication, not durability. Also, Olympus already has a persisted global `Density = "comfortable" | "compact"` preference (`ui/src/theme.tsx:19-30`, `:62-69`, `:77-80`). A second independent sidebar density setting is speculative complexity unless product evidence requires it.

**Required ADR correction:** retain configurable metadata visibility, but reuse the existing global density for row spacing first. Add a Sessions-only density override only if operators demonstrably need the sidebar denser than the rest of Olympus. Replace the durability rationale with the concrete workflow rationale.

### MEDIUM 9 — Acceptance gates omit the failure cases that decide whether this workbench is safe

The listed gates are mostly reducer shape, shallow component behavior, build commands, and screenshots (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:248-255`). Existing tests already mock `NotePage`, so they cannot prove draft retention or save correctness (`ui/src/views/vaults/components/VaultWorkspace.test.tsx:6-12`). Existing reducer tests even validate duplicate note objects with identical IDs across panes (`ui/src/views/vaults/vaultWorkspace.test.ts:55-62`), the ambiguity the ADR intends to remove.

**Required additions:**

- real-editor byte-exact save/reload;
- type-during-in-flight-save;
- duplicate-note concurrent save/conflict;
- dirty vault switch, collapse, close, rename, and delete;
- URL Back/Forward and active-tab close synchronization;
- persistence corruption, unknown version recovery, localStorage quota/write failure, and v1 migration retry;
- actual terminal socket survival across hide, tab switch, allowed layout mutations, and blocked moves;
- plugin/view-provider work excluded until its separate security/lifecycle ADR is approved;
- ARIA tab keyboard model and Sessions keyboard/touch action access;
- desktop/mobile geometry assertions, not screenshots alone.

## What is ready

| Before / current constraint | ADR direction to keep | Why |
|---|---|---|
| Fixed `single/columns/rows/grid` layout (`ui/src/views/vaults/vaultWorkspace.ts:3-30`) | Recursive binary split tree (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:115-132`) | It directly solves nested horizontal/vertical groups and branch-local ratios without a window manager. |
| Split percentages are component-local (`ui/src/views/vaults/components/VaultWorkspace.tsx:48-56`) | Store bounded ratios in split nodes (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:117-124`) | Geometry then survives remounts and can be normalized/tested as data. |
| Vault and Cockpit duplicate flat tab concepts | Share only the pure manifest/tree operations, keep surface rendering/policy separate (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:10-14`, `:216-224`) | This is the smallest reusable seam; sharing runtime/plugin machinery now would be speculative. |
| Session rows hide useful metadata on hover (`ui/src/views/sessions/components/SessionSidebar.tsx:225-230`) | Configurable direct metadata in detailed rows (`docs/adrs/0023-shared-workbench-tabs-and-split-grid.md:154-161`) | It improves scanability and touch/keyboard availability without adding redundant session tabs. |

## Readiness conditions

ADR 0023 can move to **READY** after it is revised to:

1. prohibit duplicate writable note runtimes until shared-draft or revision-conflict semantics exist;
2. scope all Vault resource keys by vault and specify switch/rename/delete/remote-disappearance behavior;
3. define route/workbench synchronization for every close/activate/navigation transition;
4. reduce the initial shared API to pure manifest/tree mechanics, moving plugin UI loading to a separate security/lifecycle ADR;
5. separate Cockpit tab identity from terminal identity and fully specify one-shot migration and runtime-host gates;
6. define principal-scoped, redacted persistence and iframe constraints;
7. include complete ARIA tab/separator and Sessions keyboard/touch requirements; and
8. add behavioral loss/concurrency/migration tests to the acceptance gates.
