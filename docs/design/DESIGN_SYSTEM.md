# Olympus Design System

> **Canonical reference for every visual decision in Olympus.**
> Owned by the `design-lead` agent. All tokens live in `ui/src/index.css`;
> all shared primitives in `ui/src/components/shell.tsx`.
> **Rule: never hardcode a hex — only `var(--token)`.**

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

- **Dark default** (`midnight` theme).
- **Typefaces:** IBM Plex Sans (UI) + IBM Plex Mono (data/code/meta).
- **Accent:** Electric cyan in midnight; blue-700 in daylight; warm amber in amber-crt.
- **NOT:** generic AI slop — no Inter, no purple gradients, no Material Design.
- **Every visual value is a CSS custom property** scoped to `[data-theme]`.

### Themes (3 shipped)

| Theme | `data-theme` value | Mood |
|-------|-------------------|------|
| **Midnight** | `midnight` | Deep dark, cool, default. Near-black `#090b0e` base. |
| **Daylight** | `daylight` | Clean light. Off-white `#f7f8fa` base. Blue accent. |
| **Amber CRT** | `amber-crt` | Warm phosphor terminal. Dark amber `#120d08` base. |

Extensible: add a new `[data-theme="..."]` block to `index.css` redefining every
token below.

---

## 2. Color Tokens

All tokens are defined per-theme in `ui/src/index.css`. Reference as
`var(--token-name)`.

### Surface tokens (backgrounds)

| Token | Midnight | Daylight | Amber CRT | Usage |
|-------|----------|----------|-----------|-------|
| `--bg` | `#090b0e` | `#f7f8fa` | `#120d08` | Page / list background |
| `--bg-elev` | `#101318` | `#ffffff` | `#1a1308` | Elevated panels, cards, sidebar |
| `--bg-elev2` | `#161a21` | `#eef1f5` | `#221a0c` | Secondary elevation (inputs, nested) |
| `--bg-hover` | `#1c2129` | `#e4e8ee` | `#2c2210` | Row / item hover state |

### Border tokens

| Token | Midnight | Daylight | Amber CRT | Usage |
|-------|----------|----------|-----------|-------|
| `--border` | `#1e252e` | `#dce1e8` | `#3a2c12` | Default borders (dividers, card edges) |
| `--border-bright` | `#2a3340` | `#c2cbd6` | `#4d3a18` | Emphasized borders (input focus ring, active edges) |

### Text tokens

| Token | Midnight | Daylight | Amber CRT | Usage |
|-------|----------|----------|-----------|-------|
| `--text` | `#e2e8f0` | `#1a2230` | `#ffd79a` | Primary text (headings, body, titles) |
| `--text-dim` | `#8595a8` | `#51607a` | `#c79a5c` | Secondary text (meta, descriptions) |
| `--text-faint` | `#5a6878` | `#8090a4` | `#8a6a3c` | Tertiary text (timestamps, hints, labels) |

### Semantic color tokens

| Token | Midnight | Daylight | Amber CRT | Role |
|-------|----------|----------|-----------|------|
| `--accent` | `#7dd3fc` | `#0284c7` | `#ffb347` | Primary action / selection / links |
| `--accent-hover` | `#a5e0ff` | `#0369a1` | `#ffc870` | Accent hover state |
| `--on-accent` | `#0a0e14` | `#ffffff` | `#1a1308` | Text on solid accent backgrounds |
| `--green` | `#86efac` | `#16a34a` | `#b6e36b` | Success / running / managed / AI role |
| `--amber` | `#fcd34d` | `#b45309` | `#ffcf5c` | Warning / tool role / fork badge |
| `--red` | `#fca5a5` | `#dc2626` | `#ff8a6b` | Error / blocked / failed / stop button |

### Alpha-derived tokens (theme-correct semi-transparent)

These prevent hardcoded `rgba()` values in components. Each is defined
per-theme so opacity/blending adapts to the theme's base colors.

| Token | Purpose |
|-------|---------|
| `--accent-dim` | Accent-tinted fill for accent-semantic surfaces (ready badge, brand mark, fork btn, jump-latest). NOT for amber/green/red elements — use their own `*-soft`. |
| `--accent-subtle` | Faint wash (user message gutter) |
| `--accent-soft` | Badge fill / live indicator bg |
| `--accent-border` | Decorative accent borders |
| `--accent-border-strong` | Emphasized accent borders (live badge) |
| `--accent-hover-fill` | Hover backgrounds with accent tint |
| `--accent-glow` | Pulse dot shadow / glow effects |
| `--green-soft` | Managed / AI badge fill |
| `--green-border` | Green decorative borders |
| `--green-glow` | Connected status-dot shadow |
| `--amber-subtle` | Tool message wash |
| `--amber-soft` | Fork / tool badge fill |
| `--amber-border` | Amber decorative borders |
| `--amber-highlight` | Search hit highlight background |
| `--red-soft` | Error badge fill |
| `--red-border` | Error decorative borders |
| `--hover-subtle` | Row hover wash (very faint) |
| `--scrollbar-thumb-hover` | Scrollbar thumb on hover |
| `--spinner-track` | Spinner track (loading indicators) |

---

## 3. Typography Scale

Two typeface families:

