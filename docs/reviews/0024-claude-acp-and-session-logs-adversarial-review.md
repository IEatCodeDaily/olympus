# 0024 — Claude ACP startup and Hall-backed session logs: adversarial review

**Date:** 2026-07-13  
**Status:** Pre-implementation adversarial review  
**Verdict:** **NO-GO as one combined slice.** The Claude framing diagnosis is correct, but the proposed command pin and direct-child reap are not sufficient for deterministic startup or leak-free failure handling. A new Hall `SessionLog` store is also not admissible before ADR 0018's prerequisites. The acceptable immediate logs slice is only a UI projection of existing Hall product truth, with live diagnostics remaining explicitly ephemeral.

## 1. Scope and reviewed snapshot

Repository: `/home/rpw/olympus`

- HEAD: `f784b04aa6d04e6759eef999012f48ac3f0f1622`
- Pre-report tracked diff SHA-256: `64174873dc5f00f75454ca059913590a0820d4941caef852c1d81b3afda8b9bc`
- Relevant reviewed file hashes:
  - `crates/envoy/src/bridge/hermes.rs`: `6d879b7e03e21cbf3a33e4facb2389a0b9cb1a6f6a5b3551541899ac7a3e2424`
  - `crates/envoy/src/bridge/child.rs`: `66b909564f443eb92772f2752b584271a8944ca85087b3a4f02640c0b52f8130`
  - `crates/envoy/src/bridge/client.rs`: `67eb124774104026d0a807704562ce87c8db6baea09654230839c4ca1132602d`
  - `crates/envoy/src/runtime_table.rs`: `48e9e1a470ced0835627e513fbb7a4228bd8d2de53c60023a9fc0e6a15528a61`
  - `ui/src/views/sessions/components/BottomPanel.tsx`: `0e963b56af5f5525f3befc80333082d13801ccd6b9bfc8ae44b0ba81e3b8305e`
  - ADR 0018: `f2df8e838a378798d2290dd79df83441b342feb5ab870ce8cc0bdaf5a7cde330`

The working tree was already dirty and changed during the review. The current draft contains the command/framing edits and a UI-only message-derived log projection. This report treats those drafts as the proposed implementation; it did not edit source.

Post-write verification found the full tracked diff had moved to `912721643539a69ff40779b985ca86d21be124b354b774fb853ce4a3a2040351` because other work continued concurrently. HEAD and every scoped file hash listed above remained unchanged; findings are anchored to those per-file hashes rather than to unrelated working-tree changes.

Authoritative adapter examined:

- npm package: `@agentclientprotocol/claude-agent-acp@0.58.1`
- npm integrity: `sha512-LWaiPVfvjy8TwQ8WjDWrQpnOeIq3+fIBPDT0/1BFgeYGq7ihorjDi0mFMBsIQRT4PFVQtTezGDEOYgRGS7fi2g==`
- package engine: Node `>=22`
- upstream tag commit inspected: `f3d8ae3eb389ee9367f48c8762562a55280ade0a`

## 2. What is confirmed

The reported startup failure has a real protocol mismatch, not merely a slow process:

1. HEAD selected bare `claude` for `claude-code`; the draft changes that to `npx -y @agentclientprotocol/claude-agent-acp@0.58.1` (`child.rs:18,43-57`). Bare Claude Code is not the pinned ACP adapter contract.
2. The exact `0.58.1` adapter uses newline-delimited JSON on stdio. An empirical probe received an `initialize` response for newline JSON; a Content-Length request produced a parse error on stderr.
3. The draft changes Claude to `AcpFraming::NewlineJson` (`hermes.rs:29-36`). This is the correct transport for that exact adapter.
4. The exact adapter advertises both `loadSession` and `sessionCapabilities.resume`, and implements `resumeSession` through `getOrCreateSession`.
5. The targeted table test passed on the reviewed draft:

   ```text
   test bridge::hermes::tests::command_and_framing_tables_preserve_harness_contracts ... ok
   test result: ok. 1 passed; 0 failed
   ```

