# Olympus — Handover

_Last updated: 2026-07-04 · branch `main` (clean) · latest `f47ef1f`_

Olympus is a local-first, multi-node AI-agent workbench: a Rust control plane
+ Vue/React UI that drives coding agents (Hermes profiles, Claude Code, Codex)
over ACP, with Sessions / Vaults / Projects / Fleet / Settings surfaces.

## Run it

```bash
# Control plane — runs as a systemd USER unit (NOT tmux/manual):
systemctl --user status olympus.service      # :8799, journalctl --user -u olympus
systemctl --user restart olympus.service      # after a `cargo build --release`

# UI dev server (vite):  http://localhost:5177  (proxies /api + /ws → :8799)
cd ~/olympus/ui && npm run dev                # already running in most sessions

# Rebuild after Rust changes:
cd ~/olympus && cargo build --release && systemctl --user restart olympus.service
# Rebuild UI:
cd ~/olympus/ui && npm run typecheck && npm run build
```

Token: `~/.olympus/token` (regenerated each server restart — keep
`ui/.env.local` `VITE_API_TOKEN` in sync or the WS 401s).

## Hard-won landmines (read before debugging)

- **NEVER run the control-plane binary manually or let a kanban worker spawn
  its own server.** A 2nd process fights the redb lock (`~/.olympus/eventlog.redb`
  "Database already open") AND the port → `ensure_runtime` fails → silent
  "chat broken". If chat dies, check `fuser ~/.olympus/eventlog.redb`.
- **The patch-tool linter FALSELY reports E0670 `async fn` errors** on every
  edit to server files. Ignore it; trust `cargo build --release` (edition 2021).
- The systemd unit sets `Environment=PATH` to include `~/.local/bin` +
  node bin dir, or claude/codex discovery finds nothing.
- UI dev: `VITE_API_BASE` is empty → `getWsUrl()` derives WS origin from
  `window.location.origin` (never `new URL("")`, which throws & silently kills
  the socket — that was the original "chat doesn't work" bug).

## What works (verified in-browser)

- **Chat end-to-end** for Hermes profiles + Claude Code (live WS streaming,
  reply renders without reload). Codex is installed + ChatGPT-authed on this host.
- **Session state machine**: `idle → running → input-required → idle`
  (managed sessions use the authoritative in-flight flag, not a recency window).
- **ACP permission flow** (`session/request_permission`): agent blocks → amber
  prompt in transcript (Allow/Reject/Cancel) → `POST /api/sessions/:id/permission`
  → agent resumes. Verified with a real file-write gate.
- **Session titles** auto-derived from first user message (no more all-Untitled).
- **Composer**: `+` menu bottom-left (attach/mention — stubs), agent-scoped
  model selector (`GET /api/agents/:id/models` — codex never sees Claude),
  node+agent meta row OUTSIDE/below the box.
- **Brand icons** (`ui/src/components/BrandIcons.tsx`): real Claude/OpenAI/Codex/
  Nous/Z.ai marks, in brand color; `agentBrand(kind,provider)` picks by harness
  KIND first.
- **Per-node agent discovery** (ADR 0007): each node's envoy owns its agent
  list; local node discovers in-process at boot; `NodeInfo.agents[]`; manual
  "Detect agents" button in Fleet › Agents (`POST /api/nodes/:id/agents/refresh`).
- **qa-engineer profile** (`~/.hermes/profiles/qa-engineer`, claude-sonnet-4-6):
  clarify → test live → Playwright e2e → watch-it-fail persona.

## Architecture quick map

- `crates/control-plane/src/`
  - `server/mod.rs` — all HTTP routes + handlers (huge file).
  - `server/agents.rs` — `discover_local_agents()` (Hermes profiles + PATH CLI
    probe), `list_models_for(provider)`.
  - `node.rs` — `NodeRegistry` (per-node agents, in-flight, awaiting_input),
    UDS envoy protocol.
  - `bridge/{mod,acp,hermes}.rs` — ACP client, `AgentRuntime`, permission
    respond, spawn routing (hermes acp / npx @zed-industries adapters).
  - `server/bridge_mgr.rs` — runtime lifecycle, liveness flags.
- `ui/src/` — TanStack Router (URL-persistent), TanStack Query (server), Zustand
  (ephemeral UI). Views: `SessionsView`, `FleetView`, `VaultsView`,
  Projects/Settings (`PlaceholderViews`). Composer + ChatPage under
  `views/sessions/`. WS + fetch in `api.ts`; hooks in `hooks/queries.ts`.
- ADRs in `docs/adrs/` — 0002 (fleet boundary), 0005 (resource model),
  0007 (per-node discovery). Read these before touching node/agent code.

## Open follow-ups (nothing blocking)

1. **Standalone `olympus-envoy` binary** — remote nodes currently can't report
   agents (local node runs its envoy in-process; remote refresh returns 501).
   The discovery contract is defined; the binary + transport wiring is the work.
   Also: remote agent *installation* ("install agent" in Fleet).
2. **Discovery checks binary-exists only, not auth** — a codex that's installed
   but logged-out still lists as ready, then fails at first message with
   `Authentication required`. Consider probing auth during detect and marking
   "needs login".
3. **Permission timeout leaves session stuck** — if an ACP permission request
   times out at the adapter (~60s, no UI answered), `awaiting_input` isn't
   cleared → liveness stuck at `input-required`. Fix: clear on turn Error/timeout
   + default-deny policy.
4. **Composer `+` menu actions are stubs** (attach file / mention session /
   reference file) — menu renders, handlers do nothing yet.
5. **react-doctor** score ~36/100 — most remaining items are accepted
   architectural opinions + a11y warnings on pre-existing views. Run
   `cd ui && npx react-doctor@latest --diff` before big UI PRs.

## Testing

```bash
cd ~/olympus && cargo test -p olympus-control-plane        # 254 passing
cd ~/olympus/ui && npm run test                            # vitest
cd ~/olympus/ui && npx playwright test                     # e2e, MSW-mocked, :5188
```

## Conventions

- Commit trailer: `Authored-by: Zephyr (AI Assistant) <raisalpwardana+zephyr@gmail.com>`
- Don't reintroduce hardcoded hex in CSS — use the `--space-*` / color tokens.
- Frontend state: URL-persistent via TanStack Router; TanStack Query for server,
  Zustand for ephemeral UI only.
