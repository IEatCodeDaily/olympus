# ADR 0023 — Surface-Scoped Views and Split Workbenches

- Status: Proposed
- Date: 2026-07-16
- Relates to: ADR 0021 (operator cockpit), ADR 0020 (client projection correctness), ADR 0004 (knowledge vaults)
- Supersedes: the fixed `VaultWorkspaceLayout = single | columns | rows | grid` model

## 1. Decision

Olympus will share a small, framework-free **view identity and split-tree model**, while each surface owns its chrome and capability policy.

- **Cockpit stays single-pane.** Its existing horizontal tab strip switches one live tool surface. Cockpit does not consume split-tree operations.
- **Vault is a multi-pane editor workbench.** Every editor group owns a horizontal document tab strip. Groups may be nested with horizontal and vertical splits.
- **Sessions is a multi-pane monitor/work surface.** The existing left sidebar is the only tab list and is presented vertically. Transcript panes have no horizontal tab bar. The sidebar opens, activates, closes, and places sessions into panes.

**Doctrine:** view records own presentation identity; split trees own geometry; surface policy owns chrome and placement; domain services own durable resources.

The shared model is not a backend business entity and does not force the same visual component onto unrelated surfaces.

## 2. Current state, verified from source

### 2.1 Vault

`ui/src/views/vaults/vaultWorkspace.ts` defines a closed tab-kind union, pane arrays, and four fixed layouts. `setWorkspaceLayout` duplicates the active tab object into newly created panes. `ui/src/views/vaults/components/VaultWorkspace.tsx` owns independent local split percentages, so geometry is lost on remount.

The current editor runtime is not safe to duplicate. Each `NotePage` owns an independent draft and saves without a revision precondition. Two writable views of one note can overwrite one another. Save also clears dirty state after the request even if the operator typed after the submitted snapshot.

### 2.2 Cockpit

`ui/src/cockpit/store.ts` already has a flat kind-polymorphic tab list. This is appropriate for a single-pane tool switcher. It does not need groups or a split tree.

`ui/src/cockpit/Cockpit.tsx` currently returns `null` while hidden. Terminal cleanup closes the WebSocket and disposes xterm. This violates ADR 0021 and is tracked in `docs/postmortems/0033-cockpit-visibility-disposed-live-tabs.md`; it is a lifecycle bug independent of multi-pane architecture.

### 2.3 Sessions

`ui/src/views/sessions/components/SessionSidebar.tsx` is route-backed navigation with pinned and recent sections. A row is currently a non-focusable `div`; operational metadata is hover-only; pin/archive actions are hover-only. `SessionsView.tsx` renders exactly one `SessionChatLayout` selected by the route.

The sidebar is already the strongest session-switching affordance. Adding a horizontal tab bar above every transcript would duplicate it and reduce transcript space.

## 3. Shared core

The initial shared package contains data and pure operations only:

```text
WorkbenchState<TView>
  version
  root ───────────────► LayoutNode<TView>
  activeGroupId              │
                             ├─ GroupNode<TView>
                             │    id
                             │    views[] ─────► ViewRecord<TView>
                             │    activeViewId       id (view-instance identity)
                             │                       resourceKey (domain identity)
                             │                       kind
                             │                       title
                             │                       payload (bounded JSON value)
                             │
                             └─ SplitNode<TView>
                                  id
                                  axis: horizontal | vertical
                                  ratio: 20..80
                                  first: LayoutNode<TView>
                                  second: LayoutNode<TView>
```

The core provides:

- stable view and node identity;
- open, activate, close, reorder, and move operations;
- split-right and split-down operations;
- group removal with sibling promotion;
- nested split resizing with bounded ratios;
- normalization and invariant validation;
- preset commands that construct trees rather than a stored layout enum.

The core does **not** provide React renderers, plugin loading, runtime construction, draft ownership, domain mutations, or implicit cloning.

## 4. Surface policies

### 4.1 Cockpit: single pane

Cockpit retains its flat `CockpitTab[]` and active tab ID.

- One horizontal tab strip is appropriate because it switches tools inside one floating pane.
- No split controls, group IDs, grid presets, or cross-group movement are exposed.
- Hiding the cockpit changes presentation only; all renderers stay mounted.
- Inactive renderers stay mounted where their live runtime requires it.
- `tabInstanceId` and backend `terminalId` must become separate identities before terminal persistence is called durable.

Keeping Cockpit out of the split tree is deliberate simplicity, not an exception to be removed later without product evidence.

### 4.2 Vault: nested editor groups

Vault consumes the full split tree.

- Every group owns one horizontal document tab strip and one active document panel.
- Split Right creates a horizontal split: left/right groups.
- Split Down creates a vertical split: top/bottom groups.
- Splits may nest arbitrarily.
- Each split stores its own ratio and exposes pointer and keyboard resizing.
- Closing a group promotes its sibling; no unary split remains.
- Responsive narrow layouts may stack leaves visually but do not mutate the saved desktop tree.

