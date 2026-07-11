# ARCH-B — Route map for splitting `server/mod.rs`

Mechanical split of `crates/control-plane/src/server/mod.rs` (6239 lines) into
per-resource modules under `crates/control-plane/src/server/routes/*.rs`.
**Pure movement: no DTO, handler behavior, or route-name changes.** This document
is the pre-move inventory; it is the contract the post-move route-set assertion
test enforces.

## Decisions / notes

- **GitNexus MCP impact tools are UNAVAILABLE in this session** (confirmed via
  tool discovery — no `gitnexus_*` tools are exposed). Per the task, this is
  documented here and work proceeds; the split touches no HIGH/CRITICAL-risk
  behavior (it relocates private handlers/structs within one crate, preserving
  every route and DTO). The route-set assertion test + `cargo test/clippy/fmt`
  gates are the safety net in lieu of the impact graph.
- **Router signature.** Each resource module exposes
  `pub fn router() -> Router<AppState>` (state-generic). This *is* the existing
  axum 0.8 state pattern used by `build_router`: routers are assembled
  state-generic and `.with_state(state)` is applied exactly once at the end of
  `build_router` (previously mod.rs:274). The task text says
  `pub fn router(state: AppState)`; the parameterless generic form is chosen
  because it reproduces the original assembly byte-for-byte (single terminal
  `with_state`, one auth `route_layer` over the merged protected group) and thus
  most faithfully preserves behavior. State still flows through `build_router(state)`.
- **`mod tests` moves to `server/tests.rs`** (`#[cfg(test)] mod tests;`) so
  `mod.rs` stays under 600 lines. Tests are integration-style (through
  `build_router`) and reference no moved private item, so `use super::*` keeps
  resolving against `mod.rs`.
- **Stays in place (untouched):** `dto.rs`, `ws.rs`, `bridge_mgr.rs`,
  `envoy_conn.rs`, `identity.rs`, `principal.rs`, `test_support.rs`.
- **Handler visibility.** Moved handlers become `pub(crate)` so
  `organization_resource_routes` (org-scoped aliases) can reference them.

## What `mod.rs` retains (< 600 lines target)

`ImportState` + impl + `IMPORT_*` consts, `AppState`, `build_router`
(rewritten to `.merge(routes::<r>::router())`), `static_ui_service`,
`cors_layer`, `auth_gate`, the two public handlers `health` + `metrics`, the
inline public routes (`/api/health`, `/api/metrics`, `/api/auth/login`), the
inline protected `/api/proxy` + `/api/proxy/{slug}` management routes, the
public `/proxy/*` forward router, the `/api/enroll` mint route, and `/ws`.

## Shared helpers → `routes/support.rs` (`pub(crate)`)

| Helper | Callers (cross-module) |
|---|---|
| `now_epoch()` | setup, registry, sessions, projects, repos |
| `append_and_apply()` | projects, repos, sessions, cards |
| `append_and_apply_events()` | projects, cards |

## Handler-local helpers (stay with their module, private)

| Helper | Module |
|---|---|
| `event_timestamp()` | sessions |
| `derive_title()` | sessions |
| `copy_jj_workspaces()` | sessions |
| `vault_error()` | vaults |
| `derive_base_url()` | enroll |

## Route inventory → module (method · path · handler)

### routes/sessions.rs
- GET·`/api/sessions`·list_sessions · POST·`/api/sessions`·create_session
- GET·`/api/sessions/{id}`·get_session · PATCH·`/api/sessions/{id}`·patch_session
- POST·`/api/sessions/{id}/fork`·fork_session
- POST·`/api/sessions/{id}/handover`·handover_session
- GET·`/api/sessions/{id}/messages`·get_messages · POST·`/api/sessions/{id}/messages`·post_message
- POST·`/api/sessions/{id}/cancel`·cancel_session
- POST·`/api/sessions/{id}/steer`·steer_session
- POST·`/api/sessions/{id}/permission`·respond_permission_handler
- POST·`/api/sessions/{id}/project`·attach_session_project
- POST·`/api/sessions/{id}/repos`·attach_repo
- GET·`/api/sessions/{id}/subsessions`·list_subsessions · POST·…·create_subsession
- POST·`/api/sessions/{id}/complete`·complete_session
- structs: SessionsQuery, MessagesQuery, PostMessageBody, CreateSessionBody, PatchSessionBody, ForkSessionBody, HandoverBody, SteerBody, PermissionBody, CreateSubsessionBody, CompleteBody, AttachRepoBody, AttachProjectBody

### routes/irc.rs
- GET·`/api/irc/peers`·list_irc_peers · POST·`/api/irc/send`·irc_send · struct IrcSendBody

