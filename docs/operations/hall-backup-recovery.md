# Hall backup and recovery

Olympus runs exactly one Hall per network. Do not run a standby, election, or
second writer. The Hall ID is the public half of `~/.olympus/iroh.key`; its
private 32-byte key remains outside every database and must stay mode `0600`.

## Backup

A usable backup set always contains the database and `iroh.key` from the same
Hall. Store the key as a secret, separately from ordinary database access.

- SQLite: call the Hall `Log::backup_sqlite` operation (SQLite online-backup
  API), then copy `iroh.key`. Never copy a live `olympus.db`, WAL, or SHM file.
- PostgreSQL: use the deployment's normal `pg_dump --format=custom "$OLYMPUS_DATABASE_URL"`
  convention, then back up `iroh.key`. PostgreSQL PITR/base-backup policies are
  external database operations; Hall does not shell out with database secrets.

`auth.sqlite`, vaults, repositories, and workspace files keep their existing
resource-specific backup policies and are not silently folded into the event
store snapshot.

## Restore and reconcile

1. Stop Hall. Restore the SQLite snapshot or PostgreSQL dump.
2. Restore the matching `iroh.key` with mode `0600`. Hall fails startup when an
   existing event database lacks this key rather than generating a new Hall ID.
3. Start one Hall. Envoys whose pinned public key matches reconnect and replay
   buffered observations. Hall reconciles active runtime/session/job locations
   from those Envoys; restored Hall-only authority remains authoritative.

## Limited salvage when the Hall backup is lost

Salvage may import only data a reconnecting Envoy still owns: live runtime
observations, local session/job metadata, and its durable event spool. It cannot
reconstruct users, organizations, grants, enrollment decisions, canonical
vault/config state, application registry, or other Hall-only authority. Create
a new Hall explicitly, locally re-pin Envoys after fingerprint confirmation,
and treat salvaged records as observed evidence—not restored authority.