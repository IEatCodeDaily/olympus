# Postmortem 0013: organization index ran before the SQLite column migration

## Summary

The first production start with organization tenancy failed while opening the existing Olympus SQLite database. The base schema executed `CREATE INDEX ... sessions(org_id)` before the incremental migration added `org_id` to pre-tenancy `sessions` tables.

## Impact

Hall stayed offline after the binary upgrade. Envoy remained active as designed, and the database was not modified because schema initialization failed before the migration.

## Root cause

Fresh-database DDL and upgrade DDL were mixed in one schema batch. `CREATE TABLE IF NOT EXISTS` does not add columns to an existing table, but the following index statement assumed the fresh table shape.

## Resolution

The base schema no longer creates the organization index. `Log::open` first detects and adds the missing `org_id` column with the fail-closed legacy value `personal`, then creates the index for both fresh and upgraded databases.

A regression test creates the exact pre-tenancy session-table shape, opens it through `Log`, and verifies that the existing row is readable with `org_id = personal`.

## Prevention

- Run upgrade tests from every production schema version, not only against empty databases.
- Keep indexes on newly introduced columns in the migration that establishes those columns.
- Verify deployment against a copy of production state before flipping service symlinks.