That test only proves constant selection. It does not execute the adapter, verify package acquisition, correlate JSON-RPC failures, resume a session, or prove process-tree cleanup.

## 3. Blocking findings

### B1 — `npx -y package@version` is version selection, not deterministic execution

**Severity:** Blocker

The draft pins the top-level package name and version, but runtime still depends on:

- `npx`, `node`, registry availability, DNS/TLS, npm configuration and cache state;
- the package remaining available from the configured registry;
- runtime dependency resolution. The published tarball does not ship its upstream `package-lock.json`; its manifest includes ranges/peers, so the full graph is not locked by Olympus;
- a cold download completing inside the same 30-second ACP startup timeout.

Deployment currently fails closed only for `hermes`. `scripts/install-envoy.sh:155-171` does not require `node >=22` or `npx`; it merely warns about `bunx`. The systemd unit's PATH comment still describes Bun adapters (`systemd/olympus-envoy@.service:11-13`). A string-equality unit test cannot detect any of these failures.

**Failure scenario:** a clean or offline envoy starts correctly for Hermes but every first Claude session blocks on package acquisition, fails because Node is too old/missing, or resolves a different transitive graph. The user sees the same generic 30-second timeout and startup behavior differs by host/cache.

**Required correction:** provision the adapter during install/deploy from a checked-in consumer lockfile with recorded integrity, then invoke its installed binary by a fixed path. Validate Node `>=22` and the binary's `--version` before starting the service. Runtime session startup must not install software or require the network.

If runtime `npx` is intentionally retained, it must be documented as a non-deterministic operational dependency and gated by an install-time cold-cache/offline policy. That is an inferior fallback, not a pin.

### B2 — failed startup still leaks the adapter/process tree

**Severity:** Blocker

The current start path has no failure funnel:

- `HermesAgentRuntime::start` spawns, writes `initialize`, writes `session/new|resume`, then returns any error directly (`hermes.rs:188-200`).
- `RuntimeTable::ensure_runtime` registers the runtime only after `start` succeeds (`runtime_table.rs:135-155`). A failed runtime therefore has no table owner available for later stop.
- `tokio::process::Command` is not configured with `kill_on_drop`; dropping `tokio::process::Child` does not establish the required cleanup contract.
- `ChildHandle::reap` only targets the immediate child with `Child::kill()` and then waits (`child.rs:125-132`). On Unix this is a forceful kill, not the adapter's graceful shutdown protocol.
- The stdin writer has already been moved from `ChildHandle` into `AcpClient` (`hermes.rs:113-120`), so `ChildHandle::reap` cannot actually close stdin at line 127 while any client `Arc` remains.
- With `npx`, the immediate child can be a wrapper. The adapter then owns a Claude SDK/native child. Killing only the wrapper or adapter can orphan descendants.

The exact `0.58.1` entrypoint explicitly disposes the agent when stdin/connection closes and handles SIGTERM/SIGINT. Immediate SIGKILL bypasses that cleanup.

**Failure scenario:** each 30-second retry leaves an `npx`, Node adapter, Claude native child, or reader task behind. The next retry competes for memory/files/session state, turning a framing bug into a host stability incident.

**Required correction:** every error after spawn—write failure, JSON-RPC error, decoder error, EOF, timeout, and cancellation—must pass through one cleanup routine. That routine must:

1. release all client/writer owners so stdin can close;
2. allow a short grace period for the adapter's connection-close disposal;
3. send SIGTERM to a dedicated process group if still alive;
4. after a bounded grace, SIGKILL the process group;
5. `wait` the direct child and join/abort stdout/stderr tasks.

A test must spawn a fake adapter that creates a grandchild, force startup failure, and assert that both PIDs disappear before retry. Reaping only the direct PID does not satisfy this blocker.

