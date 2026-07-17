# ADR 0016: Vault authority, synchronization, and backup are separate layers

- Status: Accepted
- Date: 2026-07-13
- Relates to: ADR 0004 (Markdown + jj), ADR 0012 (programmable environment), Vault workspace

> **Amended by ADR 0026 (2026-07-17).** The authoritative working Vault now may
> contain project descriptors, Markdown, and native board structured artifacts.
> jj remains the editable synchronization path for Markdown/descriptors only;
> logical cr-sqlite replication is a separate structured adapter and live
> `board.db`/WAL/SHM files are excluded from jj and file-copy adapters. Backup
> bindings must create coherent per-board SQLite snapshots plus a sovereign
> export and record their version vectors beside the Vault jj revision. The
> encryption, content-addressing, binding-deletion, and restore-test rules below
> remain intact.

## Context

The first Vault implementation modeled GitHub as a required `VaultBackend` and nested `jj-git` inside that descriptor. This collapses three different concerns:

1. where the authoritative working vault lives;
2. how editable replicas converge;
3. where recoverable backup copies are retained.

That model makes an external GitHub repository mandatory even when a user wants an Olympus-only vault, and it makes future synchronization transports appear to be storage backends. It also makes a Git remote look like a backup policy despite Git synchronization and disaster-recovery retention having different correctness requirements.

## Decision

### Olympus is the default authority

Every vault has an Olympus-managed authoritative working copy under the organization workspace. Markdown and attachments remain usable when no external service is configured. Creating a vault requires only a name.

A vault with no synchronization bindings and no backup bindings is valid. It is **Olympus-only**: available through its owning Olympus installation, with no external replica. This is the default.

The authoritative copy is still a jj repository because jj provides local history, snapshots, and merge semantics. A repository is an implementation detail of the content authority; it does not imply GitHub or any network remote.

### Synchronization is optional and pluggable

Synchronization bindings exchange editable history between authorities or working replicas. They are not the primary storage descriptor.

Initial adapter:

- `github`: a GitHub repository reached through jj's Git interoperability.

Planned adapter:

- `olympus`: direct Olympus-to-Olympus synchronization over the Olympus transport and identity plane.

The contract is adapter-oriented and provider-neutral. A vault may have zero or more named synchronization bindings. Each binding has its own direction, schedule, status, and conflict state. Adapters must feed incoming changes through the same jj merge boundary; they may not overwrite Markdown trees directly.

Multiple bindings are allowed, but Hall serializes sync operations per vault. A change imported from one binding is committed to the local jj history before another binding exports it. Binding IDs and imported revision identities provide loop suppression. A sync failure never makes the local vault unavailable.

### Backup is a separate, optional policy

Backup bindings produce recoverable, immutable snapshots. They do not create editable replicas and never participate in merge.

Initial target family:

- S3-compatible object storage, including AWS S3, Cloudflare R2, MinIO, and compatible services.

A backup records a manifest, content hashes, encrypted payload objects, source vault revision, creation time, and retention metadata. Backup credentials live in the Olympus secret store and are referenced by credential ID; they are never persisted in vault metadata.

Restores are explicit operations. They restore into a new vault or require an explicit destructive-replace workflow; a background backup job never writes into the live working tree.

### Manifest shape

`.vault/metadata.json` moves toward this conceptual shape:

```json
{
  "schemaVersion": 2,
  "name": "Engineering",
  "authority": { "kind": "olympus" },
  "syncBindings": [],
  "backupBindings": []
}
```

Provider-specific fields live inside tagged binding configurations. Runtime state such as last attempt, remote revision, errors, leases, and schedules belongs in Hall state, not in the portable vault manifest.

The API uses `syncBindings` and `backupBindings`; it does not expose a generic `backend` field. Creation defaults both arrays to empty. Configuration is performed after creation through dedicated binding endpoints so adding or removing a transport does not recreate the vault.

## Invariants

1. Local read/write does not depend on GitHub, object storage, or network availability.
2. Synchronization and backup failures do not block local edits.
3. Only sync adapters invoke merge; backup adapters only snapshot and restore.
4. All sync adapters converge through jj; no adapter directly replaces the working tree.
5. Secrets are referenced, never embedded in portable metadata.
6. Sync is serialized per vault and idempotent per binding/revision.
7. Backups are content-addressed, encrypted before leaving Olympus, and restore-tested.
8. Deleting a binding does not delete the vault or its local history.

## Migration

1. Existing metadata with no `backend` becomes an Olympus-only vault.
2. Existing `backend.kind = "github"` becomes one `github` synchronization binding. The local vault remains authoritative.
3. New vault creation stops requiring a repository.
4. The compatibility reader accepts schema-v1 metadata during migration; all writes emit schema v2. After live migration evidence confirms no v1 metadata remains, delete the compatibility path.
5. UI language changes from **Backend store** to separate **Synchronization** and **Backups** surfaces.

## Consequences

- Users can create fully local/Olympus-only vaults.
- GitHub becomes one optional synchronization engine instead of a hard dependency.
- Object storage can provide real retention and recovery without pretending to be a live merge peer.
- Direct Olympus synchronization can be added behind the same binding contract.
- Hall owns more orchestration state: leases, schedules, status, loop suppression, and restore jobs.
- Multi-target synchronization is more complex than a single Git remote, so it is serialized and observable rather than run concurrently.

## Rejected alternatives

- **GitHub as the required backend.** Couples core availability to an external provider and prevents Olympus-only vaults.
- **Treat object storage as a sync engine.** Object snapshots have no safe multi-writer merge semantics.
- **One generic provider list for sync and backup.** Hides materially different contracts and makes unsafe operations easy to express.
- **Allow adapters to copy directly into the working tree.** Bypasses jj merge and makes data loss transport-dependent.