| Token | Value | Usage |
|-------|-------|-------|
| `--sans` | `"IBM Plex Sans", -apple-system, system-ui, sans-serif` | All UI text (nav, labels, prose) |
| `--mono` | `"IBM Plex Mono", "JetBrains Mono", "SF Mono", "Cascadia Code", monospace` | Code, meta, timestamps, badges, data |

### Font size scale (15 steps, named by role)

| Token | Size | Weight | Line-height | Role |
|-------|------|--------|-------------|------|
| `--font-xs` | 9px | inherited | inherited | Tiny tags: managed/fork, section labels |
| `--font-sm` | 10px | inherited | inherited | Micro: role badges, source pills, placeholder badge |
| `--font-sm-up` | 10.5px | inherited | inherited | Kicker labels, stat-pill labels, card age |
| `--font-base` | 11px | inherited | inherited | Mono meta: timestamps, status, counts, column hints |
| `--font-base-up` | 11.5px | inherited | inherited | Card secondary meta, filter chips, usage values |
| `--font-md` | 12px | inherited | inherited | Controls: sort, toggles, snippets, settings hints, inputs |
| `--font-md-lg` | 12.5px | inherited | inherited | Subtitles, settings rows, primary/ghost buttons |
| `--font-lg` | 13px | 500 | inherited | UI default: nav items, row titles, headings, chat empty/error |
| `--font-lg-up` | 13.5px | inherited | 1.6 | Body prose: message content, composer input |
| `--font-xl` | 14px | inherited | inherited | Chat title, empty-state title, stat-pill value, search input |
| `--font-xxl` | 15px | 600 | inherited | Brand name, section headers, content H1-H3, workflow panel title |
| `--font-xxxl` | 17px | 600 | inherited | Sub-page title: node detail title |
| `--font-page` | 18px | 600 | inherited | Page title H1, board detail title |

**Weight convention:**
- `400` (normal): body prose, descriptions
- `500`: nav items, row titles, filter chips, interactive labels
- `600`: page titles, brand, card titles, headings
- `700`: role badges, status badges, mono tags (managed/fork/live)

### Tracking (letter-spacing) scale

