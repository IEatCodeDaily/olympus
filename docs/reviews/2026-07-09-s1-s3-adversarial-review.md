# Adversarial Code Review — S1/S2/S3 Hall-Envoy Split

**Date:** 2026-07-09  
**Reviewer:** Hermes Agent (adversarial subagent)  
**Scope:** all commits since `652bf94` (S1 proto, S2 envoy lib, S3 partial)  
**Commits reviewed:**
- `652bf94` feat(proto): extract olympus-proto shared wire-type crate (S1)
- `e5fa237` feat(envoy): extract envoy library crate (S2)
- `9892fae` feat(envoy): olympus-envoy binary + olympus-hall bin name + envoy_conn module (S3 partial)

**Design references:** ADR 0008 (`docs/adrs/0008-hall-envoy-split-rolling-deploy.md`),  
plan (`docs/plans/2026-07-09-hall-envoy-split.md`), postmortem 0001.

---

## Summary

The code compiles (`cargo check --workspace` passes, proto tests: 14/14 green). The
crate graph is acyclic (`proto` is a true leaf, `envoy → proto`, `control-plane → proto + envoy`). The
serde tag strategy is consistent (`tag = "kind"`, `rename_all = "snake_case"` on both `HallFrame` and
`EnvoyFrame`, with explicit camelCase renames on field names). The ADR §1 frame families are complete.

However, there are **3 BLOCKERs**, **5 RISKs**, and several NITs. The most critical: the
postmortem 0001 error-rendering fix (`{e:#}`) was **never applied** — it's still plain `{e}` at
every user-facing error site. The `envoy_conn` module is entirely dead code (S3 is partial). And
the envoy binary's event drain loop will **hang forever** on real ACP runtimes because it never
breaks on `AgentEvent::Done`.

---

## BLOCKERs

### B1 — Postmortem 0001 error fix was never applied: all user-facing errors still use `{e}` not `{e:#}`

**File:** `crates/control-plane/src/server/mod.rs:2493` (and 14+ other sites)  
**Classification:** BLOCKER  
**Postmortem ref:** `docs/postmortems/0001-acp-deps-missing.md` §"Fix" item 1

Postmortem 0001 documents that `format!("⚠ Failed to start agent: {e}")` was the root cause —
anyhow's `Display` (`{e}`) prints only the outermost context, hiding the cause chain. The documented
fix is to use `{e:#}` (alternate Display). **There are zero occurrences of `{e:#}` in the entire
codebase.** The postmortem claims the fix was applied and verified end-to-end, but the code
disagrees:

```
$ grep -c '{e:#}' crates/control-plane/src/server/mod.rs
0
$ grep -c '{e}' crates/control-plane/src/server/mod.rs
14
```

The specific line the postmortem calls out:
- **Line 2493:** `format!("⚠ Failed to start agent: {e}")` — still `{e}`, should be `{e:#}`

And the same pattern at every other user-facing error surface:
- Line 2525: `format!("error: failed to start agent: {e}")`
- Line 2583: `format!("Prompt send failed: {e}")`
- Line 2587: `format!("error: {e}")`
- Line 2818: `format!("⚠ agent error: {e}")`
- Line 2819: `format!("error: {e}")`
- Line 428: `format!("{e}")` (tail_events error response)
- Lines 537, 617, 1494, 1988, 2044, 2123, 2403

**Impact:** The exact user-blocking bug the postmortem was written about — useless error messages
that hide the real cause — is still live. The plan's standing rule explicitly states:
> "anyhow errors rendered `{e:#}` (postmortem 0001)"

This is violated everywhere in the existing monolith code. (The new envoy binary at `main.rs:250`
etc. correctly uses `{e:#}`, making the inconsistency more egregious.)

**Fix:** `grep -rn 'format!("{e}")' crates/` and replace every user-facing occurrence with
`{e:#}`. At minimum, line 2493 must be `{e:#}`.

---

### B2 — Envoy binary event drain loop never terminates: hangs on real ACP runtimes

**File:** `crates/envoy/src/main.rs:354-368`  
**Classification:** BLOCKER

```rust
let mut events = runtime.events();       // broadcast stream
while let Some(event) = events.next().await {   // <-- never breaks
    // ... forward as EnvoyFrame::Event ...
}
```

`runtime.events()` returns a `BroadcastStream<AgentEvent>` — a stream backed by a
`tokio::sync::broadcast::Sender<AgentEvent>` that lives for the lifetime of the runtime (it's
created once in `HermesAgentRuntime::new_arc` / `MockAgentRuntime::new_arc` and never closed).