#### Note duplication policy

Writable notes are **not clone-safe today**. Splitting a Vault group therefore creates an empty group. Opening a note that is already open activates its existing view rather than creating a second writable runtime.

Duplicate writable note views remain blocked until one of these is separately implemented and reviewed:

1. one shared draft owner keyed by `{vaultId, canonicalPath}` with multiple projections; or
2. independent drafts backed by revision/ETag compare-and-swap and explicit conflict UI.

Graph/table views may be recreated only if their adapter declares them stateless and clone-safe. The generic reducer never performs that decision.

#### Resource identity

Every Vault resource key is scoped by vault:

- note: `vault:<vaultId>:note:<canonical-path>`;
- graph: `vault:<vaultId>:graph`;
- table: `vault:<vaultId>:table`.

There is one in-memory workbench manifest per active vault in the first implementation. Switching vaults must ask once before discarding any dirty runtime in the outgoing vault. Persistence across vaults is deferred until dirty-draft ownership and principal-scoped storage are defined.

#### Destructive lifecycle

- **Rename:** atomically update every matching view resource key, path, title, and route after the domain rename succeeds. Dirty matching views veto rename until saved or explicitly discarded.
- **Delete:** enumerate every matching view instance. Dirty instances require confirmation; successful deletion closes all matching views and selects/navigates to a successor.
- **Remote disappearance:** retain the view as a recoverable missing-resource panel; never silently drop an unsaved draft.
- **Vault deletion/switch:** enumerate dirty runtimes before unmounting and fail closed without confirmation.

#### Save snapshot rule

Saving captures the submitted draft snapshot. Dirty state is cleared only if the live draft still equals that submitted snapshot when the request completes. Typing during an in-flight save therefore remains dirty.

### 4.3 Sessions: sidebar-owned vertical tabs and multiple panes

Sessions consumes the split tree with a strict surface policy: **one session view per group**.

The sidebar is the only tab list:

- open sessions appear as vertical tab rows in pinned/recent/open sections;
- clicking a row opens or activates that session in the active pane;
- a visible session row indicates which pane contains it and which pane is active;
- explicit **Open to Right** and **Open Below** actions place a session into a new group;
- closing a session view removes only the presentation view; it never archives, cancels, or deletes the Hall session;
- each transcript pane has a compact pane header for title, status, pane actions, and close, but no horizontal tab strip;
- the URL names the active pane's session only; the remaining pane layout stays client presentation state.

A session may appear in at most one pane initially. Requesting a visible session activates its existing pane. This avoids duplicated live subscriptions and ambiguous composer ownership. Side-by-side operation is achieved with different sessions.

The sidebar reuses the existing global comfortable/compact density. It adds configurable metadata visibility for fields already projected by Hall: agent, model, node, source, message count, token total, and last activity. A Sessions-only density override is deferred until operators demonstrate a need distinct from global density.

## 5. Route/workbench synchronization

Routes identify the active resource, not the complete layout.

### Vault

| Event | Workbench transition | Route transition |
|---|---|---|
| Navigate/deep-link | activate existing resource or open in active group | route is source of requested resource |
| Activate tab | set active group/view | replace route with selected resource |
| Close active tab | choose adjacent successor in same group, else active view in another group | replace with successor or vault root |
| Close inactive tab | remove view | unchanged |
| Browser Back/Forward | activate/open requested resource | no second navigation |
| Invalid/deleted route | show recoverable missing state or choose vault root | replace only after authoritative absence |

### Sessions

| Event | Workbench transition | Route transition |
|---|---|---|
| Navigate/deep-link | activate existing session pane or replace active pane | route is source of requested active session |
| Sidebar click | activate existing pane or replace active pane | replace with selected session |
| Open to Right/Below | split active group and place requested session | replace with requested session |
| Activate pane | set active group | replace with pane's session |
| Close active pane | promote sibling and activate its session | replace with successor session or `/sessions` |
| Browser Back/Forward | activate existing pane or replace active pane | no second navigation |

Route effects must not resurrect a tab immediately closed by a user action. Navigation and workbench mutations are treated as one command at the surface adapter.

## 6. Runtime and persistence boundaries

The workbench manifest stores bounded JSON-safe presentation state only. It never stores note bytes, terminal output, unsaved drafts, credentials, sockets, editor handles, or arbitrary plugin values.

Initial persistence policy:

- Vault and Sessions split state remains in memory while draft and principal-scoping semantics are hardened.
- Cockpit keeps its existing local manifest, but browser URLs must reject userinfo and redact or omit sensitive query/fragment data before any future persistence hardening claims.
- Cross-device persistence moves to Hall user UI preferences only after the schema is principal/org scoped.

Unknown manifest versions fail closed to a safe single group. Any future persisted migration must define old/new keys, source precedence, validated shape, successful-write verification, retry behavior, and recovery copy.

## 7. Accessibility and interaction

