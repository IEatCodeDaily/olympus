# ADR 0010: Hall authentication and client connection ownership

- Status: Accepted
- Date: 2026-07-10
- Relates to: ADR 0005 (organization ownership), ADR 0008 (Hall/Envoy split)

## Context

Olympus currently protects every API request with one installation-wide bearer token compiled into the Web UI. It has a hard-coded `default` organization and no user, membership, login-session, or client-connection model. This is sufficient for a loopback prototype but cannot safely support a remotely served Hall or multiple organizations.

The Web UI and installed clients have different connection boundaries. A Web UI is served by one Hall and must not become an arbitrary Hall client. A desktop or mobile installation must be able to retain independent authenticated connections to multiple Halls.

## Doctrine

**A Hall owns its users, organizations, memberships, nodes, and resources; installed clients own a list of Hall connections, while a Web UI is permanently scoped to the Hall that served it.**

## Decision

### Hall-owned identity and authorization

Each Hall owns:

- local user accounts and password hashes;
- organizations and immutable organization slugs;
- user membership and organization roles;
- revocable login sessions;
- organization-scoped Envoy registrations and resources.

There is no global Olympus account, cross-Hall session, or organization spanning multiple Halls in v1.

The credential store is `~/.olympus/auth.sqlite`. It is operational/security truth, separate from the business event log: password hashes and revocable login-session records are not replayable domain events and must not be copied into projections or search. Organization resource records remain Hall business truth and follow ADR 0005.

The existing installation bearer token remains supported for native/operator automation and migration. Browser login uses an opaque random session token in an HttpOnly cookie; only a BLAKE3 hash is persisted. Passwords use Argon2id.

Every organization-scoped request identifies its organization explicitly. Hall derives authorization from the authenticated principal's membership; a client-selected organization is context, never authority.

### Sources of truth and ERD

`auth.sqlite` is authoritative for security identity and authorization relationships. The append-only Hall event log is authoritative for resource ownership; in this delivery that means `SessionCreated` followed in the same batch by `SessionOrganizationAssigned`. The session projection joins those events into `SessionRow.org_id`. Browser selection is disposable client state and is never a source of authorization truth.

```mermaid
erDiagram
    USER ||--o{ ORGANIZATION_MEMBERSHIP : has
    ORGANIZATION ||--o{ ORGANIZATION_MEMBERSHIP : grants
    USER ||--o{ LOGIN_SESSION : owns
    ORGANIZATION ||--o{ SESSION : owns

    USER {
        text id PK
        text username UK
        text password_hash
    }
    ORGANIZATION {
        text id PK
        text slug UK
        text display_name
    }
    ORGANIZATION_MEMBERSHIP {
        text organization_id FK
        text user_id FK
        text role
    }
    LOGIN_SESSION {
        text token_hash PK
        text user_id FK
        integer expires_at
        integer revoked_at
    }
    SESSION {
        text session_id PK
        text organization_id FK
    }
```

Organizations and memberships intentionally remain in the transactional security store for v1. They are durable Hall business authorization truth, but not replayed into the general search/read projections. Any later move into domain events requires a migration ADR and must preserve the security store as a fail-closed authorization index during transition.

### Web UI

- Hall identity and URL come from the document origin.
- The Web UI has no add-Hall or edit-Hall-URL flow.
- It authenticates with a Hall-issued HttpOnly cookie.
- It lists only the organizations available to the authenticated user.
- The selected organization is URL/local UI context and is sent on API requests; Hall validates membership.
- Logout revokes only the current Hall session.

### Desktop and mobile

An installed client owns local `HallConnection` records containing Hall URL, pinned Hall identity, account display information, selected organization, and a reference to an encrypted refresh credential. Secrets live in the operating-system credential store, not ordinary app configuration.

Installed clients may retain several Hall connections and switch among them. Authentication, organization selection, and logout are independent per Hall. An installed client is not an Envoy merely because it connects to a Hall.

No desktop/mobile application exists in this repository today. This ADR defines its boundary; implementation begins when that client scaffold exists.

## Security rules

- Fail closed on invalid or expired sessions and non-member organization selection.
- Usernames and organization slugs are unique within one Hall, not globally.
- Login errors do not distinguish unknown users from wrong passwords.
- Session cookies are HttpOnly and SameSite=Strict; Secure is enabled for HTTPS deployments.
- Browser requests require the exact serving origin (scheme, host, and effective port), or an explicitly configured exact development/reverse-proxy origin. Cookie requests without Origin require same-origin Fetch Metadata; valid native bearer credentials may omit Origin.
- Envoy Iroh identity remains distinct from user and client-installation identity.

## Initial provisioning

A Hall may bootstrap one administrator from `OLYMPUS_ADMIN_USERNAME` and `OLYMPUS_ADMIN_PASSWORD` when its user table is empty. The password is consumed at startup and never persisted in plaintext. Without bootstrap credentials, installation-token access remains available but password login fails closed until an administrator is provisioned.

## Consequences

- Remote Web UI access no longer requires embedding the installation token in JavaScript.
- Installed clients can later support several Halls without weakening the Web UI origin boundary.
- New browser-created sessions are atomically assigned to the selected organization in the event log; scoped reads, mutations, forks, subsessions, handovers, and WebSocket frames enforce that ownership. Session workspaces are rooted under the durable organization ID. Legacy imported sessions remain `personal` until an explicit migration assigns them.
- Vault files are rooted under the durable organization ID and Vault routes select that filesystem partition from the authenticated organization scope. Legacy installation-token Vault routes remain available against the configured legacy/default organization root during migration.
- Resource classes that do not yet carry durable organization ownership are not registered in the organization-scoped router. Legacy installation-token routes remain available for migration, but membership authorization alone is never presented as tenant isolation.
- The Web UI exposes organization-owned Sessions and Vaults. It hides unsupported Project and Fleet navigation and shows a fail-closed unavailable state for direct legacy links until those resources gain durable ownership.
- Authentication data has a narrow, auditable persistence boundary rather than contaminating the event log with secrets.
