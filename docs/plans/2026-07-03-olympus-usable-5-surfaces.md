# Olympus — "Usable" Milestone: 5-Surface Cockpit (Sessions · Vaults · Projects · Fleet · Settings)

> **Goal (operator directive):** make Olympus good enough to *move to* — replace
> the operator's daily driver. Focus on FIVE surfaces only: **Sessions, Vaults,
> Projects, Fleet (Nodes), Settings.** Everything else in the concept
> (Workflow, Plugins, Workbench, Console, Ledger, Atlas) is out of scope for
> this milestone — do not build it.
>
> This is the durable brief for a Hermes-kanban swarm. Each card below is
> self-contained: goal, files, exact backend contract, gates, `Done =`. A worker
> claims a card, reads THIS doc + its card body, and ships. Long-horizon,
> final-product quality — not a stub.

**Board:** `olympus-usable`. **Repo:** `/home/rpw/olympus`. **UI:** `ui/` (React
18 + Vite + TanStack Router/Query + Zustand + MSW). **Backend:** Rust
control-plane (`crates/control-plane`).

---

## Design language — LOCKED (the "Instrument" system)

The concept design system is **vendored into the repo** at `ui/src/design/`:

- `ui/src/design/tokens/*.css` — colors, type, spacing, radius, motion, fonts.
  Default theme `obsidian` (neutral near-black, **silver** accent `#C9C9C9`);
  light theme `[data-theme="light"]` ("daybreak"). **Never hardcode a hex in a
  component — reference `var(--token)` only.** A new color need = add a token to
  every theme block.
- `ui/src/design/styles/components.css` — the `.ol-*` class contract
  (`.ol-btn`, `.ol-card`, `.ol-badge`, `.ol-input`, `.ol-nav`, `.ol-tabs`,
  `.ol-stat`, `.ol-dot`, `.ol-live`, `.ol-menu`, `.ol-dialog`, `.ol-switch`,
  `.ol-tag`, `.ol-skel`, `.ol-avatar`, etc.). Render these classes; don't
  reinvent primitives.
- `ui/src/design/readme.md` — voice, casing, iconography, density rules. READ IT.
- **Concept reference (groundtruth for layout):**
  `docs/design/concept/olympus-app-concept.html` — open it in a browser to see
  the exact target for every view. This is what "done" looks like.

**Voice/casing:** sentence case for prose/buttons/titles ("New session", not
"New Session"); UPPERCASE mono micro-labels for field labels / badges / column
titles / kickers (`SELECTED NODE`, `RUNNING`, `BRANCH`). Addresses the user as
**you**; the product is **Olympus**, never "we". Terse, operator-to-operator.

**Hard UI rules (all view workers):**
- Only `var(--token)` colors. No raw hex, no Inter, no purple gradients.
- URL-persistent state via TanStack Router (active surface, selected id on URL);
  TanStack Query for server state; Zustand for ephemeral UI only.
- Every view: header → toolbar (if filterable) → content → empty state (when
  empty) → skeletons (while loading).
- **Real browser e2e for any UI change** — never claim a view works from a build
  alone. Screenshot against the concept.

---

## Architecture: View → Page (LOAD-BEARING — every worker must understand this)

The app shell is a **two-level hierarchy**. Naming is fixed and enforced:

```
┌─────────────────────────────────────────────────────────────────┐
│ TopBar: [sidebar toggle] · [View selector: 5 chips] · search ⌘K │
│         · theme · org · profile                                  │
├────────────────┬──────────────────────────┬──────────────────────┤
│  Left Sidebar  │  Viewport                │  Right Sidebar       │
│  (View-owned)  │  (Page-owned content)    │  (View-owned)        │
│   NavItems +   │                          │                      │
│   context      │                          │                      │
│                ├──────────────────────────┤                      │
│                │  Bottom Panel (View-owned)│                     │
└────────────────┴──────────────────────────┴──────────────────────┘
```

### View (topbar selector chip)
- **Selected by:** a topbar chip (Sessions, Vaults, Projects, Fleet, Settings).
- **Owns:** the **left sidebar content** (NavItems + context lists), the
  **viewport LAYOUT** (how viewport/right-sidebar/bottom-panel are arranged),
  the **right sidebar** content, and the **bottom panel** content.
