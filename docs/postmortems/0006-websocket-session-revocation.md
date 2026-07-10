# Postmortem 0006 — Revoked browser sessions kept live WebSockets authorized

Date: 2026-07-10
Status: resolved

## Impact

Logging out or revoking a Hall browser session stopped subsequent HTTP requests, but an already-open organization WebSocket continued receiving authorized frames until it disconnected or the user's organization membership was removed.

## Root cause

The WebSocket upgrade resolved the opaque session token once and retained only the resulting user and organization IDs. Its periodic authorization check revalidated organization membership but did not revalidate the login session itself.

## Resolution

`WsAuthorization` now retains the connection's browser session token in process memory. The existing 30-second authorization tick resolves that token again and verifies that it still belongs to the expected user before checking organization membership. Revoked or expired sessions therefore terminate their live WebSocket on the next tick. Legacy installation-token operator sockets remain outside the browser-session model.

A regression test authorizes a cookie WebSocket, revokes its session, and proves the periodic authorization predicate changes from allowed to denied.

## Prevention

Long-lived connections must revalidate every revocable capability used at upgrade time, not only downstream membership or resource scope. Future WebSocket credentials must be included in the periodic authorization predicate and covered by a revocation test.
