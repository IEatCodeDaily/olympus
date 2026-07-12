# Identity-aware proxy vs. Hall-backed forward authentication

**Context:** Olympus ADR 0014; ephemeral per-session web applications and static artifacts are exposed through an application gateway. Hall already owns users, organizations, password verification, browser sessions, and capability decisions. Sandboxed applications must never receive the user's primary Hall credential.

## Executive conclusion

Use a conventional reverse proxy as the only reachable data plane and make **Hall's authorization endpoint the forward-auth decision point**. Do not add Pomerium, oauth2-proxy, Authelia, authentik, or Teleport merely to obtain SSO: each would create another session/identity/policy authority and a synchronization problem Hall has already solved.

The important qualification is that “forward auth” is only a protocol shape, not a limitation. Caddy, nginx, or Traefik sends a subrequest; Hall can still perform route-aware capability checks, device checks, step-up requirements, audit decisions, and mint an audience-bound upstream assertion. What a dedicated identity-aware proxy adds is a *prebuilt implementation* of some of those functions—OIDC login and callback flows, its own cookies, route policy language, device integrations, upstream JWTs, access request workflows, and an operational UI—not a fundamentally stronger delegation boundary.

For Olympus:

1. Keep the primary Hall session in a **host-only `__Host-...`, Secure, HttpOnly cookie** on the Hall/shell origin. Never set `Domain=.example.com` and never forward `Cookie` or `Authorization` to an app.
2. For ordinary top-level app requests where the primary Hall cookie is intentionally available to the gateway, perform a Hall forward-auth check on every new HTTP request and inject only a small, namespaced identity contract.
3. For sandboxed cross-origin iframes, have the shell request a **single-use app-launch code** from Hall. Redeem it at the app gateway, immediately set an opaque, host-only, HttpOnly **per-app grant cookie**, and redirect to a clean URL. The cookie identifies only `{user, org, app instance, allowed actions, expiry}` and cannot call Hall generally.
4. When an upstream supports verifiable assertions, let **Hall**, not another identity product, sign a short-lived JWT with exact `aud = app-instance/route`, narrow scopes/capabilities, and a short expiry; the gateway injects it after stripping any client copy. For apps that only understand trusted headers, keep the upstream network non-bypassable.
5. Authenticate WebSockets at the HTTP upgrade and add explicit connection expiry/revocation behavior. For cross-origin or cookie-less sockets, use a 30–60 second, audience-bound, single-use WebSocket ticket.

---

## 1. What the products actually add

### Decision matrix for this deployment

| Pattern/product | Primary design center | Useful additions | Architectural cost when Hall is authoritative | Fit for Olympus |
|---|---|---|---|---|
| **Hall + Caddy/nginx/Traefik forward auth** | Delegate each request's allow/deny decision to an HTTP endpoint | Minimal moving parts; Hall sees the canonical route and current capability state; proxy can copy allowlisted identity headers; no second identity database | Olympus must define the auth contract, launch grants, signed assertions if desired, and connection-revocation behavior | **Best fit** |
| **oauth2-proxy** | OAuth/OIDC login broker in front of an upstream | Provider integrations, OAuth callback/session cookies, group/email restrictions, NGINX `auth_request` response headers, optional upstream access-token forwarding | A second browser session and IdP integration; policy is comparatively simple; passing the IdP access token increases credential exposure; no reason to use it if Hall already authenticates users | Redundant |
| **Pomerium** | BeyondCorp-style identity-aware reverse proxy | OIDC federation, request-time route policy, contextual/device inputs, centralized routes, signed upstream identity JWT/JWKS, access logs | Its Authenticate/Proxy/Authorize/Databroker session model and policy engine overlap Hall; Enterprise is needed for integrations such as FleetDM posture | Consider only if Olympus deliberately delegates access policy/device posture to Pomerium |
| **Authelia** | Reverse-proxy companion IAM/SSO portal | Cookie SSO, MFA/passkeys, path/domain/method/network rules, trusted-header SSO, OpenID Provider | Another user/session/MFA/policy authority; Hall would have to become an upstream identity source or duplicate state | Redundant |
| **authentik proxy / embedded outpost** | Full IAM plus distributed protocol/proxy outposts | Proxy or forward-auth modes, application policies, flows/stages/MFA, entitlements, dynamic backend selection, broad OIDC/SAML/etc. integration | Largest identity/control-plane overlap; its outpost still depends on authentik's own application/provider/session model | Redundant unless authentik replaces Hall IAM |
| **Teleport Application Access** | Infrastructure access plane for apps, SSH, Kubernetes, databases, desktops | Resource labels/RBAC, short-lived credentials, app-specific sessions, signed `Teleport-Jwt-Assertion`, access requests/locks, broad audit system, device-bound/device-trust capabilities in applicable editions | Introduces Teleport Auth Service, users/roles/CAs/app agents and a second policy hierarchy; operationally much heavier | Wrong abstraction for ephemeral Hall-owned apps unless Olympus adopts Teleport as its infrastructure access plane |