Letter-spacing is theme-agnostic (glyph geometry doesn't change per theme).
Only **three** intents exist — never hardcode a raw `em` value:

| Token | Value | Usage |
|-------|-------|-------|
| `--tracking-tight` | -0.01em | Display / page titles (`.page-title`, `.brand-name`) — large weights read compact and editorial with a hair of negative tracking |
| `--tracking-caps` | 0.05em | **THE** tracking for every uppercase mono label / badge / status / column title / field label. One value = zero drift |
| `--tracking-caps-wide` | 0.06em | Eyebrow / kicker labels only (`.node-detail-kicker`, `.workflow-panel-kicker`, `.composer-assign-label`) — the smallest caps text wants the most tracking to read as a section marker |

**Rule:** all-caps labels use `--tracking-caps`. Tiny eyebrow kickers use
`--tracking-caps-wide`. Display titles use `--tracking-tight`. Sentence-case
body text sets no letter-spacing (default `normal`).

---

## 4. Spacing & Layout

### Layout constants

| Token | Default | Compact | Usage |
|-------|---------|---------|-------|
| `--sidebar-w` | 220px | 220px | Sidebar width (fixed) |
| `--space-view-y` | 20px | 16px | Vertical padding inside view scroll area |
| `--space-view-x` | 24px | 20px | Horizontal padding inside view scroll area |
| `--space-toolbar-y` | 12px | 10px | Toolbar vertical padding |
| `--space-toolbar-x` | 16px | 14px | Toolbar horizontal padding |
| `--space-nav-y` | 8px | 6px | Nav item vertical padding |
| `--space-nav-x` | 12px | 10px | Nav item horizontal padding |
| `--space-panel` | 12px | 10px | Inner panel/card padding (small) |
| `--space-panel-lg` | 16px | 14px | Inner panel/card padding (large) |
| `--page-gap` | 18px | 14px | Gap between page header and content |
| `--session-row-h` | 68px | 60px | Session row height |

### Spacing STEP scale (inter-element gaps)

A primitive 2px-base rhythm for ad-hoc `gap:` between siblings — **distinct**
from the semantic layout constants above (those name a structural role and flex
under `[data-density]`; the step scale is fixed geometry, theme- and
density-agnostic).

| Token | Value | Typical use |
|-------|-------|-------------|
| `--space-1` | 2px | Tightest: nav-item list gap, swatch-meta stack |
| `--space-2` | 4px | Tool-call list, density-btn label stack, back-btn icon |
| `--space-3` | 6px | Status panel, source filters, msg meta, composer assign |
| `--space-4` | 8px | **The workhorse** — search box, page-header actions, board cards, skeletons, badges row |
| `--space-5` | 10px | Brand, nav-item content, toolbar controls, session rows, node/usage lists |
| `--space-6` | 12px | Toolbar row, message gutter, board columns/toolbar, settings panel |
| `--space-7` | 14px | Page-gap (compact), board/node detail cards, workflows layout |
| `--space-8` | 16px | Widest: chat/search header, page header, board view, settings token/row |

**Rule:** every `gap:` **and every on-scale `padding:`** uses a `--space-*` step
(or a semantic layout constant where one fits). Never a raw `Npx` gap or padding.
Steps are even (2px rhythm); if a design needs an in-between value, round to the
nearest step rather than introducing a new literal.

**Padding coverage:** the even-rhythm padding values (`2/4/6/8/10/12/14/16px`,
single- or multi-value, `0` mixed in) are all tokenized to `--space-*`. Padding
values that are **odd or off-scale** (`1/3/5/7/9/11/13px`) or **large structural
one-offs** (`20/24/40/42/48px` — view-scroll gutters, empty-state insets) remain
raw by design: they don't map to a step, and rounding them would move real
pixels. Those large gutters want their own semantic constants in a later pass.

**Known exception (deliberate, not drift):** `.settings-rows { gap: 1px }` is a
load-bearing hairline-divider primitive (1px gap over a border-colored
background paints the row separators) — it is *not* a spacing gap and stays raw.
A small odd-value tail (`3/5/7/9px`) on a few chips/metrics also remains raw
pending a follow-up rounding pass; rounding them now would shift real pixels, so
they were left untouched to keep this change zero-pixel.

### Spacing philosophy
- Use the token variables above — never raw pixel values for padding/margin/gaps.
- The gap between sibling elements should match the most specific applicable token.
- `--space-panel` / `--space-panel-lg` are the "atomic unit" for card/panel
  interiors — and they **flex under `[data-density="compact"]`**. A card
  interior MUST use these constants, never a raw `12px`/`16px`, or it won't
  tighten in compact mode.
- Density mode (`[data-density="compact"]`) reduces all spacing by ~20%.

---

## 5. Radius & Elevation

| Token | Value | Usage |
|-------|-------|-------|
| `--radius-xs` | 3px | Tiny: mono tags (managed/fork), inline-code, search-hit highlight, skeleton rails, theme swatch |
| `--radius-sm` | 4px | Small: badges, pills-in-cards, usage/slot bars, scrollbar thumb, small buttons |
| `--radius` | 6px | Default: cards, panels, inputs, buttons, search boxes, composer selects |
| `--radius-full` | 999px | Fully-round: source pills, filter chips, workflow/node chips, jump-latest, source badges |

A `50%` literal is used for equal-sided **circles** (status dots, live dots, brand-mark-adjacent dots) — that's a geometric primitive, not an arbitrary radius, so it stays inline.

**Rule:** never hardcode a raw radius (`3px`, `20px`, …) in a rule — pick the
nearest scale token. Pill-shaped elements use `--radius-full`, not a large px
value; it renders identically at any height and reads as intentional.

**Elevation model:** No drop shadows. Depth is communicated through:
1. Background layering (`bg` → `bg-elev` → `bg-elev2`)
2. Border presence and brightness (`border` vs `border-bright`)
3. Subtle hover-state background changes

This keeps the cockpit flat, scannable, and terminal-like.

---

## 6. Motion System

### Duration tokens

| Token | Value | Use case |
|-------|-------|----------|
| `--dur-1` | 80ms | Immediate: row hover, message hover (direct manipulation) |
| `--dur-2` | 120ms | Fast: nav focus, border color shifts, pills, lightweight controls |
| `--dur-3` | 150ms | Base: buttons, cards, composer, panels, settings |

### Easing tokens

| Token | Value | Use case |
|-------|-------|----------|
| `--ease-standard` | `ease` | Default control transitions |
| `--ease-in-out` | `ease-in-out` | Looping animations (skeleton shimmer, thinking bounce) |

### Rules
- **Max interaction transition: 150ms.** Nothing feels sluggish.
- **Looping keyframe animations** define their own duration (characteristic rhythm):
  - `shimmer`: 1.3s (skeleton loading)
  - `thinking-bounce`: 1.2s (agent thinking dots)
  - `pulse`: 1.5s (streaming tag blink)
  - `spin`: 0.7s–1s (spinner, tool-running indicator)
  - `olympus-pulse`: 1.6s (live dot pulse)
- **Reduced motion:** `@media (prefers-reduced-motion: reduce)` collapses all
  transitions to instant and all looping animations to their final frame.

---

## 7. Density Modes

Two modes via `[data-density]` on `<html>`:

| Mode | `data-density` | Feel |
|------|---------------|------|
| **Comfortable** (default) | omitted or `"comfortable"` | Standard spacing, roomy rows |
| **Compact** | `"compact"` | ~20% tighter spacing, shorter rows |

Compact mode overrides all `--space-*` and `--session-row-h` tokens. No other
tokens change — font sizes, radii, colors stay identical.

---

## 8. Component Inventory

All shared primitives are in `ui/src/components/shell.tsx`. View-specific
components live in each `views/*.tsx` file but must use design-system tokens
and follow these patterns.

### 8.1 PageHeader

```
Props: { title: string, subtitle?: string, actions?: ReactNode }
States: default (title + optional subtitle + actions row)
CSS classes: .page-header, .page-title, .page-subtitle, .page-header-actions
```

**Pattern:** Every view opens with `<PageHeader>`. Title uses `--font-page` (18px, 600).
Actions slot accepts buttons, stat pills, badges.

### 8.2 EmptyState

```
Props: { icon?: ReactNode, title: string, message?: string, cta?: ReactNode }
States: default (centered, vertical stack), with icon, with CTA
CSS classes: .empty-state, .empty-state-icon, .empty-state-title,
             .empty-state-message, .empty-state-cta
```

**Pattern:** Used when a list/board has zero items. Icon at 50% opacity.
Title in `--text-dim`, message in `--text-faint`. CTA button in `.empty-state-cta`.

### 8.3 PlaceholderBadge

```
Props: { epic: string }
States: default (shows "placeholder · {epic}")
CSS class: .placeholder-badge
```

**Pattern:** Signals a view whose backend epic hasn't landed yet. Amber mono text.

### 8.4 StatPill

```
Props: { label: string, value: ReactNode }
States: default
CSS classes: .stat-pill, .stat-pill-value, .stat-pill-label
```

**Pattern:** Metric chip. Elevated background, border, mono value in `--font-xl`,
uppercase label in `--font-sm-up`.

### 8.5 Badge

```
Props: { kind?: string, children: ReactNode }
Kind variants: todo, ready, running, warning, blocked, failed, done (or unstyled)
States: default, todo, ready, running, warning, blocked, failed, done
CSS classes: .badge, .badge-{kind}
```

**Pattern:** Status pill. Mono, uppercase, small. Each `kind` maps to semantic
color tokens (green=running, red=blocked/failed, etc.).

### 8.6 Button variants

Not a shell primitive yet (inline in views), but follow this contract:

| Class | Variant | Bg | Text | Border | Radius |
|-------|---------|-----|------|--------|--------|
| `.btn-primary` | Primary action | `--accent` | `--on-accent` | none | `--radius-sm` |
| `.new-chat-btn` | Emphasized primary | `--accent` | `--on-accent` | none | `--radius-sm` |
| `.board-ghost-btn` | Secondary | `--bg-elev` | `--text-dim` | `--border` | `--radius-sm` |
| `.composer-send` | Icon send | `--accent` | `--bg` | none | `--radius-sm` |
| `.composer-stop` | Stop (destructive) | `--red` | `--bg` | none | `--radius-sm` |
| `.composer-fork-btn` | Fork (outline) | transparent | `--accent` | `--accent-border` | `--radius-sm` |
| `.back-btn` | Ghost back | transparent | `--text-dim` | `--border-bright` | `--radius-sm` |
| `.settings-action-btn` | Settings action | `--bg` | `--text-dim` | `--border-bright` | `--radius-sm` |
| `.source-pill` | Filter pill | transparent | `--text-dim` | `--border-bright` | 20px (pill) |
| `.nodes-filter` | Filter chip | `--bg-elev` | `--text-dim` | `--border` | 999px (pill) |
| `.workflow-filter` | Workflow filter | transparent | `--text-dim` | `--border` | 999px (pill) |

**Button states (all variants):** default → `:hover` (brighten/tint) → `:active`
→ `:disabled`. Every `:disabled` control uses **`opacity: var(--opacity-disabled)`**
(0.45) + `cursor: not-allowed` — one token so a non-interactive button/input reads
identically everywhere. Never inline a raw opacity for a disabled state; use the
token. (Icon-dimming, hover, and keyframe opacities are distinct and stay inline.)

### 8.7 Input / Field patterns

| Class | Type | Focus treatment |
|-------|------|----------------|
| `.search-box` | Search input with icon | `border-color: --accent` on `:focus-within` |
| `.search-input-wrap` | Full-width search | Same as above |
| `.composer-input-row` | Composer textarea container | `border-color: --accent` on `:focus-within` |
| `.board-field input` | Board form field | `outline: none; border-color: --accent` on `:focus` |
| `.sort-select` | Native select dropdown | `border-color: --accent` on `:hover` |
| `.composer-assign-select` | Model picker select | `border-color: --accent` on `:focus` |

### 8.8 Card / Panel patterns

| Class | Context | Hover |
|-------|---------|-------|
| `.board-card` | Kanban card | `translateY(-1px)`, border brightens |
| `.node-card` | Fleet node card | `translateY(-1px)`, border brightens |
| `.workflow-list-item` | Workflow list item | Background elevates, border brightens |
| `.tool-call-card` | Tool call expandable | Background elevates on header hover |
| `.node-detail-metric` | Metric box inside detail | No hover (read-only) |
| `.stat-pill` | Metric chip | No hover (read-only) |

### 8.9 Loading states

| Class | Context | Animation |
|-------|---------|-----------|
| `.skel-row` / `.skel-line-*` | Session list skeleton | `shimmer` gradient sweep |
| `.skel-msg` / `.skel-line` | Chat transcript skeleton | `shimmer` gradient sweep |
| `.board-skeleton-card` / `.board-skeleton-line-*` | Board skeleton | Solid placeholder blocks |
| `.node-skel-*` | Node grid skeleton | Gradient sweep |
| `.workflow-skeleton-card` / `.workflow-skeleton-step` | Workflow skeleton | Solid placeholder blocks |
| `.thinking-indicator` / `.thinking-dot` | Agent thinking | `thinking-bounce` (staggered dots) |
| `.composer-spinner` | Composer sending | `spin` (0.7s) |
| `.search-spinner` | Search in flight | `spin` (0.8s) |

### 8.10 Message / Chat components

| Class | Role | Visual signature |
|-------|------|-----------------|
| `.msg-user` | User message | Cyan left border + subtle cyan bg wash |
| `.msg-assistant` | AI reply | Green left border |
| `.msg-tool` | Tool call/result | Amber left border + amber bg wash |
| `.role-badge.role-user` | User label | Cyan filled pill |
| `.role-badge.role-ai` | AI label | Green filled pill |
| `.role-badge.role-tool` | Tool label | Amber filled pill |
| `.reasoning-content` | Model reasoning | Neutral bg, hairline left rule, italic |

---

## 9. Accessibility

### 9.1 Focus rings

Single global `:focus-visible` rule (index.css L208–211):

```css
:where(button, a, select, textarea, summary, [role="button"], [tabindex]):focus-visible {
  outline: 2px solid var(--ring);   /* --ring = var(--accent), auto-themes */
  outline-offset: 2px;
}
```

- Fires on keyboard/programmatic focus only — never on mouse click.
- Uses `:where()` for zero specificity — can't be overridden accidentally.
- Wrapped inputs (`.search-box`, `.composer`) route focus via `:focus-within`.

### 9.2 Reduced motion

```css
@media (prefers-reduced-motion: reduce) {
  * { animation-duration: 0.01ms !important; transition-duration: 0.01ms !important; }
}
```

All motion collapses instantly. Information is never carried by animation alone
— every animated state has a text label.

### 9.3 Color contrast

- Midnight theme: light-on-dark text passes WCAG AA for `--text`/`--bg-elev`.
- Daylight theme: dark-on-light text passes WCAG AA for `--text`/`--bg`.
- Amber CRT: warm-on-dark passes WCAG AA for `--text`/`--bg`.
- **Debt:** Formal contrast audit with axe/wave not yet run. `--text-faint` on `--bg`
  may fail AAA in some themes — flagged for v0.5 pass.

### 9.4 Semantic HTML

- Views use `<main>`, `<nav>`, `<aside>`, `<button>` (not div onclick).
- Icons are decorative SVGs (no aria-label needed unless standalone).
- Live regions for WS-driven updates should use `aria-live` (partial implementation).

---

## 10. Do / Don't Gallery

### ✅ Do

- **DO** use `var(--token)` for every color, spacing, font-size, radius, duration.
- **DO** add a new token to ALL THREE theme blocks when you need a new color/alpha.
- **DO** use `--font-{name}` tokens — never inline pixel values for font-size.
- **DO** use `--space-{name}` tokens for padding/margins in layout contexts.
- **DO** use `--dur-{n}` + `--ease-{name}` for transitions.
- **DO** use shell primitives (`PageHeader`, `EmptyState`, `StatPill`, `Badge`) instead of reinventing.
- **DO** keep hover transitions ≤150ms.
- **DO** provide text labels alongside color-coded states.
- **DO** test in midnight AND daylight after any visual change.
- **DO** use IBM Plex Mono for data, timestamps, badges, code.
- **DO** use `:focus-visible` for keyboard focus (don't add custom outline rules).

### ❌ Don't

- **DON'T** hardcode hex colors (`#fff`, `rgba(…)`) in components.
- **DON'T** use Inter, Roboto, or system-ui as the primary face (IBM Plex only).
- **DON'T** use purple gradients, glassmorphism, or neon glow effects.
- **DON'T** use drop shadows for elevation (use layering + borders).
- **DON'T** exceed 150ms for interaction transitions.
- **DON'T** use Material Design components or patterns.
- **DON'T** animate information-carrying properties without a static fallback.
- **DON'T** skip the compact density test after changing spacing.
- **DON'T** add `!important` to token usages (fix specificity properly).
- **DON'T** import a CSS framework (Tailwind, Bootstrap, etc.).

---

## 11. Changelog

### 2026-07-02 — Tokenize on-scale padding (42 sites → --space-*; closes the last raw-spacing drift class)

- **Problem:** the spacing scale was fully enforced for `gap:` and single-value
  `margin:`, but **`padding:` was still raw everywhere** — the last open member
  of the spacing drift class (radius, tracking, opacity, gaps, margins all
  already closed). §4 mandated "never raw pixel values for padding," yet 78 raw
  `padding:` shorthand declarations sat un-tokenized. "Which padding is correct
  here" was unenforceable and the workhorse values (`8px`, `10px`, `12px`,
  `16px`) drifted un-named across dozens of rules.
- **Fix:** repointed all **42 on-scale padding declarations** — every value (or
  every axis of a 2/3/4-value shorthand) that is `0` or an *exact* member of the
  existing `--space-*` step scale (`2/4/6/8/10/12/14/16px`) — to the matching
  token. Multi-value paddings convert per-axis (e.g. `10px 12px` →
  `var(--space-5) var(--space-6)`; `16px 16px 14px` →
  `var(--space-8) var(--space-8) var(--space-7)`). Done via a single scripted
  regex pass that **only** touched declarations where every non-zero axis mapped
  to a scale step, verified by dumping the before/after of all 78 sites.
- **Zero-pixel guarantee:** each token equals the literal it replaced — **not a
  single pixel moved** in comfortable mode. (These are the fixed step scale, not
  the density-flexing layout constants, so compact mode is unaffected too.)
- **Deliberately left raw (36 sites):** all-zero resets (`0`); odd/off-scale
  values (`1/3/5/7/9/11/13px` on chips, badges, metrics, small controls); and
  large structural one-offs (`20/24/40/42/48px` — view-scroll gutters,
  empty-state insets). None map to a step; rounding would shift real pixels. The
  large gutters are flagged in §4 as wanting their own semantic constants.
- **Scope guard:** only the `padding:` shorthand was touched — no color, layout,
  font, border, or view logic. `padding-top/right/bottom/left` longhands (none
  on-scale in this file) untouched.
- **Verified:** `bun run typecheck` + `bun run build` both exit 0 (CSS bundle
  52.69 kB). Browser screenshot in **both midnight (Sessions) and daylight
  (Settings)** — nav/toolbar/pills/session-rows and settings palette/density/
  token panels all evenly padded, zero regressions. Fully reversible.
- **Debt still open (next design runs):**
  - The large structural padding one-offs (`24px` view gutters, `40/48px`
    empty-state insets, `20px` chat/search sections) are the natural next move:
    they deserve **named semantic layout constants** (e.g. `--space-gutter`,
    `--space-empty-inset`) that flex under `[data-density]`, not step tokens.
    This is the same treatment `--space-view-*`/`--space-panel*` already got.
  - Odd padding tail (`3/5/7/9/11px`) on chips/badges/metrics wants a rounding
    pass once ±1px shifts are confirmed acceptable — mirrors the odd-gap and
    odd-margin tails left in prior runs.
  - `ChatView.tsx:512` still passes inline `borderRadius`/`fontSize` to a
    highlighter prop (view owner, not a system concern).

### 2026-07-02 — Card interiors use the semantic panel constants (fixes compact-density bug)

- **Problem:** §4 mandates that card/panel interiors are the "atomic unit" —
  `--space-panel` (12px) / `--space-panel-lg` (16px) — and those constants
  *flex under `[data-density="compact"]`* (12→10px, 16→14px). But five card
  rules had drifted to raw `padding: 12px` / `padding: 16px`, so they were
  **out of compliance with the doc AND silently broken in compact mode**: the
  cards kept their comfortable padding while every other panel tightened. A real
  density bug hiding inside the padding-literal debt, not cosmetic drift.
- **Fix:** repointed the five exact-match card interiors to the semantic panel
  constants they were supposed to use:
  - `.board-detail`, `.node-detail-card` → `padding: var(--space-panel-lg)` (was 16px)
  - `.node-card`, `.workflow-list-item`, `.workflow-run-card` → `padding: var(--space-panel)` (was 12px)
- **Behavior:** **zero pixel delta in comfortable mode** (each constant equals
  the literal it replaced); in compact mode these cards now correctly tighten to
  14px/10px alongside the rest of the cockpit. Net: a bug fix that is a no-op in
  the default view. No color, layout, font, or view logic touched — only
  `padding` on card containers, matched by full class-prefixed rule to avoid
  collateral hits.
- **Scope guard:** only the five cards whose raw value *exactly matched* a panel
  constant were converted. Asymmetric/two-value paddings (`10px 12px`,
  `8px 10px`, buttons, chips, toolbars) were left raw — they don't map cleanly to
  the panel constants and changing them would move real pixels; they remain the
  documented follow-up.
- **Verified:** `bun run typecheck` + `bun run build` both exit 0 (CSS bundle
  51.94 kB). Fully reversible (5 one-line edits).
- **Debt still open (next design runs):**
  - Remaining raw `padding:` literals (~85 sites) are mostly asymmetric
    two-value paddings on buttons/pills/toolbars/rows — they need a proper
    padding *scale* (or dedicated semantic tokens), not a blind sweep, since each
    changes box size. This is the natural next system-level move.
  - Odd single-value paddings (`3px`, `5px`, `7px`, `9px`) on chips/metrics
    still want a rounding pass once ±1px shifts are confirmed acceptable.

### 2026-07-02 — Tokenize single-value margins (41 → 10 raw px margins; 34 sites → --space-*)

- **Problem:** The gap-spacing run (previous) killed ~93 raw `gap:` literals, but
  **~41 single-value `margin:`/`margin-*` declarations** remained raw pixel
  values across the same CSS file — the same drift class (radius, tracking,
  opacity, gaps already closed). Every `margin-bottom: 8px`, `margin-top: 4px`,
  `margin-left: 6px`, etc. was unenforceable and drifting.
- **Fix:** repointed all **34 single-value even-margin** sites to the existing
  `--space-*` step scale (`--space-1: 2px` … `--space-8: 16px`). Each token
  equals the literal it replaced — **zero pixel delta**, fully reversible.
  Used regex-based substitution with per-rule context anchors for safety.
- **Deliberately excluded (7 remaining raw px margins):**
  - `0` (resets — not spacing)
  - `auto` (layout keyword — not a distance)
  - Odd values: `3px`, `5px` (sub-perceptual; rounding would shift pixels)
  - `28px` (settings-section — large structural margin, needs its own semantic token)
  - Multi-value margins: `6px 0 6px 20px`, `12px 0 6px`, `14px 16px`,
    `5px 24px 5px 42px`, `0 6px`, `8px 0`, `4px 0`, `6px 0 6px 20px`,
    `12px 0 6px`, `0 auto`, `6px 0 6px 20px` (asymmetric/compound — need
    individual property audit in a follow-up pass)
- **Scope guard:** only `margin` properties touched. No color, layout, font,
  padding, border, or view logic changed.
- **Verified:** `bun run typecheck` + `bun run build` both exit 0 (CSS bundle
  51.86 kB). Browser screenshot in **both midnight (Sessions) and daylight
  (Sessions)** — nav/toolbar/pills/session-rows/messages/composer/settings/
  board/nodes/workflows all evenly spaced, zero regressions.
- **Debt still open (next design runs):**
  - Raw `padding:` literals (~120 sites, many asymmetric two/four-value) are
    the same drift class but require per-property audit — unsafe to blind-sweep.
  - Remaining multi-value + odd + structural raw margins (~15 sites) want a
    follow-up pass once someone confirms ±1px shifts are acceptable.

### 2026-07-02 — Tokenize the gap spacing scale (0 → 8-step scale; kill ~93 raw gap literals)

- **Problem:** §4 mandated "never raw pixel values for gaps," but the CSS had
  **no primitive spacing-step scale** — only semantic layout constants
  (`--space-view-*`, `--space-panel*`, `--page-gap`). Every inter-element `gap:`
  was a raw literal: ~104 of them, drifting across `2/4/6/8/10/12/14/16px` (the
  even 2px rhythm) **plus** an off-rhythm `1/3/5/7/9px` tail. Same drift class
  the radius, tracking, and opacity scales already closed — "which gap is
  correct here" was unenforceable, and `gap: 8px` (the workhorse) sat un-named
  in 29 places.
- **Fix:** added an 8-token **step scale** in `:root` —
  `--space-1: 2px` … `--space-8: 16px` (theme- and density-agnostic; distinct
  from the semantic layout constants, which flex under `[data-density]`).
  Migrated every even-rhythm gap to its step via 8 `replace_all` passes
  (93 sites incl. one multi-value `gap: 8px 16px` → `var(--space-4) var(--space-8)`).
- **Zero-pixel guarantee:** only *exact* even values were repointed — each token
  equals the literal it replaced, so **not a single pixel moved**. The
  off-rhythm odd tail (`3/5/7/9px` on a few chips/metrics) was deliberately left
  raw rather than rounded, because rounding would shift real pixels; noted as a
  follow-up. `.settings-rows { gap: 1px }` is a load-bearing hairline-divider
  primitive (paints row separators) — explicitly excluded and documented.
- Documented the full 8-step scale in §4 with per-token usage, a "never a raw
  `Npx` gap" rule, and the two deliberate exceptions.
- **Verified:** `bun run typecheck` + `bun run build` both exit 0 (CSS bundle
  51.49 kB). Browser screenshot in **both midnight (Sessions) and daylight
  (Board)** — nav/toolbar/pills/session-rows and board columns/cards/stat-pills
  all evenly spaced, zero regressions. Fully reversible (8 token defs + N
  mechanical substitutions).
- **Debt still open (next design runs):** raw `padding:`/`margin:` literals
  (~120 sites) are the same drift class and the natural follow-up — but they
  change box size, so they need a per-property audit, not a blind sweep. The
  odd-gap tail (`3/5/7/9px`) wants a rounding pass once someone confirms the
  ±1px shifts are acceptable. `ChatView.tsx:512` still passes inline
  `borderRadius: "6px"` + `fontSize: "13px"` to a highlighter prop (view owner).

### 2026-07-02 — Tokenize disabled-state opacity (0 → 1 token; kill the .35–.55 drift)

- **Problem:** the "button states" contract in §8.6 claimed disabled controls sit
  at "opacity 0.4–0.5" but the CSS had drifted across **five** values for the same
  semantic state — `.35` (`.composer-send`), `.4` (`.btn-primary`), `.45`
  (`.composer-fork-btn`, `.board-ghost-btn`), `.5` (`.new-chat-btn`,
  `.composer-stop`), `.55` (`.composer-assign-input`) — 7 sites, none tokenized.
  A disabled Send looked meaningfully fainter than a disabled New-chat; "how
  inert does inert look" was unenforceable. Same drift class the radius and
  tracking scales already fixed.
- **Fix:** added a single **`--opacity-disabled: 0.45`** token in `:root`
  (theme-agnostic — opacity, not color) and repointed all 7 `:disabled` rules to
  it. 0.45 is the median of the old spread and sits inside the documented .4–.5
  band: dimmed enough to read as non-interactive, still legible.
- **Scope guard:** deliberately did NOT touch the distinct-intent opacities —
  `.empty-state-icon`/`.row-source` icon-dimming (.5/.8), the `pulse`/
  `thinking-bounce` keyframe opacities, `.group-open-icon` hover reveal (0→1),
  `.workflow-step-pending` (.84). Those aren't "disabled control" and each carries
  its own meaning; documented the boundary in the token comment + §8.6.
- **Behavior:** sub-perceptual shifts only — Send .35→.45 and assign-input .55→.45
  converge ±0.1 toward the median; the two .45 sites are unchanged. No color,
  layout, size, or view logic touched.
- **Verified:** `bun run typecheck` + `bun run build` both exit 0 (CSS bundle
  50.40 kB). Change is 8 one-line edits + 1 token; fully reversible.
- **Debt still open (for view owners, not touched):** `ChatView.tsx:512` still
  passes inline `borderRadius: "6px"` + `fontSize: "13px"` to a syntax-highlighter
  prop — should become `var(--radius)` / `var(--font-lg)` when that view is next
  edited.

### 2026-07-02 — Tokenize the tracking (letter-spacing) scale (0 tokens → 3)

- **Problem:** the type system defined size + weight scales but **no tracking
  scale**. Letter-spacing on uppercase labels had drifted across four raw
  values — `.03em` (×3), `.04em` (×7), `.05em` (×11), `.06em` (×3) — plus
  `-0.01em` (×2) on display titles, 26 sites total, none tokenized and no
  rationale for which caps label got which value. The signature editorial-
  terminal tracking on the cockpit's many all-caps mono labels (STORE, PROFILE,
  column titles, badges, kickers) was unenforceable and inconsistent.
