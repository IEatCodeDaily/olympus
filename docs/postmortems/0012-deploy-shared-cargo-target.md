# Postmortem 0012: deployment assumed a repository-local Cargo target directory

## Summary

The production release build completed successfully, but `scripts/deploy.sh` failed while installing the binary because it copied from `target/release`. This host configures Cargo to use a shared target directory under `~/.cache`.

## Impact

No service was restarted and the old binaries remained active, but the deploy consumed a full release build before failing at the installation step.

## Root cause

The script treated Cargo's default target path as a repository invariant. Cargo permits that path to be changed by environment and user configuration.

## Resolution

The script now reads Cargo's effective `target_directory` from `cargo metadata` and installs Hall and Envoy from that absolute path.

## Prevention

- Discover build artifact locations through the build tool rather than reconstructing them.
- Keep hash-suffixed installation atomic: do not flip symlinks until the source artifact exists.
- Exercise deployment scripts on hosts with shared build caches.
