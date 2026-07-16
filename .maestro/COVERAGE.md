# Olympus E2E Coverage (Maestro)

Feature → flow map for the **mock** tier (`.maestro/flows/mock/`, run via
`npm run test:e2e:desktop`). Mock tier is the comprehensive one: MSW intercepts
every API, so it needs no backend and runs in CI. `live`/`prod` tiers are thin
smokes against a real control plane.

| Surface | Feature | Flow | Notes |
|---|---|---|---|
| Sessions | select + composer, model/thinking pick, persistence | `sessions.yaml` | |
| Sessions | new-session agent picker | `agent-picker.yaml` | default / coding-agent / olympus agents |
| Sessions | fork observed → managed | `session-fork.yaml` | fork confirm modal |
| Sessions | right Overview panel + bottom-panel tabs | `session-panels.yaml` | AGENT/MODEL/STARTED; Terminal/Logs/Output/Debug |
| Sessions | Agents page | `agents.yaml` | provider + model per profile |
| Sessions | Usage page | `usage.yaml` | per-model token + cost rows |
| History | search filter + empty state | `history.yaml` | |
| History | text filter, clear, archived toggle | `history-filters.yaml` | column headers, "N of M sessions" |
| Vaults | create vault + note workflow, dup-name guard | `vaults.yaml` | + graph/table view toggles |
| Vaults | rich/source editor, dirty state, jj conflict | `vault-workbench.yaml` | Milkdown ↔ CodeMirror, conflict opens source |
| Projects | board loads | `projects-fail-closed.yaml` | smoke |
| Projects | columns, card detail, assignee + **unassigned** filters | `projects-board.yaml` | regression guard for postmortem 0023 |
| Fleet | overview loads | `fleet-fail-closed.yaml` | 3 fixture nodes |
| Fleet | node detail, detect agents, add-node enroll | `fleet-detail.yaml` | STATUS/SLOTS/HEARTBEAT; enroll command |

## Known gaps (deliberately uncovered)

- **TopBar theme toggle / org switcher** — theme is a visual-only toggle (no
  assertable text state change in headless), and the mock exposes exactly one
  org ("Personal") so there is nothing to switch to. Surface navigation is
  already exercised by every flow launching at its own URL.
- **Chat live streaming (delta/tool/reasoning frames)** — the mock WS
  (`ws-mock.ts`) does not drive a full streaming turn; live-turn UX is covered
  by the `live` tier against a real agent (token-spending, operator-run only).
- **Right-panel Diff/Git/Browser/AI tabs** — icon-only buttons that Maestro
  web omits from its hierarchy; not reliably tappable by text. Their content is
  data-derived (needs tool-call artifacts in the fixture) — future work if
  those tabs gain stable test hooks.

## Maestro-web gotchas (learned)

- Icon-only controls (panel tabs, kebab menus, back chevrons) expose no
  accessible name in Maestro web's hierarchy — target visible **text** or a
  fixed viewport **coordinate**, never an aria-label.
- A wrapped paragraph is one text node — match a substring with a regex
  (`.*needle.*`), not a bare exact string.
- After `inputText`, the placeholder is gone — clear via `longPressOn:<value>`
  + `eraseText`, not by re-tapping the placeholder.
- A detail/right panel that repeats the selected item's title will defeat an
  `assertNotVisible` on that title — close the panel before filter assertions.
