# 0028 — Cloudflare Access intercepted Envoy enrollment

**Status:** Resolved in production; installer redirect hardening added and awaiting the normal Hall deployment gate

## Executive summary

The Fleet-generated one-line installer targeted token-gated Hall routes under `olympus.entelechia.cloud`, but the host-wide Cloudflare Access application intercepted those unauthenticated requests and returned its HTML login flow. Because the command followed redirects and piped the resulting HTML into Bash, enrollment failed at `<!DOCTYPE html>` instead of reporting an edge-authentication error.

Production was restored by adding a more-specific Cloudflare Access application for `olympus.entelechia.cloud/api/enroll/*` with a Bypass policy. The operator-only token mint endpoint at `/api/enroll` remains covered by the existing Access application and Hall authorization.

## Impact

A remote operator could mint an enrollment token in Fleet but could not use the generated command on a new host. Script fetch, binary fetch, registration, and status polling all required unauthenticated access to token-bearing URLs, so the complete remote enrollment flow was blocked.

No node was enrolled by the failed command and no Hall API credential was exposed. The enrollment token was already present in the operator-visible command by design and remained constrained to the enrollment capability and its 15-minute lifetime.

## Timeline

- **2026-07-14 00:45:58 UTC:** Reproduced the public script request returning HTTP 302 to `nevrlabs.cloudflareaccess.com` with `text/html`.
- **2026-07-14 00:46:53 UTC:** Confirmed the same token returned a syntactically valid shell script directly from Hall on loopback, isolating the failure to the Cloudflare Access boundary.
- **2026-07-14 00:50:12 UTC:** Created the path-specific Cloudflare Access application `Olympus enrollment bootstrap`.
- **2026-07-14 00:50:14 UTC:** Created its `Bearer enrollment capability` Bypass policy.
- **2026-07-14 00:50:32 UTC:** Verified the public script, binary, and status routes reached Hall while `POST /api/enroll` still redirected to Access authentication.

## Root cause

Olympus deliberately models the URL enrollment token as a short-lived bearer capability. Hall therefore exposes these routes without a Hall login:

- `GET /api/enroll/{token}/install.sh`
- `GET /api/enroll/{token}/binary`
- `POST /api/enroll/{token}`
- `GET /api/enroll/{token}/status`

The external edge did not implement the same boundary. Cloudflare Access protected the entire `olympus.entelechia.cloud` host, including the token-bearing enrollment routes. The generated command used `curl -L`, so Access's redirect/login HTML crossed the shell-script boundary and produced a misleading Bash parser error.

The route-level Hall tests covered authenticated minting and unauthenticated token use inside Axum, but there was no production edge contract test proving that the same public paths bypassed interactive Access authentication.

## Resolution

Cloudflare Zero Trust now has a more-specific self-hosted application:

- **Application:** `Olympus enrollment bootstrap`
- **Path:** `olympus.entelechia.cloud/api/enroll/*`
- **Policy:** `Bearer enrollment capability`
- **Decision:** Bypass
- **Selector:** Everyone

The broad `olympus` Access application still protects the host. Path specificity makes only URLs below `/api/enroll/` bypass interactive Access; `/api/enroll` itself does not match and remains protected.

The generated installer command also gains `--max-redirs 0`. If an edge regression reintroduces a login or error redirect, curl now fails before any response body can be piped into Bash.

## Verification

Production checks proved:

1. A valid token's `install.sh` returned HTTP 200 and `content-type: text/x-shellscript`; `bash -n` passed.
2. The token-gated binary returned HTTP 200 and `application/octet-stream`.
3. The token-gated status route returned Hall JSON.
4. Random invalid token paths reached Hall and returned HTTP 403 rather than an Access login redirect.
5. `POST /api/enroll` still returned the Cloudflare Access authentication redirect without an authenticated operator session.
6. `curl -L --max-redirs 0` returns curl error 47 on a protected redirect and emits no login body to Bash.

## Follow-up

1. Keep the Cloudflare path-specific application and Bypass policy as part of the Olympus production edge contract.
2. Deploy the Hall redirect-hardening change after its focused Rust test and deployment gates pass.
3. Add a recurring external probe that asserts script content type, invalid-token Hall rejection, and continued protection of the mint endpoint.
4. Codify the Cloudflare Access application/policy in the future Olympus edge infrastructure source of truth rather than relying only on dashboard/API state.
