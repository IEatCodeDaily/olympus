# Olympus UI & Feature Roadmap

> Companion to `2026-06-29-olympus-long-horizon-roadmap.md` (the backend epics).
> This is the **UI/UX path**: what screens exist, what they show, the theme
> system, and the order to build them. Designed so each screen is an independent
> view file that a worker can build in parallel without colliding (the app shell,
> nav, routing, and theme are the locked contract — owned by the controller).

**Goal:** A comfortable, theme-addressable, functional operator cockpit for the
whole Olympus control plane — not just session browsing, but agent/task
management, fleet (nodes), workflows, and usage.

**Design language (locked):** Linear-meets-terminal. Dark default, calm, dense,
editorial. IBM Plex Sans + Mono. Electric-cyan accent (#7dd3fc). NOT generic AI
slop (no Inter, no purple gradients, no Material bootstrap). Theme-addressable:
all color/spacing via CSS variables under `[data-theme]`, switchable at runtime.

---

## Information architecture (the nav)

The left sidebar is the primary nav. Each item is a top-level view:

| Nav item | View file | Purpose | Backend |
|---|---|---|---|
| **Sessions** | `views/SessionList` + `ChatView` | unified cross-channel history; open/continue/new chat | `/api/sessions`, `/ws` ✅ |
| **Search** | `views/SearchView` | full-text across all sessions | `/api/search` ✅ |
| **Board** | `views/BoardView` | kanban: cards by status, assign/claim/block/complete, 1:1 to worker session | `/api/cards*` ✅ (C1 merged) |
| **Nodes** | `views/NodesView` | fleet: registered nodes, heartbeat, slots, health | `/api/nodes` (TODO backend) |
| **Workflows** | `views/WorkflowsView` | durable workflow graphs (code-review loop etc.), runs, status | `/api/workflows` (TODO backend) |
| **Usage** | `views/UsageView` | budget/subscription/token spend per model/provider/context | `/api/usage` (TODO backend) |
| **Settings** | `views/SettingsView` | theme switcher, profile, model routing, token, prefs | local + `/api/health` |

Sessions + Search exist. Board has a live backend already. Nodes/Workflows/Usage
get **placeholder views now** (real-looking mock data + the layout), wired to
real endpoints as the backend epics (D/H/I/L) land.

---

## Theme system (the "theme-addressable" requirement)

- All visual tokens are CSS custom properties scoped to `:root[data-theme="..."]`.
- Ship **3 themes** at minimum: `midnight` (current dark default), `daylight`
  (light), `amber-crt` (warm terminal). Each redefines the same ~20 variables.
- A `ThemeProvider` (React context) reads/writes `localStorage["olympus-theme"]`
  and sets `document.documentElement.dataset.theme`. Default `midnight`.
- The Settings view hosts the switcher; a quick toggle also lives in the sidebar
  footer.
- HARD RULE for all view workers: **never hardcode a hex color** — only
  `var(--token)`. A new color need = add a token to every theme block.

---

## Component conventions (so views feel consistent)

Shared primitives live in `ui/src/components.tsx` (extend, don't duplicate):
- `PageHeader` (title + actions row), `Card`/`Panel`, `Badge` (status pills),
  `EmptyState` (icon + message + optional CTA), `SkeletonRows` (loading),
  `Toolbar` (filters/search/sort), `StatPill` (metric chips).
- Every view: PageHeader → Toolbar (if filterable) → content (list/grid/board)
  → EmptyState when empty → SkeletonRows while loading. Real browser e2e gate.

---

## Build order (UI)

**Phase U0 — App shell + theme + placeholders (CONTROLLER, do first).**
Expand nav to all 7 items; add routing for the 5 new views; build the theme
system (3 themes + provider + switcher); create placeholder stubs for Board,
Nodes, Workflows, Usage, Settings (each renders PageHeader + EmptyState +
representative mock layout). This locks the contract so the rest parallelizes.
**Done =** all 7 nav items render a real (if placeholder) screen; theme switch
works live; `bun run build` + e2e green.

**Phase U1 — Board (parallel, real backend).** Flesh out `BoardView`: columns
(todo/ready/running/blocked/done), card rows with assignee + source + age,
create-card, drag/click to assign/claim/block/complete, click → open worker
session. Wire to `/api/cards*`. **Done =** operator manages the live board in
browser.

**Phase U2 — Settings (parallel, mostly local).** Theme switcher UI, profile +
model-routing display (from `/api/health` + a future `/api/models`), token
reveal/copy, density toggle. **Done =** theme + prefs persist; switching is live.

**Phase U3 — Usage (parallel, mock→real).** Token/budget spend charts per
model/provider/context, subscription-limit bars, time-range filter. Mock data
until `/api/usage` (Epic I) lands; flip to real with no layout change. **Done =**
usage dashboard renders mock data convincingly; swaps to real via one flag.

**Phase U4 — Nodes (parallel, mock→real).** Fleet grid: node cards (id, status,
slots used/free, last heartbeat, runtime), drill-in to running sessions. Mock
until `/api/nodes` (Epic L). **Done =** node grid renders; live when backend lands.

**Phase U5 — Workflows (parallel, mock→real).** Workflow list + run history +
a simple DAG/step view (the code-review loop). Mock until `/api/workflows`
(Epic H). **Done =** workflow + runs render; live when backend lands.

**Phase U6 — New Chat / Fork polish (depends on A2/A3).** New Chat button →
model picker → managed olympus session; Fork action on observed sessions. Wire
to the now-merged POST endpoints. **Done =** start + continue a chat fully in UI.

U1/U2 are real-backend now. U3/U4/U5 are mock-first (placeholder data shaped
like the future contract) so they're built and demoable before their backend
epics land — then flip a flag. Each Ux after U0 is one worker, one view file.

---

## Mock-first contract discipline

For views whose backend doesn't exist yet (Nodes/Workflows/Usage): define the
TypeScript types in `types.ts` first (the contract), add MSW handlers with
realistic fixtures in `mocks/`, build the view against the mock. When the backend
epic lands, the real endpoint returns the same shape → flip `VITE_USE_MOCKS` /
swap the fetch, zero component change. (Same pattern that worked for the original
session UI.)

---

## Status Ledger (UI)

| Phase | Screen | Status | Notes |
|---|---|---|---|
| U0 | shell + theme + placeholders | DONE | 7-view nav, 3 themes, live switch (5402da7) |
| U1 | Board | DONE | wired to live /api/cards (09da64c) |
| U2 | Settings | DONE | theme switcher + density + token/routing (a0576ca) |
| U3 | Usage | DONE | mock-first, /api/usage contract (c68db53) |
| U4 | Nodes | DONE | mock-first, /api/nodes contract (c68db53) |
| U5 | Workflows | DONE | mock-first, /api/workflows contract (c68db53) |
| U6 | New Chat / Fork polish | TODO | depends A2(done)/A3 |

**Design system:** owned by the `design-lead` (opus) agent — `docs/design/DESIGN_SYSTEM.md`
+ `docs/design/VISION.md` (cacb2af). Runs every 3h to keep improving the system +
the live UI against the north star (cron olympus-design-lead).
