# Postmortem 0020: event codec migration rejected a valid timestamp

## Summary

Deploying Hall commit `f784b04` caused a restart loop while the one-shot event
codec migration was converting historical postcard payloads to JSON+zstd. Hall
failed closed at event 1642 with `event changed during codec migration`.

## Impact

Hall was unavailable during the failed deployment. Envoy remained alive and
retried its UDS connection as designed. The deployment was rolled back to the
previous Hall binary; no event rows changed because the migration transaction
never committed.

## Root cause

The migration correctly compared each decoded event with its JSON round-trip,
but the workspace used serde_json without its `float_roundtrip` feature. The
historical `started_at` value `1782729530.9432595` serialized to that decimal
and parsed back as `1782729530.9432597`. Exact `Event` equality therefore
rejected the row. The test fixture used `1.0`, which could not exercise this
precision boundary.

## Resolution

- Enable serde_json's `float_roundtrip` feature workspace-wide.
- Use the production timestamp as the migration regression fixture.
- Keep the migration's exact semantic equality check; do not weaken it with a
  timestamp tolerance, because persisted event conversion must preserve every
  representable value.
- Retain the pre-deploy SQLite backup and redeploy only after the focused
  migration test and workspace gates pass.

## Prevention

1. Migration fixtures must include adversarial values from live data, not only
   round-number examples.
2. Storage codec changes require a copy-of-production dry run before restart.
3. Deployment health gates must fail on restart-loop state, not treat an empty
   response body as successful health.