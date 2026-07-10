# Postmortem 0011: Envoy deployment health gate used a redacted token

## Summary

`make deploy-envoy` loaded the installation token into `TOKEN` but sent the literal value `***` in its authorization header. The node-registration health gate therefore always received an unauthorized response and eventually reported that a healthy Envoy had not registered.

## Impact

The automated Envoy deployment choreography could build and start an Envoy but could not verify it, producing a false rollback/failure signal after 30 seconds.

## Root cause

A secret-redaction edit was applied to executable shell code rather than only to displayed logs. The command already avoided printing the header, so replacing the runtime value provided no security benefit.

## Resolution

The curl request now uses the shell-local `TOKEN` value. The command remains silent and never prints the token.

## Prevention

- Redact command output and logs, never executable credentials.
- Keep deployment health gates exercised against a running Hall.
- Prefer a small script with explicit argument handling when Makefile shell recipes become stateful.
