# 0044 — Legacy allowlist migration duplicated a node under its raw iroh key

Date: 2026-07-17 · Severity: high (fleet identity split) · Author: Terminus

## Symptom

After durable node inventory landed, first boot correctly exposed the existing
Talos allowlist identity as offline. Re-registering the known mapping
`talos -> f79c…` then produced two offline rows: `talos` and the raw 64-character
iroh public key.

## Root cause

The first durable-inventory boot must seed legacy allowlist entries without a
name because the old Hall persisted only public keys. `NodeRegistry::enroll`
then inserted the supplied node ID without removing the placeholder carrying the
same authenticated iroh identity. The later live-registration path already
renamed by iroh identity, but the enrollment path did not share that invariant.

## Fix

Enrollment now treats the iroh public key as the stable identity: it removes any
other rows with that key, migrates the placeholder state when possible, updates
the human node ID/hostname, and persists one row. A restart regression verifies
that only `talos` remains offline across reload.

## Prevention

- Every inventory mutation must enforce one row per authenticated iroh identity.
- Migration tests must cover the actual legacy state (allowlist key with no
  node-name mapping), not only an empty inventory.
- Fleet acceptance checks assert uniqueness by both `nodeId` and `irohNodeId`.
