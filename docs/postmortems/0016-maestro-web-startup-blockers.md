# Postmortem 0016: Maestro web startup blockers on Ubuntu

## Summary

The first Maestro web spike could not start its managed Chromium on Ubuntu, then a second run exercised the wrong long-lived Vite process and rendered the login screen instead of the authenticated MSW application. During suite conversion, Maestro's beta CSS selector path also entered an unbounded CDP retry loop, and web recording emitted no frames plus executor-shutdown exceptions.

## Impact

The migration could have shipped a CI command that either failed before every flow or tested stale code from another process. The same stale-server class had previously affected Playwright, so reintroducing server reuse would have repeated a known failure mode.

## Root causes

1. Ubuntu keeps `kernel.apparmor_restrict_unprivileged_userns=1`. Maestro 2.6.1 downloads Chrome for Testing under `~/.cache/selenium/`; that binary had no AppArmor profile granting `userns`, so Chromium's sandbox aborted.
2. Killing an npm wrapper did not kill its Vite child. The old child retained `NODE_ENV=production`, making Vite compile `import.meta.env.DEV=false`; mock identity handlers were therefore bypassed.
3. Maestro 2.6.1 web CSS queries can repeatedly fail JSON decoding instead of honoring the assertion timeout. Its web recorder is not yet reliable enough to be a required evidence path.

## Resolution

- Keep the global user-namespace restriction enabled and install a narrow AppArmor profile for `/home/*/.cache/selenium/chrome/**/chrome`.
- Run Vite in its own process group, trap process-group cleanup, use a checkout-derived strict port, and refuse any occupied port rather than reusing a server.
- Set `NODE_ENV=development`, `VITE_USE_MOCKS=true`, and the mock API base explicitly in the runner.
- Prefer visible text and accessible labels over CSS selectors, wrap the CLI in a hard timeout, and keep deterministic screenshots while web recording remains unstable.

## Verification

A real Maestro 2.6.1 Chromium flow selected a session, opened the model selector, changed thinking level, sent a message, asserted the rendered message, and passed headlessly at 1280x800.
