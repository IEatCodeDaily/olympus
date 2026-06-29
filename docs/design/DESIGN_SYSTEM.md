# Olympus Design System

> Canonical design system for the Olympus control-plane UI. This document is the
> source of truth for tokens, type/space scales, components, motion, density, and
> accessibility. View workers build against it; the **Design Lead** owns it.
>
> **Anchor:** Linear-meets-terminal — a calm, dense, editorial operator cockpit
> for a power user running an agent fleet. Dark default. IBM Plex Sans + Mono.
> Electric-cyan accent. NOT generic AI slop (no Inter, no purple gradients, no
> Material). Every color/space value is a CSS variable (theme-addressable).
>
> **Hard rule:** never hardcode a hex/`rgba()` in a component. A new visual need
> is a new token added to **every** `[data-theme]` block in `ui/src/index.css`,
> referenced as `var(--token)`.

Companion docs: `docs/design/VISION.md` (the north star feel + look path),
`docs/plans/2026-06-29-olympus-ui-roadmap.md` (the screen/IA path).

---

## 1. Themes

Three themes ship today, each redefining the **same semantic token set** under a
`[data-theme]` block. `midnight` is the default (also bound to bare `:root`).
The runtime switch lives in `ui/src/lib/theme.ts` (persists to
`localStorage["olympus-theme"]`, sets `document.documentElement.dataset.theme`).

| Theme | id | Personality |
|---|---|---|
| **Midnight** | `midnight` | Dark default. Near-black blue-grey, electric-cyan accent. The cockpit at night. |
| **Daylight** | `daylight` | Light. Cool paper-white, deep-sky-blue accent. Same density, readable in sun. |
| **Amber CRT** | `amber-crt` | Warm terminal. Deep brown-black, amber phosphor accent. Nostalgic, focused. |

Adding a theme = add one `[data-theme="name"]` block defining all semantic
tokens below, add the id to `THEMES` + `THEME_LABELS` in `lib/theme.ts`. No
component changes.

---

## 2. Color tokens (semantic)

Every color is one of these semantic names. Values per theme:

### Surfaces & structure

| Token | Role | midnight | daylight | amber-crt |
|---|---|---|---|---|
| `--bg` | App background (deepest) | `#090b0e` | `#f7f8fa` | `#120d08` |
| `--bg-elev` | Elevated surface (sidebar, headers, cards) | `#101318` | `#ffffff` | `#1a1308` |
| `--bg-elev2` | Higher surface (active row, inset blocks) | `#161a21` | `#eef1f5` | `#221a0c` |
| `--bg-hover` | Hover wash on interactive rows | `#1c2129` | `#e4e8ee` | `#2c2210` |
| `--border` | Default hairline border / divider | `#1e252e` | `#dce1e8` | `#3a2c12` |
| `--border-bright` | Emphasized border (inputs, controls) | `#2a3340` | `#c2cbd6` | `#4d3a18` |

### Text

| Token | Role | midnight | daylight | amber-crt |
|---|---|---|---|---|
| `--text` | Primary text | `#e2e8f0` | `#1a2230` | `#ffd79a` |
| `--text-dim` | Secondary text / labels | `#8595a8` | `#51607a` | `#c79a5c` |
| `--text-faint` | Tertiary / meta / placeholders | `#5a6878` | `#8090a4` | `#8a6a3c` |

### Accent & status

| Token | Role | midnight | daylight | amber-crt |
|---|---|---|---|---|
| `--accent` | Primary accent (focus, active, primary CTA) | `#7dd3fc` | `#0284c7` | `#ffb347` |
| `--accent-dim` | Accent wash (tinted bg/borders) | `rgba(125,211,252,.12)` | `rgba(2,132,199,.10)` | `rgba(255,179,71,.14)` |
| `--green` | Success / running / managed / "you-adjacent AI" | `#86efac` | `#16a34a` | `#b6e36b` |
| `--amber` | Warning / tool / fork / in-flight | `#fcd34d` | `#b45309` | `#ffcf5c` |
| `--red` | Error / blocked / destructive | `#fca5a5` | `#dc2626` | `#ff8a6b` |