### B3 — startup does not await or correlate ACP responses; resume can falsely succeed

**Severity:** Blocker

`AcpClient` tracks pending requests only as `id -> PendingKind` (`client.rs:31-51`). `initialize()` and session methods return after writing bytes, not after receiving their JSON-RPC response (`client.rs:119-162,273-315`). `HermesAgentRuntime::start` consequently emits `initialize` and `session/new|resume` without awaiting initialization success.

On a response, `handle_message` removes the pending ID before examining whether the response contains `error` (`client.rs:249-270`). Errors are broadcast as generic `AgentEvent::Error`, but startup has no receiver/correlation path for them.

This creates two distinct bad outcomes:

- `session/new` error: no session ID is learned, so startup waits the full 30 seconds despite already having the real JSON-RPC error.
- `session/resume` error: `session_resume` pre-seeds `session_id` before sending. Once the error response removes the pending ID, `wait_for_handshake` can see “not pending + session ID present” and return success.

Reader EOF and decode failures are also not stored as terminal client state, so they degrade to a timeout if the child remains observable.

**Required correction:** handshake requests need per-ID completion channels returning `Result<Value, AcpError>`. Await `initialize` before issuing a session method, await the exact session response, and surface EOF/decode errors immediately. `wait_for_handshake` should not infer success from shared flags.

Minimum regression cases:

- initialize JSON-RPC error fails immediately with the adapter message;
- session/new JSON-RPC error fails immediately;
- session/resume JSON-RPC error cannot report success;
- stdout EOF and malformed newline JSON fail immediately;
- an unrelated response cannot satisfy the handshake.

### B4 — resume currently changes the session-defining MCP configuration

**Severity:** Blocker for resume compatibility

New sessions include `self.config.mcp_servers` (`hermes.rs:193-197`; `client.rs:296-303`). Resumes call `build_session_resume_request(..., &[])` (`client.rs:305-315`). Fork has the same omission.

The exact Claude adapter fingerprints session-defining parameters as `{cwd, mcpServers}` and recreates the underlying Query when they differ. Its cross-process resume path creates the resumed session with the provided `cwd` and `mcpServers`. Passing an empty list therefore does not preserve the session environment created by Olympus.

**Failure scenario:** a Claude session created with Olympus MCP servers resumes without them after an envoy restart. Tools disappear or behavior changes even though the UI reports a resumed session.

**Required correction:** pass the same normalized MCP configuration to new, resume, and fork requests. Add a real exact-adapter smoke test that creates a session, restarts the adapter process, resumes the returned session ID with the same cwd/MCP set, and confirms a successful response. Also test an intentional configuration change so recreation semantics are understood rather than accidental.

### B5 — a new durable Hall `SessionLog` store would contradict ADR 0018

**Severity:** Architecture blocker

`SessionLog` is currently a WS frame only (`crates/control-plane/src/server/ws.rs:125-136`). Its producers are human-oriented lifecycle strings in `sessions.rs`; some include arbitrary adapter errors and warnings, and `steer.delivered` includes the first 80 characters of steering text (`sessions.rs:1839-1847`). The browser also synthesizes lifecycle entries from other frames using client timestamps.

Persisting this stream directly in `olympus.db`, adding a second ad-hoc Hall table, or treating it as permanent event-log product truth would create an unbounded, sensitive diagnostic authority outside the approved architecture:

- ADR 0018 separates permanent product truth from TTL diagnostics (`ADR 0018:48-65,175-191`).
- Durable diagnostics belong in local `telemetry.db` with explicit TTL, row/byte caps, bounded pagination, pressure behavior and delete semantics (`ADR 0018:193-241`).
- Prompt/reasoning/tool payloads and secrets are excluded; collection must pass the shared redaction firewall (`ADR 0018:269-302,356-379`). Current `SessionLog.message` is not safe by construction.
- Remote observability and identity scope remain prerequisite-gated by ADR 0017 Tasks 1.1-1.4 / PRE-OBS (`ADR 0018:71-99`; implementation plan `2026-07-13-otel-observability-session-diagnostics.md:86-143,590-593`).
- ADR 0018 explicitly forbids completeness claims when telemetry coverage is partial (`ADR 0018:383-393`).

