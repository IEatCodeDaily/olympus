# Postmortem 0015: release binaries retained a stale embedded Git hash

## Summary

Hash-suffixed deployment artifacts were installed from the final commit, but Envoy reported an older Git hash in Fleet. The protocol crate's build script watched `.git/HEAD`; on a normal branch that file contains a stable symbolic-ref string and does not change when new commits advance the branch.

## Impact

The running executable and symlink were current, but its advertised build identity was stale. This undermined version-based drain/evict decisions and made deployment verification ambiguous.

## Root cause

Cargo reran the build script only when `.git/HEAD` changed. It did not watch the loose branch ref (`.git/refs/heads/main`) or `packed-refs`, where the actual commit ID changes.

## Resolution

The build script now resolves symbolic `HEAD` through Git, watches the effective branch-ref path, and also watches `packed-refs`. Detached-HEAD builds remain covered by the existing `.git/HEAD` watch.

## Prevention

- Verify both the installed filename and the version advertised over the wire.
- For Git-derived build metadata, watch the symbolic ref target rather than only `.git/HEAD`.
- Keep `packed-refs` in the build-script dependency set for repositories that compact refs.