**Semantic color mapping (do not invent new meanings):**
- `--accent` = the user's focus & primary action. Active nav, focus ring, primary button, user role.
- `--green` = healthy / running / managed sessions / assistant role.
- `--amber` = attention / tool activity / forked sessions / highlights.
- `--red` = failure / blocked / disconnected / destructive.

### Non-theme primitives (`:root`)

| Token | Value | Role |
|---|---|---|
| `--mono` | `"IBM Plex Mono", "JetBrains Mono", "SF Mono", "Cascadia Code", monospace` | All metrics, IDs, code, labels |
| `--sans` | `"IBM Plex Sans", -apple-system, system-ui, sans-serif` | All prose & UI text |
| `--radius` | `6px` | Default corner radius (cards, inputs) |
| `--radius-sm` | `4px` | Small radius (buttons, pills, badges) |
| `--sidebar-w` | `220px` | Fixed sidebar width |

---

## 3. Typography

**Two families only.** IBM Plex Sans for everything human-readable; IBM Plex Mono
for anything machine-shaped — timestamps, IDs, counts, model names, code, tool
output, and the small-caps meta labels that give the cockpit its terminal feel.

Base: `14px / 1.0` on `html`, `-webkit-font-smoothing: antialiased`,
`text-rendering: optimizeLegibility`.

### Type scale (as used today)

| Step | Size | Weight | Use |
|---|---|---|---|
| Page title | `18px` | 600 | `.page-title` — view H1 |
| Section / brand | `15px` | 600 | brand name, content H1–H3 |
| Chat title | `14px` | 500 | conversation header |
| Body | `13.5px` | 400 | message content, composer input |
| UI default | `13px` | 500 | nav items, row titles, buttons |
| Control | `12.5px` | 400–600 | subtitles, settings rows, primary btn |
| Meta | `12px` | 400 | sort, toggles, hit snippets |
| Mono meta | `11px` | 400–500 | timestamps, status, counts |
| Micro label | `10px` | 600–700 | role/source badges, uppercase mono labels |
| Tiny | `9px` | 600–700 | managed/fork tags, section labels |

**Line-height:** prose `1.5–1.6`; UI rows `1.2`; tight mono labels `1.0`.
**Letter-spacing:** uppercase mono labels `+0.03–0.05em`; large titles `-0.01em`.

> Debt: these sizes are currently inline literals. Target — promote to a
> `--font-*` scale (see §11).

---

## 4. Spacing

A loose 4px-based rhythm. Common step values in use: **2, 3, 4, 6, 8, 10, 12,
14, 16, 18, 20, 24, 28, 48 px**. Containers: page scroll padding `20px 24px`;
toolbar `12px 16px`; message rows `12px 24px`.

> Debt: spacing is inline. Target — a `--space-1…8` scale token set (see §11)
> so density modes can rescale globally.

---

## 5. Radius & elevation

- **Radius:** `--radius` (6px) for cards/inputs/panels; `--radius-sm` (4px) for
  buttons/badges/pills; pills/chips that should read as fully round use `20px`.
- **Elevation:** Olympus is **flat** by design — depth comes from layered
  surface tokens (`--bg` → `--bg-elev` → `--bg-elev2`) and hairline borders, not
  drop shadows. The only glow is functional: the connected status dot
  (`box-shadow` on `--green`) and input focus. No decorative shadows.

> Debt: no `--shadow-*` / `--ring-*` tokens yet; focus & glow values are inline.

---

## 6. Component inventory

Shared primitives live in `ui/src/components/shell.tsx` (extend, never
duplicate). Their styling lives in `ui/src/index.css`. Each must define the full
state set below.

### Primitives (`shell.tsx`)