- A View switch re-renders the sidebar + viewport layout entirely.
- File: one `views/<View>View.tsx` per View (e.g. `SessionsView.tsx`).

### Page (left-sidebar NavItem)
- **Selected by:** a NavItem inside the View-owned left sidebar.
- **Owns:** the **viewport content only**. A Page CANNOT modify the left
  sidebar, right sidebar, or bottom panel — those are fixed by the View.
- Each Page is a **URL-persistent route** (TanStack Router knows it;
  back/forward and deep-linking work). Switching NavItems swaps viewport
  content and changes the URL, nothing else.
- File: one component per Page, rendered inside the View's viewport slot.

### Viewport layout varies per View

The viewport is not a fixed shape. Each View defines its own layout:

| View | Left sidebar (View-owned) | Viewport layout | Right sidebar (View-owned) |
|---|---|---|---|
| **Sessions** | New Session; NavItems (Agents, Usage); session list (pinned/recent/observed) | chat transcript + composer; collapsible bottom panel (Terminal/Output/Debug) | tabbed (Outline, Context, Git, Diff, AI) |
| **Vaults** | vault selector; NavItems (Notes tree, Tables, Graph); directory tree | tabbed note editor | vault agent / info / properties |
| **Projects** | NavItems (Boards list); filters | kanban columns (per board) | card detail |
| **Fleet** | NavItems (Nodes list); Add node | node grid / drill-in | node detail (running sessions) |
| **Settings** | NavItems (Appearance, Model routing, Runtime) | settings sections | (none or minimal) |

### File naming convention (ENFORCED)
- `views/<View>View.tsx` — the View component (owns sidebar + viewport layout).
- `views/<view>/pages/<Page>Page.tsx` — one file per Page (viewport content).
  e.g. `views/sessions/pages/ChatPage.tsx`, `views/sessions/pages/AgentsPage.tsx`.
- `components/` — cross-View shared primitives (the `.ol-*` wrappers, if any).
- The old `ChatView.tsx` is replaced by `SessionsView.tsx` + its Pages. Delete
  the old file when the View lands.

---

## The 5 Views (this milestone) — routes

All routes are URL-persistent (TanStack Router). Active View = topbar chip;
active Page = left-sidebar NavItem. Both live on the URL.

| View | View route | Pages (sidebar NavItems) | Backend |
|---|---|---|---|
| **Sessions** | `/sessions` | `/sessions` (list/empty), `/sessions/$id` (chat), `/sessions/agents`, `/sessions/usage` | `/api/sessions*`, `/api/sessions/:id/messages`, `/ws`, POST/PATCH/fork/cancel |
| **Vaults** | `/vaults` | `/vaults` (vault picker), `/vaults/$vaultId` (note), `/vaults/$vaultId/tables`, `/vaults/$vaultId/graph` | `/api/vaults*` ✅ |
| **Projects** | `/projects` | `/projects` (default board), `/projects/$boardId` | `/api/cards*` ✅ |
| **Fleet** | `/fleet` | `/fleet` (grid), `/fleet/$nodeId` (drill-in) | `/api/nodes` ✅ |
| **Settings** | `/settings` | `/settings/appearance`, `/settings/models`, `/settings/runtime` | `/api/health`, `/api/agents`, `/api/models` |

---

## Cards (dependency-ordered)

### F0 — Design-system foundation + AppShell (BLOCKS all UI cards)
Adopt the Instrument system and lock the shell contract so views parallelize.
- Import `ui/src/design/tokens/*.css` + `ui/src/design/styles/components.css`
  into the app (via `main.tsx` or `index.css` `@import`). Keep the existing
  `index.css` only for app-shell layout that isn't covered by `.ol-*`; migrate
  colors to tokens.
- `ThemeProvider` (React context) → reads/writes `localStorage["olympus-theme"]`,
  sets `document.documentElement.dataset.theme`. Default `obsidian`. Themes:
  `obsidian` (dark) + `light`.
- Rebuild `AppShell.tsx`: left icon rail with the 5 surfaces (order above) using
  `.ol-nav`; collapse toggle; top bar (search ⌘K, notifications, org chip,
  profile avatar); a per-surface secondary sidebar slot (each view supplies its
  own left list — sessions list, vault tree, board list, etc.).