There is also no stable log ID or sequence. A fetch-plus-WS merge would miss or duplicate rows at the hydration boundary, and timestamp/source/message is not a valid cursor. “Clear” in the current UI is local view state, not an authorization or retention policy.

**Required correction:** do not add a new `SessionLog` event, table, endpoint, or retention path in this immediate fix.

The acceptable immediate interpretation of “Hall-backed” is narrower: rebuild a **recent product lifecycle view** from the existing Hall message projection, whose authority and retention already exist, and keep `session.log` as a clearly labelled live-only tail. The concurrent `logsFromMessages()` draft in `BottomPanel.tsx:88-142,174-190` follows that direction and does not create a second store. It must not be described as durable diagnostic-log persistence.

If strict “query only; no client ring” semantics are required, omit live `session.log` rows from the Logs tab for now. Do not persist them merely to make the tab look complete.

## 4. High-priority non-blockers

### P1 — the draft changes Codex framing without evidence from the Claude incident

`hermes.rs:33-36` changes both Claude and Codex to newline JSON and claims every supported adapter uses it. The fresh evidence in this incident proves only `@agentclientprotocol/claude-agent-acp@0.58.1`.

Keep the immediate diff Claude-specific. Change Codex only after an independent probe of the exact pinned `@zed-industries/codex-acp@0.16.0` executable. This is both safer and the smaller fix.

### P1 — stale framing code and tests can reintroduce the bug

Three authorities disagree:

- active composition now selects newline JSON (`hermes.rs:29-36`);
- `acp.rs:4-12,355-380` still states ACP/Hermes use Content-Length and contains a second `Frame` implementation;
- `bridge/mod.rs:68-97,145-178` still tests that obsolete Content-Length helper;
- `docs/cards/arch/arch-f-bridge-seams.md:21-25` says Claude/Codex use Content-Length.

The green table test can coexist with green obsolete framing tests, giving false confidence. Delete the unused duplicate helper/tests if no caller remains, or relabel them as a generic codec for a future explicitly selected adapter. Correct the architecture card/comment in the same transport fix.

### P1 — product-derived Logs are deliberately partial

`useMessages` requests at most 100 messages, while the panel can show 500 log rows. The message-derived draft is therefore a bounded recent projection, not complete session history. That is acceptable now, but the UI should say “Recent lifecycle” or otherwise avoid a completeness claim.

The live IDs built from timestamp/source/message can collide for repeated diagnostics. This is tolerable only while those rows are ephemeral. A future durable design needs server-issued monotonic IDs and a snapshot high-water mark.

### P1 — do not persist raw stderr or arbitrary adapter strings

The bounded 8 KiB stderr tail improves local diagnosis, but adapter/npm stderr can contain filesystem paths, registry URLs, headers, environment-derived values or user content. Do not copy it verbatim into a permanent system message or future Hall log store. Persist a structured/sanitized error class and keep raw process stderr in local service diagnostics until ADR 0018's redaction firewall exists.

### P2 — `session.log` is also being used as a control signal

`ChatPage` watches for the human string `steer.delivered` to clear steering UI, while the Logs panel displays the same frame. Replaying persisted logs must never retrigger control behavior. A future cleanup should use a dedicated typed acknowledgement frame, but that is not required for the Claude startup fix. For now, keeping `session.log` live-only avoids making the coupling worse.

## 5. Minimum safe implementation slice

### 5.1 Claude startup — admissible slice

