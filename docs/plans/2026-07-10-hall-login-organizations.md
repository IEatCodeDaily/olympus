# Hall Login, Organizations, and Client Connections Implementation Plan

**Goal:** Replace the Web UI's embedded installation token with simple Hall login and organization selection while preserving bearer-token automation and defining multi-Hall installed-client ownership.

**Architecture:** Hall stores credentials and revocable sessions in a narrow SQLite security store. Axum accepts either the legacy installation bearer token or an HttpOnly Hall session cookie, attaches a principal to protected requests, and validates organization membership. The React bootstrap is gated by `/api/auth/session`; its Hall URL remains the document origin.

**Tech stack:** Rust, Axum, rusqlite, Argon2id, BLAKE3, React, TanStack Query, Vitest/Playwright.

## Delivery slices

1. **Security store (TDD)**
   - Add `crates/control-plane/src/auth_store.rs` and integration tests.
   - Cover first-admin bootstrap, password verification, opaque session creation/revocation/expiry, organization creation, unique slugs, and membership filtering.
2. **Hall API and middleware (TDD)**
   - Add login/logout/session and organization routes.
   - Accept legacy bearer tokens or Hall session cookies.
   - Add authenticated principal and organization membership validation.
   - Keep health and login public; preserve strict Origin checks.
3. **Hall boot wiring**
   - Open `~/.olympus/auth.sqlite`, seed `default`, and conditionally bootstrap the administrator from environment variables.
   - Do not log or persist plaintext passwords.
4. **Hall-scoped Web UI (TDD/E2E)**
   - Add an auth bootstrap gate and login screen.
   - Send cookies with API and WebSocket connections.
   - Replace the fake profile/org chip with the authenticated user and organization selector.
   - Persist selected organization per Hall origin and send it as request context.
5. **Contracts and migration**
   - Update `docs/api-contract.md` and deployment documentation.
   - Retain bearer-token compatibility for CLI/native clients.
   - Do not invent desktop/mobile source files; ADR 0010 is their contract until a client project exists.
6. **Verification**
   - Run focused Rust and UI tests during RED/GREEN cycles.
   - Run `make verify` on the isolated worktree.
   - Perform adversarial review for cookie, CSRF, enumeration, expiry, membership, and cross-Hall boundary failures.

## Deferred explicitly

- SSO/OIDC and password reset.
- Global accounts or cross-Hall federation.
- Organizations spanning Halls.
- Cross-Hall aggregate views.
- Full migration of every existing resource record to an `organization_id`; existing resources remain under seeded `default` during this slice.
- Desktop/mobile implementation before a real client scaffold exists.