### oauth2-proxy

oauth2-proxy is primarily an OAuth/OIDC **client and session broker**. Its configuration documents a separate secure cookie (HttpOnly and Secure by default), provider scopes/groups, and these header modes:

- `--set-xauthrequest` returns `X-Auth-Request-User`, `-Groups`, `-Email`, and `-Preferred-Username`; with `--pass-access-token`, it also returns `X-Auth-Request-Access-Token`.
- `--pass-access-token` can send an IdP access token as `X-Forwarded-Access-Token`.
- Its default `--skip-auth-strip-headers=true` strips conflicting `X-Forwarded-*` authentication and `Authorization` headers it would set.

Those are useful when the application has no login system and the organization has an external IdP. They are not useful when Hall already owns login and authorization. In particular, oauth2-proxy does **not** add a richer application-capability model than Hall, and forwarding an upstream IdP access token is contrary to Olympus's least-credential rule. See [oauth2-proxy configuration](https://oauth2-proxy.github.io/oauth2-proxy/configuration/overview).

### Pomerium

Pomerium authenticates against OIDC and creates its own local session containing OAuth/OIDC data. It then performs context-aware authorization at routes. Its strengths over a bare off-the-shelf forward-auth middleware include:

- a packaged OIDC callback/session lifecycle and continuous request authorization;
- route policy language and external data sources;
- Enterprise device posture—for example, its FleetDM integration matches a browser-presented client certificate to Fleet host data and can deny access for stale agents, failed policies, or vulnerabilities ([FleetDM integration](https://www.pomerium.com/docs/integrations/device-context/fleetdm));
- a signed `X-Pomerium-Jwt-Assertion`, verified against JWKS with route-bound issuer/audience, as demonstrated by its [Jenkins integration](https://www.pomerium.com/docs/guides/jenkins);
- centrally managed routes/certificates and access telemetry.

None of these requires a second proxy in principle. Hall can check device context and sign assertions. Pomerium's value is avoiding that implementation and operating a mature packaged policy plane. Its cost is that Pomerium's [authentication/session](https://www.pomerium.com/docs/capabilities/authentication) and [authorization](https://www.pomerium.com/docs/capabilities/authorization) become another source of truth.

Pomerium should not be selected on the assumption that it records arbitrary browser sessions. Its recently documented session-recording capability is for native SSH, not a general replay of proxied web UI activity. HTTP access logs and audit decisions are not equivalent to DOM/video/session replay.

### Authelia

Authelia is a complete SSO/MFA portal as well as a forward-auth responder. Its rules can match domain, path, subject/group, method, and network and require one- or two-factor authentication ([access control](https://www.authelia.com/overview/authorization/access-control)). It can also act as an OpenID Provider ([OIDC Provider](https://www.authelia.com/configuration/identity-providers/openid-connect/provider)). In trusted-header mode it returns `Remote-User`, `Remote-Groups`, `Remote-Name`, and `Remote-Email` for the proxy to inject—not to expose to the browser ([trusted-header SSO](https://www.authelia.com/integration/trusted-header-sso/introduction)).

This is attractive for a homelab with heterogeneous applications and no central control plane. In Olympus, Hall already supplies all of the valuable state. Making Authelia authoritative would either duplicate users/sessions/policy or require Hall to expose and maintain an identity-provider integration solely so Authelia can return the decision to Hall's gateway.

### authentik embedded outpost / proxy provider

authentik's outpost can be the reverse proxy itself or only the forward-auth checker. Single-application mode supports distinct policies per application; domain-level mode protects many apps but cannot enforce separate application policies. The outpost injects `X-authentik-username`, groups, entitlements, email, name, UID, and application metadata. It also supports dynamic backend selection ([proxy provider](https://docs.goauthentik.io/add-secure-apps/providers/proxy), [forward auth](https://docs.goauthentik.io/add-secure-apps/providers/proxy/forward_auth)).

Its real differentiators are authentik's broader IAM platform: authentication flows and stages, MFA, protocol providers, source synchronization, application bindings/policies, entitlements, and managed outposts. Those features are valuable only if authentik is the identity authority. An embedded outpost is not “dumb”; it carries authentik's second application/session/policy model into the request path.

### Teleport Application Access

Teleport is an infrastructure access plane, not merely a web forward-auth plugin. Its Auth Service manages users, roles, configuration, and certificate authorities; the Proxy and Application Services establish a trusted tunnel to enrolled apps. Application RBAC is based on labels and Teleport roles ([Application Access](https://goteleport.com/docs/enroll-resources/application-access/protect-apps/connecting-apps)). Teleport injects a signed `Teleport-Jwt-Assertion` on every upstream request, with user, roles, traits, expiry, issuer, and audience, and publishes JWKS ([JWT assertions](https://goteleport.com/docs/enroll-resources/application-access/jwt/introduction)).

Teleport adds mature infrastructure-wide audit/access workflows and short-lived cryptographic identity. Its audit reference lists `app.session.start` and `app.session.chunk` events. However, the same reference says replayable full-stream recording applies to SSH/Kubernetes PTYs and desktop screens; do not equate app audit chunks with a general browser replay facility ([audit events and recordings](https://goteleport.com/docs/reference/deployment/monitoring/audit)).

For Olympus, Teleport is justified only if the strategic goal is to put all infrastructure access—including SSH/Kubernetes/databases and web apps—under Teleport. Adding it just for iframe SSO duplicates Hall's users/roles/session authority and requires app enrollment/agents, wildcard routing, and another CA/policy lifecycle.

### What forward auth cannot provide *by itself*

A Caddy/nginx/Traefik directive alone does not provide:

- OIDC/SAML login, callback, token refresh, consent, or per-upstream OAuth client registration;
- device enrollment, client certificates, MDM/EDR integrations, or posture inventory;
- a policy language, admin UI, access requests/approvals, or identity lifecycle;
- signed JWT minting/JWKS and upstream protocol adapters;
- global session/identity locks or mature audit export;
- content-aware recording of SSH/desktop/browser sessions.

But these are missing from the **middleware**, not from the **architecture**. Hall already implements most of the relevant control-plane pieces. It can add narrow JWT issuance and device inputs without adopting another authority. Session recording is the exception: it must live in a data plane that can observe and interpret the stream, and generic web session replay usually needs application/browser instrumentation rather than an auth subrequest.

---

## 2. Forward-auth and identity-header contracts

### There is no universal standard identity header

These are conventions, not an interoperable standard:

| Contract | Direction and meaning |
|---|---|
| `X-Forwarded-User`, `X-Forwarded-Email`, `X-Forwarded-Groups` | Common proxy-to-upstream identity fields. Names and separators vary. oauth2-proxy can set this family. |
| `X-Auth-Request-User`, `X-Auth-Request-Email`, `X-Auth-Request-Groups`, `X-Auth-Request-Access-Token` | Commonly **auth subresponse** headers produced by oauth2-proxy in nginx `auth_request` mode. nginx must explicitly copy them into the upstream request. They are not automatically a final upstream standard. |
| `Remote-User`, `Remote-Groups`, `Remote-Name`, `Remote-Email` | Authelia/Caddy trusted-header convention. `REMOTE_USER` originally appears in CGI/server environments; `Remote-User` as an HTTP header remains de facto. |
| `X-WEBAUTH-USER` and related attributes | Grafana's configurable Auth Proxy convention. Grafana explicitly recommends an IP whitelist to prevent spoofing ([Grafana Auth Proxy](https://grafana.com/docs/grafana/latest/setup-grafana/configure-access/configure-authentication/auth-proxy.md)). |
| `X-Pomerium-Jwt-Assertion` / `Teleport-Jwt-Assertion` | Product-specific signed assertions. Stronger against a network attacker or bypass path *if* the upstream verifies signature, issuer, audience, time claims, and key rotation. |

Proxy mechanics are similarly simple:

- Caddy `forward_auth` clones the request as a `GET`, adds original method/URI forwarding fields, and copies an explicit response-header list on 2xx ([Caddy](https://caddyserver.com/docs/caddyfile/directives/forward_auth)).
- nginx `auth_request` allows on 2xx, denies on 401/403, treats other statuses as errors, and exposes auth response values through `auth_request_set` ([nginx](https://nginx.org/en/docs/http/ngx_http_auth_request_module.html)).
- Traefik ForwardAuth allows on 2xx, forwards the auth server's non-2xx response otherwise, and can copy selected auth response headers while replacing conflicts. It sends method, protocol, host, URI, and source IP as `X-Forwarded-*` fields ([Traefik](https://doc.traefik.io/traefik/reference/routing-configuration/http/middlewares/forwardauth/)).

### Recommended Olympus contract

Prefer an Olympus namespace rather than overloading ambiguous ecosystem fields:

```http
# Proxy -> Hall authorization subrequest
X-Olympus-Route-Id: app-instance-uuid
X-Forwarded-Method: GET
X-Forwarded-Proto: https
X-Forwarded-Host: <validated public host>
X-Forwarded-Uri: /path?query
X-Forwarded-For: <sanitized client chain>
Cookie: __Host-hall_session=...   # only to Hall, never upstream

# Hall -> proxy on allow
204 No Content
X-Olympus-Subject: user-uuid
X-Olympus-Org: org-uuid
X-Olympus-Grant-Id: grant/session uuid
X-Olympus-App-Instance: app-instance-uuid
X-Olympus-Scopes: app.view,app.edit
X-Olympus-Identity-Assertion: <optional signed, short-lived JWT>
Cache-Control: no-store
```

Do not forward display names, email, broad organization membership, or the complete capability set unless the app needs them. Stable opaque IDs are safer than mutable usernames. Prefer one signed assertion when the upstream can verify it; otherwise copy a strict allowlist of scalar headers.

### Security invariants and failure modes

1. **Strip, then set.** At the first trusted ingress, delete every client-supplied `X-Olympus-*`, configured `Remote-*`, `X-Auth-Request-*`, `X-WEBAUTH-*`, assertion, and upstream `Authorization` header. On allow, overwrite from Hall's response. Never “set if absent.” oauth2-proxy's default header stripping and Traefik's replacement semantics demonstrate this requirement.
2. **Make the network non-bypassable.** Apps must listen on a private interface/network and accept traffic only from the gateway. An IP allowlist helps, but a Unix socket, dedicated network namespace, Kubernetes NetworkPolicy, or mTLS proxy identity is stronger. Authelia warns that trusting an entire Docker network means any compromised container on that network can spoof identity ([trusted-header SSO](https://www.authelia.com/integration/trusted-header-sso/introduction)).
3. **Authenticate the Hall hop.** Use a Unix socket where colocated, or TLS/mTLS plus service identity. Otherwise a compromised internal actor can forge an allow response.
4. **Sanitize forwarded metadata.** Hall's decision depends on method, canonical host, route, path, and sometimes client IP. Only trust forwarding headers from enumerated proxies. Authelia documents that unsanitized `X-Forwarded-For` can bypass network-based rules ([forwarded headers](https://www.authelia.com/integration/proxies/forwarded-headers)); current Traefik guidance likewise sanitizes them at trusted entry points.
5. **Bind the decision to a route ID.** Do not let a client-controlled `Host`, `X-Original-URL`, or ambiguous encoded path select another app. The gateway should resolve host/path to an internal immutable app-instance ID first and send that ID to Hall. Normalize percent encoding, dot segments, duplicate slashes, and host/port handling consistently between router and Hall.
6. **Copy an exact response allowlist.** Avoid broad patterns such as `^X-`. Never accidentally copy `Set-Cookie`, `Location`, `Authorization`, hop-by-hop headers, or diagnostic headers from Hall. Traefik notes regex-copying strips and replaces all matching headers, making an overbroad expression especially dangerous.
7. **Fail closed.** Hall timeout, malformed response, missing required identity fields, or non-2xx/non-401/non-403 status must not reach the app. Circuit breakers may shed load but must not reuse an old allow decision by default.
8. **Do not cache authorization casually.** If added later, key on opaque session/grant ID, route/app instance, normalized method/path policy bucket, capability generation, and revocation epoch, and cap TTL below grant expiry. Never cache by URL alone.
9. **Disable shared caching for private apps and auth endpoints.** Use `Cache-Control: private, no-store`; do not place identity-dependent responses in a shared CDN cache. RFC 9111 says shared caches normally must not store responses to requests containing `Authorization` absent explicit permission, but cookie/header-authenticated responses do not get that protection automatically; cache keys are often only method+URI unless `Vary` or configuration says otherwise ([RFC 9111](https://www.rfc-editor.org/info/rfc9111)). `Vary: Cookie` is not a substitute for disabling shared caching at this boundary.
10. **Treat headers as typed untrusted input.** Reject CR/LF and control characters, duplicates, oversized values, invalid UTF-8 assumptions, and delimiter ambiguity in groups/scopes. Configure one canonical spelling and account for HTTP field-name case insensitivity and proxy underscore handling.
11. **Signed JWTs reduce, but do not eliminate, boundary errors.** Verify signature, allowed algorithm, `iss`, exact `aud`, `exp`, `nbf`, route/app ID, and required scope. Keep expiry short and rotate keys through JWKS. A valid broad JWT replayed to the wrong app is still a bypass if `aud` is not checked.
12. **Do not expose identity headers in responses or logs.** They are internal request metadata. Redact cookies, launch codes, WebSocket tickets, and assertions from access/error logs and traces.
13. **Forward auth is not CSRF protection.** Cookie-authenticated mutating requests still need SameSite strategy, Origin/Referer checks where appropriate, and application/gateway CSRF tokens. WebSockets need an Origin allowlist to mitigate cross-site WebSocket hijacking.

---

## 3. Per-app credentials for sandboxed iframes

### The pattern used by real systems

Mature systems do not give every downstream the upstream identity-provider credential. They **broker a new local credential**:

- **Grafana Auth Proxy:** a trusted proxy supplies `X-WEBAUTH-USER`; Grafana can optionally validate the header only on `/login` and then issue its own login token/session cookie (`enable_login_token`). The upstream login credential does not need to reach Grafana. Grafana also supports a signed JWT injected by a reverse proxy for iframe identity. Its URL-login fallback accepts `auth_token` in the query string but warns that URLs can leak JWTs in logs and enable hijacking ([Auth Proxy](https://grafana.com/docs/grafana/latest/setup-grafana/configure-access/configure-authentication/auth-proxy.md), [JWT/iframe authentication](https://grafana.com/docs/grafana/latest/setup-grafana/configure-access/configure-authentication/jwt.md)).
- **JupyterHub:** the Hub is an internal OAuth provider even when an external IdP authenticated the user. A single-user server has its own service token, while the browser gets a distinct per-user internal OAuth token stored in an encrypted cookie. The single-user server validates that token with the Hub. JupyterHub scopes and horizontal filters can narrow access to one server/service, e.g. `access:servers!server`; it does not hand the external GitHub/Keycloak credential to the notebook server ([JupyterHub OAuth](https://jupyterhub.readthedocs.io/en/stable/explanation/oauth.html), [scopes](https://jupyterhub.readthedocs.io/en/5.2.1/rbac/scopes.html)).
- **code-server:** code-server commonly runs with its own authentication disabled behind a non-bypassable authenticated reverse proxy. That is safe only because the gateway is the security boundary; code-server itself does not provide a general per-user token-exchange protocol. Coder, the larger workspace platform, does have short-lived sessions and scoped/allowlisted tokens including `application_connect`, illustrating the brokered-token pattern ([Coder sessions and token scopes](https://coder.com/docs/admin/users/sessions-tokens)).

The general lesson is **credential translation**, not credential forwarding: primary Hall session → one-time launch authorization → app-specific session or assertion.

### Recommended launch flow

```text
Shell (Hall cookie)             Hall                 App gateway            Ephemeral app
        | POST /app/:id/launch    |                        |                       |
        |------------------------>| check capability      |                       |
        |  one-time code (30s)    | bind user/org/app     |                       |
        |<------------------------| aud, nonce, expiry     |                       |
        | iframe src=https://app.../bootstrap?code=...     |                       |
        |------------------------------------------------->| redeem with Hall      |
        |                                                  |---------------------->|
        |                                                  |<-- valid claims -------|
        |            302 / + Set-Cookie: __Host-app=opaque |                       |
        |<-------------------------------------------------|                       |
        | GET / (only app cookie)                          | forward-auth/grant    |
        |------------------------------------------------->| inject narrow identity|------>
```

Grant properties:

- opaque random code, single use, stored hashed server-side;
- 30–60 second launch-code TTL;
- exact user, org, app instance, route, allowed methods/actions, and optional frame-origin binding;
- no general Hall API access and no refresh token;
- app session TTL appropriate to risk (for example 5–15 minutes with bounded renewal while Hall remains active);
- revocable by grant ID and invalidated when the ephemeral instance stops;
- host-only Secure HttpOnly cookie with `Path=/`; choose SameSite deliberately for the actual iframe origin topology;
- `Cache-Control: no-store`, `Referrer-Policy: no-referrer`, immediate 302 to a clean URL, and query redaction in gateway logs.

Prefer an **opaque gateway session** for arbitrary/untrusted apps: the iframe never receives a bearer token in JavaScript, and the app sees only gateway-injected identity. If a trusted app supports JWT authentication, Hall may mint a short-lived signed assertion and the gateway injects it into the upstream request. Do not put a reusable JWT in the iframe URL merely because Grafana supports that fallback.

A `postMessage` flow can deliver a narrow app token to iframe JavaScript, but then the sandboxed app possesses a bearer credential and any XSS/app compromise can exfiltrate it. Use that only when the app must make direct calls and cannot rely on a gateway cookie. Validate exact target/source origins and keep the token one-app, one-instance, short-lived, and non-refreshable.

### Cookie-domain trap

SSO across `*.example.com` is often implemented with a parent-domain cookie. Do **not** do this for untrusted ephemeral apps. Any app host receiving the cookie can replay it to Hall, and a compromised subdomain can participate in cookie tossing/fixation. A `__Host-` cookie has no `Domain`, uses `Path=/`, and is sent only to the exact Hall origin. Cross-origin app SSO therefore needs the launch-code exchange above rather than widening the primary cookie.

---

## 4. WebSockets through forward-auth

### What happens

A WebSocket starts as an HTTP request. The forward-auth check runs on that **upgrade request only**. After a successful `101 Switching Protocols`, the proxy tunnels frames; there are no subsequent HTTP requests on which ordinary forward auth can re-run. Logging out or revoking a Hall session therefore does not automatically terminate an established connection.

nginx also requires explicit forwarding of hop-by-hop `Upgrade` and `Connection` headers and appropriate read timeouts/heartbeats ([nginx WebSocket proxying](https://nginx.org/en/docs/http/websocket.html)). Caddy and Traefik handle upgrades in their reverse-proxy paths, but timeout, draining, and middleware ordering still need tests.

### Required controls

1. Authorize the exact app-instance route on the upgrade request, using the app grant rather than the primary Hall cookie for a sandboxed iframe.
2. Strip spoofed identity headers before upgrade and inject the same narrow identity contract used for HTTP.
3. Validate `Origin` against the shell/app allowlist. Origin is a CSRF/CSWSH signal, not the user identity.
4. Apply an absolute connection lifetime no longer than the grant's intended lifetime, plus idle timeout and ping/pong liveness.
5. For prompt revocation, maintain a gateway connection registry indexed by Hall session/grant ID and close sockets when Hall publishes logout, capability-revocation, org-removal, or app-stop events. Otherwise document that revocation latency is bounded by the maximum socket lifetime.
6. Perform authorization inside the application for message-level resources/actions. A handshake-level “may connect” decision does not authorize every channel, document, or command sent later.
7. Preserve proxy protocol details and test HTTP/1.1 upgrade end to end; do not enable a ForwardAuth option that buffers/forwards request bodies on a streaming route. Traefik explicitly notes that `forwardBody` breaks streaming.

### Ticket-based WebSocket authentication

Browser JavaScript's `WebSocket` constructor cannot set arbitrary `Authorization` headers. The practical options are cookies, a query ticket, or an authentication first message. The `websockets` project recommends unforgeable short-lived or single-use tokens; it notes that query credentials leak into URI logs, while first-message authentication is reliable and keeps secrets out of the URI but occurs after the HTTP handshake ([WebSocket authentication](https://websockets.readthedocs.io/en/stable/topics/authentication.html)).

For Olympus, use:

```text
POST /apps/:id/ws-ticket  (authenticated shell or app-grant request)
 -> { ticket: random-opaque, expires_in: 30 }

new WebSocket("wss://app-host/ws?ticket=<single-use-ticket>")
```

The gateway atomically consumes the ticket before upgrade. Bind it to user, org, app instance, WebSocket path, expected Origin, and optional subprotocol; expire in 30–60 seconds and never refresh it. Redact the query and immediately avoid reusing the URI. If avoiding URI credentials is paramount and the upstream can enforce it, authenticate in the first application message and close unauthenticated sockets after a very short deadline; understand that the proxy has already completed the upgrade and cannot return a normal HTTP 401.

---

## Final recommendation for ADR 0014

### Adopt

- **One reverse proxy/gateway data plane** (choose based on existing Olympus operational fit; Caddy is the least verbose, nginx the most explicit, Traefik strongest if dynamic service discovery/CRDs are already desired).
- **Hall `/internal/authorize-app-request`** as the sole forward-auth authority.
- An immutable gateway-resolved `route_id/app_instance_id`, not a client-selected destination, in every decision.
- Strict identity-header stripping/allowlisting and non-bypassable upstream networking.
- **Opaque per-app grant sessions** for sandboxed iframes, bootstrapped by a one-time code from the shell.
- Optional **Hall-signed, audience-bound JWT assertions** only for apps that benefit from independently verifiable identity; publish Hall JWKS and validate `iss/aud/exp/nbf/scope`.
- A WebSocket ticket endpoint plus connection TTL/revocation hooks.
- `private, no-store` at all authenticated app/auth responses unless a route has an explicit, identity-safe cache design.

### Do not adopt now

- oauth2-proxy, Authelia, or authentik outposts: they duplicate Hall login/session/policy.
- Pomerium solely for header SSO or JWT injection: Hall plus the gateway can provide both. Reconsider only if managed device posture and a packaged policy plane become requirements that Olympus does not want to build.
- Teleport solely for web apps: too much overlapping identity/access infrastructure. Reconsider only as an organization-wide infrastructure access plane.
- Parent-domain Hall cookies, forwarding the Hall cookie/bearer, long-lived app JWTs, tokens in iframe URLs except one-time launch codes, or broad `X-*` header copying.

### ADR decision sentence

> Olympus SHALL use a conventional reverse proxy with per-request forward authorization delegated to Hall. Hall remains the sole authority for identity, organizations, sessions, and capabilities. Sandboxed applications receive only revocable, short-lived, audience-bound app grants (opaque gateway sessions by default; signed Hall assertions where required), never the primary Hall session. Dedicated identity-aware proxies are deferred until a concrete requirement—such as managed device posture or organization-wide infrastructure access—justifies introducing a second access plane.

## Primary sources

- [Caddy `forward_auth`](https://caddyserver.com/docs/caddyfile/directives/forward_auth)
- [nginx `auth_request`](https://nginx.org/en/docs/http/ngx_http_auth_request_module.html) and [WebSocket proxying](https://nginx.org/en/docs/http/websocket.html)
- [Traefik ForwardAuth](https://doc.traefik.io/traefik/reference/routing-configuration/http/middlewares/forwardauth/)
- [oauth2-proxy configuration](https://oauth2-proxy.github.io/oauth2-proxy/configuration/overview)
- [Pomerium authentication](https://www.pomerium.com/docs/capabilities/authentication), [authorization](https://www.pomerium.com/docs/capabilities/authorization), [FleetDM device context](https://www.pomerium.com/docs/integrations/device-context/fleetdm), and [signed JWT example](https://www.pomerium.com/docs/guides/jenkins)
- [Authelia proxy authorization](https://www.authelia.com/reference/guides/proxy-authorization), [trusted-header SSO](https://www.authelia.com/integration/trusted-header-sso/introduction), and [forwarded-header trust](https://www.authelia.com/integration/proxies/forwarded-headers)
- [authentik proxy provider](https://docs.goauthentik.io/add-secure-apps/providers/proxy) and [forward auth](https://docs.goauthentik.io/add-secure-apps/providers/proxy/forward_auth)
- [Teleport Application Access](https://goteleport.com/docs/enroll-resources/application-access/protect-apps/connecting-apps), [JWT assertion](https://goteleport.com/docs/enroll-resources/application-access/jwt/introduction), and [audit/recording](https://goteleport.com/docs/reference/deployment/monitoring/audit)
- [Grafana Auth Proxy](https://grafana.com/docs/grafana/latest/setup-grafana/configure-access/configure-authentication/auth-proxy.md) and [JWT iframe authentication](https://grafana.com/docs/grafana/latest/setup-grafana/configure-access/configure-authentication/jwt.md)
- [JupyterHub internal OAuth](https://jupyterhub.readthedocs.io/en/stable/explanation/oauth.html) and [scopes](https://jupyterhub.readthedocs.io/en/5.2.1/rbac/scopes.html)
- [WebSocket authentication patterns](https://websockets.readthedocs.io/en/stable/topics/authentication.html)
- [RFC 9111: HTTP Caching](https://www.rfc-editor.org/info/rfc9111)