| Primitive | Props | States covered | Notes |
|---|---|---|---|
| `PageHeader` | `title, subtitle?, actions?` | default | Every view starts with this. |
| `EmptyState` | `icon?, title, message?, cta?` | default (empty) | Centered icon+copy+CTA. |
| `PlaceholderBadge` | `epic` | default | Marks mock-first views; mono amber chip. |
| `StatPill` | `label, value` | default | Metric chip; mono value + caps label. |
| `Badge` | `kind?, children` | `running`/`blocked`/`done` + default | Status pill. |

### Interactive elements & their required states

| Element | default | hover | active/selected | focus | disabled | loading | empty |
|---|---|---|---|---|---|---|---|
| **Nav item** (`.nav-item`) | dim text | bg-hover, text | accent text + bg-elev2 | — *(gap)* | — | — | — |
| **Primary button** (`.btn-primary`) | accent bg | opacity .88 | — | — *(gap)* | opacity .4 | — | — |
| **New-chat button** (`.new-chat-btn`) | cyan bg | brighter | — | — | opacity .5 | — | — |
| **Back / ghost button** (`.back-btn`) | border-bright | accent border+text | — | — *(gap)* | — | — | — |
| **Search box** (`.search-box`) | border-bright | — | — | accent border (`:focus-within`) | — | spinner | placeholder |
| **Composer** (`.composer-input-row`) | border-bright | — | — | accent border | send btn .35 | spinner | placeholder |
| **Send button** (`.composer-send`) | accent bg | brighter | — | — | opacity .35 | spinner | — |
| **Source pill** (`.source-pill`) | dim outline | text-dim border | (filter on) | — | — | — | — |
| **Session row** (`.session-row`) | hairline | bg-elev | bg-elev2 + inset accent bar + accent title | — | — | skeleton rows | list-empty |
| **Sort select** (`.sort-select`) | border-bright | accent border | — | — *(gap)* | — | — | — |
| **Theme swatch** (`.theme-swatch`) | border | — | accent border | — *(gap)* | — | — | — |
| **Tool-call card** (`.tool-call-card`) | hairline | header bg-hover | expanded | — | — | spin status | — |

### Loading & empty patterns

- **Skeletons:** `.session-skeleton` (row shimmer) and `.transcript-skeleton`
  (message shimmer) use the `shimmer` keyframe over `--bg-elev`→`--bg-elev2`.
  Use a skeleton, never a spinner, for list/content loads.
- **Inline spinners** (`spin` keyframe) only for in-place actions: search,
  composer send, running tool status.
- **Empty:** always `EmptyState` (icon + title + message + optional CTA), never a
  bare "No data" string. Mock-first views pair it with `PlaceholderBadge`.

### View skeleton (every screen)

`PageHeader` → optional `Toolbar`/filters → `StatPill` row (if metrics) →
content (list/grid/board) → `EmptyState` when empty / skeleton while loading.

---

## 7. Motion

Calm and quick. Motion confirms an action; it never entertains.

| Transition | Duration | Easing | Where |
|---|---|---|---|
| Hover/state on controls | `0.08–0.15s` | `ease` | nav, rows, buttons, borders |
| Focus border | `0.12–0.15s` | default | inputs, composer |
| `shimmer` (skeleton) | `1.3s` | `ease-in-out` infinite | loading rows |
| `spin` (spinner) | `0.7–1s` | `linear` infinite | actions, running tools |
| `pulse` (live tag) | `1.5s` | infinite | streaming indicator |

Rules: durations ≤ 150ms for direct manipulation; no easing flourishes
(no bounce/elastic); reserve infinite animation for genuine live state
(streaming, running, loading). Respect `prefers-reduced-motion` *(debt: not yet
wired)*.

> Debt: no `--dur-*` / `--ease-*` tokens; durations are inline literals.

---

## 8. Density

Target density: **dense** — Linear-grade information per screen. Session rows are
`68px`, nav items `8px 12px`, message rows `12px 24px`. Roadmap U2 calls for a
runtime **density toggle** (comfortable / compact). It is **not yet
implemented**; when built it must rescale via the `--space-*` scale (§11), not
per-component overrides, so one token swap reflows the whole cockpit.

