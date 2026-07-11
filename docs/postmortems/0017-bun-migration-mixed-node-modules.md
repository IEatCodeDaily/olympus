# 0017 — Bun migration reused a mixed `node_modules` tree

## Summary

During the npm-to-Bun migration, the first `bun install` reused the existing npm-created `ui/node_modules` tree and reported only 65 newly installed packages. Subsequent TypeScript and Vitest gates stalled instead of producing results.

## Root cause

The package manager and lockfile changed while the generated dependency directory remained in place. That left the checkout with dependency state assembled by two package managers. Concurrent long-running Rust builds and memory pressure on the four-core host amplified dependency traversal and cleanup latency; timed commands also left a TypeScript child in uninterruptible I/O wait temporarily.

## Resolution

- Removed the generated `ui/node_modules` directory.
- Reinstalled solely from `ui/bun.lock` with `bun install --frozen-lockfile`.
- Removed `ui/package-lock.json`; `bun.lock` is now the only lockfile.
- Changed local, Make, CI, documentation, mock, and ACP-adapter commands from npm/npx to Bun/bunx.

## Prevention

A package-manager migration must remove the old generated dependency tree before its first verification run. CI installs from an empty checkout with `bun install --frozen-lockfile`, which fails closed on lock drift. Local migration instructions must do the same rather than installing Bun over an npm-populated tree.

## Verification status

The clean Bun install completed with 525 packages. Full typecheck/test/build verification was deferred until unrelated concurrent Rust compilation and host memory/I/O pressure cleared; no green result should be claimed from the interrupted runs.
