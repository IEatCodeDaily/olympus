# Card B — Envoy agent detection (zero-config agent discovery)

## Goal
The user should never "connect" their agents. They install the envoy (for the
single-node MVP, the control-plane process IS the envoy) and it **detects which
agent harnesses are installed on the host** — Hermes, Claude Code, Codex,
Cursor, Gemini, etc. — by scanning PATH and probing each binary. Detected agents
merge into the existing `/api/agents` response so the composer's Agent picker
offers real, installed, drivable agents.

This extends commit `6494e85` (real agent/model discovery from Hermes profiles).
Read `crates/control-plane/src/server/agents.rs` FIRST — `list_agents()` and
`list_models()` already exist and read Hermes profiles. You are ADDING host
binary detection alongside the profile-based agents, not replacing it.

## Settled decisions (do NOT re-litigate)
- An "agent" today = a Hermes profile (`AgentInfo {id, provider, model,
  is_default}`). Keep that. ADD detected host harnesses as additional entries
  with a `kind` discriminator so the UI can tell a Hermes profile from a
  detected external binary.
- Single-node MVP: the control plane does the `which`/probe scan itself. Do NOT
  build a separate envoy process or RPC — that's post-MVP. Just a module that
  scans the local PATH.
- Detection is via "simple search of the typical bash command" (operator's
  words): check for the known binaries on PATH. Cheap, synchronous, cached.

## CRITICAL footgun (from Zephyr's adversarial review — DESIGN AROUND THIS)
**"Detected" ≠ "drivable."** `which claude` proving a binary exists does NOT mean
we can drive it over ACP. Cursor/Codex/Gemini do not all expose a headless ACP
loop the way `hermes acp` does. So:
- Detection must yield a CAPABILITY descriptor, not just a name:
  `{ name, binary_path, version, transports: [...], drivable: bool }`.
- `drivable` = true ONLY for harnesses Olympus can actually spawn+drive today
  (Hermes via `hermes acp`; others via `acp` subcommand IF detected). For the
  rest, `drivable: false` and the UI shows them as "detected, not yet drivable".
- **Detect generously, advertise conservatively.** A non-drivable agent must NOT
  be selectable as a session driver (or selecting it must clearly no-op with a
  message — not a silent hang, which is the exact "I can't send" bug we already
  fixed once).

## Detection table (probe these; extend if trivial)
| name        | binary      | version probe            | drivable check |
|-------------|-------------|--------------------------|----------------|
| hermes      | `hermes`    | `hermes --version`       | true (has `acp`) |
| claude-code | `claude`    | `claude --version`       | probe for `--acp`/`acp` support; else false |
| codex       | `codex`     | `codex --version`        | false for MVP unless an acp mode is confirmed |
| cursor      | `cursor-agent` or `cursor` | `--version` | false for MVP |
| gemini      | `gemini`    | `gemini --version`       | false for MVP |

Run version probes with a SHORT timeout (e.g. 2s) and a structured argument
array — NEVER shell string construction (AGENTS.md hard rule). If a probe times
out or errors, record the binary as detected-but-version-unknown, drivable=false.

## Contract discipline (HARD RULE)
A contract change touches THREE files together:
1. `docs/api-contract.md` — document the new fields on the agents response
   (`kind`, `drivable`, `version`, etc.) and the new detection semantics.
2. Rust: extend `AgentInfo` (or add a parallel `DetectedAgent` and a combined
   response) in `server/agents.rs` + the `/api/agents` handler.
3. `ui/src/types.ts` — extend `AgentInfo` to match, and update
   `ui/src/mocks/{fixtures,handlers}.ts` so the mock `/api/agents` matches the
   new shape (e2e/mocks stay green).

## Concrete steps
1. New `detect_host_agents() -> Vec<DetectedAgent>` in `agents.rs`: for each row
   in the detection table, resolve the binary on PATH (use `which`-equivalent:
   scan `$PATH`, or the `which` crate IF already a dep — check Cargo.toml; do NOT
   add a dep just for this, a PATH scan in std is fine), run the version probe,
   set `drivable`.
2. Decide the merge shape: simplest is to make `/api/agents` return
   `{ agents: [...Hermes profiles with kind:"hermes-profile"...,
   ...detected host agents with kind:"host-binary"...] }`. Hermes profiles stay
   `drivable: true`. Keep `default` first.
3. Cache the detection result (it doesn't change mid-process) — compute once,
   store in `AppState` or a `OnceCell`, so `/api/agents` stays fast.
4. UI: the `useAgents()` hook in `ChatView.tsx` already fetches `/api/agents`.
   Update it + the Agent `<select>` to (a) show drivable agents as selectable,
   (b) show non-drivable detected agents as a disabled/greyed group labeled
   "detected (not yet drivable)". Update `types.ts` + MSW fixtures/handler.

## Out of scope (do NOT do)
- A separate envoy process / iroh RPC / multi-node — MVP is in-process detection.
- Actually driving non-Hermes agents — only DETECT them; driving is a later card.
- Auth-state probing (whether the binary is logged in) — note the gap; for MVP
  drivable is purely "can we spawn its ACP loop".

## Verification (REQUIRED before signaling done)
- `make verify` must print `ALL CANONICAL GATES GREEN` (run in background, ~2.5min).
- Rust tests: detect_host_agents finds `hermes` and marks it drivable; a
  non-existent binary is absent (not a crash); the `/api/agents` response
  includes both profile agents and detected agents with correct `kind`/`drivable`;
  AgentInfo/DetectedAgent serialize camelCase. Mock the PATH in tests (point at a
  temp dir with fake executable stubs) so the test is deterministic and doesn't
  depend on what's actually installed on the host.
- The standalone patch-tool linter FALSELY reports E0670 (`async fn` not allowed
  in Rust 2015). IGNORE it — verify with real `cargo build`/`cargo test` (edition
  2021, exits 0).

## Signal
When done and `make verify` is green on your worktree, set the card to
`blocked: review-required` with a comment: files touched, the detection approach,
which agents probe as drivable, test names, and the `make verify` result. The
controller (Zephyr) re-runs the gate on the merged tree and commits. Do NOT
commit/push to main yourself.

## Attribution (if you commit in your worktree)
Commit trailer: `Authored-by: Zephyr (AI Assistant) <raisalpwardana+zephyr@gmail.com>`
plus `Co-authored-by: <your-profile> (<your-model>) via Olympus swarm`.
Use `git -c commit.gpgsign=false`. Avoid "reboot"/"shutdown" in commit messages.