- Add TanStack routes for `/vaults`, `/projects`, `/fleet`, `/settings` (+ the
  nested id routes). Placeholder panes for surfaces whose card hasn't merged yet
  (render `.ol-*` header + empty state — NOT a blank screen).
- **Done =** all 5 nav items render a real (if placeholder) `.ol-*` screen; theme
  toggle works live; `bun run typecheck` + `bun run build` + existing e2e green.

### S1 — Sessions workbench (the IDE-grade chat) — depends F0
Match `docs/design/concept/olympus-app-concept.html` Sessions view.
- Left session list (secondary sidebar): PINNED / RECENT / OBSERVED groups,
  liveness dot, model pill, message count + age. New session button (model
  picker → managed olympus session via `POST /api/sessions`).
- Center transcript: user / agent / tool messages; reasoning blocks; **tool-call
  cards** (name + args summary, collapsible); **diff cards** (patch tool → render
  unified diff with file path header); per-message Copy + "Fork from here"
  (`POST /api/sessions/:id/fork`). Stream via `/ws` (`message.delta`/`.done`/
  `.appended`).
- Composer: textarea, model/agent picker (`/api/agents`), access-mode pill
  ("Full access"), send. Live for managed sessions; observed sessions show a
  Fork CTA instead.
- Right panel (tabbed, collapsible): Overview / Outline / Settings / Git / Diff /
  AI. **Session outline** (headings/turns) + **session context** pane (todo list,
  git branch, PR) as in the concept. Bottom panel (collapsible): Terminal /
  Output / Debug tabs showing session tool output.
- Wire the "SESSION CONTEXT" (todo/git/PR) and outline to real data where it
  exists; mock-shape (typed in `types.ts`) where the backend doesn't emit it yet,
  behind a clearly-labelled fixture so it flips to real with no layout change.
- **Done =** operator can open any session, read the full transcript with
  tool/diff cards, start a new managed chat and stream a reply, fork an observed
  session — all matching the concept layout. Browser e2e + screenshot.

### P1 — Projects board (durable kanban) — depends F0
Match the concept Projects view.
- Secondary sidebar: BOARDS list (multi-board; `default` + others), New card,
  filters (by agent, blocked only).
