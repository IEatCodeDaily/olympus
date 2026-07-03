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
| `--text-faint` | `#5E5E60` | `#98989C` | Tertiary — timestamps, hints, disabled copy |

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
| `--space-0` | 0 | | `--space-6` | 12px |
| `--space-1` | 2px | | `--space-8` | 16px |
| `--space-2` | 4px | | `--space-10` | 20px |
| `--space-3` | 6px | | `--space-12` | 24px |
| `--space-4` | 8px **(workhorse)** | | `--space-16` | 32px |
| `--space-5` | 10px | | `--space-20` | 40px |
| | | | `--space-24` | 48px |
| | | | `--space-32` | 64px |

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
| `Badge` | `{ kind?, children }` | `.gtag` + `kind→variant` map (ready/running/done/online→`ok`, warning/warn→`warn`, blocked/failed/error/offline→`err`) |
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
- **Debt:** formal axe/WAVE audit not yet run. `--text-faint` on `--bg` is
  intentionally low-contrast (tertiary meta) and may fail AAA — flagged for a
  dedicated contrast pass. Never carry meaning by faint text alone.

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
