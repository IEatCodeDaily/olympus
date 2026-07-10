# Postmortem 0014: Hall printed the installation bearer token to the journal

## Summary

Hall logged its long-lived installation bearer token to standard output on every startup. Under systemd, that output was retained in the user journal.

## Impact

Any principal able to read the user journal could recover the legacy operator credential and access unscoped native APIs. Browser users were still protected by Hall-local authentication and organization checks, but the compatibility credential bypasses organization scope by design.

## Root cause

Early local-development startup output printed both the listening URL and token for convenience. The token-file path was later added to structured logs, but the plaintext line was not removed before production deployment.

## Resolution

Hall no longer emits the token. It logs only the owner-protected token-file path. The exposed production token was rotated and Hall/Envoy were restarted together.

## Prevention

- Never log credentials, even at startup or in development-oriented output.
- Log secret locations and identifiers, not values.
- Include journal inspection and credential rotation in production deployment verification.
