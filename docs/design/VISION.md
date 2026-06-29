# Olympus Design Vision — The North Star

> What Olympus should *feel* like, the product personality, the references that
> anchor it, and the multi-release look path. The **Design System**
> (`DESIGN_SYSTEM.md`) is *how*; this is *why* and *where we're going*. When a
> design decision is ambiguous, resolve it toward this north star.

---

## The one-line north star

> **A calm, dense, editorial operator cockpit for a power user running an agent
> fleet — Linear's discipline wearing a terminal's clothes.**

Olympus is where one operator watches and steers every Claude Code / Hermes
session across every channel. It must feel like an instrument you *trust* to sit
in front of for hours: quiet, fast, information-dense, never noisy or cute.

---

## What it should feel like

- **Calm.** Dark by default, low-chroma surfaces, one electric accent used
  sparingly. Nothing pulses unless something is genuinely live. The screen at
  rest is still.
- **Dense, not cramped.** Linear-grade information per pixel — the operator sees
  the whole fleet without scrolling — but with deliberate rhythm so density
  reads as *mastery*, not clutter.
- **Editorial.** Typography does the work. Clear hierarchy, generous line-height
  in prose, mono for everything machine-shaped. The layout has a point of view.
- **Terminal-adjacent.** Mono labels, small-caps meta, hairline rules, IDs and
  timestamps everywhere. It should feel native to someone who lives in a shell —
  without being a skeuomorphic fake terminal.
- **Trustworthy & legible.** Status is unambiguous: you always know what's
  running, blocked, synced, or failed at a glance, by label *and* color.
- **Fast.** Motion ≤150ms, skeletons over spinners, instant theme switch. The
  UI never makes the operator wait on decoration.

### Personality adjectives

`Composed` · `Precise` · `Dense` · `Editorial` · `Engineered` · `Quietly powerful`

The anti-adjectives (what we are **not**): playful, bubbly, gradient-y,
"friendly AI," Material, enterprise-dashboard-generic, neon-cyberpunk cosplay.

---

## References (and exactly what we take from each)

1. **Linear** — *information density + restraint.* The discipline of a calm dark
   UI that shows a lot without feeling busy; keyboard-first; one accent; flat
   surfaces with hairline structure. We take the *posture*, not the purple.
2. **Vercel / Geist** — *editorial monochrome + typographic hierarchy.* Confident
   black-and-white-and-one-accent layouts where type sets the rhythm. We take the
   editorial calm and the mono/sans pairing discipline.
3. **A real terminal / tmux / `htop`** — *the cockpit feel.* Dense status rows,
   monospace columns, glanceable health, no chrome. We take the at-a-glance fleet
   legibility and the mono-meta texture.
4. **Raycast** — *command-surface speed.* Fast, focused, keyboard-driven
   interactions that feel weightless. We take the responsiveness target and the
   "get out of the way" ethos (a future command palette belongs here).
5. **Datadog / Grafana (as a cautionary reference)** — *the trap to avoid.*
   Powerful but visually noisy operator dashboards. We take the ambition of
   "watch everything" and explicitly reject the clutter.

---

## Signature elements (what makes it unmistakably Olympus)

- **Electric-cyan accent** (`#7dd3fc` in midnight) used *only* for focus, active
  state, and primary action — scarcity makes it read as "this is where you are."
- **Role/status as labeled, color-tinted gutters & badges** — never color alone.
- **Three first-class themes** (midnight / daylight / amber-crt) as a stated
  feature, proving the whole UI is token-addressable and re-skinnable.
- **The mono meta layer** — IDs, times, counts, model names in IBM Plex Mono with
  small-caps labels — the connective tissue that makes it feel engineered.
- **Flat depth** — layered surface tokens + hairlines, zero decorative shadow.

---

## Phased look-evolution path

The system is good enough to build on now. "Great" is staged:

### v0.2 — *Coherent & honest* (now → near term)
- All 7 views render against the shared primitives; every screen follows the
  `PageHeader → toolbar → content → empty/skeleton` skeleton.
- **Zero hardcoded colors** — every literal promoted to a theme token; all three
  themes verified with no regressions.
- A real **keyboard focus ring** across every control; `prefers-reduced-motion`
  honored. The cockpit is usable hands-on-keyboard.
- **Done =** the UI is internally consistent and the token contract is airtight;
  a screenshot in any theme has no off-system element.

### v0.5 — *Instrumental* (mid)
- **Scale tokens** (`--space-*`, `--font-*`, `--dur-*`) land; a **density toggle**
  (comfortable/compact) reflows the whole cockpit from one switch.
- A **command palette** (Raycast-grade) for jump-to-session / run-action —
  keyboard-first navigation across the fleet.
- Live state gets a richer but still-calm vocabulary: subtle running indicators,
  per-node health sparklines, board column motion that *informs*.
- **Done =** a power user can run the fleet almost entirely from the keyboard,
  tune density to taste, and the live views feel like instruments.

### v1.0 — *Signature* (long)
- The look is unmistakably Olympus and could be screenshotted as a reference
  itself. Editorial typographic polish, perfected dark/light parity, amber-crt as
  a beloved third option.
- Optional **fourth+ themes** contributed safely because the token contract is
  proven; possibly a high-contrast a11y theme.
- Motion, empty states, and data-viz (usage charts, workflow DAGs, node grids)
  all share one calm visual language — nothing feels bolted on.
- **Done =** Olympus is the canonical example of "Linear-meets-terminal" done
  right: dense, calm, fast, and yours.

---

## Drift guard

If a change pulls toward any of these, stop and write down why before shipping:
generic sans for content, purple/gradient accents, decorative shadows, Material
components, color-only status, decorative (non-functional) animation, or a
hardcoded color. The anchor in `AGENTS.md` and `DESIGN_SYSTEM.md` is the
contract; this vision is the reason it exists.
