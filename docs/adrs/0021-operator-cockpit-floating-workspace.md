# ADR 0021 — Operator Cockpit (Floating, Persistent, Tabbed Workspace)

- Status: Proposed
- Date: 2026-07-14
- Depends on: ADR 0020 v2 (client state correctness),
  the terminal adversarial review
  (`/home/rpw/.hermes/workspace/reviews/olympus-terminal-review.md`),
  ADR 0017 (node identity / readiness), Phase 1 (node-key binding)
- Relates to: master plan Phase 3 (terminal)
- Reference UX: Terax (crynta/terax-ai) — a floating, tabbed, AI-native
  terminal workspace. We take the *shape* (floating overlay, tabs, toggle),
  not the code.

## 1. Intent

A single **operator-only** floating "cockpit" window, toggled from a button in
the top-right of the app chrome, that persists over every view and per user.
Tabbed workspace bundling terminal (PTY) tabs, a file explorer, and a text
editor. It is the operator's control surface. **Agents never touch it** — hard
boundary from the terminal review: agents get typed capabilities, never raw
shell/PTY/file access through this window.

## 2. Hard requirements (from the user)

1. **Operator-only.** No agent path — not a `tool.*` capability, not in any
   agent operation catalog, MCP, or runtime gateway.
2. **Floating over everything** — one overlay above all app chrome, independent
   of the current route/view.
3. **Persists across view** — navigating never tears down the window or its open
   terminals; the console stays open.
4. **Tabbed** — multiple terminal/editor/explorer tabs; add/close/reorder.
5. **Toggleable** — top-right button shows/hides; hiding does NOT kill the
   sessions inside.
6. **Persists per user** — geometry, open tabs, and their targets restored for
   that operator across app loads (subject to §5 reconnect rules for live PTY).
7. **Keeps console open** — a running shell survives navigation and toggle, and
   survives reload/reconnect via durable attach-with-replay (Phase 3 backend),
   degrading honestly when replay history is unavailable.

## 3. Two persistence layers (do not conflate)

"Persists across view / keeps console open" is the same class as ADR 0020:
transient state destroyed by navigation.

### Layer A — in-session persistence (pure frontend)

Mount the cockpit **once, at the AppShell root, OUTSIDE the router outlet**.
Route changes re-render the outlet, never the cockpit. xterm.js instances, their
sockets, and scrollback live in a top-level Zustand store keyed by tab id, never
unmounted on navigation. The toggle sets `display:none` / detaches from layout —
it does NOT dispose the xterm instance or close the socket. Satisfies
requirements 3, 5, and the navigation half of 7 with **no backend change**;
ships first over a mock/echo PTY.

### Layer B — cross-reload / reconnect persistence (backend, Phase 3)

Reload/sleep/WS-drop tears down the browser, so the PTY must live on **Envoy**:

- Envoy owns a durable `PtyAttempt` (terminal review "Envoy live attempt
  model"): `terminal_id + attempt_epoch`, bounded raw-byte replay window,
  exactly-one writable `attachment_generation`.
- Hall owns a durable `TerminalRecord` — **lifecycle/audit only, never
  keystrokes or output**.
- On reload, re-attach by `terminal_id` from the last cursor and replay the
  bounded window; if history rolled, show an explicit "earlier terminal history
  unavailable" marker. Never present a partial screen as complete.

Per-user tab restoration (req 6) = persist the tab manifest (terminal_id,
target, geometry) per operator; on load re-attach live attempts, mark dead ones
closed/lost. Typed unsequenced-byte data plane; durable lifecycle.

## 4. Web-native components (no Terax dependency)

| Pane | Component | Why |
|---|---|---|
| Terminal | `xterm.js` + `addon-fit`, `-webgl`, `-serialize` | industry standard (VS Code/Hyper/Wave); serialize aids reconnect replay render |
| Editor | CodeMirror 6 | lighter than Monaco; matches Vault Milkdown/Crepe weight budget |
| File tree | own tree over existing Vault/Repo file APIs, or `react-arborist` | reuse Envoy file access; no new raw-fs surface |
| Window shell | `react-rnd` (drag/resize) + small tab bar | thin; no heavy windowing dep |

xterm.js is decorative until Layer B exists. The window is real UX from day one;
the *shell* is real only once the Envoy PTY primitive lands.

## 5. Security boundary (non-negotiable — terminal review)

- Dedicated **operator WebSocket** per terminal — NOT the shared `/ws` firehose.
  PTY bytes must never enter `onFrame`'s global listeners / debug ring, or
  secrets leak into React state and the Debug tab.
- Re-authorize the human principal on attach and on revocation; bump
  `attachment_generation` to fence stale sockets.
- Server-derived launch profile only: fixed shell, session/node-scoped cwd, no
  caller-supplied argv/env/cwd/user/SSH target.
- Session-shell tab = same sandbox identity/mounts as the pinned runtime
  attempt. Node-shell tab = dedicated low-privilege operator identity, NOT the
  Envoy service identity.
- File explorer/editor writes = operator host effects through Envoy with the
  same scoping — never a new unrestricted fs API, never reachable by agents.

## 6. Phasing

- **0.C (frontend, ships with/after ADR 0020 Phase 0):** AppShell-root cockpit
  shell — floating, tabbed, top-right toggle, per-user geometry/tab manifest in
  localStorage, over a mock PTY. Proves Layer A (navigate + toggle, console
  stays open) with browser evidence. No agent path.
- **3.A (backend, Phase 3):** Envoy PTY primitive + Hall TerminalRecord +
  operator WS + attach/replay; wire the real terminal; move tab manifest to Hall.
- **3.B:** file explorer + editor panes over Envoy-scoped file ops; Fleet
  node-shell tab with operator identity.

## 7. Open questions

- Tab manifest authority: localStorage (per-browser) vs Hall (per-user,
  cross-device). Start localStorage in 0.C; move to Hall in 3.A.
- Editor save semantics: reuse the Vault save path where the target is a vault;
  repo edits go through the repo workspace, not raw fs.
- Cockpit target: pinned **per-tab at open** (a tab binds to a session/node), so
  navigating views never silently repoints a live shell.
