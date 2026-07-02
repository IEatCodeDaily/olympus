# Olympus Design System

> The visual language, tokens, components, and UI kits for **Olympus** — a
> local-first AI control plane for supervising an autonomous software-agent
> fleet.

This is the design language of the **shipped Olympus app** (the "Instrument"
direction): the app itself is the groundtruth, and everything here — tokens,
component classes, kits — is extracted from and kept consistent with it
(`ui_kits/olympus-app/index.html` is the canonical reference).

---

## 1. What Olympus is

Olympus is an **operating environment for AI-assisted development**: part
workspace manager, part agent command center, part observability console, part
durable project archive. A power user runs a fleet of autonomous agent sessions
across many channels (CLI, Telegram, Discord, web, cron, subagents, ACP) and
Olympus unifies them into one searchable, resumable cockpit.

Core surfaces (from the real product):

- **Sessions / Chat** — the master-detail list of agent sessions + a transcript
  view (user / AI / tool messages, reasoning, tool-call cards, diffs, a composer
  that can drive a managed session or fork a read-only observed one).
- **Board** — a durable Kanban task board (todo → ready → running → blocked → done)
  wired to live card events, with an assign/claim/block/complete detail pane.
- **Nodes** — fleet view: heartbeat, slot capacity, runtime posture per node.
- **Workflows** — multi-step agent workflows (coder → reviewer → validator → merge)
  with run history and step lanes.
- **Usage** — token/cost accounting per model and subscription-limit bars.
- **Search** — full-text search across all sessions, grouped by source.
- **Settings** — theme, density, runtime token, model routing.

### Sources (store for reference; reader may not have access)

