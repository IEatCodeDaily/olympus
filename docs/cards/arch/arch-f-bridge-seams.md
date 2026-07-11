# ARCH-F · Deepen the envoy bridge: framing / ACP protocol / child lifecycle seams

## Goal
`crates/envoy/src/bridge/hermes.rs` (~1.1k loc) mixes three concerns: wire
framing (newline-JSON vs Content-Length), ACP method mapping, and child-process
lifecycle (spawn/resume/reap). Cut three INTERNAL seams so each is testable
without live children. The external interface (`AgentRuntime` + the factory
trait) must NOT change.

## Read FIRST
- `crates/envoy/src/bridge/{hermes,acp,mod}.rs` — current shape.
- `crates/envoy/src/adapter/` — how the three harness adapters feed the bridge.
- `crates/envoy/src/runtime_table.rs` and `mock_runtime.rs` — the external
  seam you must preserve.
- ARCH-E's merged result — the observer module is a bridge consumer now; don't
  break it.

## Build on
Branch from main after ARCH-E merges.

## Deliverables
1. `bridge/framing.rs`: a `Framing` trait with the two existing adapters
   (newline-JSON for hermes, Content-Length per ACP spec for claude/codex).
   Unit-tested with byte fixtures both directions, including split-buffer /
   partial-frame cases.
2. `bridge/client.rs`: harness-agnostic ACP client — method map
   (initialize/session_new/prompt/cancel/request_permission...), request-id
   correlation, event stream demux. Testable against an in-memory duplex pipe,
   no child process.
3. `bridge/child.rs`: spawn/env/cwd wiring, health, reap, the npx invocation
   table. The ONLY module allowed to touch tokio::process.
4. `bridge/hermes.rs` shrinks to composition of the three + harness-specific
   quirks. The three historical live-process bugs (documented in the repo's
   references/postmortems — check docs/postmortems/) each get a regression
   test at the seam where they hid, where feasible without a live child.
5. `cargo test -p olympus-envoy` must not require network or installed
   harnesses (npx download paths mocked/stubbed at the child seam).

## Settled decisions — do NOT re-litigate
- `AgentRuntime`'s public surface and the factory trait are FROZEN — Hall-side
  code and mock_runtime must compile untouched.
- No new harnesses, no behavior changes; this is an internal deepening.
- Capability-driven dispatch (never harness-name matching) stays as-is.
- Events channel stays broadcast + subscribe-per-turn (the take-once mpsc bug
  is settled history — do not reintroduce).

## Gates
- `cargo test --workspace` + clippy `-D warnings` + fmt green.
- The live smoke (`examples/acp_smoke.rs` pattern) is run by the CONTROLLER at
  review, not by you — do not spawn live agents.
- Do not push to main. Green → `blocked: review-required`.
