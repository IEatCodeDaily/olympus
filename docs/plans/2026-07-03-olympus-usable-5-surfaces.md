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

## The nav (this milestone)

Rebuild the left rail to exactly these five surfaces (in order), plus the
existing top-bar search (⌘K) and org/profile chip:

| Surface | Route | View file | Backend | Status |
|---|---|---|---|---|
| **Sessions** | `/` · `/sessions/$id` | `views/SessionsWorkbench.tsx` | `/api/sessions`, `/api/sessions/:id/messages`, `/ws`, POST/PATCH/fork/cancel | backend ✅ |
| **Vaults** | `/vaults` · `/vaults/$vaultId/$notePath` | `views/VaultsView.tsx` | `/api/vaults*` (NEW — card V-BE) | backend ❌→V-BE |
| **Projects** | `/projects` · `/projects/$boardId` | `views/ProjectsView.tsx` | `/api/cards*` | backend ✅ |
| **Fleet** | `/fleet` | `views/FleetView.tsx` | `/api/nodes` | backend ✅ |
| **Settings** | `/settings` | `views/SettingsView.tsx` | `/api/health`, `/api/agents`, `/api/models` | backend ✅ |

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

## Status Ledger (the swarm updates this)
| Card | Assignee | Status | Commit | Notes |
|---|---|---|---|---|
| F0 design-system + shell | glm52 | TODO | | blocks all UI |
| V-BE vaults backend | gpt55 | TODO | | no F0 dep |
| S1 sessions workbench | glm52 | TODO (dep F0) | | |
| P1 projects board | gpt55 | TODO (dep F0) | | |
| N1 fleet view | glm52 | TODO (dep F0) | | |
| ST1 settings | gpt55 | TODO (dep F0) | | |
| V-UI vaults view | glm52 | TODO (dep F0+V-BE) | | |