The broadcast channel is **never closed** after a turn completes. The stream will yield
`AgentEvent::Done { .. }` and then **block forever** waiting for the next event from the next turn.
The `while let Some(...)` will never produce `None` because the sender is alive.

Compare with the monolith's `post_message` drain loop (`server/mod.rs:2599`): it explicitly breaks
on `AgentEvent::Done` (and `AgentEvent::Error`):
```rust
AgentEvent::Done { finish_reason } => {
    // ... persist, broadcast ...
    break;  // <-- terminates the drain
}
```

The envoy binary has no such `break`. This means:
- With the mock runtime, the drain happens to work because the mock sends Text + Done in a single
  spawned task, and then the stream blocks (the spawned dispatch task never returns, but the next
  prompt arrives and the same runtime is reused, so the blocked stream sees the next turn's events
  too — interleaving events from different turns).
- With a real ACP runtime, the first prompt's drain will block forever after `Done`, the
  `send_resp` for that prompt's `reqId` will never fire, and Hall will time out waiting for the
  response. Subsequent prompts for the same session will be queued behind the blocked drain.

**Impact:** The envoy binary is non-functional for real agent sessions. Every prompt hangs after
the turn completes. The envoy can never send a `resp` back for any prompt/steer.

**Fix:** Break on terminal events:
```rust
while let Some(event) = events.next().await {
    // ... forward as EnvoyFrame::Event ...
    let is_terminal = matches!(
        &event,
        AgentEvent::Done { .. } | AgentEvent::Error(_)
    );
    // ... send frame ...
    if is_terminal {
        break;
    }
}
```

---

### B3 — `send_and_stream` races: subscribes to events AFTER sending the command

**File:** `crates/envoy/src/main.rs:348-354`  
**Classification:** BLOCKER

```rust
runtime.send(cmd).await.context("sending command to runtime")?;   // line 348-351

let mut events = runtime.events();   // line 354 — subscribe AFTER send
while let Some(event) = events.next().await {
```

The subscription happens **after** `send()`. With a fast runtime (or the mock, which spawns events
asynchronously), the agent can emit and complete the entire turn between `send()` returning and
`events()` subscribing. The broadcast channel buffers messages, but a subscriber that hasn't been
created yet will **never see messages already sent** — broadcast only delivers to existing
subscribers.

The monolith explicitly documents this and avoids it (`server/mod.rs:2572-2574`):
```rust
// Subscribe before sending the prompt so fast runtimes cannot emit and
// finish the whole turn before the drain loop is listening.
```

But it subscribes on line 2552 (`let mut stream = runtime.events();`) before `send()` on line 2575.

The envoy binary does it in the wrong order.

**Impact:** Events from the beginning of a turn (or the entire turn if fast) are silently dropped.
The drain loop may hang (no events to drain, channel stays open) or deliver a truncated reply.

**Fix:** Move `let mut events = runtime.events();` to before `runtime.send(cmd).await`:
```rust
let mut events = runtime.events();   // subscribe FIRST
runtime.send(cmd).await.context("sending command to runtime")?;
```

---

## RISKs

### R1 — EnvoyConn module is entirely dead code; S3 is more partial than the plan claims

**File:** `crates/control-plane/src/server/envoy_conn.rs` (entire file, 209 lines)  
**Classification:** RISK

`EnvoyConnection::new` is never called anywhere in the codebase (confirmed by grep + cargo dead-code
warning). `EnvoyConnections` is never instantiated. `EventSink` is defined but never used. The
module is `pub mod envoy_conn;` in `mod.rs` but nothing references it.

The plan's Status Ledger says:
> S3 binaries+RPC | PARTIAL | ... envoy bin + hall bin name + envoy_conn module; RemoteRuntime +
> UDS session dispatch + integration test TODO

This is accurate — the Hall side of the UDS session-RPC (the read loop dispatch, `RemoteRuntime`
factory, integration test) is not implemented. But the `envoy_conn.rs` code as committed cannot be
exercised or tested — it's dead code that will likely bit-rot before S3 completion.

**Impact:** No functional impact yet (dead code), but the `EnvoyConnection` design has not been
validated against the actual node.rs read loop. When S3 is completed, the wiring may reveal design
flaws that could have been caught now.

**Fix:** Either add `#[allow(dead_code)]` with a TODO linking to the S3 completion ticket, or
gate the module behind `#[cfg(test)]` until it's wired. At minimum, document that this is a stub.

---

### R2 — Heartbeat task leaks on disconnect (never aborted)

