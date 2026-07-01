# Olympus Vision — North Star

> **What Olympus should FEEL like.** The product personality, the emotional
> target, the references we steal from, and the path from "it works" to
> "it's beautiful." Owned by `design-lead`.

---

## 1. The Feeling (in one sentence)

**Olympus feels like sitting at a mission-control console for your own AI
fleet — calm, dense, trustworthy, and quietly powerful.**

Not a chat window. Not a dashboard. A **cockpit**: everything you need to see
is visible; every action is one click away; the noise stays down so you can
think.

---

## 2. Personality Adjectives

| Adjective | What it means for Olympus | What it rejects |
|-----------|--------------------------|-----------------|
| **Calm** | Dark, low-contrast surfaces. No pop, no pulse, no notification anxiety. Motion is sub-perceptual (≤150ms). | Flashy, attention-grabbing, gamified |
| **Dense** | Information-rich screens. High data-per-pixel. Small type that's still readable. Monospace metadata everywhere. | Wasted whitespace, hero banners, marketing layout |
| **Editorial** | Typography carries the design. IBM Plex has character — technical but warm. Hierarchy is clear through size/weight alone. | Generic sans-serif, flat visual hierarchy, type as afterthought |
| **Terminal-native** | Feels like a modern tmux/iterm2 session. Dark phosphor warmth (even in midnight). Keyboard-first. Monospace is first-class. | GUI-for-GUI's-sake, mouse-dependent, hiding the machine |
| **Trustworthy** | Consistent tokens. Predictable behavior. No surprises. Status is always visible and honest. Errors are clear, not scary. | Hidden loading states, cheerful error pages, inconsistent affordances |

---

## 3. Reference Products

These are NOT "copy these" — they're "steal this feeling."

### 3.1 Linear (linear.app)

**What we take:** The dark-mode density. The sidebar + master-detail layout.
The mono secondary text. The calm color palette (no purple). The keyboard-first
navigation feel. The way status is communicated with small, colored dots and
minimal badges.

**Linear's superpower:** You can scan a project list and know the state of
everything in 2 seconds. That's our bar for the Sessions list and Board view.

### 3.2 Warp Terminal (warp.dev)

**What we take:** The modern terminal aesthetic. The way command output is
rendered as structured blocks (not just raw text). The inline tool-call cards
feel like Warp's blocks. The warm/cool dark themes that feel like a place you
want to work in.

**Warp's superpower:** Making terminal output *beautiful* without losing
fidelity. Our message view (role badges, tool cards, diff views) aims here.

### 3.3 Raycast (raycast.com)

**What we take:** The command-bar speed. The minimal chrome. The way the UI
gets out of the way. The accent-color-as-personality approach (one strong
color, everything else neutral). The density of the extension/list views.

**Raycast's superpower:** Power-user tools can be beautiful AND fast. Our
search view and settings should feel this crisp.

### 3.4 VS Code (code.visualstudio.com)

**What we take:** The panel layout discipline. The editor + sidebar + terminal
zones that are familiar to every developer. The diff view styling. The status
bar information density. The way Explorer tree items convey state with icons
and color.

**VS Code's superpower:** Developers already know how to navigate it. Our
sidebar nav + detail panel pattern maps directly to this mental model.

---

## 4. The Look Path: Phased Evolution

### Phase v0.2 — "Solid Cockpit" (current → next milestone)

**Where we are now:** All 7 views render. 3 themes work. Tokens exist. Views
are functional but visually inconsistent — some views feel polished (Sessions,
Chat), others feel like wireframes (Usage, Nodes).

**What "great" looks like at v0.2:**
- Every view uses shell primitives consistently (`PageHeader`, `EmptyState`,
  `StatPill`, `Badge`) — no inline header/div patterns.
- Loading states exist for every view (skeleton or spinner).
- Empty states are helpful (illustration + description + CTA), not just "no data."
- Hover/focus states are consistent across all interactive elements.
- No hardcoded hex values remain in any component.
- Compact density mode works correctly in all views.
- **Visual consistency bar:** switch between views and feel like you're in the
  same app.

### Phase v0.5 — "Polished Operator"

**What "great" looks like at v0.5:**
- Micro-interactions land: row selection transitions, card hover lift,
  staggered list entrance animations (subtle, 80–120ms).
- The Board view has smooth drag-and-drop with visual feedback.
- Real-time indicators (live dot, streaming tag, connection status) use the
  motion system correctly — present but not distracting.
- Search hit highlighting is refined (better contrast in all themes).
- Keyboard shortcuts work for core navigation (Cmd+K for search, number keys
  for theme switch, etc.) with visible shortcut hints.
- The composer feels great: auto-resize, smooth focus transitions, attachment
  preview, clean error/retry states.
- **Polish bar:** using Olympus for 10 minutes feels satisfying, not just
  functional. No jank, no layout shift, no flash-of-wrong-theme.

### Phase v1.0 — "Control Plane"

**What "great" looks like at v1.0:**
- Custom scrollbars styled per-theme (not just webkit-default with token colors).
- Subtle background texture/gradient in the sidebar (very subtle — a 1-2%
  noise or gradient that gives depth without being noticeable consciously).
- Workflow DAG visualization: nodes connected with animated paths for running
  steps. Looks like a real operations diagram.
- Usage charts: sparklines, mini bar charts, time-series — all rendered in
  CSS/SVG, no chart library. Theme-adaptive.
- Notification toast system (non-modal, stacked, auto-dismissing) for async
  events (task complete, node disconnected, sync error).
- Split-view / multi-pane support: compare two sessions, side-by-side board +
  chat, etc.
- **V1 bar:** A developer chooses to use Olympus over their terminal + grep
  for daily agent-fleet management because it's faster, clearer, and more
  pleasant. It looks like a product they'd pay for.

---

## 5. Non-goals (explicitly out of scope)

- **Mobile / responsive** — Olympus is a desktop cockpit. We don't design for
  phones. Basic tablet tolerance is nice-to-have.
- **Animation-heavy** — We're not building Framer prototypes. Motion is
  confirmatory, not decorative.
- **White-label / theming marketplace** — We ship 3 good themes. Custom
  themes are possible (add a `[data-theme]` block) but not a product feature.
- **Accessibility over AA** — We target WCAG AA. AAA is nice where it comes
  for free but won't drive design decisions.
- **Dark-only** — Midnight is default but daylight must be equally polished.
  Some users prefer light.

---

## 6. Design Debt Log

Known issues tracked for future runs:

| # | Issue | Severity | Target phase |
|---|-------|----------|-------------|
| D1 | Formal WCAG contrast audit not done (`--text-faint` may fail AAA) | Medium | v0.5 |
| D2 | Button variants scattered across views — extract to `shell.tsx` primitives | Low | v0.2 |
| D3 | No custom scrollbar styling beyond basic webkit tokens | Low | v1.0 |
| D4 | Some views (Usage, Nodes) have thinner empty states than Sessions/Board | Medium | v0.2 |
| D5 | ~~`placeholder-badge` uses `--accent-dim` bg instead of a dedicated amber token~~ **FIXED 2026-07-01** — repointed to `--amber-soft`/`--amber-border`; also fixed `.badge-warning` (same mis-tint) | ~~Low~~ Done | ~~v0.2~~ |
| D6 | Focus ring offset (2px) may clip in tightly-packed rows | Low | v0.5 |
| D7 | No `aria-live` regions on WS-driven real-time updates | Medium | v0.5 |
| D8 | Composer lacks smooth auto-resize animation | Low | v0.5 |

---

*Last updated: 2026-07-01 (D5 closed — amber-semantic fills fixed)*
