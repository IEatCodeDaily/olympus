# Postmortem 0005: Production identity requests retained a configurable Hall base

## Summary

The central resource API correctly ignored `VITE_API_BASE` in production, but
the independently implemented authentication provider still read that variable
unconditionally. A production build supplied with `VITE_API_BASE` could send
login, current-session, organization-list, and logout requests to a different
Hall.

## Impact

This violated the Web UI origin boundary and could expose credentials to an
operator-controlled configured origin. It also split identity and resource
traffic across different Halls.

## Root cause

Hall URL derivation was duplicated between `api.ts` and `auth.tsx`; only one
copy received the production same-origin restriction.

## Resolution

`auth.tsx` now honors `VITE_API_BASE` only when `import.meta.env.DEV` is true.
Vite's build configuration additionally replaces `VITE_API_BASE` with an empty
string and disables mock mode for every production build, regardless of the
calling environment. A hostile-value production build was inspected to verify
that the configured Hall URL and mock token were absent from the bundle.

## Prevention

All browser networking layers, including pre-authentication traffic, must apply
the same origin-binding rule. Production verification must search for every
read of API-base configuration rather than checking only the main API helper.
