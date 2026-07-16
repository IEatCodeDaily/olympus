# Postmortem 0029 — Claude Code / Codex `session/set_model` "Method not found"

- Date: 2026-07-14
- Severity: high (every Claude Code and Codex turn that carried a model failed)
- Status: fix implemented; verification/deploy pending
- Related: postmortem 0024 (ACP framing/correlation), ADR 0020

## Symptom

Testing Claude Code in a managed Olympus session produced, on every prompt:

```
⚠ agent error: {"code":-32601,"data":{"method":"session/set_model"},
  "message":"\"Method not found\": session/set_model"}
```

Durable proof in `~/.olympus/olympus.db`: session
`20260713T181700Z-eea409aa`, system messages 11, 13, 15 each contain that
JSON-RPC error.

## Root cause

Olympus assumed one uniform ACP method surface across harnesses. The composer
always sends a model with a prompt. In `AcpClient::send_command`, a
`Prompt { model: Some(_) }` unconditionally emitted an
`AgentCommand::SwitchModel` first, which `AcpRequest::from_command`
(`crates/envoy/src/bridge/acp.rs`) mapped to the JSON-RPC method
`session/set_model`.

`hermes acp` implements `session/set_model`. The Zed **Claude Code** adapter
(`@agentclientprotocol/claude-agent-acp@0.58.1`) and **Codex** adapter do not —
they expose model selection through the ACP-standard
`session/set_config_option` with `configId: "model"` (verified in the adapter's
`dist/acp-agent.js:2096-2102`, `applyConfigOptionValue` with `MODEL_CONFIG_ID`).
Sending `session/set_model` to them returns `-32601 Method not found`, and
because the client awaits the correlated response, the turn failed before the
prompt was ever sent.

This is the same class of bug as postmortem 0024's framing mismatch: a single
code path assuming all ACP harnesses share a method/transport contract they do
not.

## Fix

Made model selection harness-specific, mirroring the existing `AcpFraming`
seam:

- `olympus-proto`: new `ModelSetStyle { SetModel, ConfigOption }`.
- `bridge/hermes.rs`: `model_set_style_for_agent()` returns `SetModel` for
  Hermes, `ConfigOption` for Claude Code / Codex; carried on
  `HermesRuntimeConfig.model_set_style`.
- `bridge/acp.rs`: `AcpRequest::set_model(session_id, model, style, id)` builds
  `session/set_model {sessionId, modelId}` or
  `session/set_config_option {sessionId, configId:"model", value}` per style.
- `bridge/client.rs`: `AcpClient` carries the style
  (`with_events_and_model_style`) and uses `set_model()` before a model-bearing
  prompt.
- Both runtime factories (`crates/envoy/src/main.rs`,
  `crates/control-plane/src/main.rs`) resolve the style per agent.

Tests: `set_model_uses_set_model_for_hermes_style`,
`set_model_uses_config_option_for_claude_and_codex_style`,
`model_set_style_is_harness_specific` (in `crates/envoy/src/bridge/mod.rs`).

## Verification (live)

- Debug: 4 focused tests pass (`set_model_uses_set_model_for_hermes_style`,
  `set_model_uses_config_option_for_claude_and_codex_style`,
  `model_set_style_is_harness_specific`, regression preserved); full
  `olympus-proto` + `olympus-envoy` suite 90 passed / 0 failed;
  `olympus-control-plane` builds clean (both factory call sites).
- Deployed envoy-only (`ModelSetStyle` is derived locally in the envoy factory
  and never crosses the wire; `PROTOCOL_VERSION` unchanged, so Hall needs no
  redeploy). Backed up Hall DB (integrity_check=ok) + spool first. Symlink
  flipped `olympus-envoy → olympus-envoy-0a73a86-setmodelfix`, restarted
  `olympus-envoy@1`, confirmed `terminus` online in `/api/nodes` at
  `version 0.1.0 (0a73a86b1b5a)`, `claude-code` agent `ready:true`.
- Live turn: managed `claude-code` session `20260714T052819Z-598f39ad`, prompt
  with `model: claude-opus-4-8` → assistant replied `OLYMPUS_SETMODEL_FIX_OK`;
  zero `set_model` occurrences in the session; zero `-32601`/"method not found"
  in the envoy log. The previously-failing path now works.
- NOTE: deploy tag uses git HEAD `0a73a86` but the tree is dirty (786-line
  in-flight ACP-correctness delta in the envoy crate); the installed binary is
  suffixed `-setmodelfix` to disambiguate from any clean-HEAD build.

## Lessons

- Every ACP method is a per-harness capability, not a protocol constant. Route
  method/framing/model selection through explicit `AgentKind` seams; never
  assume a shared surface.
- The correct long-term posture is capability-driven: inspect the harness
  `initialize`/session capabilities and select the model through the advertised
  mechanism, or fail with a clear message — not emit a generic method and hope.
