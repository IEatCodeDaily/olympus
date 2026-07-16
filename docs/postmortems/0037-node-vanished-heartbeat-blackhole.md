# Postmortem 0037 — Fx-ZephyrusM16 vanished from prod Fleet for ~10 hours (heartbeat black hole + connection/registration lifecycle mismatch)

- Date: 2026-07-16
- Status: Open — mitigated by Hall restart; protocol fix pending (kanban olympus-ux)
- Affected: production Hall on Terminus, envoy `Fx-ZephyrusM16` (iroh `335a99d1…`), transiently `fxcompute-01`

## Impact

`Fx-ZephyrusM16` disappeared from the prod Fleet/agent lists from 2026-07-15 ~23:30 to 2026-07-16 ~09:49 local. The operator reinstalled the envoy twice; each reinstall restored the node for ~30 seconds before it vanished again. No allowlist entry, key, or configuration was actually lost — reinstalling was not the real fix and did not durably repair the state.

## Timeline (journalctl, olympus-hall.service)

```text
Jul 15 23:29:59  envoy registered (v2) node=Fx-ZephyrusM16
Jul 15 23:30:25  node disconnected, deregistering node=Fx-ZephyrusM16
[~10 hours]      WARN heartbeat for unknown node node=Fx-ZephyrusM16  (every ~11s, continuously)
Jul 16 08:27:58  envoy enrolled via one-line installer … added=false   (operator reinstall #1)
Jul 16 08:28:00  envoy registered (v2) node=Fx-ZephyrusM16
Jul 16 08:28:30  node disconnected, deregistering node=Fx-ZephyrusM16  (+30s)
[unknown-node heartbeats resume]
Jul 16 09:41:51  envoy enrolled … added=false                          (operator reinstall #2)
Jul 16 09:41:54  envoy registered (v2)
Jul 16 09:42:23  node disconnected, deregistering                      (+29s)
[unknown-node heartbeats resume]
Jul 16 09:49    Hall restarted (controller mitigation) → all 3 nodes re-register;
                subsequent flaps now recover in ~1s (disconnect 09:52:10 → re-register 09:52:11)
```

`hall.toml` allowlist was intact throughout (3 keys, including `335a99d1…`; both enrollments logged `added=false`).

## Root cause analysis

Two protocol defects compound:

### 1. Deregistration is keyed by node ID, not by connection/epoch

When a fresh envoy connection registers `Fx-ZephyrusM16`, an older zombie connection for the same peer can still exist inside Hall. When that zombie finally dies, Hall runs "node disconnected, deregistering node=Fx-ZephyrusM16" — **deregistering the NEW, live registration** because teardown is keyed only by node ID. This exactly matches the reproducible register → deregister-after-~30s pattern following each reinstall, and matches the previously documented stale-connection selection defect (postmortem 0027 family; ADR 0017 Task 1.x "connection epoch fencing" remains unimplemented).

### 2. Heartbeat-to-unknown-node is a black hole

After Hall deregisters the node, the envoy keeps sending heartbeats over its live iroh connection. Hall logs `WARN heartbeat for unknown node` (~5/min for 10 hours) and does nothing else; the envoy never learns it is unregistered and never re-sends Hello. Neither side repairs:

- Hall does not reply "unknown — re-register" and does not close the connection.
- Envoy treats heartbeat-send success as health and never re-Hellos.

The system stays wedged until a full Hall restart forces both sides through fresh Hello.

## Why the operator's reinstall didn't work

Reinstall created a fresh envoy process → fresh Hello → registered. But the zombie connection inside Hall (from the previous process) still existed; its death ~30s later deregistered the node again (defect 1), and the fresh process then heartbeated into the black hole (defect 2).

## Mitigation applied

- SQLite backup: `~/.olympus/backups/olympus-pre-hallrestart-20260716T024740Z.db`
- Hall restart at 09:49 → `terminus`, `fxcompute-01`, `Fx-ZephyrusM16` all registered; unknown-node warnings ceased; later disconnects now re-register within ~1s.

## Required fixes (tracked on kanban board olympus-ux)

1. **Connection-epoch-keyed lifecycle**: registration/deregistration must be keyed by (node_id, connection/epoch). A stale connection's death must never deregister a newer registration. New Hello for a node supersedes and actively closes the older connection.
2. **Unknown-node heartbeat repair**: Hall must respond to a heartbeat from an unregistered node with an explicit re-register signal (or close the connection). Envoy must treat that response — and N consecutive unacknowledged heartbeats — as "re-Hello required".
3. **Durable enrolled-node inventory**: enrolled nodes (allowlist + last-seen metadata) should be a durable projection so Fleet shows *enrolled but offline* nodes instead of the node silently vanishing; connection state is presence, not identity.
4. **Flap investigation**: the underlying periodic disconnects (relay/NAT keepalive) deserve their own measurement, but with 1–2 the system self-heals in seconds instead of wedging for hours.

## Lessons

- A WARN log emitted 5×/minute for 10 hours with no repair path is a protocol hole, not observability.
- Presence (connection) and identity (enrollment) were conflated; deregistration must never destroy identity.
- "Reinstall fixed it" was coincidence-shaped: the durable state (allowlist, keys) was never damaged.
