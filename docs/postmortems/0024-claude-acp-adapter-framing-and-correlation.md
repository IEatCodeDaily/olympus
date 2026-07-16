# 0024 — Claude ACP adapter, framing, and response correlation failure

**Status:** Fixed and verified in production  
**Incident:** 2026-07-13 16:19 UTC  
**Affected session:** `20260710T235357Z-4df51aa3`

## Impact

A message sent to the `claude-code` agent failed before the turn started. Hall persisted the user-visible failure only after a 30-second delay:

> `ensure_runtime failed: starting agent runtime (lazy): timed out after 30s waiting for ACP session/new|resume response (no stderr captured)`

## Root cause

Three defects combined:

1. Envoy launched bare `claude`, not a pinned Claude ACP server.
2. Envoy selected LSP-style Content-Length framing. The tested Claude ACP adapter `@agentclientprotocol/claude-agent-acp@0.58.1` consumes newline-delimited JSON.
3. `AcpClient` polled shared handshake state instead of awaiting the correlated JSON-RPC response. An immediate adapter error or EOF therefore degraded into a misleading 30-second timeout.

A direct wire probe reproduced the protocol mismatch. Newline JSON completed both `initialize` and `session/new`; Content-Length produced `Failed to parse JSON message: Content-Length ... is not valid JSON`.

## Contributing factors

- The adapter executable and dependency graph were not part of Envoy deployment.
- Startup cleanup targeted only the direct process and could leak descendants.
- Resume pre-seeded a session ID before the adapter accepted the request.
- Resume/fork did not preserve the configured MCP server list.
- Stale comments claimed Content-Length was required by ACP and concealed the adapter-specific reality.

## Corrective actions

- Pin the complete npm graph for `@agentclientprotocol/claude-agent-acp@0.58.1` under `adapters/claude-agent-acp/`.
- Provision it at install/deploy time under `$OLYMPUS_HOME/adapters/claude-agent-acp` and invoke its absolute executable path.
- Require Node.js 22 or newer and verify the exact installed version and executable.
- Select newline JSON for the pinned Claude and Codex adapters.
- Await correlated JSON-RPC responses and propagate JSON-RPC errors, EOF, read failures, and decode failures immediately.
- Set session identity only after successful `session/new|resume|fork`.
- Preserve MCP configuration on resume/fork.
- Place each adapter in an owned process group; close stdin, wait briefly, then terminate/kill the full group on failure.
- Capture bounded stderr and include it in startup failures.

## Verification

Source gates completed:

- Direct Claude adapter wire probe: newline `initialize` and `session/new` succeeded.
- Correlated error, EOF, failed-resume, MCP-preservation, and replay-gating unit tests passed.
- Process-tree timeout regression passed with no surviving adapter/child PID.
- Envoy suite: 69 library, 2 binary, and 2 iroh tests passed.
- Envoy Clippy passed with `-D warnings`.
- Installer dry run showed locked install-time provisioning.

Production verification completed after deploying the provisioned adapter and
new Envoy:

- A fresh Olympus Claude session returned exactly `OLYMPUS_CLAUDE_OK`.
- After the runtime was gone and the Envoy connection was stable, the same
  session resumed and returned exactly `OLYMPUS_CLAUDE_RESUME_VERIFIED`.
- `terminus` remained online with 0/4 slots after the completed turns.

An initial restart test posted while systemd was active but before Hall had a
fresh Envoy connection. Hall selected the stale connection and produced another
30-second startup timeout. Retrying after Fleet reported the reconnected Envoy
succeeded. This is a separate reconnect-readiness/fencing defect covered by ADR
0017; service `active` is not a valid readiness signal.

## Prevention

Adapter command, version, framing, and runtime dependencies are one deployment contract. Adding or upgrading an adapter requires a pinned manifest, empirical wire fixture, offline runtime-start test, correlated error test, and process-tree cleanup test.