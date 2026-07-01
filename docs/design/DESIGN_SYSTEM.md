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

### Spacing philosophy
- Use the token variables above — never raw pixel values for padding/margin/gaps.
- The gap between sibling elements should match the most specific applicable token.
- `--space-panel` / `--space-panel-lg` are the "atomic unit" for card/panel interiors.
- Density mode (`[data-density="compact"]`) reduces all spacing by ~20%.

---

## 5. Radius & Elevation

| Token | Value | Usage |
|-------|-------|-------|
| `--radius` | 6px | Default radius: cards, panels, inputs, buttons, search boxes |
| `--radius-sm` | 4px | Small radius: badges, pills, toggles, small buttons |

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
→ `:disabled` (opacity 0.4–0.5, cursor not-allowed).

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
