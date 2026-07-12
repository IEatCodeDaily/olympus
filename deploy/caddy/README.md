# Olympus Caddy edge

Olympus requires Caddy 2.11.1 or newer because that release fixes CVE-2026-27589.
Verify with `caddy version` before enabling the unit. The admin API is loopback-only
and rejects requests whose Origin does not match. Hall is the only process allowed
to write the `olympus` server's route array.

The service runs unprivileged with filesystem protections. Artifact roots are
under `~/.olympus/<org>/artifacts`; Hall rejects traversal components and generated
file-server handlers disable symlink following. Protected routes strip client
credentials and identity headers before Hall forward-auth sets the narrow
`X-Olympus-User`, `X-Olympus-Org`, and `X-Olympus-Session` contract.