- **GitHub:** `https://github.com/IEatCodeDaily/olympus` (branch `main`).
  - `ui/src/index.css` — the *previous* design system's tokens (intentionally
    not carried forward here).
  - `ui/src/views/*.tsx`, `ui/src/components/shell.tsx`, `ui/src/types.ts`,
    `ui/src/mocks/fixtures.ts` — product structure, real data shapes, and
    realistic sample content this kit reuses.
  - `docs/design/DESIGN_SYSTEM.md`, `docs/design/VISION.md` — the old design
    doctrine + north-star (product feeling: "a mission-control console for your
    own AI fleet — calm, dense, trustworthy, quietly powerful").
  - `docs/adrs/`, `docs/prd/`, `docs/plans/` — architecture and product intent.

---

## 2. Content fundamentals — how Olympus writes

- **Voice:** terse, technical, operator-to-operator. Confident, never chatty.
  It respects that the reader is a power user.
- **Person:** addresses the user as **you** ("Fork it to continue from Olympus",
  "Message this session…"). The product refers to itself as **Olympus**, not "we".
- **Casing:** **sentence case** for all prose, buttons, titles, and nav ("New
  session", "Fork to continue", not "New Session"). The one deliberate exception
  is **UPPERCASE mono micro-labels** — field labels, badges, column titles,
  kickers (`CONTROL PLANE`, `RUNNING`, `SELECTED NODE`). That caps/mono contrast
  is the signature editorial-terminal move.
- **Status is honest, not cheerful.** States are named plainly: `running`,
  `blocked`, `draining`, `observed`, `idle`, `failed`. Errors are direct and
  specific ("WebSocket disconnected — retrying"), never apologetic or jokey.
- **Numbers are mono + abbreviated:** `42.4k tok`, `p95=230ms`, `$27.48`,
  `2m ago`, `5 / 6 slots`. Timestamps are relative ("just now", "3h ago", "Jun 24").
- **No emoji.** No exclamation marks in UI copy. No marketing adjectives.
- **Terminology:** *session* (an agent conversation), *managed* vs *observed*
  (Olympus-driven vs read-only imported), *fork* (never edit a source session in
  place — forking is the continuation primitive), *node* (a runtime host),
  *slot* (a concurrency unit), *source/channel* (origin: cli, telegram, acp…).

Example copy, in-voice:

> **This is an observed telegram session — read-only. Fork it to continue from
> Olympus.**   ·   **New session**   ·   **Stop agent**   ·   **runtime live ·
> binding locked**

---

## 3. Visual foundations — "Instrument"

The feeling: **a precision instrument in a dark cockpit.** Monochrome-forward,
one signal color, sharp edges, flat depth, confirmatory motion. Nothing pops
that doesn't carry meaning.

### Color & theme
- **Dark-first** (`obsidian`, default): a neutral near-black canvas (`--bg #0A0A0B`),
  no color undertone. Depth via **surface layering** (`bg → bg-elev →
  bg-elev-2 → bg-hover → bg-active`), plus soft dark shadows on floating chrome.
- **Light** (`daybreak`) is a first-class secondary (`[data-theme="light"]`).
- **One signal accent**, swappable via 5 lines in `tokens/colors.css`. Default is
  **silver** (`--accent #C9C9C9`) — primary actions, selection, and focus read as
  polished metal, not a hue. Semantic **green/amber/red** are soft pastels
  (`#86EFAC / #FCD34D / #FCA5A5`) held deliberately apart from the accent so
  status never reads as brand.
- **Source hues** are low-chroma channel-identity dots (wayfinding, not decoration).
- Imagery/vibe: cool, near-black, high-contrast, no photography-led surfaces —
  this is a tool, not a marketing site. No gradients as decoration.

### Accent — locked: silver
The shipped app uses **silver** (`#C9C9C9`) — this is the groundtruth and the
default. The accent stays isolated in 5 `--accent*` lines in `tokens/colors.css`
so it remains swappable, but silver is the decided brand signal: monochrome UI,
where the "color" of action is light itself.

### Type
- **IBM Plex Sans** — all UI + body; also display/brand at weight 600.
- **IBM Plex Mono** — data, meta, timestamps, code, badges, ids, micro-labels.
- Hierarchy is carried by **size + weight**, not color. UI default is 14px;
  mono micro-labels go down to **9–10px** (uppercase, letter-spaced).
- Tracking: `-0.02em` on display/titles; `0.05em` (~.5px) on caps mono labels;
  `0.1em` on the smallest eyebrow kickers. Body sets no tracking.

### Shape, elevation, motion
- **Softly squared:** corners are `3–8px` — 3–4px controls, 6px cards/menus,
  8px dialogs/composer. True pills (999px) for chips, search, org pill, dots.
- **Flat-ish elevation:** layered surfaces + hairline borders carry depth. Soft
  dark shadows (`--shadow-pop` / `--shadow-modal` / `--shadow-float`) are used
  **only** on floating chrome (menus, dialogs, palette, composer, toasts).
- **Borders** are the primary depth cue: `--border` (dividers/edges),
  `--border-strong` (focus/active/emphasis), `--border-faint` (internal rules).
- **Hover:** surfaces step to `bg-hover`; interactive cards brighten border +
  lift `translateY(-1px)`; icon buttons gain a `bg-hover` wash and text brightens
  dim → text.
- **Press/active:** step to `bg-active` (no scale-down — instruments don't bounce).
- **Selected:** a **silver wash** — `--accent-wash` background (+ `--accent-line`
  border on chips/pills); selected text goes silver. No left rails, no rings.
- **Focus:** a single global 2px accent `:focus-visible` ring, 2px offset,
  keyboard-only (zero-specificity `:where()` rule — never collides).
- **Motion is confirmatory, ≤150ms.** `--dur-fast 80ms` (hover), `--dur 120ms`
  (nav/borders), `--dur-slow 150ms` (buttons/cards/dialogs). Looping animations
  (live-dot pulse 1.6s, spinner 0.8s, skeleton shimmer 1.3s, streaming blink
  1.5s) own their rhythm. Reduced-motion collapses everything to a static frame.
- **Transparency/blur:** used sparingly — the dialog scrim (`--overlay` + a 2px
  backdrop blur) and token washes (`--accent-wash`, `--*-wash`). No glassmorphism.

### Density
- **Layout constants mirror the app:** topbar 34px, view headers 32px, tab strips
  30px, left sidebar 220px, right sidebar 279px, bottom panel 152px, transcript
  measure 760px (messages cap at 640px). All in `tokens/spacing.css`.
- **Density** toggles via `[data-density="compact"]` on `<html>` — retunes layout
  constants tighter. Font sizes, radii, and colors are unchanged.

---

## 4. Iconography

- **Style:** Lucide-compatible **stroke icons** — `24×24` viewBox, `stroke-width:2`,
  `stroke="currentColor"`, `fill="none"`, round joins/caps (matching the app's
  inline icon set). Icons inherit color from their context and never carry
  meaning alone
  (every colored state has a text label).
- **Delivery:** inline SVG in components and kits (the source product does the
  same — no icon font, no PNG icons, no sprite sheet). For consuming projects,
  pull the matching set from **Lucide** (`https://lucide.dev`, CDN
  `https://unpkg.com/lucide-static`) so stroke weight and geometry stay consistent.
- **Emoji:** never. **Unicode glyphs as icons:** only the tool-call status marks
  reused from the product (`◌` running / `✓` done) and the send/stop composer
  glyphs, all rendered as SVG where possible.
- **Brand mark:** `assets/olympus-mark.svg` — a **placeholder** geometric "summit"
  (two nested sharp chevrons = a peak; Olympus). It uses `currentColor` so it
  themes. **This is not a real logo** — no brand mark was provided; replace it
  with the official asset when available. Never treat the placeholder as final.

---

## 5. Index / manifest

### Root
- `styles.css` — **the entry point.** `@import` manifest only. Consumers link this.
- `tokens/` — `fonts.css`, `colors.css`, `typography.css`, `spacing.css`,
  `radius.css`, `motion.css`, `base.css`.
- `styles/components.css` — the `.ol-*` class contract the React components render
  against (shipped through `styles.css`).
- `assets/` — `olympus-mark.svg` (placeholder brand mark).
- `guidelines/` — foundation specimen cards (Colors, Type, Spacing, Brand).
- `components/` — reusable React primitives (below).
- `ui_kits/` — full-screen product recreations (below).
- `SKILL.md` — Agent-Skill wrapper for downloaded use.

### Components (`window.OlympusDesignSystem_516a9b.*`)
- **core** — `Button`, `IconButton`, `Badge`, `StatusDot`, `Tag`, `Spinner`, `Kbd`
- **forms** — `Input`, `Textarea`, `Select`, `SearchInput`, `Checkbox`, `Radio`, `Switch`
- **data** — `Card`, `StatPill`, `ProgressBar`, `Avatar`, `Skeleton`
- **feedback** — `Dialog`, `Toast`, `Tooltip`
- **navigation** — `Tabs`, `NavItem`

Each component has a sibling `.d.ts` (props contract) and most a `.prompt.md`
(usage). Each directory has one `@dsCard` HTML showcasing states.

### UI kits (`ui_kits/`)
- `olympus-app/` — **the groundtruth.** `index.html` is the shipped app itself
  (sessions + transcript + composer with context/model popovers, vaults,
  projects, workflow, plugins, fleet, settings, ⌘K palette, bottom terminal
  panel, right info sidebar). Copy patterns from here first. `states.html` is a
  canvas of empty / loading / error states per section.

---

## 6. Caveats / open items

- **Accent is locked: silver** (`#C9C9C9`), matched to the shipped app. See §3.
- **Fonts are Google Fonts** (IBM Plex Sans, IBM Plex Mono),
  loaded via `@import` in `tokens/fonts.css`. No local binaries were provided;
  swap the `@import` for self-hosted `@font-face` if you need offline/pinned fonts.
  (Because they load via a remote `@import`, the compiler's "Fonts" count reads 0
  — the fonts still load at runtime.)
- **Brand mark is a placeholder** — replace `assets/olympus-mark.svg`.
