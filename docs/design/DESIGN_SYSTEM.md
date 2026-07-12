# Olympus Design System

> **Canonical reference for every visual decision in Olympus.**
> Owned by the `design-lead` agent.
>
> **Where the system lives (as shipped):**
> - **Tokens** — modular files under `ui/src/design/tokens/`
>   (`colors.css`, `typography.css`, `spacing.css`, `radius.css`, `motion.css`,
>   `fonts.css`, `base.css`), stitched by `ui/src/design/styles.css`
>   (an `@import` manifest, imported first in `main.tsx`).
> - **Canonical component classes** — `ui/src/design/styles/components.css`
>   (the `.ol-*` primitive library: button, badge, card, input, switch, …).
> - **App-shell + view vocabulary** — `ui/src/index.css` (the `.gv-*`, `.stat`,
>   `.gtag`, `.srow`, `.topbar`, … layout classes) plus a small **alias block**
>   at the top mapping legacy short names (`--silver`, `--green`, `--sans`) to
>   canonical tokens (`--accent`, `--ok`, `--font-sans`).
> - **React primitives** — `ui/src/components/shell.tsx`
>   (`PageHeader`, `EmptyState`, `StatPill`, `Badge`, `PlaceholderBadge`);
>   they emit the live `.gv-*`/`.stat`/`.gtag` vocabulary.
>
> **Rule: never hardcode a hex — only `var(--token)`.** A new visual need is a new
> token in `tokens/` (in **both** theme blocks) *first*, then referenced.

---

## Table of Contents