- **Fix:** collapsed the drift to a principled **3-token scale** in `index.css`:
  `--tracking-tight: -0.01em` (display/page titles), `--tracking-caps: 0.05em`
  (the dominant value — now THE canonical tracking for every uppercase label /
  badge / status / column title / field label), `--tracking-caps-wide: 0.06em`
  (eyebrow kickers only — smallest caps text wants the most tracking). The
  scattered `.03`/`.04` caps values all snap to `--tracking-caps`; the `.06`
  kickers keep their wider tracking via `--tracking-caps-wide`.
- **Behavior:** the only perceptible shift is the former `.03`/`.04em` labels
  tightening/loosening by 0.01–0.02em to the unified `.05em` — sub-pixel at
  these sizes, and the point: caps labels now read identically everywhere.
  No color, layout, size, or view logic touched.
- Documented the full 3-token tracking scale in §3 with per-token usage + a
  "never hardcode a raw em; caps labels use `--tracking-caps`" rule.
- **Verified:** `bun run typecheck` + `bun run build` both exit 0; browser
  screenshot in **both midnight (Sessions) and daylight (Board)** — all caps
  labels evenly tracked and legible (CONTROL PLANE / PROFILE / STORE / IMPORT,
  source pills, TODO/READY/RUNNING/BLOCKED/DONE column titles, field labels),
  zero regressions. Fully reversible.

