# 0023 — Projects "No assignee" filter returned an empty board

## Status

Resolved before release.

## Impact

On the Projects (kanban) surface, the left sidebar renders a **No assignee**
filter whenever any card lacks an assignee. Clicking it produced an empty board
("No cards match the current filter") instead of showing the unassigned cards.
The active-filter chip also rendered the raw sentinel string `__unassigned__`
instead of a human label. Every unassigned card was effectively unreachable via
that filter.

## Root cause

`ProjectSidebar` encodes the unassigned filter as the sentinel
`filterAssignee = "__unassigned__"` (there is no real assignee id to match on).
`ProjectsView.filteredCards` had only two branches:

```ts
if (!filterAssignee) return cards;
return cards.filter((c) => c.assignedId === filterAssignee);
```

With the sentinel active, the predicate compared each card's `assignedId`
against the literal string `"__unassigned__"`, which no card carries, so the
result was always empty. The sidebar and the view disagreed on what the
sentinel means — the sidebar produces it, the view never decodes it.

Same class as the query-param-silently-dropped and
POST/GET-derived-field-disagreement pitfalls: a value flows from producer to
consumer, but the consumer never handles that shape, so the feature silently
no-ops with a green build.

## Detection

Found during an E2E coverage audit of the Projects surface (the board had no
interactive-feature E2E flow at all — only a "board loads" smoke test). Reading
`ProjectsView.tsx:65-68` against `ProjectSidebar.tsx:93-101` surfaced the
producer/consumer mismatch; the mock fixture (`card-todo`, `assignedId: null`)
confirmed the sidebar would offer the filter and the board would come back
empty.

## Resolution

- Decode the sentinel in `filteredCards`:
  `if (filterAssignee === "__unassigned__") return cards.filter((c) => !c.assignedId);`
- Render "No assignee" instead of the raw sentinel in the active-filter chip.
- Added `.maestro/flows/mock/projects-board.yaml` — a full board flow that
  selects a card (detail panel), applies the assignee filter, clears it, then
  applies the **No assignee** filter and asserts the unassigned card is visible
  and the board is NOT empty. This flow fails on the pre-fix code.

## Prevention

A "surface loads" smoke test is not feature coverage. Every filter/toggle/select
control needs an E2E assertion that the control actually changes what renders —
not just that the page returns 200 and the header text is present. When a
control encodes a sentinel value, the consumer must have an explicit branch for
it, and a test must exercise that branch.