1. [Design Anchor](#1-design-anchor)
2. [Color Tokens](#2-color-tokens)
3. [Typography Scale](#3-typography-scale)
4. [Spacing & Layout](#4-spacing--layout)
5. [Radius & Elevation](#5-radius--elevation)
6. [Motion System](#6-motion-system)
7. [Density Modes](#7-density-modes)
8. [Component Inventory](#8-component-inventory)
9. [Accessibility](#9-accessibility)
10. [Do / Don't Gallery](#10-do--dont-gallery)
11. [Changelog](#11-changelog)

---

## 1. Design Anchor

**Linear-meets-terminal.** A calm, dense, editorial operator cockpit for a power
user running an AI agent fleet.

- **Dark default** (`obsidian` theme) — a **neutral near-black** cockpit, no blue
  undertone, monochrome-forward.
- **Typefaces:** IBM Plex Sans (UI/body) + IBM Plex Mono (data/code/meta).
- **Accent:** a **single swappable SIGNAL accent — SILVER (`#C9C9C9`)** by
  default. Actions, selection, links, and the focus ring read as *polished metal,
  not a hue*. Swap the 5 accent lines in `colors.css` to re-brand the whole
  system. Status colors (green/amber/red) are held **apart** from the accent so
  status never reads as brand.
- **Depth is mostly flat:** layered surfaces (`--bg` → `--bg-elev` →
  `--bg-elev-2`) + hairline borders carry elevation; only **floating chrome**
  (menus, palette, composer, toasts) leaves the plane with a soft dark shadow.
- **NOT:** generic AI slop — no Inter, no purple gradients, no glassmorphism, no
  Material Design.
- **Every visual value is a CSS custom property.** Color/alpha tokens are scoped
  per-theme (`:root[data-theme="…"]`); type/space/radius/motion scales are
  theme-agnostic (`:root`).

### Themes (2 shipped)

| Theme | `data-theme` value | Mood |
|-------|-------------------|------|
| **Obsidian** | `obsidian` (also bare `:root`) | Deep neutral dark, default & design target. Near-black `#0A0A0B` canvas, silver accent. |
| **Daybreak** | `light` | Clean neutral light. Off-white `#F6F6F7` canvas. Silver inverts to near-black ink (`#2A2A2E`). |

Managed by `ThemeProvider` (`ui/src/theme.tsx`): persists to
`localStorage["olympus-theme"]`, applies via `document.documentElement.dataset.theme`.
Extensible — add a new `:root[data-theme="…"]` block in `colors.css` redefining
every color/alpha token.

> **History:** earlier drafts of this doc described a *cyan* accent and three
> themes (`midnight`/`daylight`/`amber-crt`). The `b9a1b0e` redesign replaced
> that with the silver, two-theme Instrument palette shipped today. Those names
> are gone from the code — this section is the current truth.

---

## 2. Color Tokens

Defined per-theme in `ui/src/design/tokens/colors.css`. Reference as
`var(--token)`. Two theme blocks: `:root, :root[data-theme="obsidian"]` (dark,
default) and `:root[data-theme="light"]` (daybreak).

### Surfaces (backgrounds)

| Token | Obsidian | Daybreak | Usage |
|-------|----------|----------|-------|
| `--bg` | `#0A0A0B` | `#F6F6F7` | App canvas / deepest ground (viewport) |
| `--bg-elev` | `#131315` | `#FFFFFF` | Chrome: topbar, sidebars, panels, floating menus |
| `--bg-elev-2` | `#1B1B1D` | `#EFEFF0` | Cards, inputs, wells, secondary elevation |
| `--bg-hover` | `#232326` | `#E7E7E9` | Row / control hover |
| `--bg-active` | `#2A2A2E` | `#DDDDE0` | Pressed / selected wash |

### Borders (hairline; carry elevation)

| Token | Obsidian | Daybreak | Usage |
|-------|----------|----------|-------|
| `--border` | `#262629` | `#E2E2E5` | Default dividers + card edges |
| `--border-strong` | `#3A3A3E` | `#C8C8CD` | Emphasized: focus edges, active borders, popovers |
| `--border-faint` | `#1D1D1F` | `#ECECEE` | Barely-there internal rules |

### Text

| Token | Obsidian | Daybreak | Usage |
|-------|----------|----------|-------|
| `--text` | `#E6E6E6` | `#171719` | Primary — headings, body, values |
| `--text-dim` | `#999A9C` | `#5D5D60` | Secondary — meta, descriptions, labels |
| `--text-faint` | `#7E7E81` | `#6E6E72` | Tertiary — timestamps, hints, disabled copy (WCAG AA ≥4.5:1 on `--bg`) |

### Signal accent (SILVER — the single brand "color")

Swap these 5 lines to re-brand the whole system.

| Token | Obsidian | Daybreak | Role |
|-------|----------|----------|------|
| `--accent` | `#C9C9C9` | `#2A2A2E` | Primary action / selection / links / focus ring |
| `--accent-bright` | `#D8D8D8` | `#414146` | Hover |
| `--accent-press` | `#B5B5B5` | `#1B1B1D` | Pressed |
| `--on-accent` | `#0B0B0B` | `#FFFFFF` | Text/glyph on a solid accent fill |
| `--accent-ink` | `#C9C9C9` | `#2A2A2E` | Accent text on surfaces (links, active nav) |

**Accent alpha-derived** (theme-correct tints — never hardcode `rgba()`):

| Token | Purpose |
|-------|---------|
| `--accent-subtle` | Whisper fill (badges on dark, faint accent surfaces) |
| `--accent-wash` | Selected-row / soft accent fill (active nav, chip, card-selected) |
| `--accent-wash-2` | Stronger accent fill / hover tint |
| `--accent-line` | Decorative accent border |
| `--accent-glow` | Live-dot pulse / focus glow |

### Semantic status — soft pastels, held apart from accent

Each has a base ink + `-ink`/`-wash`/`-line` derivatives.

| Base token | Obsidian | Daybreak | Role | Derivatives |
|-----------|----------|----------|------|-------------|
| `--ok` | `#86EFAC` | `#15803D` | Success / running / healthy / live | `--ok-ink`, `--ok-wash`, `--ok-line` |
| `--warn` | `#FCD34D` | `#A16207` | Warning / draining / needs-attention | `--warn-ink`, `--warn-wash`, `--warn-line` |
| `--err` | `#FCA5A5` | `#B91C1C` | Error / blocked / failed / stop / offline | `--err-ink`, `--err-wash`, `--err-line` |

### Source hues (channel identity in the session list)

Low-chroma per-origin dots/labels — wayfinding, not decoration.
`--src-olympus`, `--src-cli`, `--src-telegram`, `--src-discord`, `--src-webui`,
`--src-cron`, `--src-subagent`, `--src-api`, `--src-acp` (each defined in both
themes).

### Utility

| Token | Purpose |
|-------|---------|
| `--overlay` | Dialog / palette scrim |
| `--hover-veil` | Faint row wash |
| `--scrollbar` / `--scrollbar-hover` | Scrollbar thumb (idle / hover) |
| `--track` | Progress / slot-bar track |
| `--selection` | `::selection` background |

> **`index.css` alias layer:** the app-shell CSS references legacy short names
> (`--chrome`, `--elev`, `--silver`, `--green`, `--amber`, `--red`, `--sans`,
> `--mono`, …). These are **aliases** defined at the top of `index.css` that map
> to the canonical tokens above (`--silver → --accent`, `--green → --ok`, etc.).
> New work should use the **canonical** tokens; the aliases exist only so the
> large body of shipped shell/view CSS keeps theming correctly.

---

## 3. Typography Scale

Defined in `ui/src/design/tokens/typography.css`. Fonts loaded via
`tokens/fonts.css`.

| Token | Value | Usage |
|-------|-------|-------|
| `--font-display` | IBM Plex Sans, system fallback | Brand, page titles, big numerics |
| `--font-sans` | IBM Plex Sans, system fallback | All UI + body |
| `--font-mono` | IBM Plex Mono, `ui-monospace` | Data, meta, timestamps, code, badges |

### Font size scale (12 steps, role-named by pixel value)

The scale is **dense**: 14px body / 12–12.5px controls; mono micro-labels drop to
9–10px (uppercase, letter-spaced — the signature editorial-terminal move).
Hierarchy is carried by **size + weight**, not color.

| Token | Size | Role |
|-------|------|------|
| `--fs-9` | 9px | Micro mono: kbd hints, tiny meta, row counts |
| `--fs-10` | 10px | Mono labels: section headers, badges, source pills |
| `--fs-11` | 11px | Small mono data: artifact names, log meta |
| `--fs-12` | 12px | Meta + controls: nav items, timestamps, key-values |
| `--fs-13` | 13px | Inputs, composer text, dense body |
| `--fs-14` | 14px | **UI default:** body, message content |
| `--fs-15` | 15px | Comfortable body prose |
| `--fs-16` | 16px | Card titles, list-item headings |
| `--fs-18` | 18px | Section headers, panel titles, doc H1 |
| `--fs-22` | 22px | Page title (H1) |
| `--fs-28` | 28px | Display / big numerics / hero metric |
| `--fs-40` | 40px | Jumbo display (empty hero) |

### Weights

| Token | Value | Use |
|-------|-------|-----|
| `--fw-regular` | 400 | Body prose, descriptions |
| `--fw-medium` | 500 | Nav items, row titles, interactive labels |
| `--fw-semibold` | 600 | Page titles, card titles, headings, brand |
| `--fw-bold` | 700 | Badges, status pills, mono tags (managed/fork/live) |

### Line-heights

| Token | Value | Use |
|-------|-------|-----|
| `--lh-tight` | 1.15 | Display + large titles |
| `--lh-snug` | 1.35 | Headings, card titles, controls |
| `--lh-normal` | 1.5 | Dense UI text (body default) |
| `--lh-relaxed` | 1.65 | Body prose / message content |

### Tracking (letter-spacing)

| Token | Value | Usage |
|-------|-------|-------|
| `--tracking-tight` | -0.02em | Display / page titles (compact, editorial) |
| `--tracking-snug` | -0.01em | Medium headings, brand wordmark |
| `--tracking-normal` | 0 | Body |
| `--tracking-caps` | 0.05em | **THE** tracking for every uppercase mono label / badge / status / column title / field label |
| `--tracking-caps-wide` | 0.1em | Eyebrow / kicker section markers only |

**Rule:** all-caps labels use `--tracking-caps`; tiny eyebrow kickers use
`--tracking-caps-wide`; display titles use `--tracking-tight`; sentence-case body
sets no tracking. Never hardcode a raw `em`.

---

## 4. Spacing & Layout

Defined in `ui/src/design/tokens/spacing.css`. A tight **2px-quantum** step
scale (the app is dense), named `--space-N` by pixel value, plus **semantic
layout constants** that flex under `[data-density]`.

### Step scale (inter-element gaps + on-scale padding)

Theme- and density-agnostic fixed geometry.

| Token | Value | | Token | Value |
|-------|-------|-|-------|-------|
| `--space-0` | 0 | | `--space-5-5` | 11px |
| `--space-1` | 2px | | `--space-6` | 12px |
| `--space-2` | 4px | | `--space-8` | 16px |
| `--space-3` | 6px | | `--space-10` | 20px |
| `--space-3-5` | 7px | | `--space-12` | 24px |
| `--space-4` | 8px **(workhorse)** | | `--space-16` | 32px |
| `--space-4-5` | 9px | | `--space-20` | 40px |
| `--space-5` | 10px | | `--space-24` | 48px |
| | | | `--space-32` | 64px |

> **Odd half-step tier (`--space-3-5`/`-4-5`/`-5-5` = 7/9/11px):** interleaved
> half-steps that name the odd-pixel spacing the dense older view CSS needs
> (`.step`/`.kcard`/`.col-h`/`.mi`/`.pal-*` padding + gaps). They mirror the
> `--fs-10-5…--fs-13-5` control type tier — a first-class home for values that
> fall between the 2px-quantum steps. Prefer a full step where one fits; reach
> for a half-step only to name genuinely odd dense-control geometry.

### Layout constants (chrome measurements; flex under compact)

| Token | Default | Compact | Usage |
|-------|---------|---------|-------|
| `--topbar-h` | 34px | — | Global top bar |
| `--toolbar-h` | 32px | — | View headers (`.vp-head` / `.gv-head`) |
| `--tabbar-h` | 30px | — | Tab strips (`.dtabs` / `.bp-tabs`) |
| `--sidebar-w` | 220px | 190px | Left session sidebar |
| `--rsidebar-w` | 279px | 240px | Right info sidebar |
| `--bpanel-h` | 152px | — | Bottom terminal/output panel |
| `--palette-w` | 560px | — | ⌘K command palette |
| `--drawer-w` | 300px | — | Detail drawer |
| `--view-pad-y` | 16px | 12px | View scroll vertical padding |
| `--view-pad-x` | 18px | 14px | View scroll horizontal padding |
| `--panel-pad` | 13px | 10px | Card interior (`.gcard`) |
| `--panel-pad-lg` | 16px | 13px | Dialog / large panel interior |
| `--nav-pad-y` | 5px | 4px | Nav item vertical padding |
| `--nav-pad-x` | 10px | — | Nav item horizontal padding |
| `--gap-page` | 16px | 12px | Header → content gap |
| `--measure` | 760px | — | Transcript column (`.tcol`) |
| `--measure-msg` | 640px | — | AI message / tool-card max width |

### Philosophy

- Every `gap:` and every on-scale `padding:` uses a `--space-*` step (or a
  semantic layout constant where one fits). Never a raw `Npx`.
- Card/panel interiors use `--panel-pad` / `--panel-pad-lg` (they **flex** under
  compact) — never a raw `12px`/`16px`, or the card won't tighten in compact mode.
- Compact mode (`[data-density="compact"]`) tightens the layout constants only;
  the step scale and all other tokens are unchanged.

---

## 5. Radius & Elevation

Defined in `ui/src/design/tokens/radius.css`. Softly-squared; never larger than 8px.

| Token | Value | Usage |
|-------|-------|-------|
| `--radius-sm` | 3px | kbd, tiny tags, inline code, skeleton, arch buttons |
| `--radius` | 4px | Buttons, inputs, chips, rows, tool cards |
| `--radius-md` | 6px | Cards, panels, menus, drawers |
| `--radius-lg` | 8px | Dialogs, command palette, composer, message bubbles |
| `--radius-full` | 999px | Pills, search field, org pill, status dots |

A `50%` literal is used for equal-sided **circles** (status/live dots) — a
geometric primitive, not an arbitrary radius, so it stays inline.

### Elevation

Depth is **mostly flat** — communicated through:
1. Surface layering (`--bg` → `--bg-elev` → `--bg-elev-2`)
2. Border presence and brightness (`--border` vs `--border-strong`)
3. Subtle hover-state background changes

**Floating chrome that leaves the plane gets a soft dark shadow** (this is the
one deliberate use of shadow — menus, palette, composer, toasts):

| Token | Value | Usage |
|-------|-------|-------|
| `--shadow-pop` | `0 12px 34px rgba(0,0,0,.55)` | Menus, popovers, overlays, tooltips, toasts |
| `--shadow-modal` | `0 20px 60px rgba(0,0,0,.6)` | Dialogs, command palette |
| `--shadow-float` | `0 6px 20px rgba(0,0,0,.45)` | Composer box |

### Ring & misc primitives (radius.css)

| Token | Value | Usage |
|-------|-------|-------|
| `--ring` | `var(--accent)` | Focus-ring color (auto-themes) |
| `--ring-w` | 2px | Focus-ring width |
| `--ring-offset` | 2px | Focus-ring offset |
| `--border-w` | 1px | Every hairline border |
| `--opacity-disabled` | 0.42 | The one opacity for any inert control |

---

## 6. Motion System

Defined in `ui/src/design/tokens/motion.css`. Confirmatory, never decorative —
interaction transitions are sub-perceptual (≤150ms). Looping animations own their
rhythm.

### Durations

| Token | Value | Use case |
|-------|-------|----------|
| `--dur-fast` | 80ms | Immediate: row / message hover |
| `--dur` | 120ms | Base: nav, borders, pills, controls |
| `--dur-slow` | 150ms | Buttons, cards, panels, dialogs |

### Easing

| Token | Value | Use case |
|-------|-------|----------|
| `--ease` | `cubic-bezier(.2,0,0,1)` | Standard control transitions |
| `--ease-out` | `cubic-bezier(.16,1,.3,1)` | Enter / expand |
| `--ease-in-out` | `ease-in-out` | Looping animations |

### Loop durations + keyframes

`--loop-pulse` 1.6s, `--loop-spin` 0.8s, `--loop-shimmer` 1.3s, `--loop-blink`
1.5s. Keyframes: `olympus-pulse` (live-dot), `olympus-spin` (spinner),
`olympus-shimmer` (skeleton), `olympus-blink` (streaming tag), `olympus-bounce`
(thinking dots).

### Rules

- **Max interaction transition: 150ms.**
- **Reduced motion:** `@media (prefers-reduced-motion: reduce)` collapses all
  animations/transitions to ~instant and disables smooth scroll. Information is
  never carried by animation alone — every animated state has a text label.

---

## 7. Density Modes

Two modes via `[data-density]` on `<html>` (managed by `ThemeProvider`, persisted
to `localStorage["olympus-density"]`):

| Mode | `data-density` | Feel |
|------|---------------|------|
| **Comfortable** (default) | `comfortable` | Standard spacing, roomy rows |
| **Compact** | `compact` | Tighter chrome for small viewports |

Compact overrides only the **layout constants** in §4 (`--sidebar-w`,
`--rsidebar-w`, `--view-pad-*`, `--panel-pad*`, `--nav-pad-y`, `--gap-page`). The
step scale, font sizes, radii, and colors are identical across modes.

---

## 8. Component Inventory

Olympus has **two component layers**, both fully token-driven:

### 8.A — `.ol-*` canonical primitive library (`design/styles/components.css`)

A complete, documented primitive set matched 1:1 to the shipped app's patterns.
Every rule reads from tokens; no raw hex. Each primitive covers
default → hover → active → focus-visible → disabled where applicable.

| Primitive | Classes | States / variants |
|-----------|---------|-------------------|
| **Button** | `.ol-btn` + `-primary`/`-secondary`/`-ghost`/`-danger`, sizes `-sm`/`-lg`/`-block` | default, `:hover`, `:active`, `:focus-visible`, `:disabled` (all via `:not(:disabled):hover`) |
| **Icon button** | `.ol-iconbtn` + `-bordered`/`-sm` | hover wash, focus ring, disabled |
| **Badge** | `.ol-badge` + `-accent`/`-ok`/`-warn`/`-err`/`-solid` | wash + line tint per semantic |
| **Live pill** | `.ol-live` + `.ol-live-label` | green wash pill + pulsing dot |
| **Status dot** | `.ol-dot` + `-ok`/`-warn`/`-err`/`-accent`/`-live` | 6px; `-live` pulses |
| **Tag / chip** | `.ol-tag` + `-btn`/`-active`/`-dot` | quiet → hover → active (accent wash) |
| **Input / textarea / select** | `.ol-input`/`.ol-textarea`/`.ol-select` + `.ol-field-label`, `.ol-input-mono` | hover/focus border-strong, disabled, custom select arrow |
| **Input group** | `.ol-inputgroup` | leading-icon pill; `:focus-within` |
| **Checkbox / radio** | `.ol-check` + `.ol-check-box`, `.ol-check-radio` | checked (accent fill), focus, disabled |
| **Switch** | `.ol-switch` + `.ol-switch-track`/`.ol-switch-thumb` | off → on (accent), focus |
| **Card** | `.ol-card` + `-interactive`/`-selected`/`-accent` | hover lift `-1px` + border brighten; selected = accent wash |
| **Stat pill** | `.ol-stat` + `.ol-stat-value`/`-label`/`-delta`(`.up`/`.down`), `.ol-stat-lg` | read-only metric chip |
| **Progress bar** | `.ol-bar` + `.ol-bar-fill`(`.ok`/`.warn`/`.err`), `-sm` | animated width |
| **Avatar** | `.ol-avatar` + `-sm`/`-lg`/`-agent` | circle, mono initial or img |
| **Skeleton** | `.ol-skel` | shimmer sweep |
| **Spinner** | `.ol-spinner` + `-lg` | accent-top ring, spin |
| **Tabs** | `.ol-tabs` + `.ol-tab`/`.ol-tab-active` | faint → text, 2px accent underline |
| **Nav item** | `.ol-nav` + `-active`/`.ol-nav-badge` | hover wash; active = accent wash |
| **Tooltip** | `.ol-tooltip-wrap` + `.ol-tooltip`(`-visible`) | floating chrome + pop shadow |
| **Menu** | `.ol-menu` + `.ol-menu-item`/`-label`/`-div` | pop shadow surface |
| **Dialog** | `.ol-overlay` + `.ol-dialog` + `-head`/`-title`/`-body`/`-foot` | scrim + modal shadow |
| **Toast** | `.ol-toast` + `-ok`/`-warn`/`-err`, `-icon`/`-title`/`-msg` | pop shadow |
| **Kbd** | `.ol-kbd` | mono key cap |

### 8.B — React shell primitives (`ui/src/components/shell.tsx`)

Thin React wrappers that **emit the live app-shell/view class vocabulary** in
`index.css` (`.gv-*`, `.stat`, `.gtag`). View workers build on these so every
screen is consistent. Do NOT invent new class names in `shell.tsx` — a new visual
need is a new rule in `index.css` (or a new `.ol-*` primitive) first.

| Component | Props | Emits |
|-----------|-------|-------|
| `PageHeader` | `{ title, subtitle?, actions? }` | `.gv-head` / `.gv-title` / `.gv-sub` / `.gv-actions` |
| `EmptyState` | `{ icon?, title, message?, cta? }` | `.empty-state` / `-icon` / `-title` / `-msg` / `-cta` |
| `StatPill` | `{ label, value }` | `.stat` / `.v` / `.l` |
| `Badge` | `{ kind?, children }` | `.gtag` + **case-insensitive** `kind→variant` map covering the full status vocabulary — `ok` (ready/running/run/active/live/online/connected/done/complete[d]/success/succeeded/ok/healthy/pass[ed]), `warn` (warning/warn/pending/pend/queued/waiting/paused/idle/draining/degraded/stale/unknown), `err` (blocked/fail[ed]/error/offline/disconnected/stopped/cancel[l]ed/killed/crashed/timeout/timed-out). Unknown kinds → neutral `.gtag`. |
| `PlaceholderBadge` | `{ epic }` | `.gtag warn` (amber; signals a view whose backend epic isn't live) |

### 8.C — App-shell / view classes (`index.css`)

The structural layout vocabulary the two primitive layers sit inside:
`.app`, `.topbar`, `.rail`, `.sidebar`, `.viewport`, `.vp-head`, `.chatcol`,
`.transcript`, `.composer`, `.bpanel`, `.rsidebar`, generic-view classes
(`.gv-*`, `.gcard`, `.gtag`, `.btn`, `.stat`, `.kv`), chat/message classes
(`.msg-user`, `.msg-ai`, `.toolcard`, `.msg-acts`), and the command palette
(`.pal*`). All token-driven; documented inline in `index.css`.

---

## 9. Accessibility

### 9.1 Focus rings

Single global `:focus-visible` rule (`tokens/base.css`):

```css
:where(button, a, select, textarea, input, summary, [role="button"], [tabindex]):focus-visible {
  outline: var(--ring-w) solid var(--ring);   /* --ring = var(--accent), auto-themes */
  outline-offset: var(--ring-offset);
}
```

- Fires on keyboard/programmatic focus only — never on mouse click.
- `:where()` → zero specificity, can't be overridden accidentally.
- Wrapped inputs (`.ol-inputgroup`, `.tb-search`, `.composer`) route focus via
  `:focus-within`.

### 9.2 Reduced motion

`@media (prefers-reduced-motion: reduce)` (`tokens/motion.css`) collapses all
animation/transition durations to `0.01ms`, forces `animation-iteration-count: 1`,
and sets `scroll-behavior: auto`. Every animated state also has a text label.

### 9.3 Color contrast

- Obsidian: `--text` `#E6E6E6` on `--bg` `#0A0A0B` / `--bg-elev` `#131315` passes
  WCAG AA comfortably.
- Daybreak: `--text` `#171719` on `--bg` `#F6F6F7` passes AA.
- `--text-faint` (tertiary meta: timestamps, counts, hints, section labels) now
  clears **WCAG AA (≥4.5:1)** on the canvas and primary chrome in **both** themes
  — obsidian `#7E7E81` (4.89:1 on `--bg`, 4.58:1 on `--bg-elev`), daybreak
  `#6E6E72` (4.70:1 on `--bg`, 5.08:1 on `--bg-elev`). On the deepest wells
  (`--bg-elev-2`) and hover washes it lands ~3.6–4.5:1 — above the 3:1 UI floor.
  Never carry meaning by faint text alone regardless.
- Status inks on their washes all pass: obsidian ok/warn/err ≥8.9:1; daybreak
  ≥4.0:1 (err 5.3:1) — ok/warn sit just under 4.5:1 on their own washes but are
  used for pill/badge chrome and short labels, not body copy.
- **Remaining a11y debt:** a full automated axe/WAVE sweep across every live
  surface (beyond the token-level ratios computed here) is still outstanding.

### 9.4 Semantic HTML

- Views use `<button>` for interactive controls (not `div onClick`); shell
  primitives render semantic elements.
- Icons are decorative SVGs; standalone icon buttons need an accessible label.
- WS-driven live regions should use `aria-live` (partial implementation).

---

## 10. Do / Don't Gallery

### ✅ Do

- **DO** use `var(--token)` for every color, spacing, font-size, radius, duration.
- **DO** add a new color/alpha token to **both** theme blocks in `colors.css`.
- **DO** use `--fs-*` tokens — never inline a pixel font-size.
- **DO** use `--space-*` steps + layout constants for gaps/padding/margins.
- **DO** use `--dur-*` + `--ease-*` for transitions (≤150ms).
- **DO** reach for an `.ol-*` primitive or a `shell.tsx` component before hand-rolling.
- **DO** keep the accent as the single signal color; status stays green/amber/red.
- **DO** provide text labels alongside color-coded states.
- **DO** test in **obsidian AND daybreak**, and in **compact** density, after any visual change.
- **DO** use IBM Plex Mono for data, timestamps, badges, code.
- **DO** rely on the global `:focus-visible` ring (don't add custom outline rules).

### ❌ Don't

- **DON'T** hardcode hex / `rgba()` in a component or view.
- **DON'T** use Inter, Roboto, or system-ui as the primary face (IBM Plex only).
- **DON'T** introduce purple gradients, glassmorphism, or neon glow.
- **DON'T** use drop shadows for in-plane elevation — only floating chrome
  (menus/palette/composer/toasts) gets `--shadow-*`.
- **DON'T** exceed 150ms for interaction transitions.
- **DON'T** use Material Design components or patterns.
- **DON'T** animate information-carrying properties without a static fallback.
- **DON'T** skip the compact-density check after changing spacing/layout constants.
- **DON'T** add `!important` to token usages (fix specificity properly).
- **DON'T** import a CSS framework (Tailwind, Bootstrap, etc.).
- **DON'T** invent class names in `shell.tsx` — add the rule to `index.css`/`.ol-*` first.

---

## 11. Changelog

### 2026-07-12 — Reinstate Milkdown without forced source ejection

- Reversed postmortem 0018's editor resolution while retaining its QA and losslessness lessons: Milkdown/Crepe is again the default Vault note surface, with real-editor component and browser coverage required.
- Replaced the Rich/Source toggle and syntax denylist with an overflow **Edit source / Edit rich** action. Unsupported Markdown stays in rich mode through byte-preserving literal passthrough blocks; unresolved jj conflicts are the sole automatic CodeMirror path.
- Retained explicit Save/Cancel and the VS Code-style dirty `*` tab marker. Frontmatter remains outside Milkdown and byte-identical across rich edits.

### 2026-07-11 — Put Vault dirty state in the tab title

- Replaced the editor toolbar's `Unsaved` label with a VS Code-style ` *` suffix on the affected pane's tab title.
- Saving or reloading the note removes the suffix; split panes track their own editor draft state.
- The formatting toolbar keeps only editing and persistence actions, reducing duplicate status chrome.

### 2026-07-11 — Make Vault notes an always-editable, full-pane writing surface

- Removed the remaining View/Edit interaction. Opening a note now opens the live-preview editor directly; there is no second rendered-note state or Edit/Cancel ceremony.
- The note canvas owns the complete editor-group area below its tab. Removed the centered 680px card, outer padding, and bordered editor frame.
- Added a compact formatting toolbar for undo/redo, headings, emphasis, inline code, links, lists, and blockquotes. Save state and destructive actions stay reachable at the right edge while formatting controls scroll on narrow screens.
- Desktop, split-pane, and 412px mobile Maestro evidence confirms the canvas fills the pane, toolbar actions remain reachable, and no horizontal content clipping returns.

### 2026-07-11 — Replace the Vault's dual editor with a lossless live-preview workbench

- Removed the Rich/Source mode switch and the source-only syntax denylist. Vault editing now uses one CodeMirror document model that preserves canonical Markdown byte-for-byte.
- Added Obsidian-style live preview: inactive syntax markers collapse while heading, emphasis, link, list, and blockquote structure remains visible; the active line reveals exact Markdown for direct editing.
- Moved layout controls onto the active pane's tab row, matching the VS Code editor-group model instead of spending a second row on global layout chrome.
- New split groups inherit the active document instead of opening as empty dead space. Column and row separators support mouse and keyboard resizing with 20–80% bounds.
- QA now enters the real editor and captures editor/split screenshots. The previous suite mocked Milkdown and never exercised the user-visible mode fallback; see postmortem 0018.

### 2026-07-10 — Redesign + reimplement the Hall login screen (`auth.tsx`) — kill inline styles, adopt tokens/`.ol-*`, add editorial identity, make it responsive

- **The debt this closes:** the unauthenticated auth surface (`ui/src/auth.tsx`)
  was the last screen built entirely from **inline styles + hardcoded hexes**
  (`#2b3038`, `#15181d`, `#ff8b8b`, raw `px`) — a bare card that ignored the
  token system, both themes' correctness (it hardcoded a single dark palette),
  and the `.ol-*` primitive library. It violated the §10 hard rules and looked
  like a wireframe next to the cockpit.
- **What changed (scope: unauthenticated/loading surface only — `AuthGate` login
  API flow, endpoints, redirects, and org-selection semantics untouched):**
  - **`auth.tsx`** — replaced the inline-styled `AuthPanel`/`LoginPanel` with a
    token-driven `AuthShell` (shared editorial frame), `LoadingPanel`, and a
    rebuilt `LoginPanel`. Fields now use the canonical `.ol-field-label` +
    `.ol-input`; the submit uses `.ol-btn.ol-btn-primary.ol-btn-block`; the
    loading state uses `.ol-spinner.ol-spinner-lg`. **Zero inline styles, zero
    hex** remain in the file.
  - **`index.css`** — added a new **`.auth-*` app-shell section** (screen, card,
    head/kicker/brand/wordmark/title/sub, form, error, status, footer). Every
    value is a `var(--token)`; both `obsidian` + `light` resolve correctly by
    construction (surfaces on `--bg`/`--bg-elev`, error on `--err`/`--err-wash`/
    `--err-line`, footer dot on `--accent`).
- **Identity (restrained, on-system):** a mono `CONTROL PLANE` kicker
  (`--tracking-caps-wide`), an `Olympus` wordmark in `--font-display` beside a
  monochrome twin-peak accent SVG mark (altitude/signal — no gradient, no glow),
  and a mono status footer that names the **Hall origin host** (`window.location.
  host`, read-only) so the operator sees which Hall they are authenticating to —
  reinforcing the origin-binding security note in the code. Editorial-terminal,
  not marketing-page.
- **Accessibility:** preserved the exact `h1` "Sign in to this Hall" heading and
  the "Username"/"Password" accessible names (wrapping `<label>` + label span);
  `autoFocus`, `autoComplete`, and `required` retained; the error keeps
  `role="alert"` and now also drives `aria-invalid` + `aria-describedby` on both
  inputs; the loading state is a `role="status"` live region; the SVG mark is
  `aria-hidden`. Focus rings come from the global `:focus-visible` token rule.
- **Responsive:** the cockpit is desktop-first, but this is the one surface a
  user can hit on a phone. The card is `max-width: 384px; width: 100%`, control
  heights bumped to 36px for touch, `min-height: 100dvh`, and a `max-width: 480px`
  media query tightens padding. Verified graceful down to **320px**.
- **Verified:** `bun run typecheck` (exit 0), `bun run test src/auth.test.tsx`
  (**4 passed** — added coverage for the loading `role="status"` region and the
  bad-credential alert + `aria-invalid`), `bun run build` (exit 0). Screenshotted
  in-browser via Playwright/Chromium in **both themes** at desktop (1280),
  mobile (380), narrow (320), plus the loading and error states — all render
  clean with no overflow or layout shift. (No local "React Doctor" CLI exists in
  this repo; the `_ds_*` bundle + `DesignSync` tool target claude.ai design
  projects, and `_adherence.oxlintrc.json` is the local token-adherence rule set.)

### 2026-07-04 — Kill last hardcoded hexes + deduplicate conflicting CSS rules (3 fixes in `index.css`)

- **The debt this closes (top of the visible list — blocked since 2026-07-04):**
  two hardcoded `#e0a030` hex fallbacks in `.srow-dot.needs-input` and
  `.perm-prompt`. The fallback was dead code (`--amber` always resolves to
  `#FCD34D`/`#A16207`) and violated the §10 hard rule ("never hardcode a
  hex"). Also fixed: **two duplicate rule blocks** that were shadowing
  canonical definitions with conflicting values.
- **Fix 1 — hex cleanup:** dropped `, #e0a030` from both sites → bare
  `var(--amber)`. Zero visual change; `--amber` always resolves.
- **Fix 2 — `.navitem` dedup:** merged S2 override into canonical L101
  definition (absorbs button-reset props: `cursor:pointer`, `background:none`,
  `border:none`, `width:100%`, `text-align:left`). Deleted 11-line duplicate
  at L627 that was **breaking** `.navitem.on` accent color (rendered
  `color:var(--text)` instead of `color:var(--silver)`). The active nav item
  now correctly reads silver/accent in both themes.
- **Fix 3 — `.gv-head`/`.gv-title`/`.gv-body` dedup:** removed S2
  overrides that shadowed canonical app-shell definitions with different
  sizing (`gap: var(--space-3)` vs `var(--space-4)`, `padding: 0 var(--space-8)`
  vs `var(--space-10)`, `font-size: var(--fs-16)` vs `var(--fs-12-5)`).
  All views using `PageHeader` now get consistent header geometry.
- **Net: −2 hex literals, −14 lines of dead-duplicate CSS, 1 file.**
- **Verified:** `cd ui && bun run typecheck` (exit 0), `bun run build`
  (exit 0, CSS 65.09 kB). Grep confirms **zero** remaining hardcoded hexes
  in `index.css` (only a comment reference to `#efefef`). Fully reversible.
- **Top design debts now visible (next runs, in priority order):**
  1. **Adoption gap (the big one).** Live views still lean on bespoke
     `index.css` classes rather than `.ol-*` primitives / `shell.tsx`
     components. A view-worker task to spec + spot-fix.
  2. **Full automated axe/WAVE sweep** across every live surface.
  3. **`.ol-*` visual QA in-browser** in both themes — rule-by-rule
     screenshot verification against live views.
  4. **`index.css` structural hygiene** — the file is 650+ lines of
     mixed concerns (app-shell, view-specific, S2 additions). A future
     refactor could split view-specific rules into per-view CSS modules,
     leaving index.css as pure shell layout.

### 2026-07-04 — Repoint the `.ol-*` primitive library onto the `--space-*` half-step tier — carry the odd-pixel-spacing fix into the source-of-truth component CSS (extends the earlier `index.css` sweep)

- **The debt this closes:** on an earlier 2026-07-04 run the odd-pixel half-step
  spacing tier (`--space-3-5`=7px / `--space-4-5`=9px / `--space-5-5`=11px) was
  formalized in `design/tokens/spacing.css` and every off-scale `7/9/11px`
  padding+gap site in **`index.css`** was repointed to it. But the sweep stopped
  at the app-shell CSS — the **canonical `.ol-*` primitive library**
  (`design/styles/components.css`), which §8.A documents as the source-of-truth
  primitive set ("Every rule reads from tokens; no raw hex"), still shipped the
  **identical** raw `2px/4px/7px/8px/9px/11px` padding+gap literals it was built
  with. The library that view workers are meant to copy from carried the exact
  debt that had just been fixed downstream — a consistency inversion.
- **Fix (system-level, `design/styles/components.css` only — no view internals,
  no token definitions changed; all tokens already existed):**
  - `.ol-badge` — `padding: 2px 7px` → `var(--space-1) var(--space-3-5)`.
  - `.ol-live` — `padding: 2px 7px` → `var(--space-1) var(--space-3-5)`.
  - `.ol-tag` — `padding: 4px 8px` → `var(--space-2) var(--space-4)`.
  - `.ol-tab` — `padding: 0 11px` → `0 var(--space-5-5)`.
  - `.ol-menu-item` — `gap: 9px; padding: 7px 9px` → `var(--space-4-5)` /
    `var(--space-3-5) var(--space-4-5)`.
- **Behavior:** **exactly zero pixels move** — every new token equals the literal
  it replaced (2→2, 4→4, 7→7, 8→8, 9→9, 11→11). Pure tokenization: the primitive
  library's padding/gap axes are now scale-addressable like the rest of the system,
  so a future density or spacing rescale flows through `.ol-*` from one place. The
  handful of remaining raw values in `components.css` are element-**dimension**
  primitives on purpose (control heights 26/28/30px, dot/thumb/box sizes, the 5px
  `gap` on `.ol-live`, spinner geometry, the 2px tab underline) — geometry, not
  spacing steps, exactly as §4/§5 leave `.gbar`/`.spin` dimensions raw.
- **Verified:** `cd ui && bun run typecheck` (exit 0) and `bun run build` (exit 0,
  CSS 65.72 kB). The edit is color-token-agnostic and zero-pixel-delta, so both
  `obsidian` and `light` are unaffected by construction (the zero-delta shortcut).
  `components.css` was confirmed **clean at HEAD** before the edit and is the only
  code file staged (plus this doc).
- **Blocked this run (noted for the view worker, not swept in):** `index.css` is
  currently **dirty** with a session view worker's uncommitted work (SessionSidebar
  needs-input dot + permission-prompt), which added **two new hardcoded-hex
  violations** — `.srow-dot.needs-input` and `.perm-prompt` both use
  `var(--amber, #e0a030)`. That `#e0a030` fallback is a raw hex (violates the §10
  hard rule) **and wrong-valued** — `--amber` always resolves (aliased to `--warn`
  = `#FCD34D`/`#A16207`), so the fallback is dead code that would render an
  off-palette orange if it ever fired. The fix is trivial (drop the `, #e0a030`
  fallback in both sites, leaving bare `var(--amber)`), but committing it would
  sweep the worker's in-flight view work into a design commit — deferred until
  `index.css` is clean at HEAD. **Flagged to the SessionSidebar/ChatPage owner.**
- **Top design debts now visible (next runs, in priority order):**
  1. **Two hardcoded `#e0a030` hexes in `index.css`** (`.srow-dot.needs-input`,
     `.perm-prompt`) — drop the dead `, #e0a030` fallback → bare `var(--amber)`.
     Blocked only by the dirty tree; do it the moment `index.css` is clean at HEAD.
  2. **Adoption gap (the big one).** The `.ol-*` primitive library + `shell.tsx`
     React primitives exist but live views still lean on bespoke `index.css`
     classes — the largest remaining consistency win; a view-worker task to
     *spec + spot-fix*, not rewrite wholesale.
  3. **`.ol-*` visual QA in-browser** in both themes — the primitive library was
     built to spec but hasn't been screenshot-verified rule-by-rule against the
     live views.
  4. **Full automated axe/WAVE sweep** across every live surface (token-level
     ratios are done; per-surface DOM audit — focus order, ARIA, component-state
     contrast — remains).

### 2026-07-04 — Consolidate the two app-shell spinners onto the canonical `olympus-spin` keyframe + `--loop-spin` token; kill the last local keyframes, raw `0.7s` duration, and raw `border-radius: 50%` in `index.css`

- **The debt this closes (the residual of debt #2, "`index.css` still carries
  raw literals"):** the motion layer (`design/tokens/motion.css`) defines ONE
  canonical rotation keyframe — `@keyframes olympus-spin { to { transform:
  rotate(360deg) } }` — and the `.ol-spinner` primitive in
  `design/styles/components.css` already consumed it as `olympus-spin
  var(--loop-spin)`. But `index.css` still shipped **two private, byte-identical
  copies** of that keyframe — `@keyframes rot` (used only by `.spin`) and
  `@keyframes spin` (used only by `.srow-spinner`) — and `.srow-spinner`
  additionally hardcoded a bespoke **`0.7s`** duration (bypassing the
  `--loop-spin` = 0.8s token) and a raw **`border-radius: 50%`** (bypassing
  `--radius-full`). Three spinners, three different sources of truth for the
  same motion.
- **Fix (all in `index.css`):**
  - `.spin` — `animation: var(--loop-spin) rot …` → `animation: olympus-spin
    var(--loop-spin) …` (name-order also normalized to the shorthand convention
    used everywhere else). Deleted the now-orphaned `@keyframes rot`.
  - `.srow-spinner` — `animation: spin 0.7s …` → `animation: olympus-spin
    var(--loop-spin) …`; `border-radius: 50%` → `border-radius: var(--radius-full)`.
    Deleted the now-orphaned `@keyframes spin`.
  - Net: **−2 duplicate keyframe blocks, −1 raw duration, −1 raw radius.** All
    three spinners in the codebase (`.spin`, `.srow-spinner`, `.ol-spinner`) now
    share the single `olympus-spin` + `--loop-spin` definition — one place to
    retune spinner rhythm system-wide.
- **Behavior:** geometry is **byte-identical** — `olympus-spin` and the deleted
  `rot`/`spin` are the same `to { rotate(360deg) }`, and `border-radius: 50%`
  renders identically to `--radius-full` (999px) on a 10px circle. The ONLY
  functional delta is intentional: `.srow-spinner` (the running-session row
  indicator) now spins at the canonical **0.8s** instead of an off-token 0.7s,
  matching every other spinner. Color-token-agnostic, so both `obsidian` and
  `light` are verified by construction (per the zero-delta shortcut); shell was
  also loaded in-browser to confirm no render regression.
- **Verified:** `bun run typecheck` clean, `bun run build` exits 0 (CSS
  64.93 kB). Fully reversible (this file's edits are self-contained; motion.css
  and components.css untouched). Working tree was clean at HEAD before the edit —
  only `index.css` + this doc staged.
- **Top design debts now visible (next runs, in priority order):**
  1. **Adoption gap (unchanged, the big one).** The `.ol-*` primitive library +
     `shell.tsx` React primitives exist but live views (`AppShell`, `FleetView`,
     `ChatView`, the vault/project/session views) still lean on bespoke
     `index.css` classes. Migrating views onto the shared primitives is the
     largest remaining consistency win — but it edits view internals, so it's a
     view-worker task the design-lead should *spec + spot-fix styling for*, not
     rewrite wholesale.
  2. **`.ol-*` visual QA in-browser** in both themes — the primitive library was
     built to spec but hasn't been screenshot-verified rule-by-rule against the
     live views; drift between `.ol-*` and the `index.css` vocabulary they
     shadow (e.g. `.ol-spinner` vs `.spin`/`.srow-spinner`, now unified on motion
     but still separate size/border rules) is the kind of thing this surfaces.
  3. **Remaining raw geometry literals in `index.css`** are now element-DIMENSION
     primitives on purpose (spinner sizes 9/10px, `.todo .bx` 11px, `.divider`
     20px, border widths `1.5px`) — these are geometry, not spacing/motion steps,
     and are correctly left raw. If a future run wants to tokenize border widths,
     add a `--border-w-2: 1.5px` companion to `--border-w` rather than snapping.

### 2026-07-04 — Formalize the odd-pixel half-step spacing tier (`--space-3-5`/`-4-5`/`-5-5`) + repoint all 15 off-scale `7/9/11px` padding+gap sites — closes debt #1

- **The debt this closes (top of the visible list the last two runs — the
  `9px`/`11px` "off-scale raw `Npx`" item):** 15 spacing declarations across the
  older view CSS in `index.css` carried **odd-pixel padding/gap literals with no
  matching step token** — `9px` and `11px` (the two axis values called out in the
  prior run's debt #1) plus their `7px` sibling. Sites: `.sec-head`, `.rs-sec`,
  `.dtab`, `.tc-body`, `.col-h`, `.kcard`, `.step`, `.agrow`, `.ubar-row`, `.art`,
  `.mi`, `.md th/td`, `.pal-in`, `.pal-r`. These `7/9/11px` values fall **between**
  the 2px-quantum steps (`--space-3`=6, `--space-4`=8, `--space-5`=10, `--space-6`=12),
  so they were the last on-scale-spacing holdouts and could not be repointed without
  a token to name them.
- **Why now / why this shape:** the prior two runs' debt #1 explicitly posed the
  decision — *"add `--space-4-5`(9px)/`--space-5-5`(11px) half-steps (mirrors the
  `--fs-*-5` half-step precedent) or snap each to the nearest step."* I chose the
  **half-step tier** over snapping: snapping would move real pixels (a visible
  ≤2px tightening on already-dense chrome), whereas the half-step tier is **exactly
  zero pixel delta** and gives the odd-pixel dense-control rhythm a first-class,
  scale-addressable home — the direct analogue of the `--fs-10-5…--fs-13-5` control
  type tier landed on 2026-07-03. The working tree was **clean at HEAD** (the
  precondition the last two runs cited), so the deferred sweep is finally safe.
- **Fix (system-level, token layer + `index.css` spacing repoints only — no view
  internals/business logic touched):**
  - **`design/tokens/spacing.css`** — added three interleaved half-steps to the
    step scale: `--space-3-5` (7px, dense control padding), `--space-4-5` (9px,
    dense row/card padding + gaps), `--space-5-5` (11px, panel/section padding),
    each role-commented as the "odd tier."
  - **`index.css`** — repointed all 15 sites' padding/gap axes to the new tokens
    (e.g. `.step padding: 9px 11px` → `var(--space-4-5) var(--space-5-5)`;
    `.sec-head padding: 9px … 4px` → `var(--space-4-5) … var(--space-2)`;
    `.mi gap: 9px; padding: 7px 9px` → `var(--space-4-5)` / `var(--space-3-5)
    var(--space-4-5)`).
- **Behavior:** **exactly zero pixels move** — every new token equals the literal
  it replaced (7→7, 9→9, 11→11). Element-dimension primitives (`.spin` 9px×9px,
  `.srow-spinner` 10px, `.todo .bx` 11px, `.rs-tab` widths, `.divider` 20px) were
  **left raw on purpose** — they are geometry sizes, not spacing steps, exactly as
  the bar thicknesses (`.gbar` 6px) stay raw per §5's precedent. Fully reversible
  (`spacing.css` + the 15 `index.css` lines).
- **Verified:** `cd ui && bun run typecheck` (exit 0) and `bun run build` (exit 0,
  CSS 64.98 kB). Grep confirms **zero** remaining `padding|gap: …7/9/11px` sites.
  Screenshotted the live mock UI (obsidian): session sidebar `.sec-head` (RECENT +
  count), the open-session transcript with `.tc-body` tool cards, and the right
  sidebar `.rs-sec` sections (AGENT/NODE/MODEL, CONTEXT WINDOW IN/OUT/MSGS) — all
  render with clean, even padding and correct alignment, no layout shift. The edit
  is color-token-agnostic and zero-delta, so daybreak is structurally unaffected by
  construction. `index.css` confirmed clean at HEAD before the change.
- **Top design debts now visible (next runs, in priority order):**
  1. **Adoption gap.** The `.ol-*` primitive library exists but live views
     (`AppShell`, `FleetView`, `ChatView`, `ProjectsView`, `ContextRing`,
     `VaultsView`) still lean on bespoke `index.css` classes — a view-worker task
     to *spec + spot-fix*, now the single largest consistency lever.
  2. **Full automated axe/WAVE sweep** across every live surface (token-level
     ratios are done; per-surface DOM audit — focus order, ARIA, component-state
     contrast — remains).
  3. **`.ol-*` visual QA in-browser** in both themes — primitives built to spec
     but not all screenshot-verified against the live views.
  4. **Remaining raw geometry primitives** (element sizes like `.spin`/`.rs-tab`
     dimensions) are intentionally raw today; if a density-scalable icon-size tier
     is ever wanted, that would be the next tokenization frontier.

### 2026-07-04 — Tokenize the `.ctx-*` ContextRing geometry (radius/space/motion) + fix its 300ms transition — closes long-standing debt #1

- **The debt this closes (top of the visible list for the last 3+ runs):** the
  ContextRing block in `index.css` (`.ctx-bar-track/-fill`, `.ctx-io`,
  `.ctx-breakdown`, `.ctx-gk`, `.ctx-bd-val`, `.ctx-mini-track/-fill`) carried
  **raw off-scale geometry** that predated the modular scales: `border-radius: 2px`
  (×6), `gap: 2px/3px`, `margin-bottom: 2px`, `gap: 6px`, `width: 48px`, and a
  `transition: width 0.3s ease` that **violated §6's 150ms interaction cap**. It
  was blocked run after run because `index.css` was **dirty** with the ContextRing
  view worker's uncommitted `.ctx-*` work — committing a tokenization pass would
  have swept their unfinished feature into a design commit.
- **Why now:** the working tree is **clean at HEAD** (`git status --short` empty)
  — the ContextRing worker has committed. The blocking condition every prior run
  cited is cleared, so the deferred sweep is finally safe to do in isolation.
- **Fix (system-level, `index.css` — no view internals/business logic touched):**
  - **Radius:** all six `border-radius: 2px` → `var(--radius-full)`, matching the
    shipped convention of the sibling progress bars `.gbar` (line 355) and
    `.cp-bar` (line 456), which both pill their tracks with `--radius-full`. On a
    3–4px-tall bar, full-round and 2px are visually identical, so zero perceptible
    change — but the value is now scale-addressable.
  - **Spacing:** `gap: 2px` → `var(--space-1)`; `gap: 3px` → `var(--space-1)`
    (2px, a 1px snap onto the step scale); `margin-bottom: 2px` → `var(--space-1)`;
    `gap: 6px` → `var(--space-3)` (identical); `width: 48px` → `var(--space-24)`
    (identical). All ≤1px, imperceptible.
  - **Motion:** `transition: width 0.3s ease` → `transition: width var(--dur-slow)
    var(--ease-out)` — brings the context-bar fill animation onto the token motion
    system AND under the §6 150ms cap (300ms → 150ms) with the enter/expand easing.
  - Bar **heights** (3px/4px) intentionally left raw — they are thickness
    primitives, exactly as `.gbar` (6px) and `.cp-bar` (5px) leave theirs.
- **Behavior:** no color, no layout shift — the radius change is invisible on
  thin bars, the space snaps are ≤1px, and the only functional change is a faster,
  spec-compliant fill transition. Fully reversible (this one block).
- **Verified:** `cd ui && bun run typecheck` (exit 0) and `bun run build` (exit 0,
  CSS 64.64 kB). Screenshotted the live session right-sidebar CONTEXT WINDOW
  section in-browser — the context bar + IN/OUT/MSGS breakdown render cleanly with
  no misalignment (bars empty at 0 tokens for this session; geometry intact). The
  edit is color-token-agnostic so both obsidian + daybreak are structurally
  unaffected. `index.css` was confirmed clean at HEAD before + the only staged
  file after (`git diff --stat` = 1 file, 8 lines).
- **Top design debts now visible (next runs, in priority order):**
  1. **Off-scale raw `Npx` in the remaining older view CSS:** `.kcard`/`.step`
     `padding: 9px 11px`/`9px 10px`, `.col-h`/`.tc-body`/`.sec-head` `padding: 9px …`,
     `.rs-sec` `11px …`, `.pal-in`/`.ubar-row` `11px …` — the `9px`/`11px` axis
     literals have no matching step token (9px≈`--space-4`+1, 11px between `--space-5`
     and `--space-6`). Decide: add `--space-4-5`(9px)/`--space-5-5`(11px) half-steps
     (mirrors the `--fs-*-5` half-step precedent) or snap each to the nearest step.
  2. **Adoption gap.** The `.ol-*` primitive library exists but live views
     (`AppShell`, `FleetView`, `ChatView`, `ProjectsView`, `ContextRing`) still
     lean on bespoke `index.css` classes — a view-worker task to *spec + spot-fix*.
  3. **Full automated axe/WAVE sweep** across every live surface (token-level
     ratios are done; per-surface DOM audit — focus order, ARIA, component-state
     contrast — remains).
  4. **`.ol-*` visual QA in-browser** in both themes — primitives built to spec
     but not all screenshot-verified against the live views.

### 2026-07-03 — Raise `--text-faint` to WCAG AA in both themes — closes the tertiary-text contrast debt (was debt #3)

- **The debt this closes (the standing "formal contrast audit" item, #3 on the
  last two runs' lists):** the tertiary text token `--text-faint` — which paints
  **timestamps, row counts, section labels (`PINNED`/`RECENT`), search
  placeholder, ⌘K hint, and empty-state copy** across the whole cockpit — failed
  WCAG AA in **both** themes. Obsidian `#5E5E60` = **3.06:1** on `--bg` (2.87 on
  `--bg-elev`, 2.42 on `--bg-hover`); daybreak `#98989C` = **2.66:1** on `--bg`.
  This is *informational* text (relative times, counts), not pure decoration, so
  sub-4.5:1 is a real legibility gap, not an intentional de-emphasis.
- **How it was found:** computed WCAG contrast ratios for every text token
  against `--bg` / `--bg-elev` / `--bg-elev-2` / `--bg-hover` and every status
  ink against its own wash-over-canvas, in both theme blocks. `--text` /
  `--text-dim` / `--accent` and all status inks already pass; `--text-faint` was
  the lone failure in each theme.
- **Fix (token layer only — `design/tokens/colors.css`, clean at HEAD):**
  - obsidian `--text-faint`: `#5E5E60` → **`#7E7E81`** (4.89:1 on `--bg`,
    4.58:1 on `--bg-elev`).
  - daybreak `--text-faint`: `#98989C` → **`#6E6E72`** (4.70:1 on `--bg`,
    5.08:1 on `--bg-elev`).
  Both stay clearly the **faintest tier** — visibly below `--text-dim`
  (obsidian dim 7.03:1, daybreak dim 6.08:1) — so hierarchy is preserved; the
  tokens just clear the AA floor. Value-only change: no geometry, no new tokens,
  no class or component touched.
- **Verified:** `cd ui && bun run typecheck` (exit 0) and `bun run build`
  (exit 0, CSS 61.47 kB — byte-identical size, color-only diff). Screenshotted
  the live app in **both** themes: obsidian empty-state hint + search/⌘K hint,
  and daybreak session sidebar (timestamps `1m`/`4m`/`7h`, `PINNED`/`RECENT`
  labels + counts) all render clearly legible with the faint tier intact and no
  layout shift. Fully reversible (two hex values).
- **Why not debt #2 (tokenize `.ctx-*`/`.kcard`/`.step` raw geometry) this run:**
  those rules are still **uncommitted** in `index.css` (the ContextRing view
  worker's in-flight change, alongside AppShell/RightPanel edits). Committing a
  tokenization pass over them would sweep unrelated feature work into a design
  commit. It stays blocked until `index.css` is clean at HEAD — unchanged from
  the prior run's note.
- **Top design debts now visible (next runs, in priority order):**
  1. **Off-scale raw `Npx` in newly-landed view CSS** (`.ctx-*` ContextRing:
     `border-radius: 2px`, `height: 3px`, `transition: width 0.3s` — the last
     also violates §6's 150ms cap — plus `.kcard`/`.step` `padding: 9px 11px`,
     `.ctx-mini-track width: 48px`). Tokenize once `index.css` is clean at HEAD.
  2. **Adoption gap.** The `.ol-*` primitive library exists but live views
     (`AppShell`, `FleetView`, `ChatView`, `ProjectsView`, `ContextRing`) still
     lean on bespoke `index.css` classes — a view-worker task to *spec + spot-fix*.
  3. **Full automated axe/WAVE sweep** across every live surface (this run
     closed the token-level ratios; a per-surface DOM audit — focus order, ARIA,
     component-state contrast — is the remaining a11y work).
  4. **`.ol-*` visual QA in-browser** in both themes — primitives built to spec
     but not all screenshot-verified against the live views.

### 2026-07-03 — Harden the `Badge` primitive: case-insensitive lookup + full status vocabulary — closes an information-carrying-color gap

- **The debt this closes (a fresh a11y/correctness gap, not on the prior list):**
  the shared `Badge` shell primitive (§8.B) mapped only **10 lowercase** kinds
  (`ready/running/done/online → ok`, `warning/warn → warn`,
  `blocked/failed/error/offline → err`) and looked them up **case-sensitively**.
  Any status a view worker passes that is capitalized (`"Running"`, `"Failed"`)
  or a common-but-unmapped synonym (`pending`, `queued`, `stopped`, `paused`,
  `cancelled`, `succeeded`, `timeout`, …) fell through to `""` and rendered as a
  **neutral gray badge** — silently mis-signaling a live / failed / needs-attention
  state as inert. That directly violates §9's rule that color must confirm meaning
  (a red state must read red), and it's a latent trap: the first view worker to
  pass a real fleet status string would have shipped a wrongly-colored badge with
  no error.
- **Why `Badge`, and why `shell.tsx`:** the in-lane top debt (raw-`Npx` geometry
  in the fresh `.ctx-*` ContextRing block) lives in `index.css`, which is
  currently **dirty with an uncommitted view-worker's `ContextRing.tsx` work** —
  tokenizing it now would entangle my system commit with their unfinished view.
  The brief forbids that ("change the SYSTEM, not a view's business logic"), so I
  picked the highest-leverage improvement in a file that is **clean at HEAD**.
  `Badge` is a core shared primitive every status chip across Fleet/Sessions will
  render, so hardening it is a genuine system-level, all-views win.
- **Fix (system-level, `ui/src/components/shell.tsx` only — no CSS/token touched):**
  - Lookup normalizes input: `BADGE_KIND[kind.trim().toLowerCase()]` — capitalized
    and whitespace-padded kinds now resolve.
  - Expanded `BADGE_KIND` from 10 → ~45 entries covering the full fleet/session
    status vocabulary, grouped + commented by semantic: **ok** (ready, running,
    run, active, live, online, connected, done, complete[d], success, succeeded,
    ok, healthy, pass[ed]); **warn** (warning, warn, pending, pend, queued,
    waiting, paused, idle, draining, degraded, stale, unknown); **err** (blocked,
    fail[ed], error, offline, disconnected, stopped, cancel[l]ed, killed, crashed,
    timeout, timed-out). Unknown kinds still fall back to the neutral `.gtag` on
    purpose — the guarantee is that *known* states always carry their correct color.
  - Rewrote the JSDoc to document the case-insensitivity, the fallback contract,
    and the §9 "label text always carries the meaning too" tie-in.
- **Behavior:** every existing correct call-site renders **identically** (all 10
  original kinds still map the same); the only change is that previously-degrading
  kinds now resolve to their correct variant. No token, CSS, or view internals
  touched — pure primitive correctness. Fully reversible (this file + `shell.tsx`).
- **Verified:** `cd ui && bun run typecheck` (exit 0) and `bun run build` (exit 0).
  Pure TS/logic change with no new markup or CSS, so no visual regression is
  possible in either theme; the existing `.gtag[.ok|.warn|.err]` rules (already
  screenshot-verified in obsidian + daybreak in prior runs) are unchanged.
- **Top design debts now visible (next runs, in priority order):**
  1. **Off-scale raw `Npx` in the `.ctx-*` ContextRing block** (`border-radius: 2px`
     ×4, `gap: 2px/3px/6px`, `margin-bottom: 2px`, `transition: width 0.3s` — the
     last one also **violates §6's 150ms interaction cap**). Blocked this run
     because it's uncommitted in `index.css`; tokenize it once the ContextRing
     view worker has committed and `index.css` is clean at HEAD. It should reuse
     `--radius-full` (matching `.gbar`/`.cp-bar`) and a `--dur-*` token.
  2. **Adoption gap.** The rich `.ol-*` primitive library exists but live views
     (`AppShell`, `FleetView`, `ChatView`, `ProjectsView`) still lean on bespoke
     `index.css` classes. Migrating views onto `.ol-*` / `shell.tsx` is the big
     consistency win — a view-worker task the design-lead should *spec + spot-fix*.
  3. **Formal contrast audit** (axe/WAVE) across both themes, especially
     `--text-faint`, `--text-dim`, and status inks on their washes.
  4. **`.ol-*` visual QA in-browser** in both themes — the primitives were built
     to spec but haven't all been screenshot-verified against the live views.

### 2026-07-03 — Formalize the half-step control type tier (`--fs-10-5 … --fs-13-5`) + repoint all 18 sub-pixel font sizes — closes the sub-pixel drift debt

- **The debt this closes (top in-lane item, prior run's debt #2):** `index.css`
  carried **18** raw half-pixel `font-size` literals — `10.5px` (×2), `11.5px`
  (×10), `12.5px` (×4), `13.5px` (×2) — on `.vp-title`, `.gv-title`, `.dr-title`,
  `.btn`, `.dtab`, `.bp-tab`, `.bp-body`, `.tc-out`, `.ovl-it`, `.todo`, `.cp-row`,
  `.cp-row .v`, `.stat .l`, `.md`, `.md td`, `.msg-user`, `.msg-ai pre code`, and
  `.pal-r`. They had **no matching `--fs-*` token**, so the file's type layer was
  the last scale still leaking raw values. Notably these are **not** accidental
  drift: `typography.css` already documents a deliberate "14px body / **12–12.5px
  controls**" dense-control tier — the half-steps were an intended tier that was
  simply never given tokens.
- **Fix (system-level, in the token layer — no view internals touched):**
  - **`design/tokens/typography.css`** — promoted the half-step tier to first-class
    tokens interleaved into the existing scale: `--fs-10-5` (10.5px, stat labels /
    dense mono values), `--fs-11-5` (11.5px, tabs / tool output / list+todo rows /
    buttons), `--fs-12-5` (12.5px, view+drawer titles / palette rows), `--fs-13-5`
    (13.5px, user-message + markdown body). Each carries a role comment.
  - **`index.css`** — repointed all 18 sites to the new tokens (e.g.
    `font-size: 12.5px` → `var(--fs-12-5)`).
- **Behavior:** **exactly zero pixels move** — every new token equals the literal
  it replaces (10.5→10.5px, etc.). This is pure tokenization: the dense-control
  tier is now theme-/scale-addressable like every other size, and the type layer
  of `index.css` reads only `var(--fs-*)`. A future density or accessibility pass
  can now rescale the control tier from one place instead of hunting 18 literals.
- **Verified:** `cd ui && bun run typecheck` (exit 0) and `bun run build` (exit 0,
  CSS 61.47 kB). Screenshotted the live app in **both** themes — **obsidian**
  (dark) and **daybreak** (light): `.gv-title` ("Sessions"), empty-state copy,
  topbar controls, and the "New session" button all render legibly with identical
  sizing and no contrast/layout regression. Fully reversible (this file +
  `typography.css` + `index.css` type repoints).
- **Top design debts now visible (next runs, in priority order):**
  1. **Adoption gap.** The rich `.ol-*` primitive library exists but live views
     (`AppShell`, `FleetView`, `ChatView`, and the new `ProjectsView`/`ContextRing`)
     still lean on bespoke `index.css` classes. Migrating views onto `.ol-*` /
     `shell.tsx` is the big consistency win — but it edits view internals, so it's
     a view-worker task the design-lead should *spec + spot-fix styling for*.
  2. **Off-scale raw `Npx` in newly-landed view CSS.** The in-flight `ContextRing`
     block (`.ctx-*`) and a few older rules still carry raw geometry
     (`border-radius: 2px`, `height: 3px`, `gap: 6px`, `padding: 9px 11px`,
     `width: 48px`, `transition: width 0.3s`). A radius/space/motion tokenization
     sweep of the new `.ctx-*`/`.kcard`/`.step` clusters mirrors the earlier passes.
  3. **Formal contrast audit** (axe/WAVE) across both themes, especially
     `--text-faint`, `--text-dim`, and status inks on their washes.
  4. **`.ol-*` visual QA in-browser** in both themes — the primitives were built
     to spec but haven't all been screenshot-verified against the live views.

### 2026-07-03 — Tokenize the last raw literals in `index.css` (vault editor / ask-input / tab-close) — closes debt #2

- **The debt this closes (was #2 in the visible list):** `index.css` was fully
  tokenized in the 2026-07-02 sweeps *except* three blocks added later — the
  vault-view CSS (`.vault-editor`, `.vault-ask-input`) and the detail-tab close
  button (`.tab-close`). These still carried raw literals that predate the
  modular scales: `border: 1px solid`, bare `border-radius: 6px`/`2px`,
  `font-size: 12px`, off-scale `padding: 12px 14px`, and a raw `line-height: 1.7`.
  They were the file's only remaining off-scale holdouts.
- **Fix (system-level, in `index.css` — no view internals touched):**
  - `.vault-editor` — `border` → `var(--border-w) solid …`; `border-radius: 6px`
    → `var(--radius-md)` (identical 6px); `font-size: 12px` → `var(--fs-12)`
    (identical); `padding: 12px 14px` → `var(--space-6)` (snaps the odd 14px x-axis
    onto the 12px step — a ≤2px, imperceptible tightening onto scale);
    `line-height: 1.7` → `var(--lh-relaxed)` (1.65, the body-prose token).
  - `.vault-ask-input` — `font-size: 12px` → `var(--fs-12)` (identical).
  - `.tab-close` — `border-radius: 2px` → `var(--radius-sm)` (3px; a 1px snap onto
    the smallest radius token — this control had no matching 2px token).
- **Behavior:** the `--radius-md`/`--fs-12` repoints are **exactly value-identical**
  (zero pixel delta). The three snaps (`14px→12px` pad x-axis, `1.7→1.65` lh,
  `2px→3px` radius) are all ≤2px / ≤0.05 and imperceptible; they bring the last
  three blocks onto the canonical scale so the whole file now reads only
  `var(--token)` for radius, border-width, font-size, and on-scale padding.
- **Verified:** `cd ui && bun run typecheck` (exit 0, clean — the previously-flagged
  `Icon.test.tsx` error is gone) and `bun run build` (exit 0, CSS 59.40 kB). Fully
  reversible (this file + the changelog). Debt #2 is now closed.
- **Top design debts now visible (next runs, in priority order):**
  1. **Adoption gap.** The rich `.ol-*` primitive library exists but live views
     (`AppShell`, `FleetView`, `ChatView`) still lean on bespoke `index.css`
     classes. Migrating views onto `.ol-*` / `shell.tsx` is the big consistency
     win — but it edits view internals, so it's a view-worker task the design-lead
     should *spec + spot-fix styling for*, not rewrite wholesale.
  2. **Sub-pixel font-size drift.** A cluster of `12.5px`/`13.5px`/`11.5px`/`10.5px`
     literals remain on `.vp-title`, `.gv-title`, `.dr-title`, `.btn`, `.dtab`,
     `.bp-tab`, `.msg-user`, `.md`, `.stat .l`, etc. — half-pixel sizes with no
     matching `--fs-*` token. Either add `--fs-11-5`/`--fs-12-5`/`--fs-13-5` half
     steps to `typography.css` and repoint, or snap each to the nearest existing
     step. A single focused type-scale pass would close it.
  3. **Formal contrast audit** (axe/WAVE) across both themes, especially
     `--text-faint`, `--text-dim`, and status inks on their washes.
  4. **`.ol-*` visual QA in-browser** in both themes — the primitives were built
     to spec but haven't all been screenshot-verified against the live views.

### 2026-07-03 — Full reconciliation: rewrite §1–§10 to the shipped modular token system (silver, obsidian/light, `design/tokens/*`)

- **The debt this closes (was the #1 flagged item):** since the `b9a1b0e`
  redesign, this doc's §1–§10 described a **cyan**, **three-theme**
  (`midnight`/`daylight`/`amber-crt`), `--font-*`/`--green,--amber,--red`,
  single-`index.css` system that **no longer exists in any file.** The prior run
  added a ⚠️ STATUS banner deferring the rewrite because a UI refactor was in
  flight and `bun run typecheck` was **red**. That refactor has **landed and is
  committed** (TanStack Query + Zustand + router; `AppShell`/`FleetView`/`ChatView`
  live), the working tree is clean, and **`bun run typecheck` + `bun run build`
  are both green** — the deferral condition is cleared, so the promised
  reconciliation is done.
- **What the doc now documents (verified against the shipped source):**
  - **Location:** the token layer is modular under `ui/src/design/tokens/*`
    (`colors/typography/spacing/radius/motion/fonts/base.css`), stitched by
    `design/styles.css`, with `.ol-*` primitives in `design/styles/components.css`
    and an **alias block** in `index.css` mapping legacy short names
    (`--silver`→`--accent`, `--green`→`--ok`, …) to canonical tokens.
  - **§1 anchor:** silver signal accent (`#C9C9C9`), neutral near-black cockpit,
    **two** themes — `obsidian` (dark default) + `light` (daybreak) via
    `[data-theme]`, managed by `ThemeProvider`.
  - **§2 color:** the real full inventory — surfaces (5), borders (3), text (3),
    accent (5 + 5 alphas), status `--ok`/`--warn`/`--err` (each + `-ink`/`-wash`/
    `-line`), 9 `--src-*` channel hues, utility — with **both** theme values.
  - **§3 type:** `--font-display/-sans/-mono`, the **12-step** `--fs-9…--fs-40`
    scale, `--fw-*` weights, `--lh-*` line-heights, `--tracking-*`.
  - **§4 spacing:** the `--space-0…--space-32` step scale + the semantic layout
    constants and their compact overrides + `--measure`/`--measure-msg`.
  - **§5 radius/elevation:** `--radius-sm…-full`; **corrected the old "no drop
    shadows" claim** — floating chrome uses `--shadow-pop/-modal/-float`;
    documented `--ring*`, `--border-w`, `--opacity-disabled` (0.42).
  - **§6 motion:** `--dur-*`, `--ease/-out/-in-out`, loop durations, the five
    `olympus-*` keyframes, reduced-motion.
  - **§7 density:** `comfortable`/`compact` (layout constants only).
  - **§8 components:** documented the previously-**undocumented** `.ol-*`
    primitive library (24 primitives with states) as layer A, the `shell.tsx`
    React primitives as layer B, and the `index.css` app-shell vocabulary as
    layer C.
  - **§9 a11y** and **§10 do/don't** re-pinned to silver / two-theme / IBM Plex /
    `--fs-*` / `.ol-*` reality.
- **Removed** the obsolete ⚠️ STATUS banner (its blocking condition is resolved
  and its "intent, not code" caveat no longer applies — §1–§10 are now the code).
- **Scope:** **doc-only** — no CSS, token, primitive, or view code touched, so
  HEAD's build is unaffected. The `typecheck`+`build` gate was nonetheless run
  green first, both to confirm main isn't broken and to confirm the
  reconciliation precondition (a green tree) that the prior run was waiting on.
  Fully reversible (this file only). Changelog history below is preserved as the
  design audit trail.
- **Top design debts now visible (next runs, in priority order):**
  1. **Adoption gap.** The rich `.ol-*` primitive library exists but the live
     views (`AppShell`, `FleetView`, `ChatView`) still lean on bespoke `index.css`
     classes. Migrating views onto `.ol-*` / `shell.tsx` primitives is the big
     consistency win — but it edits view internals, so it's a view-worker task the
     design-lead should *spec + spot-fix styling for*, not rewrite wholesale.
  2. **`index.css` still carries raw `Npx`** literals (odd/off-scale paddings,
     `12.5px`/`13.5px`/`11.5px` font sizes, a few inline radii) that predate the
     modular tokens — a tokenization sweep of `index.css` against the new scales
     mirrors the 2026-07-02 gap/padding passes.
  3. **Formal contrast audit** (axe/WAVE) across both themes, especially
     `--text-faint`, `--text-dim`, and status inks on their washes.
  4. **`.ol-*` visual QA in-browser** in both themes — the primitives were built
     to spec but haven't all been screenshot-verified against the live views.

### 2026-07-03 — Resurrect shell primitives against live CSS (fix the #1 flagged debt) + tokenize amber/red gtag fills

- **Problem (was the top debt in this doc's status banner):** after the
  `b9a1b0e` silver redesign, `shell.tsx` still emitted the OLD class vocabulary
  (`.page-header`, `.stat-pill`, `.empty-state-message`, `.badge`/`.badge-*`)
  that **no longer exists** in the shipped `index.css`. Every shared primitive
  — `PageHeader`, `EmptyState`, `StatPill`, `PlaceholderBadge`, `Badge` —
  therefore rendered **completely unstyled**, silently breaking the exact
  "every view uses shell primitives consistently" contract VISION v0.2 depends
  on. No live view imports them yet, so the break was latent — the first view
  worker to adopt a primitive would have shipped naked markup.
- **Fix:** rewrote `shell.tsx` to emit the **live** class names, same public
  component API (drop-in, zero call-site changes):
  - `PageHeader` → `.gv-head` / `.gv-title` / `.gv-sub` / `.gv-actions`
  - `EmptyState` → `.empty-state` / `-icon` / `-title` / `-msg` / `-cta`
  - `StatPill` → `.stat` / `.v` / `.l`
  - `Badge` → `.gtag` + a `kind→variant` map (ready/running/done/online→`ok`,
    warning→`warn`, blocked/failed/error/offline→`err`)
  - `PlaceholderBadge` → `.gtag warn` (amber, matches its semantic)
- Added one missing rule `.empty-state-cta { margin-top:6px }` so the CTA slot
  has spacing (token-consistent 6px = the existing `--space-3` rhythm).
- **Also killed the only rule-level hardcoded hexes** feeding these primitives:
  `.gtag.warn`/`.gtag.err` inlined `#fcd34d1a`/`#fcd34d40`/`#fca5a51a`/`#fca5a540`.
  Added `--amber-wash`/`--amber-line`/`--red-wash`/`--red-line` to `:root`
  (value-identical to the literals they replace) and repointed both rules. The
  green variant already used `--green-wash`/`--green-line`; amber/red now match.
- **Behavior:** **zero pixels move** — the new tokens equal the old literals,
  and no view currently renders these primitives, so nothing on screen changes
  today. This is pure correctness restoration: the primitives now match the CSS
  a view worker will encounter.
- **Verified:** `bun run build` exits 0 (CSS 25.49→25.64 kB, exactly the added
  rule + tokens). `bun run typecheck` shows only the **pre-existing** unrelated
  error (`Icon.test.tsx` imports uninstalled `@testing-library/user-event`) —
  my changes add zero new type errors. Fully reversible.

### 2026-07-03 — Truth reconciliation: flag that the shipped CSS redesign has out-run this spec (doc-only; no code touched)

- **What happened:** commit `b9a1b0e` ("feat(ui): redesign to reference design
  system", 1896-line `index.css` rewrite) landed a **new visual direction** —
  **silver accent, a single `:root` (no `[data-theme]` blocks, so no
  midnight/daylight/amber-crt), and none of the `--font-*` / `--space-*` /
  `--tracking-*` / `--radius-*` / `--dur-*` scales** that the 2026-07-01/02
  changelog runs built up. Primitive class names also changed
  (`.page-header`→`.gv-head`, `.stat-pill`→`.stat`,
  `.empty-state-message`→`.empty-state-msg`). Net: **§2–§8 of this doc now
  describe a design system that no longer exists in code.**
  *(Note, 2026-07-03 later: the token scales were subsequently rebuilt as the
  modular `design/tokens/*` layer — see the reconciliation entry above. This
  entry's "scales dropped" observation was true only for the transient
  single-file state.)*
- **Why no token/primitive fix this run:** the working tree was mid-refactor and
  **uncommitted** and `cd ui && bun run typecheck` was **red**. A code/token edit
  cannot be verified while the tree won't compile; the correct move was to
  surface the divergence, not silently rewrite 800 lines against a moving target.
- **What this run did:** added a prominent ⚠️ STATUS banner marking §2–§8 as
  intent-not-code and pointing view workers to read `index.css` + `shell.tsx`
  directly until reconciliation. Doc-only; committed on its own path.

### 2026-07-02 — Transcript content gutter becomes a density-flexing constant (`--space-content-x`; fixes latent compact bug)

- Added a semantic `--space-content-x` (24px comfortable / 20px compact) and
  repointed the horizontal axis of the 7 transcript/search gutter rules to it, so
  the transcript tightens in compact mode like the rest of the cockpit. Zero
  pixel delta in comfortable mode. *(Historical: predates the modular token move;
  the current spacing model is in §4.)*

### 2026-07-02 — Tokenize on-scale padding (42 sites → --space-*)

- Repointed all on-scale `padding:` declarations to the `--space-*` step scale.
  Zero-pixel; each token equals the literal it replaced. *(Historical.)*

### 2026-07-02 — Card interiors use the semantic panel constants (fixes compact-density bug)

- Repointed five card interiors to `--space-panel`/`--space-panel-lg` so they flex
  under compact. *(Historical; the constants are now `--panel-pad`/`--panel-pad-lg`
  in §4.)*

### 2026-07-02 — Tokenize single-value margins (34 sites → --space-*)

- Repointed single-value even margins to the step scale. Zero-pixel. *(Historical.)*

### 2026-07-02 — Tokenize the gap spacing scale (0 → 8-step scale)

- Introduced the primitive `--space-*` step scale and migrated ~93 raw `gap:`
  literals to it. *(Historical; the scale is now `--space-0…--space-32` in §4.)*

### 2026-07-02 — Tokenize disabled-state opacity (0 → 1 token)

- Added `--opacity-disabled` and repointed all `:disabled` rules to it.
  *(Historical; current value 0.42, in §5.)*

### 2026-07-02 — Tokenize the tracking (letter-spacing) scale (0 tokens → 3)

- Collapsed drifting caps tracking to a principled scale.
  *(Historical; current tracking scale in §3.)*

### 2026-07-02 — Tokenize the radius scale (2 tokens → 4)

- Promoted the radius scale and killed raw radii.
  *(Historical; current radius scale in §5.)*

### 2026-07-01 — Fix amber-semantic fills mis-tinted with cyan accent (D5 + badge-warning)

- Repointed `.badge-warning` and `.placeholder-badge` off the cyan `--accent-dim`
  wash onto amber tokens. *(Historical — predates the silver redesign; there is no
  cyan accent or `--accent-dim` token today.)*

### 2026-07-01 — Initial creation (first design-lead run)

- Created `DESIGN_SYSTEM.md` and `VISION.md` from a live audit of the then-current
  `index.css` + `shell.tsx` + view files. *(Historical — described the pre-redesign
  cyan/three-theme system; superseded by the 2026-07-03 reconciliation above.)*
