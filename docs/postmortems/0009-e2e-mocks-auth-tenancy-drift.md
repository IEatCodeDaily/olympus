# Postmortem 0009: E2E mocks bypassed Hall authentication and tenancy

## Summary

After Hall-local authentication and organization-scoped APIs were integrated, the Playwright application booted into the login screen and every feature test failed. The MSW fixture server still exposed legacy unscoped session and resource URLs and had no identity endpoints.

## Impact

The production UI behavior was correct, but the canonical browser suite could not exercise it. Forty desktop/mobile tests either failed or timed out behind the login gate.

## Root cause

The auth feature branch temporarily made tenant-unsafe surfaces unavailable, while the Vault branch evolved its own scoped mocks. During consolidation, the full UI was retained without migrating the shared E2E mock contract for identity and all organization-owned resources.

## Resolution

The MSW server now supplies a Hall-local user and organization membership, implements login/logout endpoints, and serves session, search, usage, node, workflow, and Vault fixtures through organization-scoped URLs. Feature tests therefore traverse the same URL shape as production.

## Prevention

- Treat mock handlers as API contract implementations, not test-only stubs.
- When an API gains a tenancy prefix, migrate all browser mocks in the same change.
- Keep focused browser smoke tests for both authentication bootstrap and an organization-scoped resource before running the full suite.