1. Provision `@agentclientprotocol/claude-agent-acp@0.58.1` at install/deploy from a checked-in lock with integrity; validate Node `>=22`; invoke the installed binary directly.
2. Select `NewlineJson` for Claude only. Leave other adapter mappings unchanged unless separately probed.
3. Replace shared-flag handshake inference with per-request response correlation. Await initialize, then new/resume/fork; propagate JSON-RPC error, decoder error and EOF immediately.
4. Pass identical cwd/MCP session-defining parameters to new, resume and fork.
5. Put every post-spawn error through graceful connection close, bounded SIGTERM, process-group SIGKILL fallback, direct-child wait and task cleanup.
6. Keep the existing 30-second knob. Do not raise it to hide runtime package acquisition or swallowed protocol errors.

This is the smallest root-cause fix that does not leave the next outage in place.

### 5.2 Logs — admissible slice

1. Add no storage schema, event variant, telemetry DB or log endpoint.
2. Derive recent durable lifecycle rows from Hall's existing message query with stable message-derived IDs. Do not copy user prompts, reasoning, tool arguments or tool results.
3. Either:
   - keep `session.log` as an explicitly live-only tail merged in memory, or
   - for strict Hall-only behavior, omit `session.log` from the rehydrated tab.
4. Make “clear” explicitly clear the current view only; it must not imply server deletion.
5. Label the view recent/partial. Persisted TTL diagnostics remain ADR 0018 OBS work after PRE-OBS, not part of this bug fix.

This reuses existing product truth and avoids implementing a second observability architecture.

## 6. Required verification before merge

### Offline/unit

- Claude command/provisioned path and version contract.
- Claude newline encode/decode fixtures, including split reads and multiple frames.
- Initialize/new/resume JSON-RPC error propagation.
- EOF/malformed-frame immediate failure.
- Resume preserves MCP parameters.
- Fake adapter process-tree cleanup after timeout and after response error.
- Repeated failed starts leave no descendant and permit immediate retry.
- Product-derived log mapping excludes user text, reasoning, tool arguments/results and has deterministic IDs.
- UI session switching and “clear current view” behavior.

### Exact-adapter integration

Run against the provisioned `0.58.1` binary with runtime network disabled:

1. `initialize` returns package/version/capabilities.
2. `session/new` returns a real session ID.
3. terminate and fully reap the process tree.
4. start a new adapter process and `session/resume` that ID with identical cwd/MCP parameters.
5. exercise an invalid resume/error and verify immediate failure plus complete cleanup.

### Operational

- Install on a clean host with no npm cache.
- Start the systemd envoy with its exact service PATH/environment.
- Confirm runtime startup performs no registry/network access.
- Confirm failed startup stores no raw secret-bearing stderr and leaves no `npx`/Node/Claude descendants.

## 7. Final decision

| Slice | Decision | Reason |
|---|---|---|
| Bare `claude` → exact Claude ACP adapter | **Required, but revise** | Correct adapter, but runtime `npx` is not a deterministic deployment contract. |
| Claude Content-Length → newline JSON | **Approve** | Confirmed against exact `0.58.1`; keep the change Claude-scoped. |
| “Reap failed child” | **Approve only with process-tree semantics** | Direct PID SIGKILL is insufficient and bypasses adapter cleanup. |
| Current handshake/resume behavior | **Reject** | Swallows JSON-RPC errors, can falsely succeed on failed resume, drops MCP config. |
| New permanent Hall `SessionLog` store | **Reject** | Duplicates authority and bypasses ADR 0018 TTL/redaction/prerequisites. |
| Rehydrate recent lifecycle from existing Hall messages | **Approve with honest partial/live-only labels** | Reuses product truth; no new retention architecture. |
| Full TTL diagnostic history | **Defer to ADR 0018 after PRE-OBS** | Already approved and sequenced; do not build a competing shortcut. |

**Overall: NO-GO until B1-B4 are resolved. Do not couple the startup repair to new diagnostic persistence.**