### Vault group tabs

- each group owns `role="tablist"`;
- tabs use stable IDs, `aria-selected`, `aria-controls`, and roving `tabIndex`;
- panels use `aria-labelledby`;
- Left/Right, Home, and End move tab focus; Enter/Space activates;
- closing moves focus to the selected successor or group action;
- split buttons expose pressed/state semantics where applicable.

### Split separators

- `role="separator"`;
- correct `aria-orientation`;
- `aria-valuemin`, `aria-valuemax`, and `aria-valuenow`;
- arrow-key resizing with bounded increments;
- visible focus state and a pointer hit target larger than the hairline.

### Sessions vertical tabs

- rows are buttons or links, not clickable `div`s;
- active row uses `aria-current="page"`; visible-but-inactive rows expose pane presence programmatically;
- pin, archive, split, and close-view actions are keyboard/focus/touch reachable, not hover-only;
- operational status has text available to assistive technology;
- the sidebar separator is labeled and keyboard-resizable.

## 8. Plugin boundary

The initial shared model has no dynamic plugin lifecycle contract. Built-in surface adapters may map a known `kind` to a renderer.

Dynamic `view_provider` loading, trust/isolation, capability intersection, name collision ownership, payload schema migration, unload/revocation, and suspend/resume require a separate security/lifecycle ADR. Olympus must not inject arbitrary package JavaScript into Hall UI.

## 9. Implementation sequence

1. Add `ui/src/workbench/model.ts` with generic view/group/split entities, pure operations, normalization, and tests.
2. Migrate Vault to the recursive tree, preserving existing document tabs, dirty-close protection, drag/drop, and mounted inactive tab panels. Splits start empty; duplicate writable notes are prohibited.
3. Fix Vault route synchronization, save snapshot semantics, and rename/delete reconciliation before persistence.
4. Migrate Sessions from one route-selected chat to a split tree with one session per group. Keep the sidebar as the only tab list and add explicit right/below placement actions.
5. Improve Session sidebar metadata configuration, keyboard/touch behavior, and labeled resizing.
6. Keep Cockpit flat and single-pane. Independently fix hide/show renderer lifetime and add a socket-survival test.
7. Design `view_provider` and principal-scoped UI preference persistence in separate ADRs.

## 10. Rejected alternatives

### Give every surface the same horizontal tab bar

Rejected. Vault needs per-group document tabs; Sessions already has a vertical tab list in its sidebar; Cockpit only needs a single tool switcher. Shared semantics do not require shared chrome.

### Make Cockpit multi-pane

Rejected. There is no current operator workflow that justifies a window manager inside the floating cockpit, and live runtime movement has dangerous React/socket lifecycle implications.

### Keep Sessions single-pane

Rejected by product direction. Operators need side-by-side session monitoring/interaction. The sidebar can own those views without adding horizontal tab bars.

### Duplicate the same session into multiple panes

Rejected initially. It duplicates subscriptions and creates ambiguous composer/steering ownership. Activate the existing pane instead.

### Keep the four-value Vault layout enum

Rejected. It cannot express arbitrary nested splits or branch-local ratios.

### Duplicate writable notes on split

Rejected until shared-draft or revision-conflict semantics exist. Current last-write-wins saves can silently erase edits.

### CSS Grid coordinates as canonical state

Rejected. Recursive split insertion/removal and independent branch resizing are simpler and safer with a binary split tree.

## 11. Acceptance gates

### Shared reducer

- unique stable view/node IDs;
- split right/down and arbitrary nesting;
- ratio clamping and keyboard increments;
- close-group sibling promotion;
- invalid IDs return unchanged state;
- malformed tree normalization;
- no implicit cloning.

### Vault

- nested horizontal and vertical groups render correctly;
- split creates an empty group;
- opening an already-visible note activates it rather than duplicating it;
- real-editor byte-exact save/reload;
- type-during-in-flight-save remains dirty;
- dirty vault switch, collapse, close, rename, and delete fail closed;
- URL Back/Forward and active-tab close stay synchronized;
- full ARIA tabs and separator keyboard behavior;
- desktop, narrow, obsidian, and daybreak geometry evidence.

### Sessions

- two or more different sessions can render side by side through nested splits;
- sidebar is the only tab list; no pane renders a horizontal tab bar;
- sidebar click activates an existing pane rather than duplicating it;
- Open to Right/Below, pane close, and sibling promotion synchronize the route;
- compact/comfortable global density and metadata field configuration render correctly;
- rows and all actions are keyboard/touch reachable;
- WebSocket reconnect convergence remains correct independently per session pane;
- desktop, narrow, obsidian, and daybreak geometry evidence.

### Cockpit

- no split controls exist;
- hide/show preserves the same renderer and does not close its terminal socket;
- inactive live tool tabs remain mounted as required.

### Repository gates

- focused reducer/component tests pass;
- full Vitest passes;
- `tsc --noEmit` passes;
- production Vite build passes.