- Columns BACKLOG / ACTIVE / REVIEW / DONE mapped from `CardStatus`
  (`todo`→backlog; `assigned`+`claimed`→active; `blocked`→review;
  `done`→done — confirm mapping against `types.ts` `CardStatus` and adjust the
  column model, don't invent statuses). Card rows: title, assignee, status tag.
- Card detail pane on click: assign / claim / block / complete
  (`/api/cards/:id/*`), and **open the 1:1 worker session** (`currentSessionId`
  → navigate to `/sessions/$id`). Create card (`POST /api/cards`).
- Live updates via `/ws` `cards.changed` → refetch.
- **Done =** operator manages a live board; moving/assigning a card updates
  instantly; clicking a card opens its worker session. Browser e2e.

### N1 — Fleet view (nodes) — depends F0
Match the concept Fleet view; wire to the REAL `/api/nodes` (already shipped via
UDS node registration).
- Grid of node cards (`.ol-card`): nodeId, hostname, status dot
  (online/draining/offline), slots used/total (`.ol-bar`), version, local badge,
  last-heartbeat-ago. "Add node" affordance (can be a doc/help popover for now —
  registration is UDS-side).
- Click a node → drill-in: its running sessions (filter `/api/sessions` by
  `node`), slot detail.
- Replace the current mock `FleetView.tsx` with the real endpoint; keep the
  `NodesResponse`/`NodeInfo` contract in `types.ts`.
- **Done =** fleet grid renders live nodes from `/api/nodes`; heartbeat + slots
  correct; drill-in lists that node's sessions. Browser e2e.

### ST1 — Settings — depends F0
Match the concept Settings view.
- Appearance: theme switcher (obsidian/light), density toggle (comfortable/
  compact) → both persist to localStorage and apply live.
- Model routing: list drivable agents (`/api/agents` → profile, provider, model)
  and available models (`/api/models`); show the default agent; allow selecting
  the default model for new olympus sessions (persist to localStorage; the
  Sessions composer reads it).
- Runtime: install token reveal/copy (from health/env — do NOT print secrets to
  logs), hermes profile (`/api/health`), server version.
- **Done =** theme + density + default-model persist and apply live; agent/model
  routing shows real data from `/api/agents` + `/api/models`. Browser e2e.

### V-BE — Vaults backend (Epic K core, jj markdown) — no F0 dep, start now
ADR 0004: **markdown-first + jj merge.** MVP scope (single-node; no iroh/cr-sqlite
sync yet — that's Epic K/L later):
- Vault storage under `~/.olympus/<org>/vaults/<vaultId>/` — a **jj-colocated**
  git repo of `.md` files + YAML frontmatter. One vault = one jj repo.
- REST (add to `crates/control-plane/src/server/mod.rs`, DTOs in `dto.rs`,
  update `docs/api-contract.md`):
  - `GET /api/vaults` → list vaults `{ id, name, noteCount, updatedAt }`.
  - `POST /api/vaults` `{ name }` → create vault (jj init + colocate).
  - `GET /api/vaults/:id/notes` → the note tree (folders + `.md` files,
    path + title + updatedAt).
  - `GET /api/vaults/:id/note?path=...` → `{ path, title, markdown, frontmatter,
    linkedNotes }` (parse `[[wikilinks]]` / `· `-separated links into
    `linkedNotes`).
  - `PUT /api/vaults/:id/note?path=...` `{ markdown }` → write `.md` (+ jj
    snapshot commit); returns updated note.
  - `DELETE /api/vaults/:id/note?path=...`, and rename via PUT of a new path.
- All under the existing auth gate. Register routes BEFORE the proxy catch-all.
- Tests: hermetic (temp dir vault); create → write → read round-trips; jj commit
  lands; tree reflects new note. `#[ignore]` any test needing a real `jj` binary
  if not in CI, and document the manual gate.
- **Done =** `cargo test` green for vault module; a vault can be created, notes
  written/read/listed, each write producing a jj snapshot; contract doc updated.

### V-UI — Vaults view — depends F0 **and** V-BE
Match the concept Vaults view (Obsidian-like).
- Secondary sidebar: vault picker + NOTES tree (folders `redb`, `runbooks`…),
  VIEWS (Graph / Tables — Graph can be a stub panel this milestone), New doc.
- Center: open-note tabs; markdown editor. Render markdown richly (headings,
  tables, code blocks, lists) and serialize to `.md` on save
  (`PUT .../note`). Use a lightweight markdown editor (e.g. render with the
  existing `react-markdown` for view + a textarea/CodeMirror for edit; a full
  WYSIWYG is a later polish — a solid view+source-edit toggle is acceptable for
  "usable"). Show LINKED NOTES footer.
- Right panel: "VAULT AGENT" — agent/model/scope, recent activity, related
  notes, "Ask about this vault…" composer (can post to a managed session scoped
  to the vault — reuse `POST /api/sessions`; wiring the scope can be a follow-up,
  but the panel must render).
- **Done =** operator can browse a vault's note tree, open + edit + save a
  markdown note (persisted via V-BE, jj snapshot lands), see linked notes and the
  vault-agent panel. Browser e2e + screenshot.

---

## Gates (controller runs on the merged tree — never trust self-report)
- Rust: `cargo test --workspace` + `cargo clippy --all-targets -- -D warnings` +
  `cargo fmt --check`.
- UI: `bun run typecheck` + `bun run build` + `bun run test:e2e`.
- Real browser e2e + screenshot vs `docs/design/concept/olympus-app-concept.html`
  for every UI card.
- Adversarial source-review before any new Hermes/jj-integration code (V-BE).

## Standing rules (don't re-derive)
1. Event log / state.db is read-only to Olympus except via proven patches.
   Cross-channel continuation is a FORK, never in-place edit.
2. DTO layer (`server/dto.rs`) is the only place view rows become wire JSON
   (camelCase). Contract changes update `docs/api-contract.md` + both sides.
3. Auth-gate all `/api/*` + `/ws`; bind 127.0.0.1; register local routes before
   proxy catch-all.
4. Only `var(--token)` colors in components. Add tokens to every theme block.
5. Build only these 5 surfaces. Do not build Workflow/Plugins/Console/Ledger/Atlas.

## Bug-fix backlog (operator-reported, against the old shipped ChatView)

These are concrete defects found while testing the live UI. Most are symptoms
of the old `ChatView.tsx` still being served instead of the View/Page rebuild
(S1). Fix them as part of the Sessions View rebuild — S1 must address each:

1. **Fork warning modal not visible when chat is long** — the modal must use a
   fixed-position overlay (`.ol-overlay` + `.ol-dialog`), not inline, so it
   floats above a long scrollable transcript. Test with a 200+ message session.
2. **Composer missing agent/model/thinking selector** — the composer needs
   three controls: **Agent** (`/api/agents`), **Model** (inferred from agent,
   overridable via `/api/models`), **Thinking** toggle (on/off; if the backend
   exposes a thinking/reasoning mode flag, wire it; else store in localStorage
   and pass with the send). Match the concept composer.
3. **"ACP and CLI" on configured agents — what's the difference?** Remove the
   confusing dual-label. An agent is a Hermes profile; it is driven over ACP.
   There is no separate "CLI" agent type. Show ONE label: the profile id +
   provider (e.g. `coding-agent · openai-codex`). If a backend field implies a
   transport, drop it from the UI.
4. **Claude and Codex missing from agent list** — the LIVE `/api/agents` already
   returns them (default=claude-opus, coding-agent/gpt55=codex gpt-5.x,
   glm52=zai). The bug is the UI not fetching `/api/agents`. Wire it.
5. **Dot on inactive session** — remove the idle liveness dot from session
   rows. Show a dot ONLY when `liveness === "active"` (a turn is in-flight).
   Idle/unknown = no dot.
6. **No right sidebar and bottom panel** — the Sessions View viewport layout
   MUST include the right sidebar (tabbed) + collapsible bottom panel. This is
   View-owned layout, not Page content. (Part of S1's scope.)
7. **Chat feature not working — sent message not visible, no agent status** —
   TWO root causes confirmed by the controller:
   (a) `createSession()` reads `.session.id` from the response, but the backend
       returns the session **flat** (no `session:` wrapper). Fix the helper to
       read the flat `id`.
   (b) `sendMessage` returns 202 immediately; the UI must show the user's
       message **optimistically** (append to transcript before the server
       round-trip) and surface agent status (idle → thinking → streaming →
       done) via `/ws` frames. Currently the UI waits for a message.done that
       may not arrive if the runtime is slow; show "thinking…" immediately.
8. **Tool output not formatted** — tool messages need real rendering: a
   **collapsible dropdown** with the tool symbol + tool name + execution time
   in the header; expanding shows the input parameters and the output (syntax-
   highlighted where possible). See bug 9.
9. **Remove "assistant" / "tool" header per message bubble** — styling alone
   conveys role (user = right-aligned or distinct bg; agent = left/default).
   Tool calls render as a collapsed dropdown (icon + `toolName` + `Xms`),
   expandable to show args + result. No text header line.

10. **Session creation should ask which agent to use** — the "New Session"
    action opens an **agent picker** (list from `/api/agents`); selecting an
    agent creates the managed session bound to that profile (POST
    `/api/sessions` with `{ agent }`), which infers the node to run it on.
    Later: the Agents NavItem/Page configures which agents appear in this
    picker. For now, show all agents from `/api/agents`.

### AMENDMENT to bug 2 (agent is LOCKED at creation):

**Bug 2 (revised):** The composer's model configuration is
`( agent logo | model | thinking level )` — NOT an agent picker.
- **Agent is locked** once a session is created. The session's bound agent
  (Hermes profile) is immutable. Show the agent's LOGO (not a picker) — a
  brand glyph: Hermes, Claude (Anthropic), or ChatGPT/Codex (OpenAI), inferred
  from the provider field.
- The only configurable controls in the composer are: **model** (selectable,
  scoped to the locked agent's available models) and **thinking level**
  (on/off or low/medium/high — store in localStorage, pass with send).
- Do NOT show an agent selector inside an open session. Agent selection happens
  ONLY at session-creation time (bug 10).

### Round 2 bugs (operator-reported after S1 merge):

11. **Composer is inside the bottom panel, not the chatbox** — the composer
    must be rendered INSIDE the chat column (chatcol), directly below the
    transcript. The bottom panel is a SEPARATE sibling below the chatcol
    (transcript + composer). Fix the layout: `chatcol = transcript(flex:1) +
    composer(fixed height)` then `rz-y` then `bottompanel`. The composer must
    NEVER appear inside or below the bottom panel.

12. **Bottom panel hide button not working** — the toggle must actually
    collapse/expand the panel (state already exists; verify the onClick wires
    to `setBpCollapsed`). ALSO: the bottom panel should show ONLY two tabs:
    **Logs** (Olympus server logs — streamed or tailed) and **Terminal**
    (tabbed, session-persistent via tmux or equivalent; user can add/remove
    terminals). Remove the "Output" and "Debug" tabs. Terminal starts at the
    session's workdir.

13. **Favicon not using Olympus mark** — create an SVG favicon from the Olympus
    mountain glyph (`<svg viewBox="0 0 24 24"><path d="m4 19 8-12 8 12"/><path
    d="m8.5 19 3.5-5.5L15.5 19"/></svg>`), wire it in `index.html` as
    `<link rel="icon" type="image/svg+xml" href="/olympus.svg">`. Put the same
    glyph on the **left sidebar toggle button** (replacing the panel-left icon).

14. **Session outline + context show fake data** — replace ALL mock data with
    **"Coming soon…"** placeholders. Never show fabricated data (todos, git
    branch, PR) — it confuses users into thinking it's real. When the backend
    emits this data, wire it; until then, a labelled placeholder.

15. **Agent type icons missing** — show a brand logo per provider:
    - **Hermes** profiles → Hermes glyph
    - **Anthropic/Claude** → Claude/Anthropic logo
    - **OpenAI/Codex/ChatGPT** → OpenAI/ChatGPT logo
    - **ZAI** → a generic or ZAI glyph
    In the composer (agent logo | model | thinking), in the session header,
    and in the Fleet view (each node's bound agent shows its logo).

16. **Light/dark toggle icon is wrong** — use a **sun** icon for "switch to
    light" and a **moon** icon for "switch to dark" (the universal convention).
    Currently it uses globe/sparkles icons.

17. **Resize bars not working** — `.rz-x` and `.rz-y` are styled but have NO
    drag handler. Implement a `useResizable` hook (mousedown → mousemove tracks
    delta → updates a CSS var or state for the panel width/height). Apply to:
    left sidebar ↔ viewport (`.rz-x`), viewport ↔ right sidebar (`.rz-x`),
    chatcol ↔ bottom panel (`.rz-y`). Panels must have sensible min/max.

18. **Send still not working (no chat bubble, no agent status)** — verify the
    full chain: `createSession()` reads the FLAT response `id` (not
    `.session.id`); the optimistic message (bug 7b) renders as a visible
    bubble; `agentStatus` transitions render a "thinking…" indicator. If the
    runtime is slow, the "thinking…" indicator must stay until the first
    `/ws delta` or a timeout. Test end-to-end with a real session.

---

## Status Ledger (the swarm updates this; controller verifies before marking done)
| Card | Assignee | Status | Commit | Notes |
|---|---|---|---|---|
| F0 design-system + shell | glm52 | DONE | `ec7f00d` | + controller fixes `d30012a` (button reset), `3a5fd5e` (topbar selector) |
| V-BE vaults backend | gpt55 | DONE | `88e00ee` | jj markdown vault, 235 tests |
| S1 sessions workbench | glm52 | DONE | `e0db113` | merged as monolith; S2 refactors |
| S2 Sessions View/Page + bugs 1-18 | glm52 | RUNNING | (wt) | View/Page refactor + all bug fixes |
| V2 Vaults View | glm52 | RUNNING | (wt) | |
| P2 Projects View | gpt55 | RUNNING | (wt) | |
| N2 Fleet View | gpt55 | RUNNING | (wt) | |
| ST2 Settings View | gpt55 | RUNNING | (wt) | |