### 2026-07-02 — Tokenize the radius scale (2 tokens → 4; kill raw radii)

- **Problem:** §5 claimed a 2-token radius scale (`--radius`, `--radius-sm`),
  but the CSS hardcoded six raw radius values — `2px`, `3px`, `4px`, `6px`,
  `20px`, `999px` — across ~30 rules, none tokenized. Corners were unenforceable
  and drifting (two nearly-identical pill radii `20px`/`999px`; two stray `2px`).
- **Fix:** promoted the scale to **4 tokens** in `index.css`:
  `--radius-xs: 3px` (mono tags, inline-code, hit-highlight, skeleton rails,
  swatch), `--radius-sm: 4px` (badges, bars, small controls), `--radius: 6px`
  (cards/inputs/panels — unchanged default), `--radius-full: 999px` (all
  pill/round-chip shapes). Replaced every raw radius with the nearest token
  via 6 `replace_all` passes (~30 sites). `50%` circle literals (status/live
  dots) left inline as an intentional geometric primitive.
- **Behavior:** the only pixel deltas are two sub-perceptual `2px→3px` bumps on
  a skeleton rail and search-hit highlight; `20px→999px` renders identically on
  all ≤40px-tall pills (radius already caps at half-height). No color, no
  layout, no view logic touched.