### routes/search.rs
- GET·`/api/search`·search · struct SearchQuery

### routes/agents.rs
- GET·`/api/models`·models · GET·`/api/agents`·list_agents_handler · GET·`/api/agents/{id}/models`·agent_models

### routes/cards.rs
- GET·`/api/cards`·list_cards · POST·`/api/cards`·create_card
- GET·`/api/cards/{id}`·get_card
- POST·`/api/cards/{id}/assign`·assign_card · `/claim`·claim_card · `/block`·block_card · `/complete`·complete_card · `/reassign`·reassign_card
- structs: CardsQuery, CreateCardBody, AssignCardBody, BlockCardBody, ReassignCardBody

### routes/events.rs
- GET·`/api/events`·tail_events · struct EventsQuery

### routes/setup.rs
- GET·`/api/setup`·get_setup · PUT·`/api/setup`·put_setup · structs SetupQuery, PutSetupBody

### routes/registry.rs
- GET·`/api/registry`·list_registry · PUT·`/api/registry`·put_registry_entry · structs RegistryQuery, PutRegistryBody

### routes/nodes.rs
- GET·`/api/nodes`·list_nodes · GET·`/api/nodes/hall-identity`·hall_identity
- GET·`/api/nodes/{id}/agents`·node_agents · POST·`/api/nodes/{id}/agents/refresh`·refresh_node_agents
- POST·`/api/nodes/{id}/drain`·drain_node · DELETE·`/api/nodes/{id}`·remove_node

### routes/vaults.rs
- GET·`/api/vaults`·list_vaults · POST·`/api/vaults`·create_vault
- GET·`/api/vaults/{id}/notes`·list_vault_notes · GET·`/api/vaults/{id}/documents`·list_vault_documents
- GET/PUT/DELETE·`/api/vaults/{id}/note`·get_vault_note/put_vault_note/delete_vault_note
- GET·`/api/vaults/{id}/graph`·get_vault_graph · GET·`/api/vaults/{id}/collections`·list_vault_collections
- GET·`/api/vaults/{id}/collections/{path}`·get_collection_rows
- structs: VaultNoteQuery, CreateVaultBody, PutVaultNoteBody · helper vault_error

### routes/projects.rs
- GET·`/api/projects`·list_projects · POST·`/api/projects`·create_project
- GET/PATCH/DELETE·`/api/projects/{id}`·get_project/patch_project/delete_project
- structs: CreateProjectBody, PatchProjectBody

### routes/repos.rs
- GET·`/api/repos`·list_repos · POST·`/api/repos`·register_repo
- GET/DELETE·`/api/repos/{slug}`·get_repo/remove_repo · struct RegisterRepoBody

### routes/enroll.rs
- (public router()) GET·`/api/enroll/{token}/install.sh`·enroll_install_script · `/binary`·enroll_binary · `/status`·enroll_status · POST·`/api/enroll/{token}`·enroll_register
- (mint, registered in mod.rs protected group) POST·`/api/enroll`·mint_enroll
- struct EnrollRegisterBody · helper derive_base_url

### routes/organizations.rs
- GET·`/api/auth/session`·identity::current_session · POST·`/api/auth/logout`·identity::logout
- GET·`/api/organizations`·identity::list_organizations
- ANY·`/api/organizations/{organization_id}/{*resource}`·organization_resource_proxy
- ANY·`/api/organizations/{organization_id}`·organization_resource_proxy
- owns `organization_resource_routes()` (org-scoped aliases that reuse the
  sessions/cards/projects/vaults handlers) + `organization_resource_proxy()`

### Kept in mod.rs (public / assembly)
- GET·`/api/health`·health · GET·`/api/metrics`·metrics · POST·`/api/auth/login`·identity::login
- GET/POST·`/api/proxy`·crate::proxy::list_proxy_endpoints/create_proxy_endpoint
- DELETE·`/api/proxy/{slug}`·crate::proxy::delete_proxy_endpoint
- `/proxy/{slug}/{rest}` + `/proxy/{slug}` forward (crate::proxy::proxy_forward*)
- GET·`/ws`·ws::ws_handler

## Route order

Distinct paths → axum's trie is insertion-order-independent, so per-resource
merge order is behaviorally identical. The one overlap pair
(`/api/organizations/{id}/{*resource}` vs `/api/organizations/{id}`) stays
co-located and ordered inside `organizations::router()`. **Local `/api/*` routes
are merged before the `/proxy/*` catch-all forward and the static-UI
`fallback_service`, exactly as before.** The route-set assertion test
(`route_contract_all_expected_routes_exist`, extended) probes the full
method/path inventory against the rebuilt router.