**File:** `crates/envoy/src/main.rs:176-191`  
**Classification:** RISK

```rust
tokio::spawn(async move {
    loop {
        tokio::time::sleep(HEARTBEAT_INTERVAL).await;
        let hb = EnvoyFrame::Heartbeat { ... };
        if hb_conn.send_frame(&hb).await.is_err() {
            break;
        }
    }
});
```

The heartbeat task is spawned and then forgotten — its `JoinHandle` is dropped (not stored). When
the read loop exits (line 198-228, Hall disconnect), `run_connection` returns `Ok(())` and the
function exits. The heartbeat task is still running.

The heartbeat task **will** eventually break on its own: when Hall disconnects, `send_frame` will
error (the UDS write half is broken), and the `is_err()` check will break the loop. But there's a
race: between the read loop detecting EOF and the heartbeat task's next `send_frame` attempt (up to
`HEARTBEAT_INTERVAL` = 10 seconds later), the heartbeat task holds an `Arc<Conn>` clone, keeping the
`Conn` struct (and its writer, table, seq/turn maps) alive.

More importantly, `run_connection` returns, `main()` returns, and the process exits — so the leak
is bounded by process lifetime. But if the binary is extended to reconnect (which the ADR §2
reconnect semantics require), this pattern will leak a heartbeat task per reconnect cycle.

**Impact:** Today: benign (process exits). With reconnect (S4): accumulates dead heartbeat tasks +
`Arc<Conn>` clones per reconnect.

**Fix:** Store the heartbeat `JoinHandle` and abort it when the read loop exits, or use a
`CancellationToken` shared between the read loop and the heartbeat task.

---

### R3 — `stderr` still uses `Stdio::inherit()` despite postmortem 0001 claiming it was fixed

**File:** `crates/envoy/src/bridge/hermes.rs:534, 632`  
**Classification:** RISK

Postmortem 0001 §"Fix" item 3 states:
> `bridge/hermes.rs` — capture child stderr + detect early exit. Both `start()` and `fork_session()`
> now pipe stderr into a bounded 8 KiB buffer (`spawn_stderr_capture`) instead of `Stdio::inherit()`

The current code:
```rust
cmd.stderr(Stdio::inherit()); // logging goes to our stderr   (line 534, in start())
cmd.stderr(Stdio::inherit());                                 (line 632, in fork_session())
```

There is no `spawn_stderr_capture`, no `tail_or_empty`, no `try_wait` early-exit detection. The
postmortem describes fixes that are not in the code. (They may have been lost during the S2 move
from `control-plane/src/bridge/` to `envoy/src/bridge/`, or they were never committed.)

**Impact:** When the agent child dies early (e.g., binary not found, crash during init), the error
runs out the full 30-second `start_timeout_secs` timeout instead of failing fast with the stderr
tail. The postmortem's "detect early exit" improvement is missing.

**Fix:** Implement stderr capture + `try_wait` as described in postmortem 0001, or update the
postmortem to reflect the actual state.

---

### R4 — `send_request` in envoy_conn holds pending lock during writer lock acquisition — potential deadlock under load

**File:** `crates/control-plane/src/server/envoy_conn.rs:73-84`  
**Classification:** RISK

```rust
self.pending.lock().await.insert(id, tx);       // line 78 — pending lock acquired
let json = serde_json::to_string(&frame_with_id)...;
let mut w = self.writer.lock().await;           // line 81 — writer lock acquired while pending held
w.write_all(json.as_bytes()).await?;
```

The `pending` mutex is held while awaiting the `writer` mutex. If another task holds the `writer`
lock (e.g., sending a different frame) and that task or a third task tries to acquire `pending`
(though `resolve` does acquire `pending` at line 104), there's a lock-ordering hazard.

Current code paths: `send_request` acquires `pending → writer`. `resolve` acquires only `pending`.
`fail_all` acquires only `pending`. So the current lock ordering is consistent (no reverse ordering
exists), and tokio `Mutex` is not reentrant but doesn't deadlock on different locks. This is
**technically safe** today.

However, if a future code path acquires `writer → pending` (e.g., a write-completion callback that
resolves pending requests), this will deadlock. The pattern of holding `pending` across an `await`
on `writer` is fragile.

**Impact:** No deadlock today, but the pattern is a trap for future development. The `pending`
lock is held across an `await` on `write_all` (line 82), which is a network I/O — this blocks all
pending-request resolution for the duration of the write.