- Documented the full 4-token scale in §5 with per-token usage + a "never
  hardcode a raw radius; pills use `--radius-full`" rule to prevent recurrence.
- **Verified:** `bun run typecheck` + `bun run build` both exit 0; browser
  screenshot in **both midnight and daylight** (Sessions + Board) — pills fully
  round, cards/inputs subtle-cornered, zero regressions. Fully reversible.
- **Noted for view owner (not touched):** `ChatView.tsx:512` passes an inline
  `borderRadius: "6px"` to a syntax-highlighter prop — should become
  `var(--radius)` when that view is next edited.

### 2026-07-01 — Fix amber-semantic fills mis-tinted with cyan accent (D5 + badge-warning)

- `.badge-warning` used `background: var(--accent-dim)` (a **cyan** wash) with
  `--border-bright` — the only colored badge whose fill didn't match its text
  color (running=green-soft, blocked/failed=red-soft). Repointed to
  `--amber-soft` + `--amber-border` so a warning reads as amber in all 3 themes.
- `.placeholder-badge` (debt **D5**) had amber text on the same cyan
  `--accent-dim` bg + neutral border → muddy amber-on-cyan. Repointed to
  `--amber-soft` + `--amber-border` (self-consistent amber). Closes D5.
- Documented the previously-undocumented `--accent-dim` token in §2 with a
  usage rule: it's the accent-tinted fill for *accent-semantic* surfaces only;
  amber/green/red elements must use their own `*-soft`. This guards against the
  same mis-tint recurring.
- No new tokens; existing per-theme amber tokens reused. No view logic touched.
  Fully reversible. Gate: `bun run typecheck` + `bun run build` both green.

### 2026-07-01 — Initial creation (first design-lead run)

- Created `DESIGN_SYSTEM.md` from live audit of `ui/src/index.css` (1624 lines),
  `ui/src/components/shell.tsx` (5 primitives), and all 8 view files.
- Full token inventory: 13 surface+border+text tokens, 8 semantic color tokens,
  27 alpha-derived tokens, 15 font-size tokens, 2 typeface families, 11 spacing
  tokens, 2 radius tokens, 3 duration tokens, 2 easing tokens.
- Documented component inventory: 5 shell primitives + 6 button variants +
  6 input patterns + 8 card/panel patterns + 9 loading states + 10 chat/message
  components, all with states documented.
- Created companion `VISION.md` with north-star definition, 4 reference products,
  personality adjectives, and phased look-evolution path (v0.2 → v0.5 → v1.0).
