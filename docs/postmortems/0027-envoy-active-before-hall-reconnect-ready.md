# 0027 — Envoy process active before Hall reconnect readiness

**Status:** Open; contained by readiness polling, architectural fix belongs to ADR 0017

## Impact

Immediately after restarting `olympus-envoy@1`, systemd reported the service as active. A message posted at that point selected the stale Hall-side Envoy connection and failed after the 30-second `EnsureRuntime` timeout. Retrying the same Claude session after Fleet showed the fresh `terminus` connection succeeded.

## Root cause

Process liveness and control-plane readiness are different states. `systemctl is-active` proves only that the Envoy process exists; it does not prove that:

- the new Envoy completed Hello/authentication,
- Hall replaced the previous connection epoch,
- runtime requests are fenced to the current connection,
- the node is ready to accept work.

Hall can retain a stale selectable connection during the disconnect/reconnect race.

## Containment

Deployment and tests must poll Hall/Fleet for the newly connected Envoy identity/epoch before submitting work. Service-active is not an acceptable readiness gate.

## Required corrective architecture

ADR 0017 must provide:

1. An authoritative Envoy connection epoch bound to authenticated node identity.
2. Atomic replacement/fencing of the old connection before the node becomes selectable.
3. A distinct `ready` state after Hello, capability inventory, and runtime admission are installed.
4. Request failure or retry on connection-epoch mismatch; never silently wait on a stale connection.
5. Hostile restart tests that submit work before, during, and after reconnect.

No retry loop was added as a local patch because that would hide missing fencing and could duplicate at-least-once host effects.