**Fix:** Insert into `pending` after successfully writing:
```rust
let mut w = self.writer.lock().await;
w.write_all(json.as_bytes()).await?;
w.write_all(b"\n").await?;
w.flush().await?;
drop(w);
// Now register the pending slot — small window where resp arrives before slot exists,
// but resolve() handles missing slot gracefully (no-op).
self.pending.lock().await.insert(id, tx);
```
Or use a single combined lock. At minimum, document the lock ordering requirement.

---

### R5 — `BuildVersion::current()` uses `env!` which embeds the proto crate's version, not the binary's

**File:** `crates/proto/src/version.rs:38-44`  
**Classification:** RISK

```rust
pub fn current() -> Self {
    Self {
        semver: env!("CARGO_PKG_VERSION").to_string(),     // always "0.1.0" (proto crate version)
        git_hash: env!("OLYMPUS_GIT_HASH").to_string(),
        built_at: env!("OLYMPUS_BUILT_AT").to_string(),
    }
}
```

`env!("CARGO_PKG_VERSION")` is evaluated in the crate where `current()` is defined (`olympus-proto`),
so `semver` is always `"0.1.0"` (the proto crate's version from `Cargo.toml`), regardless of which
binary calls it. ADR §1 says `version` is the envoy's **build identity** — the version of the envoy
binary, not the proto crate.

The git hash and build timestamp come from `build.rs` and are workspace-wide (correct), but the
semver is misleading. If `olympus-envoy` is at version `0.2.0` and `olympus-proto` is at `0.1.0`,
the hello frame will report `semver: "0.1.0"`.

**Impact:** The Nodes UI and drain decisions that key on `version.semver` will see the proto
crate version, not the envoy binary version. The git hash is the real discriminator, so this is
not blocking, but the semver field is semantically wrong.

**Fix:** Either remove `semver` from `BuildVersion` (git hash is sufficient per ADR), pass the
binary's version as a parameter, or use `option_env!` in the binary and pass it in.

---

## NITs

### N1 — `_turn_id` assigned but never used in `send_and_stream`

**File:** `crates/envoy/src/main.rs:346`  
**Classification:** NIT

```rust
let _turn_id = conn.next_turn(session_id).await;   // assigned, prefixed with _ to suppress warning
```

The turn id is assigned but the actual turn id used in event frames comes from
`next_turn_id_for_event` (line 356), which re-reads the turn counter. The `next_turn` call at line
346 increments the counter as a side effect, and `next_turn_id_for_event` reads the incremented
value. This works but is confusing — the `_turn_id` binding exists only for the increment side
effect, and naming it `_turn_id` (suppress-unused) obscures the intent.

**Fix:** Use `let _ = conn.next_turn(session_id).await;` or add a comment explaining the side
effect.

---

### N2 — `next_turn` increments before formatting, but `next_turn_id_for_event` reads without incrementing — naming mismatch

**File:** `crates/envoy/src/main.rs:111-116, 379-384`  
**Classification:** NIT

`next_turn` (line 111) increments the counter then returns `format!("turn-{}", v)` with the
incremented value. But `next_turn_id_for_event` (line 379) reads the current value without
incrementing. This means the first turn gets id `turn-1` (from `next_turn`), and
`next_turn_id_for_event` returns `turn-1` for all events in that turn. The second turn gets
`turn-2`, etc. This is correct but the two methods' relationship is non-obvious.

**Fix:** Consider a single method that returns the current turn id, and a separate
`bump_turn()` that increments.

---

### N3 — Postmortem 0001 references stale file paths (`crates/control-plane/src/bridge/hermes.rs`)

**File:** `docs/postmortems/0001-acp-deps-missing.md:6, 149`  
**Classification:** NIT

The postmortem references `crates/control-plane/src/bridge/hermes.rs` which has been moved to
`crates/envoy/src/bridge/hermes.rs` by the S2 extraction. The "Affected code" and "Related" sections
should be updated.

---

### N4 — `Probe` handler re-runs `discover_local_agents()` on every probe — potentially slow

**File:** `crates/envoy/src/main.rs:322`  
**Classification:** NIT

```rust
HallFrame::Probe { req_id } => {
    let agents = discovery::discover_local_agents();   // fresh discovery each time
```

This is intentional per the ADR ("no process-lifetime cache, so a manual refresh picks up newly-
installed CLIs") and matches the `discover_cli_harnesses_now` doc comment. But
`discover_local_agents()` calls `command_version_with_timeout` with a 2-second timeout per CLI, so
a probe on a host with both `claude` and `codex` installed takes up to 4 seconds. Hall's health-gate
per the ADR §5 choreography polls probe — this could slow the gate.

**Fix:** Consider a short TTL cache (e.g., 30s) for probe results, or accept the latency.

---

### N5 — `format!` warning on unreachable pattern in `dispatch_frame`

**File:** `crates/envoy/src/main.rs:232`  
**Classification:** NIT

The `dispatch_frame` function handles all 10 `HallFrame` variants explicitly. This is correct and
exhaustive (no `_ =>` catch-all), which is good — it means a future variant addition will be a
compile error. No action needed; this is praise, not a nit.

---

### N6 — `MockAgentRuntime` does not override `resumable()` — returns `false` by default

**File:** `crates/envoy/src/mock_runtime.rs` (no `resumable` override)  
**Classification:** NIT

The mock runtime inherits the default `resumable() -> false` from the `AgentRuntime` trait
(`bridge/mod.rs:60`). This is correct for a mock (it has no real ACP capabilities), but the S3
integration test (when written) should verify that `resumable` is actually `false` for the mock and
`true` for a real Hermes runtime to confirm the capability parsing works.

---

## Key Questions Answered

### Q1: Is `resumable()` actually wired or always false?

**Wired correctly.** The capability parsing chain is complete:
1. `HermesAgentRuntime::start()` sends `initialize` and arms the `init_id` gate (line 580).
2. The stdout reader's `handle_incoming_message` matches the `initialize` response by JSON-RPC id
   (line 242-249), calls `parse_resumable_capability(&resp.result)` (line 245), and stores the
   result in `self.resumable` (line 246).
3. `parse_resumable_capability` (line 200-213) checks both `agentCapabilities.loadSession == true`
   AND `sessionCapabilities.resume` presence — capability-driven, not harness-name-driven, exactly
   per ADR §3 and the plan.
4. `HermesAgentRuntime::resumable()` (line 790) reads the stored flag.
5. `RuntimeTable::ensure_runtime` captures `resumable` from the runtime and stores it in
   `RuntimeEntry` (line 92).
6. `RuntimeTable::resumable()` (line 140) exposes it per-session.

The chain is correct. The default trait method returns `false` (fail closed), and `MockAgentRuntime`
inherits that. `HermesAgentRuntime` overrides it with the real capability. **This is not a bug.**

### Q2: Any deadlocks in `envoy_conn`?

**No deadlock today**, but R4 (lock-ordering fragility) applies. The `send_request` method acquires
`pending → writer` locks sequentially (pending held across writer await). No reverse ordering
exists in the current code (`resolve` and `fail_all` only touch `pending`). See R4 for details.

### Q3: Does the envoy binary's event drain terminate correctly?

**No — this is B2 (BLOCKER).** The drain loop (`main.rs:355`) iterates a broadcast stream that is
never closed and never breaks on terminal events. It hangs forever after the first turn's `Done`.

### Q4: Are there missing `HallFrame` variants in the dispatch?

**No — all 10 variants are handled.** The `dispatch_frame` match (line 232-333) covers
`EnsureRuntime`, `Prompt`, `Steer`, `Cancel`, `Stop`, `RespondPermission`, `Drain`, `Probe`, `Ack`,
`ResumeFrom`. This is exhaustive with no `_ =>` catch-all, so adding a variant will be a compile
error — good practice.

### Q5: Does the crate graph have cycles?

**No cycles.** The dependency graph is:
```
proto (leaf: serde, serde_json only)
  ↑
envoy → proto
  ↑
control-plane → proto + envoy
```
`proto` is a true leaf with no workspace deps. `envoy` depends only on `proto`. `control-plane`
depends on both. No circular dependencies.

---

## Verdict

The S1 (proto) and S2 (envoy lib extraction) milestones are well-executed: clean separation,
correct serde strategy, good test coverage, complete frame types. The ADR §1 wire protocol is fully
realized in types.

S3 is correctly labeled "PARTIAL" — the envoy binary exists and compiles, but has two critical bugs
(B2: drain never terminates, B3: subscribe-after-send race) that make it non-functional for real
sessions. The Hall-side wiring (`envoy_conn`) is dead code.

The most concerning finding is B1: the postmortem 0001 error-rendering fix — the impetus for this
entire work cycle's error-chain discipline — was never applied. The plan's standing rule ("anyhow
errors rendered `{e:#}`") is violated at every error site in the existing monolith. This should be
fixed immediately as it directly reproduces the user-blocking bug documented in the postmortem.

**Recommendation:** Fix B1, B2, B3 before any further S3/S4 work. Address R3 (stderr capture) to
match the postmortem's claims or update the postmortem.