---

## 9. Accessibility

- **Contrast:** primary text on `--bg` meets WCAG AA in all three themes;
  `--text-dim` is for secondary content only, `--text-faint` for non-essential
  meta — do not put primary information in faint.
- **Focus:** *current gap* — most controls show hover/active but **no dedicated
  keyboard focus ring**. Target: a single `--ring` token + a shared
  `:focus-visible` outline applied to every interactive element. (Top a11y debt.)
- **Targets:** interactive rows/buttons keep a ≥28–32px hit height.
- **Color is never the only signal:** status uses badge text + color (e.g.
  `RUNNING`/`BLOCKED` labels), role uses a labeled badge + gutter tint, not hue
  alone.
- **Motion:** honor `prefers-reduced-motion` (debt — see §7).

---

## 10. Do / Don't

**Do**
- Reference `var(--token)` for every color; add a token to all themes for any new need.
- Use `--mono` for anything machine-shaped (IDs, times, counts, code) — it's the terminal half of the personality.
- Layer surface tokens for depth; keep the UI flat.
- Lead every view with `PageHeader`; use `EmptyState` + skeletons for the empty/loading edges.
- Map status strictly to the semantic colors (§2): accent=focus, green=healthy, amber=attention, red=failure.
- Keep motion ≤150ms and purposeful.

**Don't**
- Hardcode a hex or `rgba()` in a component or in `index.css` outside a theme block. *(Several literals exist today — see §11 debts.)*
- Introduce Inter, Roboto, system-default sans for content, or any Material component.
- Add purple/violet or gradient accents. *(The reasoning block currently uses a `rgba(240,171,252)` purple — a drift to fix.)*
- Use drop shadows for decoration, or `box-shadow` where a surface/border token reads cleaner.
- Put primary information in `--text-faint`.
- Add a one-off spacing/size value when a scale step fits.

---

## 11. Known debts (path for future runs)

Ranked by leverage. Each future run picks the top unblocked item, fixes it at
the system level, verifies, and logs it in §12.

1. **Hardcoded color literals** break the "tokens only" rule and the theme
   contract. Known offenders in `index.css`:
   - scrollbar thumb hover `#3a4452`
   - `.new-chat-btn` text `#0a0e14`, bg `#22d3ee`, hover `#67e8f9`
   - `.composer-send` hover `#a5e0ff`
   - dozens of `rgba(125,211,252,…)` / `rgba(134,239,172,…)` / `rgba(252,211,77,…)`
     accent/green/amber alpha literals (badges, washes, role tints) that should be
     `--accent-dim`-style tokens or theme-defined alpha tokens.
   - `.reasoning-content` purple `rgba(240,171,252,…)` — also violates the no-purple anchor.
2. **No keyboard focus ring** (a11y, §9). Add a `--ring` token + shared
   `:focus-visible` rule across nav, buttons, selects, swatches.
3. **No spacing/type/motion scale tokens** (§3, §4, §7). Inline literals block a
   clean density toggle and consistent rhythm. Introduce `--space-*`,
   `--font-*`, `--dur-*`/`--ease-*`.
4. **Density toggle** (§8) — depends on (3).
5. **`prefers-reduced-motion`** not honored (§7).
6. **No `Toolbar` / `SkeletonRows` shared primitives** in `shell.tsx` though the
   roadmap names them; loading/filter UI is re-implemented per view.

---

## 12. Changelog

| Date | Change | Why | Files |
|---|---|---|---|
| 2026-06-29 | **Created the design system.** Documented all 3 themes + full semantic token table, type scale, spacing rhythm, radius/elevation stance, component inventory with required states, motion table, density target, a11y posture, do/don't, and a ranked debt backlog grounded in the live `index.css` + `shell.tsx`. | First run: establish the canonical system every view worker builds against, and make the existing token-rule violations visible as a fixable backlog. | `docs/design/DESIGN_SYSTEM.md` (new), `docs/design/VISION.md` (new) |
