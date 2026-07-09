//! axum HTTP server: REST read endpoints + auth gate (ADR 0002 §10.3.1, §3.5.2).
//!
//! The `/ws` delta stream lives in [`crate::server::ws`]. This module owns the
//! router, shared state, the auth middleware, and the read-only REST handlers
//! that back the UI's session list, transcript view, and search.
pub mod bridge_mgr;
pub mod dto;
pub mod envoy_conn;
pub mod ws;

// Agent discovery moved to `olympus-envoy` (ADR 0008 S2) — probing the host
// for Hermes profiles + CLI harnesses is the envoy's job. Re-exported so
// existing `server::agents::…` call sites keep working unchanged.
pub use olympus_envoy::discovery as agents;

#[cfg(test)]
pub mod test_support;

use std::sync::{
    atomic::{AtomicBool, AtomicU8, Ordering},
    Arc,
};

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::bridge::{AgentCommand, AgentEvent};
use crate::log::Log;
use crate::search::SearchIndex;
use crate::views::{CardFilters, Filters, ViewManager};
use bridge_mgr::BridgeManager;
use dto::{
    CardDto, MessageDto, NoteDocumentDto, NoteTreeEntryDto, ProjectDto, RegistryEntryDto,
    SearchHitDto, SessionDto, SetupDto, VaultSummaryDto,
};
use ws::ServerFrame;

/// Import progress, surfaced on `/api/health`. Stored as an atomic so the
/// background import thread can flip it to Done without a lock.
#[derive(Debug, Clone)]
pub struct ImportState(pub Arc<AtomicU8>);

impl ImportState {
    pub fn running() -> Self {
        Self(Arc::new(AtomicU8::new(IMPORT_RUNNING)))
    }
    pub fn done() -> Self {
        Self(Arc::new(AtomicU8::new(IMPORT_DONE)))
    }
    pub fn set_done(&self) {
        self.0.store(IMPORT_DONE, Ordering::SeqCst);
    }
    pub fn as_str(&self) -> &'static str {
        match self.0.load(Ordering::SeqCst) {
            IMPORT_IDLE => "idle",
            IMPORT_RUNNING => "running",
            IMPORT_DONE => "done",
            _ => "unknown",
        }
    }
}

pub const IMPORT_IDLE: u8 = 0;
pub const IMPORT_RUNNING: u8 = 1;
pub const IMPORT_DONE: u8 = 2;

/// Shared server state. Cheap to clone (everything behind `Arc`).
#[derive(Clone)]
pub struct AppState {
    pub views: Arc<RwLock<ViewManager>>,
    pub search: Arc<RwLock<SearchIndex>>,
    pub token: Arc<String>,
    pub import_state: ImportState,
    pub hermes_profile: Arc<String>,
    /// Delta fan-out: every view mutation is broadcast here; `/ws` subscribers
    /// forward frames to connected clients.
    pub deltas: broadcast::Sender<ServerFrame>,
    pub snapshot_sessions: u64,
    pub snapshot_messages: u64,
    /// The durable event log — sole source of truth. Appended to on new
    /// session creation and message events.
    pub log: Arc<Log>,
    /// Bridge manager: owns agent runtimes for managed (olympus-source) sessions.
    pub bridge: Arc<BridgeManager>,
    /// Whether the live `state.db` sync worker has successfully connected.
    pub sync_connected: Arc<AtomicBool>,
    /// In-process IRC bus for inter-agent messaging (ADR 0006 §2).
    pub irc: crate::irc::IrcBus,
    /// Fleet node registry — tracks connected envoys (UDS) + the local node.
    pub nodes: crate::node::NodeRegistry,
    /// Remote envoy connections (UDS write halves for RemoteRuntime).
    pub envoy_conns: crate::server::envoy_conn::EnvoyConnections,
    /// Reverse proxy routing table — slug → backend target.
    pub proxy: crate::proxy::ProxyTable,
    /// Markdown-first knowledge vault storage (ADR 0004).
    pub vaults: Arc<crate::vault::VaultStore>,
    /// Project (context container) storage — dir/manifest/symlink.
    pub projects: Arc<crate::projects::ProjectStore>,
    /// Managed repo store — clone/sync/attach jj workspaces.
    pub repos: Arc<crate::repos::RepoStore>,
}

/// Build the full router (REST + WS) with the auth gate applied to `/api/*` and
/// `/ws`. `/api/health` is intentionally left unauthenticated so a client can
/// probe readiness before it has the token.
pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", get(get_session).patch(patch_session))
        .route("/api/sessions/{id}/fork", axum::routing::post(fork_session))
        .route(
            "/api/sessions/{id}/handover",
            axum::routing::post(handover_session),
        )
        .route("/api/irc/peers", get(list_irc_peers))
        .route("/api/irc/send", axum::routing::post(irc_send))
        .route(
            "/api/sessions/{id}/messages",
            get(get_messages).post(post_message),
        )
        .route("/api/sessions/{id}/cancel", post(cancel_session))
        .route("/api/sessions/{id}/steer", post(steer_session))
        .route(
            "/api/sessions/{id}/permission",
            post(respond_permission_handler),
        )
        .route("/api/search", get(search))
        .route("/api/models", get(models))
        .route("/api/agents", get(list_agents_handler))
        .route("/api/agents/{id}/models", get(agent_models))
        .route("/api/cards", get(list_cards).post(create_card))
        .route("/api/cards/{id}", get(get_card))
        .route("/api/cards/{id}/assign", post(assign_card))
        .route("/api/cards/{id}/claim", post(claim_card))
        .route("/api/cards/{id}/block", post(block_card))
        .route("/api/cards/{id}/complete", post(complete_card))
        .route("/api/cards/{id}/reassign", post(reassign_card))
        .route("/api/events", get(tail_events))
        .route("/api/setup", get(get_setup).put(put_setup))
        .route("/api/registry", get(list_registry).put(put_registry_entry))
        .route("/api/nodes", get(list_nodes))
        .route("/api/nodes/{id}/agents", get(node_agents))
        .route("/api/nodes/{id}/agents/refresh", post(refresh_node_agents))
        .route("/api/vaults", get(list_vaults).post(create_vault))
        .route("/api/vaults/{id}/notes", get(list_vault_notes))
        .route(
            "/api/vaults/{id}/note",
            get(get_vault_note)
                .put(put_vault_note)
                .delete(delete_vault_note),
        )
        .route("/api/vaults/{id}/graph", get(get_vault_graph))
        .route("/api/vaults/{id}/collections", get(list_vault_collections))
        .route(
            "/api/vaults/{id}/collections/{path}",
            get(get_collection_rows),
        )
        .route("/api/projects", get(list_projects).post(create_project))
        .route(
            "/api/projects/{id}",
            get(get_project).patch(patch_project).delete(delete_project),
        )
        .route(
            "/api/sessions/{id}/project",
            axum::routing::post(attach_session_project),
        )
        .route("/api/repos", get(list_repos).post(register_repo))
        .route("/api/repos/{slug}", get(get_repo).delete(remove_repo))
        .route("/api/sessions/{id}/repos", axum::routing::post(attach_repo))
        .route(
            "/api/sessions/{id}/subsessions",
            get(list_subsessions).post(create_subsession),
        )
        .route(
            "/api/sessions/{id}/complete",
            axum::routing::post(complete_session),
        )
        .route(
            "/api/proxy",
            get(crate::proxy::list_proxy_endpoints).post(crate::proxy::create_proxy_endpoint),
        )
        .route(
            "/api/proxy/{slug}",
            axum::routing::delete(crate::proxy::delete_proxy_endpoint),
        )
        .route("/ws", get(ws::ws_handler))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth_gate));

    // The catch-all proxy forward is PUBLIC (auth is checked per-endpoint).
    // Must be registered AFTER all /api/* routes so it doesn't shadow them.
    // The fallback handler catches all /proxy/* paths.
    let proxy_forward = Router::new()
        .route(
            "/proxy/{slug}/{rest}",
            get(crate::proxy::proxy_forward)
                .post(crate::proxy::proxy_forward)
                .put(crate::proxy::proxy_forward)
                .delete(crate::proxy::proxy_forward)
                .patch(crate::proxy::proxy_forward),
        )
        .route(
            "/proxy/{slug}",
            get(crate::proxy::proxy_forward_root).post(crate::proxy::proxy_forward_root),
        );

    Router::new()
        .route("/api/health", get(health))
        .route("/api/metrics", get(metrics))
        .merge(protected)
        .merge(proxy_forward)
        .fallback_service(static_ui_service())
        .layer(cors_layer())
        .with_state(state)
}

/// Serve the built web UI (ui/dist) for any non-API path. SPA fallback:
/// unknown paths get index.html so client-side routing works. The dir is
/// resolved from OLYMPUS_UI_DIST (default "ui/dist" relative to the CWD).
/// If the dir doesn't exist requests 404 — dev mode uses Vite on :5177.
fn static_ui_service() -> tower_http::services::ServeDir<tower_http::services::ServeFile> {
    use tower_http::services::{ServeDir, ServeFile};
    let dist = std::env::var("OLYMPUS_UI_DIST").unwrap_or_else(|_| "ui/dist".to_string());
    let index = std::path::Path::new(&dist).join("index.html");
    ServeDir::new(&dist).fallback(ServeFile::new(index))
}

/// CORS for the local web UI. The UI is served from a different port than the
/// API in dev (Vite on :5173, API on :8787/:8799), so the browser makes
/// cross-origin requests + preflights. Mirror the Origin policy: reflect any
/// loopback origin, allow the methods/headers the client uses. `tower-http`
/// answers `OPTIONS` preflight automatically (before the auth middleware), so
/// the token isn't required on the preflight itself.
fn cors_layer() -> CorsLayer {
    use axum::http::{header, Method};
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _parts| {
            origin
                .to_str()
                .map(|o| crate::auth::origin_ok(Some(o)))
                .unwrap_or(false)
        }))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

/// Auth middleware: enforce the Origin policy on every request, and the Bearer
/// token on REST. The `/ws` upgrade carries its token as a `?token=` query
/// param (browsers can't set headers on WS), so that route validates the token
/// itself; here we still enforce Origin for it.
async fn auth_gate(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    if !crate::auth::origin_ok(origin) {
        return (StatusCode::FORBIDDEN, "forbidden origin").into_response();
    }

    let path = request.uri().path();
    let is_ws = path == "/ws";
    if !is_ws {
        let auth = headers.get("authorization").and_then(|v| v.to_str().ok());
        if !crate::auth::bearer_ok(auth, &state.token) {
            return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
        }
    }
    next.run(request).await
}

// ---- query params ----

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SessionsQuery {
    source: Option<String>,
    archived: Option<bool>,
    pinned: Option<bool>,
    /// Filter by managed status: `true` → Olympus-driven sessions (your active
    /// workspace), `false` → imported agent history (read-only, fork-to-use).
    /// Absent → both. This is the basis of the Sessions/History nav split.
    managed: Option<bool>,
    /// `lastActivity` (default) | `startedAt` | `messageCount`, all descending.
    sort: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct MessagesQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct SearchQuery {
    q: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct VaultNoteQuery {
    path: String,
}

#[derive(Debug, Deserialize)]
struct CreateVaultBody {
    name: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PutVaultNoteBody {
    #[serde(default)]
    markdown: Option<String>,
    /// Optional rename target. `newPath` is the explicit API; `path` is accepted
    /// as the natural "PUT this note at a new path" shape for early UI callers.
    #[serde(default)]
    new_path: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

// ---- handlers ----

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "importState": state.import_state.as_str(),
        "snapshot": { "sessions": state.snapshot_sessions, "messages": state.snapshot_messages },
        "syncConnected": state.sync_connected.load(Ordering::SeqCst),
        "hermesProfile": state.hermes_profile.as_str(),
    }))
}

/// GET /api/metrics — lightweight process + store stats for observability.
/// Unauthenticated (like /api/health) so it can be scraped without a token.
/// Reads /proc/self on Linux for RSS/threads/CPU; falls back gracefully off-Linux.
async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let mut rss_kb: Option<u64> = None;
    let mut threads: Option<u64> = None;
    let mut cpu_ticks: Option<u64> = None;
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                rss_kb = rest.split_whitespace().next().and_then(|n| n.parse().ok());
            } else if let Some(rest) = line.strip_prefix("Threads:") {
                threads = rest.split_whitespace().next().and_then(|n| n.parse().ok());
            }
        }
    }
    // utime + stime (fields 14,15 after comm) — cumulative CPU ticks (USER_HZ).
    if let Ok(stat) = std::fs::read_to_string("/proc/self/stat") {
        if let Some(idx) = stat.rfind(')') {
            let rest: Vec<&str> = stat[idx + 2..].split_whitespace().collect();
            // rest[11]=utime, rest[12]=stime (0-indexed after the comm field).
            let utime: u64 = rest.get(11).and_then(|s| s.parse().ok()).unwrap_or(0);
            let stime: u64 = rest.get(12).and_then(|s| s.parse().ok()).unwrap_or(0);
            cpu_ticks = Some(utime + stime);
        }
    }
    let ws_subs = state.deltas.receiver_count();
    Json(json!({
        "rssKb": rss_kb,
        "threads": threads,
        "cpuTicks": cpu_ticks,
        "wsSubscribers": ws_subs,
        "snapshot": { "sessions": state.snapshot_sessions, "messages": state.snapshot_messages },
        "syncConnected": state.sync_connected.load(Ordering::SeqCst),
        "inFlight": state.bridge.in_flight_set().await.len(),
    }))
}

/// Query params for `GET /api/events` (the tail-able event stream).
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct EventsQuery {
    /// Return events with sequence > since (exclusive lower bound). Defaults to
    /// 0 (all events). Callers pass the highest seq they've seen to catch up.
    #[serde(default)]
    since: Option<u64>,
    /// Cap on events returned (default 500, max 5000 — a full log replay is
    /// served in pages). A future remote envoy tails this in a loop.
    #[serde(default)]
    limit: Option<usize>,
}

/// GET /api/events?since=<seq>&limit=<n> — the tail-able event log, the single
/// replication spine. Every state mutation (session created, message appended,
/// card lifecycle, …) flows through the append-only log; this endpoint exposes
/// it as a paginated JSON stream that any consumer (the browser today, a remote
/// envoy tomorrow) can tail from a cursor.
///
/// This is the records-replication primitive for cross-node sync (ADR 0005 §5):
/// olympus is the authority, nodes reconcile by tailing from their last
/// checkpoint. Responses are `[{seq, event}, …]` in ascending order.
async fn tail_events(State(state): State<AppState>, Query(q): Query<EventsQuery>) -> Response {
    let since = q.since.unwrap_or(0);
    let limit = q.limit.unwrap_or(500).min(5000);
    match state.log.read_from(since, limit) {
        Ok(rows) => {
            let out: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|(seq, event)| json!({ "seq": seq, "event": event }))
                .collect();
            let next = out.last().and_then(|v| v["seq"].as_u64()).map(|s| s + 1);
            Json(json!({ "events": out, "next": next })).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "tail_events read failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "log_read_failed", "message": format!("{e:#}") })),
            )
                .into_response()
        }
    }
}

/// Query params for `GET /api/setup` — which scope's declaration to fetch.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SetupQuery {
    /// `"org:<org>"` | `"project:<org>/<project>"`. If `effective` is set with
    /// an org+project, returns the merged org+project setup instead of a single
    /// scope's raw declaration.
    scope: Option<String>,
    /// When both are present, return the merged effective setup for the project
    /// (org baseline + project layer). Overrides `scope`.
    org: Option<String>,
    project: Option<String>,
}

/// GET /api/setup?scope=... OR ?org=..&project=.. — the declared agent setup.
///
/// - `?scope=org:acme` → that scope's raw declaration (or empty setup).
/// - `?org=acme&project=web` → the *effective* (merged org+project) setup the
///   envoy would materialize for a session in that project (ADR 0006 §3.1).
async fn get_setup(State(state): State<AppState>, Query(q): Query<SetupQuery>) -> Response {
    let views = state.views.read().await;
    if let (Some(org), Some(project)) = (q.org.as_deref(), q.project.as_deref()) {
        let row = views.setup.effective_for_project(org, project);
        return Json(serde_json::to_value(SetupDto::from_row(&row)).unwrap()).into_response();
    }
    let Some(scope) = q.scope.as_deref() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "missing_scope", "message": "provide ?scope= or ?org=&project=" }),
            ),
        )
            .into_response();
    };
    match views.setup.get(scope) {
        Some(row) => Json(serde_json::to_value(SetupDto::from_row(row)).unwrap()).into_response(),
        None => {
            // An undeclared scope is a valid empty setup, not a 404.
            let empty = crate::server::dto::SetupDto {
                scope: scope.to_string(),
                skills: vec![],
                mcp: vec![],
                plugins: vec![],
                hooks: vec![],
                declared_at: 0.0,
            };
            Json(serde_json::to_value(empty).unwrap()).into_response()
        }
    }
}

/// Body for `PUT /api/setup` — full-replace a scope's declaration (ADR 0006 §3).
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PutSetupBody {
    scope: String,
    #[serde(default)]
    skills: Vec<String>,
    #[serde(default)]
    mcp: Vec<String>,
    #[serde(default)]
    plugins: Vec<String>,
    #[serde(default)]
    hooks: Vec<String>,
}

/// PUT /api/setup — declare (set/replace) a scope's agent setup. PUT semantics:
/// the body fully replaces the scope's prior declaration (ADR 0006 §3).
async fn put_setup(State(state): State<AppState>, Json(body): Json<PutSetupBody>) -> Response {
    // Validate the scope shape: "org:<slug>" or "project:<org>/<project>".
    let scope = body.scope.trim();
    let valid = scope
        .strip_prefix("org:")
        .map(|s| !s.is_empty() && !s.contains('/'))
        .or_else(|| {
            scope
                .strip_prefix("project:")
                .map(|s| s.split('/').filter(|p| !p.is_empty()).count() == 2)
        })
        .unwrap_or(false);
    if !valid {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_scope",
                "message": "scope must be 'org:<slug>' or 'project:<org>/<project>'",
            })),
        )
            .into_response();
    }

    let event = crate::event::Event::SetupDeclared {
        scope: scope.to_string(),
        skills: body.skills,
        mcp: body.mcp,
        plugins: body.plugins,
        hooks: body.hooks,
        declared_at: now_epoch(),
    };
    if let Err(e) = state.log.append(&event) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "log_error", "message": format!("{e:#}") })),
        )
            .into_response();
    }
    let dto = {
        let mut views = state.views.write().await;
        views.apply(&event);
        views.setup.get(scope).map(SetupDto::from_row)
    };
    match dto {
        Some(dto) => Json(serde_json::to_value(&dto).unwrap()).into_response(),
        None => (StatusCode::INTERNAL_SERVER_ERROR, "apply failed").into_response(),
    }
}

/// Query for `GET /api/registry` — filter by kind.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RegistryQuery {
    /// Filter to one kind: "skill" | "mcp" | "plugin" | "hook". Absent → all.
    kind: Option<String>,
}

/// GET /api/registry?kind=mcp — list registry entries (ADR 0006 §9.4).
async fn list_registry(State(state): State<AppState>, Query(q): Query<RegistryQuery>) -> Response {
    let views = state.views.read().await;
    let entries: Vec<RegistryEntryDto> = match q.kind.as_deref() {
        Some(kind) => views.registry.list_kind(kind),
        None => views.registry.list(),
    }
    .iter()
    .map(|e| RegistryEntryDto::from_entry(e))
    .collect();
    Json(json!({ "entries": entries })).into_response()
}

/// Body for `PUT /api/registry` — register (or replace) an entry.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PutRegistryBody {
    kind: String,
    slug: String,
    definition: String,
}

/// PUT /api/registry — register a (kind, slug) → definition entry. PUT semantics
/// (full replace). Validates kind is one of skill/mcp/plugin/hook.
async fn put_registry_entry(
    State(state): State<AppState>,
    Json(body): Json<PutRegistryBody>,
) -> Response {
    let kind = body.kind.trim().to_string();
    if !matches!(kind.as_str(), "skill" | "mcp" | "plugin" | "hook") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_kind",
                "message": "kind must be skill | mcp | plugin | hook",
            })),
        )
            .into_response();
    }
    let slug = body.slug.trim().to_string();
    if slug.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "empty_slug", "message": "slug must be non-empty" })),
        )
            .into_response();
    }

    let event = crate::event::Event::EntryRegistered {
        kind: kind.clone(),
        slug: slug.clone(),
        definition: body.definition,
        registered_at: now_epoch(),
    };
    if let Err(e) = state.log.append(&event) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "log_error", "message": format!("{e:#}") })),
        )
            .into_response();
    }
    let dto = {
        let mut views = state.views.write().await;
        views.apply(&event);
        views
            .registry
            .get(&kind, &slug)
            .map(RegistryEntryDto::from_entry)
    };
    match dto {
        Some(dto) => Json(serde_json::to_value(&dto).unwrap()).into_response(),
        None => (StatusCode::INTERNAL_SERVER_ERROR, "apply failed").into_response(),
    }
}

async fn list_sessions(
    State(state): State<AppState>,
    Query(q): Query<SessionsQuery>,
) -> impl IntoResponse {
    let views = state.views.read().await;
    // `source` may be a comma-separated multi-select; the view filter takes one
    // value, so we filter the post-list set for multi.
    let sources: Option<Vec<String>> = q
        .source
        .as_ref()
        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect());

    let filters = Filters {
        source: None,
        archived: q.archived,
        pinned: q.pinned,
    };
    let mut rows: Vec<SessionDto> = views
        .sessions
        .list(&filters)
        .into_iter()
        .filter(|r| match &sources {
            Some(list) if !list.is_empty() => list.iter().any(|s| s == &r.source),
            _ => true,
        })
        .map(SessionDto::from_row)
        // Apply the managed filter (Sessions vs History nav split). Within
        // managed, hide phantom duplicates: legacy re-imported sessions that are
        // tagged source=olympus but were never driven by Olympus (agent unset and
        // hermes_id == id — the pre-dedup signature). They read as managed but
        // aren't real workspaces; the History view is their honest home.
        .filter(|dto| {
            let is_managed = dto.source == "acp" || dto.source == "olympus";
            let is_phantom = is_managed
                && dto.agent.is_none()
                && !dto.hermes_id.is_empty()
                && dto.hermes_id == dto.id;
            match q.managed {
                Some(true) => is_managed && !is_phantom,
                Some(false) => !is_managed || is_phantom,
                None => true,
            }
        })
        .collect();
    drop(views);

    // Stamp derived liveness. Managed sessions use the authoritative in-flight
    // + awaiting-input flags; observed sessions fall back to activity recency.
    let in_flight = state.bridge.in_flight_set().await;
    let awaiting = state.bridge.awaiting_input_set().await;
    let now = now_epoch();
    for r in rows.iter_mut() {
        let managed = r.source == "acp" || r.source == "olympus";
        r.liveness = crate::server::dto::compute_liveness(
            r.last_activity,
            now,
            in_flight.contains(&r.id),
            managed,
            awaiting.contains(&r.id),
        )
        .to_string();
    }

    // Apply the requested sort (all descending). Default = lastActivity.
    // The view returns started_at-desc order; we re-sort here so the UI's
    // sort selector (lastActivity | startedAt | messageCount) takes effect.
    match q.sort.as_deref() {
        Some("startedAt") => rows.sort_by(|a, b| {
            b.started_at
                .partial_cmp(&a.started_at)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        }),
        Some("messageCount") => rows.sort_by(|a, b| {
            b.message_count
                .cmp(&a.message_count)
                .then_with(|| a.id.cmp(&b.id))
        }),
        // "lastActivity" and anything unrecognized (incl. None) -> lastActivity desc.
        _ => rows.sort_by(|a, b| {
            b.last_activity
                .partial_cmp(&a.last_activity)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        }),
    }

    let total = rows.len();
    if let Some(limit) = q.limit {
        rows.truncate(limit);
    }

    Json(json!({ "sessions": rows, "nextCursor": serde_json::Value::Null, "total": total }))
}

async fn get_session(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let views = state.views.read().await;
    match views.sessions.get(&id) {
        Some(row) => {
            let mut dto = SessionDto::from_row(row);
            drop(views);
            let in_flight = state.bridge.in_flight_set().await;
            let awaiting = state.bridge.awaiting_input_set().await;
            let managed = dto.source == "acp" || dto.source == "olympus";
            dto.liveness = crate::server::dto::compute_liveness(
                dto.last_activity,
                now_epoch(),
                in_flight.contains(&dto.id),
                managed,
                awaiting.contains(&dto.id),
            )
            .to_string();
            Json(dto).into_response()
        }
        None => (StatusCode::NOT_FOUND, "session not found").into_response(),
    }
}

/// Current epoch seconds as f64 (for liveness recency math).
fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Extract the timestamp from a MessageAppended event (for DTO building).
fn event_timestamp(event: &crate::event::Event) -> f64 {
    match event {
        crate::event::Event::MessageAppended { timestamp, .. } => *timestamp,
        _ => now_epoch(),
    }
}

/// Derive a short human title from the first user message. First non-empty
/// line, collapsed whitespace, trimmed to ~60 chars on a word boundary. This
/// is a cheap heuristic (no LLM round-trip) so titles appear instantly instead
/// of "Untitled". A nicer LLM-summarized title can replace this later.
fn derive_title(text: &str) -> String {
    let first_line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let collapsed = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 60;
    if collapsed.chars().count() <= MAX {
        return collapsed;
    }
    // Trim to MAX chars, then back off to the last word boundary for cleanliness.
    let truncated: String = collapsed.chars().take(MAX).collect();
    let cut = truncated.rfind(' ').unwrap_or(truncated.len());
    format!("{}…", truncated[..cut].trim_end())
}

async fn get_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<MessagesQuery>,
) -> impl IntoResponse {
    let views = state.views.read().await;
    let limit = q.limit.unwrap_or(50);
    let messages: Vec<MessageDto> = views
        .messages
        .recent(&id, limit)
        .into_iter()
        .map(|row| MessageDto::from_row(&id, row))
        .collect();
    Json(json!({ "messages": messages, "nextCursor": serde_json::Value::Null }))
}

async fn search(State(state): State<AppState>, Query(q): Query<SearchQuery>) -> Response {
    let Some(query) = q.q.filter(|s| !s.trim().is_empty()) else {
        return Json(json!({ "hits": [] })).into_response();
    };
    let limit = q.limit.unwrap_or(50);

    let index = state.search.read().await;
    let hits = match index.search(&query, limit) {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!(error = %e, "search failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "search error").into_response();
        }
    };
    drop(index);

    // Enrich each hit with the session's source + the message timestamp.
    let views = state.views.read().await;
    let dtos: Vec<SearchHitDto> = hits
        .iter()
        .map(|h| {
            let source = views
                .sessions
                .get(&h.session_id)
                .map(|s| s.source.clone())
                .unwrap_or_default();
            let timestamp = views
                .messages
                .recent(&h.session_id, usize::MAX)
                .into_iter()
                .find(|m| m.message_id == h.message_id)
                .map(|m| m.timestamp)
                .unwrap_or(0.0);
            SearchHitDto::from_index_hit(h, source, timestamp)
        })
        .collect();

    Json(json!({ "hits": dtos })).into_response()
}

async fn models(State(_state): State<AppState>) -> impl IntoResponse {
    // All models across every configured agent (deduped). For an agent-specific
    // list use GET /api/agents/:id/models.
    Json(json!({ "models": agents::list_models() }))
}

/// GET /api/agents/:id/models — models the given agent can actually run
/// (scoped to that agent's provider). This is what keeps the composer's model
/// selector agent-specific — a Codex agent is never offered Claude models.
async fn agent_models(State(_state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let provider = agents::list_agents()
        .into_iter()
        .find(|a| a.id == id)
        .and_then(|a| a.provider);
    Json(json!({ "models": agents::list_models_for(provider.as_deref()) }))
}

/// GET /api/agents — flat list of agents across all fleet nodes (deduped by id).
/// Sourced from the node registry (each node's envoy-reported agents), NOT a
/// live control-plane probe. For per-node scoping use /api/nodes/:id/agents.
async fn list_agents_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({ "agents": state.nodes.all_agents().await }))
}

/// GET /api/nodes/:id/agents — the agents a specific node's envoy discovered.
async fn node_agents(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.nodes.agents(&id).await {
        Ok(agents) => Json(json!({ "agents": agents })).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/nodes/:id/agents/refresh — re-detect agents on a node (manual, as
/// requested: refreshing to detect newly-installed agents is explicit, not
/// automatic). For the local node the control plane re-runs discovery in-process;
/// a remote node would forward a detect request to its envoy (TODO when the
/// standalone envoy binary lands).
async fn refresh_node_agents(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if id != "local" {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "error": "remote agent refresh requires the node's envoy; only 'local' is supported in-process for now"
            })),
        )
            .into_response();
    }
    let fresh = agents::discover_local_agents();
    match state.nodes.set_agents(&id, fresh).await {
        Ok(agents) => Json(json!({ "agents": agents })).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn list_nodes(State(state): State<AppState>) -> impl IntoResponse {
    let nodes = state.nodes.list().await;
    Json(json!({ "nodes": nodes }))
}

async fn list_vaults(State(state): State<AppState>) -> Response {
    match state.vaults.list_vaults() {
        Ok(vaults) => {
            let vaults: Vec<VaultSummaryDto> = vaults.into_iter().map(Into::into).collect();
            Json(json!({ "vaults": vaults })).into_response()
        }
        Err(err) => vault_error(err),
    }
}

async fn create_vault(
    State(state): State<AppState>,
    Json(body): Json<CreateVaultBody>,
) -> Response {
    match state.vaults.create_vault(&body.name) {
        Ok(vault) => (
            StatusCode::CREATED,
            Json(serde_json::to_value(VaultSummaryDto::from(vault)).unwrap()),
        )
            .into_response(),
        Err(err) => vault_error(err),
    }
}

async fn list_vault_notes(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.vaults.list_notes(&id) {
        Ok(notes) => {
            let notes: Vec<NoteTreeEntryDto> = notes.into_iter().map(Into::into).collect();
            Json(json!({ "notes": notes })).into_response()
        }
        Err(err) => vault_error(err),
    }
}

async fn get_vault_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<VaultNoteQuery>,
) -> Response {
    match state.vaults.read_note(&id, &q.path) {
        Ok(note) => {
            Json(serde_json::to_value(NoteDocumentDto::from(note)).unwrap()).into_response()
        }
        Err(err) => vault_error(err),
    }
}

async fn put_vault_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<VaultNoteQuery>,
    Json(body): Json<PutVaultNoteBody>,
) -> Response {
    let new_path = body.new_path.or(body.path);
    match state.vaults.write_note(
        &id,
        &q.path,
        crate::vault::WriteNote {
            markdown: body.markdown,
            new_path,
        },
    ) {
        Ok(note) => {
            Json(serde_json::to_value(NoteDocumentDto::from(note)).unwrap()).into_response()
        }
        Err(err) => vault_error(err),
    }
}

async fn delete_vault_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<VaultNoteQuery>,
) -> Response {
    match state.vaults.delete_note(&id, &q.path) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => vault_error(err),
    }
}

fn vault_error(err: anyhow::Error) -> Response {
    let status = if crate::vault::not_found(&err) {
        StatusCode::NOT_FOUND
    } else if crate::vault::bad_request(&err) {
        StatusCode::BAD_REQUEST
    } else {
        tracing::error!(error = %err, "vault operation failed");
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (
        status,
        Json(json!({ "error": "vault_error", "message": err.to_string() })),
    )
        .into_response()
}

// ---- Vault graph + collections ----

async fn get_vault_graph(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.vaults.graph(&id) {
        Ok(g) => {
            let dto = dto::VaultGraphDto {
                nodes: g
                    .nodes
                    .into_iter()
                    .map(|n| dto::GraphNodeDto {
                        id: n.id,
                        title: n.title,
                        path: n.path,
                        cid: n.cid,
                        link_count: n.link_count,
                    })
                    .collect(),
                edges: g
                    .edges
                    .into_iter()
                    .map(|e| dto::GraphEdgeDto {
                        source: e.source,
                        target: e.target,
                    })
                    .collect(),
            };
            Json(json!({ "nodes": dto.nodes, "edges": dto.edges })).into_response()
        }
        Err(err) => vault_error(err),
    }
}

async fn list_vault_collections(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.vaults.list_collections(&id) {
        Ok(collections) => {
            let dtos: Vec<dto::CollectionSummaryDto> = collections
                .into_iter()
                .map(|c| dto::CollectionSummaryDto {
                    name: c.name,
                    path: c.path,
                    row_count: c.row_count,
                })
                .collect();
            Json(json!({ "collections": dtos })).into_response()
        }
        Err(err) => vault_error(err),
    }
}

async fn get_collection_rows(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
) -> Response {
    match state.vaults.collection_rows(&id, &path) {
        Ok(data) => Json(json!({
            "columns": data.columns,
            "rows": data.rows,
        }))
        .into_response(),
        Err(err) => vault_error(err),
    }
}

// ---- Projects (context container) ------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProjectBody {
    name: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PatchProjectBody {
    name: Option<String>,
    vaults: Option<Vec<String>>,
    repos: Option<Vec<String>>,
    boards: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachProjectBody {
    project_id: String,
}

async fn list_projects(State(state): State<AppState>) -> Response {
    let views = state.views.read().await;
    let rows: Vec<ProjectDto> = views
        .projects
        .list()
        .into_iter()
        .map(ProjectDto::from_row)
        .collect();
    Json(json!({ "projects": rows, "total": rows.len() })).into_response()
}

async fn get_project(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let views = state.views.read().await;
    match views.projects.get(&id) {
        Some(row) => Json(ProjectDto::from_row(row)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "not_found", "message": "project not found" })),
        )
            .into_response(),
    }
}

async fn create_project(
    State(state): State<AppState>,
    Json(body): Json<CreateProjectBody>,
) -> Response {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid", "message": "name is required" })),
        )
            .into_response();
    }
    let project_id = uuid::Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let event = crate::event::Event::ProjectCreated {
        project_id: project_id.clone(),
        name: name.clone(),
        created_at: now,
    };
    // Persist manifest to disk (best-effort; event is the source of truth).
    let _ = state.projects.create(&project_id, &name, now);
    append_and_apply(&state, event).await;
    let views = state.views.read().await;
    match views.projects.get(&project_id) {
        Some(row) => (StatusCode::CREATED, Json(ProjectDto::from_row(row))).into_response(),
        None => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn patch_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchProjectBody>,
) -> Response {
    {
        let views = state.views.read().await;
        if views.projects.get(&id).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "not_found", "message": "project not found" })),
            )
                .into_response();
        }
    }
    // Update on-disk manifest (best-effort).
    let _ = state.projects.update(
        &id,
        body.name.as_deref(),
        body.vaults.as_deref(),
        body.repos.as_deref(),
        body.boards.as_deref(),
    );
    let event = crate::event::Event::ProjectUpdated {
        project_id: id.clone(),
        name: body.name,
        vaults: body.vaults,
        repos: body.repos,
        boards: body.boards,
    };
    append_and_apply(&state, event).await;
    let views = state.views.read().await;
    match views.projects.get(&id) {
        Some(row) => Json(ProjectDto::from_row(row)).into_response(),
        None => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn delete_project(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    {
        let views = state.views.read().await;
        if views.projects.get(&id).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "not_found", "message": "project not found" })),
            )
                .into_response();
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let event = crate::event::Event::ProjectDeleted {
        project_id: id.clone(),
        deleted_at: now,
    };
    append_and_apply(&state, event).await;
    StatusCode::NO_CONTENT.into_response()
}

async fn attach_session_project(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<AttachProjectBody>,
) -> Response {
    // Validate session exists.
    let session_space = {
        let views = state.views.read().await;
        if views.sessions.get(&session_id).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "not_found", "message": "session not found" })),
            )
                .into_response();
        }
        // Also validate project exists.
        if views.projects.get(&body.project_id).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "not_found", "message": "project not found" })),
            )
                .into_response();
        }
        drop(views);
        // Retrieve session space from bridge manager.
        state.bridge.space_for(&session_id)
    };
    // Create symlink (best-effort).
    let _ = state
        .projects
        .attach_symlink(&body.project_id, session_space.as_deref());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let event = crate::event::Event::SessionProjectAttached {
        session_id: session_id.clone(),
        project_id: body.project_id.clone(),
        attached_at: now,
    };
    append_and_apply(&state, event).await;
    let views = state.views.read().await;
    match views.sessions.get(&session_id) {
        Some(row) => Json(json!({ "sessionId": row.session_id, "projectId": row.project_id }))
            .into_response(),
        None => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct PostMessageBody {
    #[serde(alias = "content")]
    text: String,
    #[serde(default)]
    model: Option<String>,
    /// Thinking/reasoning effort level ("low" | "medium" | "high"). Delivered
    /// to the Hermes ACP adapter as a /thinking slash command before the
    /// prompt. Absent/None = leave the session's current setting alone.
    #[serde(default)]
    thinking: Option<String>,
}

/// Body for `POST /api/sessions` — optional agent/node binding at creation. All
/// fields optional so a bare `{}` creates an unbound draft.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CreateSessionBody {
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    node: Option<String>,
}

/// Body for `PATCH /api/sessions/:id` — bind/rebind agent, node, model, or title
/// before the first send. All fields optional; only present fields are changed.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PatchSessionBody {
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default)]
    pinned: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ForkSessionBody {
    #[serde(default)]
    fork_type: Option<String>,
}

/// Request body for POST /api/sessions/:id/handover.
#[derive(Debug, Deserialize)]
struct HandoverBody {
    /// Target agent kind: "claude-code", "codex", or "hermes".
    to_agent_kind: String,
    /// Optional model override for the target session.
    #[serde(default)]
    model: Option<String>,
}

// ---- Repos (managed git/jj repos) ----

async fn list_repos(State(state): State<AppState>) -> Response {
    let views = state.views.read().await;
    let dtos: Vec<dto::RepoDto> = views
        .repos
        .list()
        .iter()
        .map(|r| dto::RepoDto::from_row(r))
        .collect();
    Json(dtos).into_response()
}

async fn get_repo(State(state): State<AppState>, Path(slug): Path<String>) -> Response {
    match state.views.read().await.repos.get(&slug) {
        Some(row) => Json(dto::RepoDto::from_row(row)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct RegisterRepoBody {
    slug: String,
    url: String,
    default_branch: String,
}

async fn register_repo(
    State(state): State<AppState>,
    Json(body): Json<RegisterRepoBody>,
) -> Response {
    let event = crate::event::Event::RepoRegistered {
        slug: body.slug.clone(),
        url: body.url.clone(),
        default_branch: body.default_branch.clone(),
        registered_at: now_epoch(),
    };
    append_and_apply(&state, event).await
}

async fn remove_repo(State(state): State<AppState>, Path(slug): Path<String>) -> Response {
    if state.views.read().await.repos.get(&slug).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let event = crate::event::Event::RepoRemoved {
        slug,
        removed_at: now_epoch(),
    };
    append_and_apply(&state, event).await
}

#[derive(Debug, Deserialize)]
struct AttachRepoBody {
    slug: String,
}

/// Body for `POST /api/sessions/:id/subsessions`.
#[derive(Debug, Default, Deserialize)]
struct CreateSubsessionBody {
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
}

/// Body for `POST /api/sessions/:id/complete` — the check gate.
#[derive(Debug, Deserialize)]
struct CompleteBody {
    verdict: String,
    #[serde(default)]
    summary: Option<String>,
}

async fn attach_repo(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<AttachRepoBody>,
) -> Response {
    if state.views.read().await.sessions.get(&session_id).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let event = crate::event::Event::SessionRepoAttached {
        session_id,
        slug: body.slug,
        attached_at: now_epoch(),
    };
    append_and_apply(&state, event).await
}

/// Best-effort: copy jj workspaces from parent to child session space.
fn copy_jj_workspaces(parent_space: &std::path::Path, child_space: &std::path::Path) {
    let parent_repos = match parent_space.join("repos").read_dir() {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in parent_repos.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let src = entry.path();
        if !src.join(".jj").is_dir() {
            continue;
        }
        let dest = child_space.join("repos").join(name);
        let root_output = tokio::task::block_in_place(|| {
            std::process::Command::new("jj")
                .arg("workspace")
                .arg("root")
                .current_dir(&src)
                .output()
        });
        let main_repo = match root_output {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
            _ => {
                continue;
            }
        };
        let _ = std::fs::create_dir_all(child_space.join("repos"));
        let add_output = tokio::task::block_in_place(|| {
            std::process::Command::new("jj")
                .arg("workspace")
                .arg("add")
                .arg(&dest)
                .current_dir(&main_repo)
                .output()
        });
        match add_output {
            Ok(out) if out.status.success() => {
                tracing::info!(workspace = %dest.display(), "copied jj workspace into child");
            }
            Ok(out) => {
                tracing::warn!(workspace = %dest.display(), stderr = %String::from_utf8_lossy(&out.stderr), "jj workspace add failed")
            }
            Err(e) => {
                tracing::warn!(workspace = %dest.display(), error = %e, "failed to invoke jj")
            }
        }
    }
}

/// POST /api/sessions/:id/subsessions — spawn a child managed session.
async fn create_subsession(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<CreateSubsessionBody>>,
) -> Response {
    let body = body.map(|Json(b)| b).unwrap_or_default();

    let (parent_agent, parent_space) = {
        let views = state.views.read().await;
        let Some(parent) = views.sessions.get(&id) else {
            return (StatusCode::NOT_FOUND, "parent session not found").into_response();
        };
        let agent = body.agent.clone().or_else(|| parent.agent.clone());
        let space = state.bridge.space_path(&id);
        (agent, space)
    };

    let spec = crate::server::bridge_mgr::RuntimeSpec {
        agent: parent_agent.clone(),
        node: None,
        cwd: None,
        mcp_servers: vec![],
        env: vec![],
    };

    let ns = match state.bridge.create_draft(&spec) {
        Ok(ns) => ns,
        Err(e) => {
            tracing::error!(error = %e, parent = %id, "create_subsession create_draft failed");
            return (StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "bridge_error", "message": format!("Failed to create subsession: {e:#}") })))
                .into_response();
        }
    };

    // Best-effort jj-workspace copy.
    if let (Some(parent_sp), Some(child_sp)) = (
        parent_space.as_ref(),
        state.bridge.space_path(&ns.session_id),
    ) {
        if parent_sp.exists() {
            copy_jj_workspaces(parent_sp, &child_sp);
        }
    }

    let created = crate::event::Event::SessionCreated {
        session_id: ns.session_id.clone(),
        hermes_id: ns.hermes_id.clone(),
        source: "olympus".into(),
        model: None,
        title: body.title.clone(),
        started_at: ns.started_at,
        message_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        agent: parent_agent.clone(),
        node: None,
    };
    {
        let mut views = state.views.write().await;
        views.apply(&created);
    }

    let forked_event = crate::event::Event::SessionForked {
        parent_session_id: id.clone(),
        child_session_id: ns.session_id.clone(),
        fork_type: "sub".into(),
        fork_point: None,
        forked_at: now_epoch(),
    };
    if let Err(e) = state.log.append(&forked_event) {
        tracing::warn!(error = %e, "failed to append SessionForked for subsession");
    }
    {
        let mut views = state.views.write().await;
        views.apply(&forked_event);
    }

    // Optionally enqueue the first user message.
    if let Some(prompt) = &body.prompt {
        if !prompt.trim().is_empty() {
            let next_id = 0u64;
            match state
                .bridge
                .append_user_message(&ns.session_id, &ns.hermes_id, next_id, prompt)
            {
                Ok(event) => {
                    {
                        let mut views = state.views.write().await;
                        views.apply(&event);
                    }
                    let dto = crate::server::dto::MessageDto {
                        message_id: next_id,
                        session_id: ns.session_id.clone(),
                        role: "user".into(),
                        content: Some(prompt.clone()),
                        tool_name: None,
                        tool_calls: None,
                        reasoning: None,
                        timestamp: event_timestamp(&event),
                        token_count: None,
                        finish_reason: None,
                    };
                    let _ = state.deltas.send(ServerFrame::MessageAppended {
                        session_id: ns.session_id.clone(),
                        message: dto,
                    });
                }
                Err(e) => {
                    tracing::warn!(error = %e, child = %ns.session_id, "failed to enqueue subsession prompt")
                }
            }
        }
    }

    let dto = {
        let views = state.views.read().await;
        match views.sessions.get(&ns.session_id) {
            Some(row) => {
                let mut d = SessionDto::from_row(row);
                d.parent_session_id = Some(id.clone());
                d.fork_type = Some("sub".into());
                d
            }
            None => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "subsession view lookup failed",
                )
                    .into_response()
            }
        }
    };

    let _ = state.deltas.send(ServerFrame::SessionAdded {
        session: dto.clone(),
    });
    (
        StatusCode::CREATED,
        Json(serde_json::to_value(&dto).unwrap()),
    )
        .into_response()
}

/// GET /api/sessions/:id/subsessions — list direct children.
async fn list_subsessions(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let children: Vec<SessionDto> = {
        let views = state.views.read().await;
        views
            .sessions
            .list(&Filters::default())
            .into_iter()
            .filter(|row| row.parent_session_id.as_deref() == Some(id.as_str()))
            .map(SessionDto::from_row)
            .collect()
    };
    Json(json!({ "subsessions": children })).into_response()
}

/// POST /api/sessions/:id/complete — check gate. Only subsessions can complete.
async fn complete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CompleteBody>,
) -> Response {
    let verdict = body.verdict.as_str();
    if verdict != "pass" && verdict != "fail" {
        return (StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_verdict", "message": "verdict must be \"pass\" or \"fail\"" })))
            .into_response();
    }

    let (parent_id, child_hermes_id) = {
        let views = state.views.read().await;
        let Some(child) = views.sessions.get(&id) else {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        };
        let Some(ref parent_id) = child.parent_session_id else {
            return (StatusCode::CONFLICT,
                Json(json!({ "error": "not_a_subsession", "message": "Only subsessions can be completed." })))
                .into_response();
        };
        (parent_id.clone(), child.hermes_id.clone())
    };

    let summary_text = body.summary.as_deref().unwrap_or("");
    let notice = format!("[subsession {id} {verdict}] {summary_text}");

    let parent_hermes_id = {
        let views = state.views.read().await;
        views
            .sessions
            .get(&parent_id)
            .map(|r| r.hermes_id.clone())
            .unwrap_or_default()
    };
    let next_id = {
        let views = state.views.read().await;
        views
            .messages
            .recent(&parent_id, usize::MAX)
            .iter()
            .map(|m| m.message_id)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0)
    };

    match state
        .bridge
        .append_system_message(&parent_id, &parent_hermes_id, next_id, &notice, None)
    {
        Ok(event) => {
            {
                let mut views = state.views.write().await;
                views.apply(&event);
            }
            let dto = crate::server::dto::MessageDto {
                message_id: next_id,
                session_id: parent_id.clone(),
                role: "system".into(),
                content: Some(notice.clone()),
                tool_name: None,
                tool_calls: None,
                reasoning: None,
                timestamp: event_timestamp(&event),
                token_count: None,
                finish_reason: None,
            };
            let _ = state.deltas.send(ServerFrame::MessageAppended {
                session_id: parent_id.clone(),
                message: dto,
            });
        }
        Err(e) => tracing::error!(error = %e, "failed to append complete-gate system message"),
    }

    // Archive the child.
    let archive_event = crate::event::Event::SessionUpdated {
        session_id: id.clone(),
        title: None,
        model: None,
        archived: Some(true),
        message_count: None,
        agent: None,
        node: None,
        hermes_id: Some(child_hermes_id),
        pinned: None,
    };
    if let Err(e) = state.log.append(&archive_event) {
        tracing::warn!(error = %e, "failed to append archive event for completed subsession");
    }
    {
        let mut views = state.views.write().await;
        views.apply(&archive_event);
    }
    let _ = state.deltas.send(ServerFrame::SessionUpdated {
        session_id: id.clone(),
        changes: json!({ "archived": true }),
    });

    Json(json!({ "sessionId": id, "parentId": parent_id, "verdict": verdict, "archived": true }))
        .into_response()
}

/// POST /api/sessions/:id/handover — switch this session to a different agent
/// harness (ADR 0006 §9.1). This is the SOLE mechanism for switching agent kind.
///
/// Creates a new session with the target agent, copies the conversation history
/// (translated to prose context for the target harness), inherits the card_id,
/// archives the source, and emits SessionHandover + SessionForked events.
async fn handover_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<HandoverBody>,
) -> Response {
    let (source, messages) = {
        let views = state.views.read().await;
        let Some(source) = views.sessions.get(&id).cloned() else {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        };
        let messages = views
            .messages
            .recent(&id, usize::MAX)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        (source, messages)
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    let to_kind = crate::adapter::AgentKind::from_agent_str(&body.to_agent_kind);
    let from_kind =
        crate::adapter::AgentKind::from_agent_str(source.agent.as_deref().unwrap_or(""));
    let to_agent_name = match to_kind {
        crate::adapter::AgentKind::Hermes => "hermes".to_string(),
        crate::adapter::AgentKind::ClaudeCode => "claude-code".to_string(),
        crate::adapter::AgentKind::Codex => "codex".to_string(),
    };

    // Create the target session.
    let target_id = format!("oly-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);

    // SessionCreated for the target.
    let created = crate::event::Event::SessionCreated {
        session_id: target_id.clone(),
        hermes_id: String::new(),
        source: "olympus".into(),
        model: body.model.clone().or(source.model.clone()),
        title: source.title.clone(),
        started_at: now,
        message_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        agent: Some(to_agent_name.clone()),
        node: source.node.clone(),
    };
    if let Err(e) = state.log.append(&created) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "log_error", "message": e.to_string() })),
        )
            .into_response();
    }
    {
        let mut views = state.views.write().await;
        views.apply(&created);
    }

    // Translate history: copy messages, and write a context summary for the
    // target harness (the adapter will materialize CLAUDE.md/AGENTS.md/etc).
    for (idx, msg) in messages.iter().enumerate() {
        let _ = state.log.append(&crate::event::Event::MessageAppended {
            session_id: target_id.clone(),
            hermes_session_id: String::new(),
            message_id: idx as u64,
            role: msg.role.clone(),
            content: msg.content.clone(),
            tool_name: msg.tool_name.clone(),
            tool_calls: None,
            reasoning: None,
            timestamp: msg.timestamp,
            token_count: msg.token_count,
            finish_reason: None,
        });
    }
    {
        let mut views = state.views.write().await;
        if let Ok(events) = state.log.read_all() {
            for (_seq, event) in events {
                match &event {
                    crate::event::Event::MessageAppended { session_id, .. }
                        if session_id == &target_id =>
                    {
                        views.apply(&event);
                    }
                    _ => {}
                }
            }
        }
    }

    // Emit SessionHandover (records the transition).
    let handover_event = crate::event::Event::SessionHandover {
        source_session_id: id.clone(),
        target_session_id: target_id.clone(),
        from_agent_kind: format!("{:?}", from_kind),
        to_agent_kind: format!("{:?}", to_kind),
        translated_message_count: messages.len() as u64,
        handed_over_at: now,
    };
    let _ = state.log.append(&handover_event);
    {
        let mut views = state.views.write().await;
        views.apply(&handover_event);
    }

    // Archive the source session.
    let archive = crate::event::Event::SessionUpdated {
        session_id: id.clone(),
        title: None,
        model: None,
        archived: Some(true),
        message_count: None,
        agent: None,
        node: None,
        hermes_id: None,
        pinned: None,
    };
    let _ = state.log.append(&archive);
    {
        let mut views = state.views.write().await;
        views.apply(&archive);
    }

    // Build the DTO for the target session.
    let dto = {
        let views = state.views.read().await;
        match views.sessions.get(&target_id) {
            Some(row) => SessionDto::from_row(row),
            None => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "target session not found after creation",
                )
                    .into_response();
            }
        }
    };

    let _ = state.deltas.send(ServerFrame::SessionAdded {
        session: dto.clone(),
    });

    Json(json!({ "session": dto, "handover": {
        "fromAgentKind": format!("{:?}", from_kind),
        "toAgentKind": format!("{:?}", to_kind),
        "translatedMessages": messages.len(),
    } }))
    .into_response()
}

/// POST /api/sessions — create a new Olympus-managed chat session **optimistically**.
///
/// Returns instantly with the new Session DTO (201). No agent runtime is
/// spawned — the expensive ACP handshake is deferred to the first send
/// (`ensure_runtime`). The session can be assigned an agent/node at creation
/// (via the body) or later via PATCH, any time before the first send.
async fn create_session(
    State(state): State<AppState>,
    body: Option<Json<CreateSessionBody>>,
) -> Response {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let spec = crate::server::bridge_mgr::RuntimeSpec {
        agent: body.agent.clone(),
        node: body.node.clone(),
        cwd: None,
        mcp_servers: vec![],
        env: vec![],
    };
    match state.bridge.create_draft(&spec) {
        Ok(ns) => {
            // Apply the one SessionCreated event directly into the view — do NOT
            // re-scan the whole log (that's O(all events) and made create slow).
            let created = crate::event::Event::SessionCreated {
                session_id: ns.session_id.clone(),
                hermes_id: ns.hermes_id.clone(),
                source: "olympus".into(),
                model: None,
                title: None,
                started_at: ns.started_at,
                message_count: 0,
                input_tokens: 0,
                output_tokens: 0,
                agent: body.agent.clone(),
                node: body.node.clone(),
            };
            let dto = {
                let mut views = state.views.write().await;
                views.apply(&created);
                views
                    .sessions
                    .get(&ns.session_id)
                    .map(SessionDto::from_row)
                    .unwrap_or_else(|| SessionDto {
                        id: ns.session_id.clone(),
                        hermes_id: ns.hermes_id.clone(),
                        org_id: "personal".into(),
                        owner_id: "rpw".into(),
                        context_id: None,
                        source: "olympus".into(),
                        model: None,
                        title: None,
                        started_at: ns.started_at,
                        last_activity: ns.started_at,
                        message_count: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                        archived: false,
                        pinned: false,
                        forked_from: None,
                        fork_point: None,
                        fork_type: None,
                        managed: true,
                        agent: body.agent.clone(),
                        node: body.node.clone(),
                        liveness: "active".to_string(),
                        parent_session_id: None,
                        card_id: None,
                    })
            };

            // A freshly-created managed draft has no in-flight turn yet → idle.
            let mut dto = dto;
            let managed = dto.source == "acp" || dto.source == "olympus";
            dto.liveness = crate::server::dto::compute_liveness(
                dto.last_activity,
                now_epoch(),
                false,
                managed,
                false,
            )
            .to_string();

            let _ = state.deltas.send(ServerFrame::SessionAdded {
                session: dto.clone(),
            });

            (
                StatusCode::CREATED,
                Json(serde_json::to_value(&dto).unwrap()),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "bridge create_draft failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "bridge_error",
                    "message": format!("Failed to create session: {e:#}"),
                })),
            )
                .into_response()
        }
    }
}

/// PATCH /api/sessions/:id — bind/rebind agent, node, model, or title.
///
/// Appends a `SessionUpdated` event and broadcasts the change. Intended to be
/// called before the first send (the typical optimistic-create flow: create
/// instantly, pick agent/model, then send). Rebinding the agent after a runtime
/// has spawned takes effect on the next runtime (not yet hot-swapped).
async fn patch_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<PatchSessionBody>>,
) -> Response {
    let body = body.map(|Json(b)| b).unwrap_or_default();

    // The session must exist. Runtime rebinds (agent/node/model) are managed-
    // only; pin/archive/title are metadata and work on ANY session (observed
    // sessions can be pinned or archived without being steerable).
    {
        let views = state.views.read().await;
        let Some(row) = views.sessions.get(&id) else {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        };
        let wants_rebind = body.agent.is_some() || body.node.is_some() || body.model.is_some();
        if wants_rebind && !(row.source == "olympus" || row.source == "acp") {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "observed",
                    "message": "Observed sessions can't be reassigned. Fork it first.",
                })),
            )
                .into_response();
        }
    }

    let event = crate::event::Event::SessionUpdated {
        session_id: id.clone(),
        title: body.title.clone(),
        model: body.model.clone(),
        archived: body.archived,
        message_count: None,
        agent: body.agent.clone(),
        node: body.node.clone(),
        hermes_id: None,
        pinned: body.pinned,
    };
    if let Err(e) = state.log.append(&event) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "log_error", "message": format!("{e:#}") })),
        )
            .into_response();
    }

    let dto = {
        let mut views = state.views.write().await;
        views.apply(&event);
        views.sessions.get(&id).map(SessionDto::from_row)
    };

    let mut changes = serde_json::Map::new();
    if let Some(a) = &body.agent {
        changes.insert("agent".into(), serde_json::Value::String(a.clone()));
    }
    if let Some(n) = &body.node {
        changes.insert("node".into(), serde_json::Value::String(n.clone()));
    }
    if let Some(m) = &body.model {
        changes.insert("model".into(), serde_json::Value::String(m.clone()));
    }
    if let Some(t) = &body.title {
        changes.insert("title".into(), serde_json::Value::String(t.clone()));
    }
    if let Some(a) = body.archived {
        changes.insert("archived".into(), serde_json::Value::Bool(a));
    }
    if let Some(p) = body.pinned {
        changes.insert("pinned".into(), serde_json::Value::Bool(p));
    }
    let _ = state.deltas.send(ServerFrame::SessionUpdated {
        session_id: id.clone(),
        changes: serde_json::Value::Object(changes),
    });

    match dto {
        Some(dto) => Json(serde_json::to_value(&dto).unwrap()).into_response(),
        None => (StatusCode::NOT_FOUND, "session not found").into_response(),
    }
}

/// POST /api/sessions/:id/fork — fork an observed session into Olympus.
async fn fork_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ForkSessionBody>,
) -> Response {
    let (source, messages) = {
        let views = state.views.read().await;
        let Some(source) = views.sessions.get(&id).cloned() else {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        };
        let messages = views
            .messages
            .recent(&id, usize::MAX)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        (source, messages)
    };

    let fork_type = body.fork_type.unwrap_or_else(|| "sub".to_string());
    let fork = match state
        .bridge
        .fork_session(
            &source.hermes_id,
            source.model.clone(),
            source.title.clone(),
            messages.len() as u64,
        )
        .await
    {
        Ok(fork) => fork,
        Err(e) => {
            tracing::error!(error = %e, source_session = %id, "bridge fork_session failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "bridge_error",
                    "message": format!("Failed to fork agent session: {e:#}"),
                })),
            )
                .into_response();
        }
    };

    for (idx, msg) in messages.iter().enumerate() {
        if let Err(e) = state.log.append(&crate::event::Event::MessageAppended {
            session_id: fork.session_id.clone(),
            hermes_session_id: fork.hermes_id.clone(),
            message_id: idx as u64,
            role: msg.role.clone(),
            content: msg.content.clone(),
            tool_name: msg.tool_name.clone(),
            tool_calls: None,
            reasoning: None,
            timestamp: msg.timestamp,
            token_count: msg.token_count,
            finish_reason: None,
        }) {
            tracing::warn!(error = %e, fork_session = %fork.session_id, "failed to append forked message");
        }
    }

    let mut dto = {
        let mut views = state.views.write().await;
        if let Ok(events) = state.log.read_all() {
            for (_seq, event) in events {
                match &event {
                    crate::event::Event::SessionCreated { session_id, .. }
                    | crate::event::Event::MessageAppended { session_id, .. }
                    | crate::event::Event::SessionUpdated { session_id, .. }
                        if session_id == &fork.session_id =>
                    {
                        views.apply(&event);
                    }
                    _ => {}
                }
            }
        }
        match views.sessions.get(&fork.session_id) {
            Some(row) => SessionDto::from_row(row),
            None => SessionDto {
                id: fork.session_id.clone(),
                hermes_id: fork.hermes_id.clone(),
                org_id: "personal".into(),
                owner_id: "rpw".into(),
                context_id: None,
                source: "olympus".into(),
                model: source.model.clone(),
                title: source.title.clone(),
                started_at: 0.0,
                last_activity: 0.0,
                message_count: messages.len() as u64,
                input_tokens: 0,
                output_tokens: 0,
                archived: false,
                pinned: false,
                forked_from: None,
                fork_point: None,
                fork_type: None,
                managed: true,
                agent: None,
                node: None,
                liveness: "active".to_string(),
                parent_session_id: None,
                card_id: None,
            },
        }
    };
    dto.forked_from = Some(id.clone());
    dto.fork_type = Some(fork_type.clone());
    dto.parent_session_id = Some(id.clone());

    // Emit SessionForked so the session tree is durable (ADR 0006 §7 footgun 3).
    // The child inherits the parent's card_id (if any) via the view projection.
    let forked_event = crate::event::Event::SessionForked {
        parent_session_id: id.clone(),
        child_session_id: fork.session_id.clone(),
        fork_type: fork_type.clone(),
        fork_point: None,
        forked_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
    };
    if let Err(e) = state.log.append(&forked_event) {
        tracing::warn!(error = %e, "failed to append SessionForked event");
    }
    {
        let mut views = state.views.write().await;
        views.apply(&forked_event);
    }
    // Re-read the child row to get the projected card_id (inherited from parent).
    if let Some(child_row) = {
        let views = state.views.read().await;
        views.sessions.get(&fork.session_id).cloned()
    } {
        dto.card_id = child_row.card_id.clone();
    }

    let _ = state.deltas.send(ServerFrame::SessionAdded {
        session: dto.clone(),
    });

    Json(json!({ "session": dto })).into_response()
}

/// POST a message to drive a session.
///
/// Only MANAGED (olympus/acp-source) sessions are steerable. Observed sessions
/// (imported telegram/cli/etc.) return 409 — the UI must FORK them into an
/// olympus-owned session first (cross-channel continuation, ADR §6.6).
///
/// For managed sessions the prompt is sent to the agent runtime and the response
/// is streamed over /ws as message.delta / message.done frames. Returns 202.
async fn post_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PostMessageBody>,
) -> Response {
    let (managed, hermes_id, agent, node) = {
        let views = state.views.read().await;
        let Some(session) = views.sessions.get(&id) else {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        };
        let managed = session.source == "olympus" || session.source == "acp";
        (
            managed,
            session.hermes_id.clone(),
            session.agent.clone(),
            session.node.clone(),
        )
    };

    if !managed {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "observed",
                "message": "This session is observed (read-only). Fork it into an Olympus-managed session to continue.",
            })),
        )
            .into_response();
    }

    // Record the user message in the log + views + broadcast IMMEDIATELY, before
    // any (potentially slow) runtime spawn — so the UI shows the user bubble and
    // the POST returns fast. `hermes_id` may be empty here for a fresh draft; the
    // user message carries the current (possibly empty) hermes id and is fine.
    // Use max(existing message_id)+1, NOT the count — message ids must be
    // monotonic and collision-free even if the hot window evicted older rows or
    // ids aren't contiguous (a count would reuse an id and clobber a message).
    let next_id = {
        let views = state.views.read().await;
        views
            .messages
            .recent(&id, usize::MAX)
            .iter()
            .map(|m| m.message_id)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0)
    };
    match state
        .bridge
        .append_user_message(&id, &hermes_id, next_id, &body.text)
    {
        Ok(event) => {
            {
                let mut views = state.views.write().await;
                views.apply(&event);
            }
            let dto = crate::server::dto::MessageDto {
                message_id: next_id,
                session_id: id.clone(),
                role: "user".into(),
                content: Some(body.text.clone()),
                tool_name: None,
                tool_calls: None,
                reasoning: None,
                timestamp: event_timestamp(&event),
                token_count: None,
                finish_reason: None,
            };
            let _ = state.deltas.send(ServerFrame::MessageAppended {
                session_id: id.clone(),
                message: dto,
            });
        }
        Err(e) => tracing::warn!(error = %e, "failed to append user message"),
    }

    // Derive a session title from the first user message when the session has
    // none — otherwise API/UI-created sessions show "Untitled" forever. Cheap
    // heuristic: first line, trimmed to ~60 chars (no LLM round-trip needed).
    if next_id == 0 {
        let needs_title = {
            let views = state.views.read().await;
            views
                .sessions
                .get(&id)
                .map(|s| s.title.as_deref().unwrap_or("").trim().is_empty())
                .unwrap_or(true)
        };
        if needs_title {
            let derived = derive_title(&body.text);
            if !derived.is_empty() {
                if let Ok(event) = state.bridge.set_title(&id, &derived) {
                    {
                        let mut views = state.views.write().await;
                        views.apply(&event);
                    }
                    let _ = state.deltas.send(ServerFrame::SessionUpdated {
                        session_id: id.clone(),
                        changes: serde_json::json!({ "title": derived }),
                    });
                }
            }
        }
    }

    // Mark in-flight up front so liveness shows "active" the instant the POST
    // returns (the runtime spawn + turn happen in the background task below).
    state.bridge.mark_in_flight(&id).await;
    // Broadcast liveness so other tabs/windows watching this session flip to
    // the thinking state immediately (and refreshes rehydrate from GET).
    let _ = state.deltas.send(ServerFrame::SessionUpdated {
        session_id: id.clone(),
        changes: serde_json::json!({ "liveness": "running" }),
    });

    // Everything expensive — lazily spawning/resuming the agent runtime, sending
    // the prompt, and draining the event stream — happens OFF the request path
    // so POST returns ~instantly (the ACP handshake can take seconds). The UI
    // shows the user message + "active" immediately and the reply streams over WS.
    let session_id = id.clone();
    let deltas = state.deltas.clone();
    let bridge = state.bridge.clone();
    let views = state.views.clone();
    let envoy_conns = state.envoy_conns.clone();
    // Bind the agent to its session space (working directory). The space was
    // materialized eagerly at create time; derive its path here so the lazily
    // spawned runtime runs scoped to it, not the host cwd.
    let cwd = state
        .bridge
        .space_path(&id)
        .map(|p| p.to_string_lossy().into_owned());

    // --- ADR 0006 §9.3: resolve the effective setup for this session's
    // org/project scope, then materialize via the Hermes adapter. ---
    // This is where the declaration manifest + registry become REAL: MCP
    // servers resolved from registry definitions get injected into the ACP
    // session/new, skills get symlinked into the session space, and env vars
    // (HERMES_SKILLS_PATH) are set on the child.
    let org_slug = std::env::var("OLYMPUS_DEFAULT_ORG").unwrap_or_else(|_| "default".to_string());
    let (mcp_servers, env_vars, adapter_warnings) = {
        let views = state.views.read().await;
        // Get the effective (merged org+project) setup. For now, no project
        // scoping — just org-level. TODO: wire project from session metadata.
        let effective = views.setup.effective_for_project(&org_slug, "");
        let resolved = crate::adapter::ResolvedSetup::from_registry(
            &views.registry,
            &effective.skills,
            &effective.mcp,
            &effective.plugins,
            &effective.hooks,
        );
        // Materialize into the session space if we have one.
        let agent_kind = crate::adapter::AgentKind::from_agent_str(agent.as_deref().unwrap_or(""));
        let adapter = crate::adapter::for_kind(agent_kind);
        if let Some(ref space_path) = cwd {
            match adapter.materialize(
                &resolved,
                std::path::Path::new(space_path),
                crate::adapter::MergeMode::Union,
            ) {
                Ok(overlay) => (overlay.mcp_servers, overlay.env, overlay.warnings),
                Err(e) => {
                    tracing::warn!(error = %e, session = %id, "adapter materialize failed; spawning with empty setup");
                    (vec![], vec![], vec![format!("adapter failed: {e:#}")])
                }
            }
        } else {
            (
                vec![],
                vec![],
                vec!["no session space; skipping adapter".into()],
            )
        }
    };
    if !adapter_warnings.is_empty() {
        for w in &adapter_warnings {
            tracing::info!(session = %id, warning = %w, "adapter warning");
            let _ = deltas.send(ServerFrame::SessionLog {
                session_id: id.clone(),
                level: "warn".into(),
                source: "adapter".into(),
                message: w.clone(),
                timestamp: crate::server::bridge_mgr::chrono_epoch_pub(),
            });
        }
    }

    let spec = crate::server::bridge_mgr::RuntimeSpec {
        agent,
        node,
        cwd,
        mcp_servers,
        env: env_vars,
    };
    let resume_hermes = if hermes_id.is_empty() {
        None
    } else {
        Some(hermes_id.clone())
    };
    let prompt_text = body.text.clone();
    let prompt_thinking = body
        .thinking
        .clone()
        .filter(|t| matches!(t.as_str(), "low" | "medium" | "high"));
    let prompt_model = body.model.clone();
    let assistant_seed_id = next_id + 1;
    let log_deltas = deltas.clone();
    let log_session_id = session_id.clone();
    let log_agent = spec.agent.clone().unwrap_or_default();
    let log_resume = resume_hermes.clone();
    tokio::spawn(async move {
        use futures::stream::StreamExt;

        // Emit structured log events for the Logs panel.
        let emit_log = |level: &str, source: &str, msg: &str| {
            let _ = log_deltas.send(ServerFrame::SessionLog {
                session_id: log_session_id.clone(),
                level: level.into(),
                source: source.into(),
                message: msg.into(),
                timestamp: crate::server::bridge_mgr::chrono_epoch_pub(),
            });
        };

        if log_resume.is_some() {
            emit_log(
                "info",
                "bridge",
                &format!("Resuming agent runtime ({})…", log_agent),
            );
        } else {
            emit_log(
                "info",
                "bridge",
                &format!("Starting new agent runtime ({})…", log_agent),
            );
        }

        // Lazily ensure a runtime (spawn for a fresh draft, resume by hermes_id
        // after a restart). This is the slow part — now off the request path.
        //
        // ADR 0008 S6 cutover: route to a connected envoy (RemoteRuntime) when
        // the session's node has an active UDS connection. This replaces the
        // in-process bridge for production — the local node is now
        // olympus-envoy@1 over UDS, not an in-process pseudo-envoy. Tests that
        // build AppState with no connected envoys fall back to the in-process
        // bridge (mock factory), so existing tests keep working unchanged.
        let node_id = spec.node.clone().unwrap_or_default();
        // If the session has no explicit node, route to the first connected
        // envoy (default for the single-operator case). Sessions with an
        // explicit node route to that specific envoy.
        let route_node = if node_id.is_empty() {
            envoy_conns.first_node().await.unwrap_or_default()
        } else {
            node_id
        };
        let conn = envoy_conns.get(&route_node).await;
        let (runtime, captured_hermes_id) = if let Some(conn) = conn {
            // Route to the connected envoy via RemoteRuntime.
            let rt = crate::server::envoy_conn::RemoteRuntime::arc_with_spec(
                conn,
                session_id.clone(),
                spec.clone(),
            );
            emit_log(
                "info",
                "bridge",
                &format!("Routing to envoy {}…", route_node),
            );
            match rt.start(resume_hermes.as_deref()).await {
                Ok(()) => {
                    let hid = rt.hermes_session_id().await.unwrap_or_default();
                    emit_log("info", "bridge", "Agent runtime ready (envoy)");
                    (rt, hid)
                }
                Err(e) => {
                    tracing::error!(error = %e, session = %session_id, "envoy ensure_runtime failed");
                    let err_msg = format!("⚠ Failed to start agent: {e:#}");
                    let hid = resume_hermes.clone().unwrap_or_default();
                    if let Ok(event) = bridge.append_system_message(
                        &session_id,
                        &hid,
                        assistant_seed_id,
                        &err_msg,
                        Some("error"),
                    ) {
                        {
                            let mut v = views.write().await;
                            v.apply(&event);
                        }
                        let _ = deltas.send(ServerFrame::MessageAppended {
                            session_id: session_id.clone(),
                            message: crate::server::dto::MessageDto {
                                message_id: assistant_seed_id,
                                session_id: session_id.clone(),
                                role: "system".into(),
                                content: Some(err_msg.clone()),
                                tool_name: None,
                                tool_calls: None,
                                reasoning: None,
                                timestamp: crate::server::bridge_mgr::chrono_epoch_pub(),
                                token_count: None,
                                finish_reason: Some("error".into()),
                            },
                        });
                    }
                    let _ = deltas.send(ServerFrame::MessageDone {
                        session_id: session_id.clone(),
                        message_id: assistant_seed_id,
                        finish_reason: Some(format!("error: failed to start agent: {e:#}")),
                    });
                    bridge.clear_in_flight(&session_id).await;
                    return;
                }
            }
        } else {
            // No connected envoy for this node — fall back to the in-process
            // bridge (tests, or a legacy deployment without an envoy service).
            match bridge
                .ensure_runtime(&session_id, &spec, resume_hermes.as_deref())
                .await
            {
                Ok(pair) => {
                    emit_log("info", "bridge", "Agent runtime ready");
                    pair
                }
                Err(e) => {
                    tracing::error!(error = %e, session = %session_id, "ensure_runtime failed");
                    // PERSIST the error as a system message so the user sees it in
                    // the transcript — the old code only broadcast a transient WS
                    // frame, so if the user wasn't watching it vanished silently.
                    let err_msg = format!("⚠ Failed to start agent: {e:#}");
                    let hid = resume_hermes.clone().unwrap_or_default();
                    if let Ok(event) = bridge.append_system_message(
                        &session_id,
                        &hid,
                        assistant_seed_id,
                        &err_msg,
                        Some("error"),
                    ) {
                        {
                            let mut v = views.write().await;
                            v.apply(&event);
                        }
                        let _ = deltas.send(ServerFrame::MessageAppended {
                            session_id: session_id.clone(),
                            message: crate::server::dto::MessageDto {
                                message_id: assistant_seed_id,
                                session_id: session_id.clone(),
                                role: "system".into(),
                                content: Some(err_msg.clone()),
                                tool_name: None,
                                tool_calls: None,
                                reasoning: None,
                                timestamp: crate::server::bridge_mgr::chrono_epoch_pub(),
                                token_count: None,
                                finish_reason: Some("error".into()),
                            },
                        });
                    }
                    let _ = deltas.send(ServerFrame::MessageDone {
                        session_id: session_id.clone(),
                        message_id: assistant_seed_id,
                        finish_reason: Some(format!("error: failed to start agent: {e:#}")),
                    });
                    bridge.clear_in_flight(&session_id).await;
                    return;
                }
            }
        };

        // Backfill the captured Hermes id onto the session row (draft → live).
        let hermes_id_clone = if resume_hermes.is_none() && !captured_hermes_id.is_empty() {
            let _ = bridge.backfill_hermes_id(&session_id, &captured_hermes_id);
            let mut v = views.write().await;
            v.apply(&crate::event::Event::SessionUpdated {
                session_id: session_id.clone(),
                title: None,
                model: None,
                archived: None,
                message_count: None,
                agent: None,
                node: None,
                hermes_id: Some(captured_hermes_id.clone()),
                pinned: None,
            });
            captured_hermes_id
        } else {
            resume_hermes.unwrap_or(captured_hermes_id)
        };

        let mut stream = runtime.events();
        let mut assistant_text = String::new();
        let assistant_msg_id = assistant_seed_id;
        // Accumulate structured tool calls seen this turn so they're persisted
        // on the assistant message (and surface in the transcript's tool UI).
        let mut tool_calls_acc: Vec<serde_json::Value> = Vec::new();

        // If a thinking level was requested, prepend it as a /thinking slash
        // command on the first line of the prompt text. Hermes processes
        // slash commands at the start of a multi-line prompt, setting the
        // session's reasoning effort for the current turn. (Sending /thinking
        // as a separate ACP turn doesn't work — it's a CLI command, not an
        // ACP primitive.)
        let final_prompt = if let Some(ref level) = prompt_thinking {
            emit_log("info", "bridge", &format!("Thinking level: {level}"));
            format!("/thinking {level}\n{prompt_text}")
        } else {
            prompt_text
        };

        // Subscribe before sending the prompt so fast runtimes cannot emit and
        // finish the whole turn before the drain loop is listening.
        emit_log("info", "bridge", "Sending prompt to agent…");
        if let Err(e) = runtime
            .send(AgentCommand::Prompt {
                text: final_prompt,
                model: prompt_model,
            })
            .await
        {
            tracing::error!(error = %e, session = %session_id, "prompt send failed");
            emit_log("error", "bridge", &format!("Prompt send failed: {e:#}"));
            let _ = deltas.send(ServerFrame::MessageDone {
                session_id: session_id.clone(),
                message_id: assistant_seed_id,
                finish_reason: Some(format!("error: {e:#}")),
            });
            bridge.clear_in_flight(&session_id).await;
            return;
        }
        let mut terminal_event_seen = false;
        // While a steer-ack is being consumed (its Text + Done), suppress the
        // ack text so it doesn't pollute the assistant reply. The ack is the
        // adapter's "⏩ Steer queued for the active turn: …" string — useful
        // in a CLI but noise in the transcript.
        let mut suppressing_steer_ack = false;

        while let Some(event) = stream.next().await {
            #[allow(unreachable_patterns)]
            match event {
                AgentEvent::Text(chunk) => {
                    if suppressing_steer_ack {
                        // Drop the ack text; the real reply comes after.
                        continue;
                    }
                    // Detect the start of a steer ack and begin suppressing.
                    // The adapter emits this exact prefix in _cmd_steer.
                    if chunk.starts_with("⏩ Steer queued")
                        || chunk.starts_with("⚠️ Steer failed")
                        || chunk.starts_with("No active turn — queued")
                    {
                        suppressing_steer_ack = true;
                        continue;
                    }
                    assistant_text.push_str(&chunk);
                    let _ = deltas.send(ServerFrame::MessageDelta {
                        session_id: session_id.clone(),
                        message_id: assistant_msg_id,
                        text_delta: chunk,
                    });
                }
                AgentEvent::ToolCall {
                    id,
                    name,
                    args,
                    status,
                    result,
                } => {
                    // Two shapes arrive here:
                    //  - `tool_call` (new invocation): has name+args, status
                    //    "pending" (queued/awaiting permission) or "in_progress".
                    //  - `tool_call_update`: status transition and/or result.
                    // Match updates to their originating call by ACP toolCallId;
                    // fall back to "most recent entry without a result" when the
                    // id is missing (some adapters omit it on updates).
                    let is_update = args.is_empty() && !tool_calls_acc.is_empty() && {
                        // An update either carries a known id or has no args.
                        id.as_deref().is_none_or(|i| {
                            tool_calls_acc
                                .iter()
                                .any(|tc| tc.get("id").and_then(|v| v.as_str()) == Some(i))
                        })
                    };
                    if is_update {
                        let idx = tool_calls_acc
                            .iter()
                            .rposition(|tc| match id.as_deref() {
                                Some(i) => tc.get("id").and_then(|v| v.as_str()) == Some(i),
                                None => tc.get("result").is_none(),
                            })
                            .or_else(|| {
                                tool_calls_acc
                                    .iter()
                                    .rposition(|tc| tc.get("result").is_none())
                            });
                        if let Some(idx) = idx {
                            let tc = &mut tool_calls_acc[idx];
                            if let Some(s) = &status {
                                tc["status"] = serde_json::json!(s);
                            }
                            if let Some(r) = &result {
                                tc["result"] = serde_json::json!(r);
                            }
                            if !name.is_empty() {
                                tc["name"] = serde_json::json!(name);
                            }
                            // Stream the updated card (full state) so the UI
                            // patches it in place — chronological position is
                            // preserved because the UI matches by id.
                            let dto = crate::server::dto::ToolCallDto {
                                id: tc.get("id").and_then(|v| v.as_str()).map(String::from),
                                name: tc
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("tool")
                                    .to_string(),
                                args: tc.get("args").cloned().unwrap_or(serde_json::json!({})),
                                label: None,
                                status: tc.get("status").and_then(|v| v.as_str()).map(String::from),
                                result: tc.get("result").and_then(|v| v.as_str()).map(String::from),
                            };
                            let _ = deltas.send(ServerFrame::MessageToolCall {
                                session_id: session_id.clone(),
                                message_id: assistant_msg_id,
                                tool_call: dto,
                            });
                        }
                    } else {
                        // New tool call — record and stream with its status.
                        // `anchor` = codepoint offset into the assistant text at
                        // the moment the call fired; the UI uses it to interleave
                        // the card chronologically inside the final message.
                        let parsed_args = serde_json::from_str::<serde_json::Value>(&args)
                            .unwrap_or(serde_json::json!({ "raw": args }));
                        let status_str = status.clone().unwrap_or_else(|| "pending".into());
                        let mut entry = serde_json::json!({
                            "name": name,
                            "args": parsed_args,
                            "status": status_str,
                            "anchor": assistant_text.chars().count(),
                        });
                        if let Some(i) = &id {
                            entry["id"] = serde_json::json!(i);
                        }
                        if let Some(r) = &result {
                            entry["result"] = serde_json::json!(r);
                        }
                        tool_calls_acc.push(entry);
                        emit_log("info", "agent", &format!("Tool call: {}", name));
                        let _ = deltas.send(ServerFrame::MessageToolCall {
                            session_id: session_id.clone(),
                            message_id: assistant_msg_id,
                            tool_call: crate::server::dto::ToolCallDto {
                                id: id.clone(),
                                name: name.clone(),
                                args: parsed_args,
                                label: None,
                                status: status.clone(),
                                result: result.clone(),
                            },
                        });
                    }
                }
                AgentEvent::AwaitingInput { .. } => {
                    emit_log("warn", "agent", "Awaiting permission decision…");
                }
                AgentEvent::Reasoning(delta) => {
                    // Stream reasoning/CoT chunks live alongside text + tool calls.
                    let _ = deltas.send(ServerFrame::MessageReasoning {
                        session_id: session_id.clone(),
                        message_id: assistant_msg_id,
                        text_delta: delta,
                    });
                }
                AgentEvent::Text(_) => {} // streamed to the chat bubble, not logs
                AgentEvent::Done { finish_reason } => {
                    // Skip the Done ack from a /steer slash command. The Hermes
                    // adapter processes /steer as a slash command that returns
                    // immediately with stop_reason="end_turn", which would
                    // terminate the original turn early. Only the genuine
                    // end-of-turn Done should break the drain loop.
                    if bridge.take_steer_pending(&session_id).await {
                        tracing::debug!(session = %session_id, "skipped steer-ack Done");
                        suppressing_steer_ack = false; // resume normal text capture
                                                       // Broadcast delivery status so the steer bubble's
                                                       // badge flips from 'pending' to 'delivered'.
                        let _ = deltas.send(ServerFrame::SessionLog {
                            session_id: session_id.clone(),
                            level: "info".into(),
                            source: "bridge".into(),
                            message: "steer.delivered".into(),
                            timestamp: crate::server::bridge_mgr::chrono_epoch_pub(),
                        });
                        continue;
                    }
                    terminal_event_seen = true;
                    emit_log(
                        "info",
                        "agent",
                        &format!(
                            "Turn finished: {}",
                            finish_reason.as_deref().unwrap_or("end_turn")
                        ),
                    );
                    let _ = deltas.send(ServerFrame::MessageDone {
                        session_id: session_id.clone(),
                        message_id: assistant_msg_id,
                        finish_reason: finish_reason.clone(),
                    });
                    let tool_calls_json = if tool_calls_acc.is_empty() {
                        None
                    } else {
                        Some(serde_json::Value::Array(tool_calls_acc.clone()).to_string())
                    };
                    // Persist the final assistant message AND apply it to the
                    // views so a subsequent GET /messages reflects it. Also
                    // broadcast a message.appended frame so the streaming UI
                    // replaces the in-flight bubble with the final message (the
                    // streamingText is cleared on message.done, but the final
                    // message only enters the list via this append frame).
                    if let Ok(event) = bridge.append_assistant_message(
                        &session_id,
                        &hermes_id_clone,
                        assistant_msg_id,
                        &assistant_text,
                        &tool_calls_json,
                        finish_reason.as_deref(),
                    ) {
                        {
                            let mut v = views.write().await;
                            v.apply(&event);
                        }
                        let dto = crate::server::dto::MessageDto {
                            message_id: assistant_msg_id,
                            session_id: session_id.clone(),
                            role: "assistant".into(),
                            content: Some(assistant_text.clone()),
                            tool_name: None,
                            tool_calls: tool_calls_json
                                .as_deref()
                                .and_then(crate::server::dto::parse_tool_calls),
                            reasoning: None,
                            timestamp: event_timestamp(&event),
                            token_count: None,
                            finish_reason: finish_reason.clone(),
                        };
                        let _ = deltas.send(ServerFrame::MessageAppended {
                            session_id: session_id.clone(),
                            message: dto,
                        });
                    }
                    break;
                }
                AgentEvent::Error(e) => {
                    terminal_event_seen = true;
                    tracing::warn!(error = %e, session = %session_id, "agent error event");
                    emit_log("error", "agent", &e);
                    let content = format!("⚠ agent error: {e:#}");
                    let finish_reason = format!("error: {e:#}");
                    if let Ok(event) = bridge.append_system_message(
                        &session_id,
                        &hermes_id_clone,
                        assistant_msg_id,
                        &content,
                        Some(&finish_reason),
                    ) {
                        {
                            let mut v = views.write().await;
                            v.apply(&event);
                        }
                        let dto = crate::server::dto::MessageDto {
                            message_id: assistant_msg_id,
                            session_id: session_id.clone(),
                            role: "system".into(),
                            content: Some(content),
                            tool_name: None,
                            tool_calls: None,
                            reasoning: None,
                            timestamp: event_timestamp(&event),
                            token_count: None,
                            finish_reason: Some(finish_reason.clone()),
                        };
                        let _ = deltas.send(ServerFrame::MessageAppended {
                            session_id: session_id.clone(),
                            message: dto,
                        });
                    }
                    let _ = deltas.send(ServerFrame::MessageDone {
                        session_id: session_id.clone(),
                        message_id: assistant_msg_id,
                        finish_reason: Some(finish_reason),
                    });
                    break;
                }
                AgentEvent::ToolCall {
                    id,
                    name,
                    args,
                    status,
                    result,
                } => {
                    // Accumulate so the final assistant message carries its tool
                    // calls (rendered in the transcript's tool UI).
                    let mut entry = serde_json::json!({
                        "name": name,
                        "args": serde_json::from_str::<serde_json::Value>(&args)
                            .unwrap_or(serde_json::Value::String(args.clone())),
                        "result": result,
                    });
                    if let Some(i) = &id {
                        entry["id"] = serde_json::json!(i);
                    }
                    if let Some(s) = &status {
                        entry["status"] = serde_json::json!(s);
                    }
                    tool_calls_acc.push(entry);
                }
                AgentEvent::Reasoning(_) => {
                    // Accumulate silently for now; reasoning rendering is separate.
                }
                AgentEvent::AwaitingInput {
                    request_id,
                    tool_call,
                    options,
                } => {
                    // The agent is blocked on a permission decision. Record the
                    // pending request (so /permission can answer it) and flip
                    // liveness to "input-required" via the awaiting set. Do NOT
                    // end the turn — the stream continues once the client
                    // responds and the agent resumes the tool call.
                    bridge.mark_awaiting_input(&session_id, &request_id).await;
                    let options_json = serde_json::Value::Array(
                        options
                            .iter()
                            .map(|o| {
                                serde_json::json!({
                                    "optionId": o.option_id,
                                    "name": o.name,
                                    "kind": o.kind,
                                })
                            })
                            .collect(),
                    );
                    let _ = deltas.send(ServerFrame::PermissionRequired {
                        session_id: session_id.clone(),
                        tool_call,
                        options: options_json,
                    });
                    // Also nudge the session list so the row shows input-required.
                    let _ = deltas.send(ServerFrame::SessionUpdated {
                        session_id: session_id.clone(),
                        changes: serde_json::json!({ "liveness": "input-required" }),
                    });
                }
            }
            // NOTE: assistant_msg_id must NOT increment per event. One prompt
            // produces one assistant message (text + accumulated tool calls),
            // persisted once on Done at assistant_seed_id. The old per-iteration
            // increment inflated the id on any turn with a tool call, colliding
            // with the next turn's user-message id and dropping/clobbering the
            // assistant reply (the multi-turn "no response" bug).
        }
        if !terminal_event_seen {
            tracing::warn!(session = %session_id, "agent stream closed without terminal event");
            let content = "⚠ agent stream closed unexpectedly".to_string();
            let finish_reason = "error: agent stream closed unexpectedly".to_string();
            if let Ok(event) = bridge.append_system_message(
                &session_id,
                &hermes_id_clone,
                assistant_msg_id,
                &content,
                Some(&finish_reason),
            ) {
                {
                    let mut v = views.write().await;
                    v.apply(&event);
                }
                let dto = crate::server::dto::MessageDto {
                    message_id: assistant_msg_id,
                    session_id: session_id.clone(),
                    role: "system".into(),
                    content: Some(content),
                    tool_name: None,
                    tool_calls: None,
                    reasoning: None,
                    timestamp: event_timestamp(&event),
                    token_count: None,
                    finish_reason: Some(finish_reason.clone()),
                };
                let _ = deltas.send(ServerFrame::MessageAppended {
                    session_id: session_id.clone(),
                    message: dto,
                });
            }
            let _ = deltas.send(ServerFrame::MessageDone {
                session_id: session_id.clone(),
                message_id: assistant_msg_id,
                finish_reason: Some(finish_reason),
            });
        }
        // Turn finished (Done, Error, or stream closed): clear the in-flight flag
        // so liveness drops back to idle. Also clear any dangling awaiting-input
        // flag (e.g. the turn was cancelled while a permission was pending).
        bridge.clear_in_flight(&session_id).await;
        bridge.clear_awaiting_input(&session_id).await;
        // Broadcast liveness=idle so the UI drops the thinking indicator
        // immediately and refreshes see idle (not stale 'running').
        let _ = deltas.send(ServerFrame::SessionUpdated {
            session_id: session_id.clone(),
            changes: serde_json::json!({ "liveness": "idle" }),
        });
    });

    (StatusCode::ACCEPTED, Json(json!({ "accepted": true }))).into_response()
}

/// POST /api/sessions/:id/cancel — stop the in-flight turn for a managed
/// session. Sends AgentCommand::Cancel to the runtime (ACP session/cancel) and
/// clears the in-flight flag. No-op (still 200) if there's no active runtime.
async fn cancel_session(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if let Some(runtime) = state.bridge.get_runtime(&id).await {
        if let Err(e) = runtime.send(AgentCommand::Cancel).await {
            tracing::warn!(error = %e, session = %id, "cancel send failed");
        }
    }
    state.bridge.clear_in_flight(&id).await;
    // Tell subscribers the turn is no longer running so the UI drops the
    // thinking indicator immediately.
    let _ = state.deltas.send(ServerFrame::SessionUpdated {
        session_id: id.clone(),
        changes: json!({ "liveness": "idle" }),
    });
    (StatusCode::OK, Json(json!({ "cancelled": true }))).into_response()
}

/// Body for POST /api/sessions/:id/steer — inject guidance into a RUNNING turn.
#[derive(Debug, Deserialize)]
struct SteerBody {
    text: String,
}

/// POST /api/sessions/:id/steer — steer the in-flight turn without stopping it.
/// Maps to the Hermes /steer command (AgentCommand::Steer). 409 when no turn
/// is running — steering an idle session is a normal message, use POST
/// /messages instead.
async fn steer_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SteerBody>,
) -> Response {
    let text = body.text.trim().to_string();
    if text.is_empty() {
        return (StatusCode::BAD_REQUEST, "steer text is required").into_response();
    }
    if !state.bridge.in_flight_set().await.contains(&id) {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "not_running",
                "message": "No turn in flight — send a normal message instead.",
            })),
        )
            .into_response();
    }
    let Some(runtime) = state.bridge.get_runtime(&id).await else {
        return (StatusCode::CONFLICT, "no runtime for session").into_response();
    };
    // Mark that a steer ack is pending BEFORE sending it — the drain loop in
    // post_message is concurrently consuming the shared ACP event stream and
    // must skip the `Done` this steer produces (the Hermes adapter returns
    // end_turn for the /steer slash-command ack, which would otherwise
    // terminate the original turn early with an empty reply).
    state.bridge.mark_steer_pending(&id).await;
    if let Err(e) = runtime
        .send(AgentCommand::Steer { text: text.clone() })
        .await
    {
        tracing::warn!(error = %e, session = %id, "steer send failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "steer_failed", "message": e.to_string() })),
        )
            .into_response();
    }
    // Persist the steer as a user message with finish_reason="steer" so the
    // transcript shows it as a distinct bubble (an interrupt, not a new turn).
    // Use max(message_id)+1 to avoid colliding with the in-flight assistant
    // message ID (count+1 can collide after window eviction).
    let (hermes_id, steer_msg_id) = {
        let v = state.views.read().await;
        let sid = v
            .sessions
            .get(&id)
            .map(|r| r.hermes_id.clone())
            .unwrap_or_default();
        let max_id = v
            .messages
            .recent(&id, usize::MAX)
            .iter()
            .map(|m| m.message_id)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        (sid, max_id)
    };
    if let Ok(event) = state
        .bridge
        .append_steer_message(&id, &hermes_id, steer_msg_id, &text)
    {
        {
            let mut v = state.views.write().await;
            v.apply(&event);
        }
        let dto = crate::server::dto::MessageDto {
            message_id: steer_msg_id,
            session_id: id.clone(),
            role: "user".into(),
            content: Some(text.clone()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: crate::server::event_timestamp(&event),
            token_count: None,
            finish_reason: Some("steer".into()),
        };
        let _ = state.deltas.send(ServerFrame::MessageAppended {
            session_id: id.clone(),
            message: dto,
        });
    }
    let _ = state.deltas.send(ServerFrame::SessionLog {
        session_id: id.clone(),
        level: "info".into(),
        source: "bridge".into(),
        message: format!(
            "Steering turn: {}",
            text.chars().take(80).collect::<String>()
        ),
        timestamp: crate::server::bridge_mgr::chrono_epoch_pub(),
    });
    (StatusCode::ACCEPTED, Json(json!({ "steered": true }))).into_response()
}

/// Body for POST /api/sessions/:id/permission — the user's decision on a
/// pending `session/request_permission`. `optionId` selects an option; omit it
/// (or send null) to cancel the request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PermissionBody {
    option_id: Option<String>,
}

/// POST /api/sessions/:id/permission — answer a pending permission request.
/// Forwards the decision to the runtime (which unblocks the agent's gated tool
/// call), clears the awaiting flag, and nudges liveness back to "running".
async fn respond_permission_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PermissionBody>,
) -> Response {
    match state
        .bridge
        .respond_permission(&id, body.option_id.as_deref())
        .await
    {
        Ok(()) => {
            // The agent resumes; it's running again until the next Done.
            let _ = state.deltas.send(ServerFrame::SessionUpdated {
                session_id: id.clone(),
                changes: json!({ "liveness": "running" }),
            });
            (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
        }
        Err(e) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CardsQuery {
    board_id: Option<String>,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateCardBody {
    board_id: String,
    title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssignCardBody {
    assigned_id: String,
    assigned_kind: String,
    session_id: String,
    attempt_bookmark: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockCardBody {
    blocked_by: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReassignCardBody {
    assigned_id: String,
    assigned_kind: String,
    session_id: String,
    attempt_bookmark: String,
    previous_session_id: String,
}

/// Append an event to the log + apply it to views + broadcast. Returns 500 on
/// log/apply failure. This is the shared mutation path for all card write ops.
// ---- IRC bus handlers (ADR 0006 §2) ----

#[derive(Debug, Deserialize)]
struct IrcSendBody {
    from: String,
    to: String,
    content: String,
}

/// GET /api/irc/peers — list registered IRC peers.
async fn list_irc_peers(State(state): State<AppState>) -> Response {
    let peers = state.irc.list_peers().await;
    Json(json!({ "peers": peers })).into_response()
}

/// POST /api/irc/send — send a DM from one peer to another.
async fn irc_send(State(state): State<AppState>, Json(body): Json<IrcSendBody>) -> Response {
    match state.irc.send(&body.from, &body.to, &body.content).await {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn append_and_apply(state: &AppState, event: crate::event::Event) -> Response {
    if let Err(e) = state.log.append(&event) {
        tracing::error!(error = %e, "failed to append card event");
        return (StatusCode::INTERNAL_SERVER_ERROR, "failed to persist event").into_response();
    }
    // Apply to views under the write lock.
    {
        let mut views = state.views.write().await;
        views.apply(&event);
    }
    // Broadcast a delta frame (fire-and-forget; no subscribers is OK).
    let _ = state.deltas.send(ServerFrame::CardsChanged);
    // Read back the updated card row (if it exists) and return it.
    let views = state.views.read().await;
    match &event {
        crate::event::Event::CardCreated { card_id, .. }
        | crate::event::Event::CardAssigned { card_id, .. }
        | crate::event::Event::CardClaimed { card_id, .. }
        | crate::event::Event::CardBlocked { card_id, .. }
        | crate::event::Event::CardCompleted { card_id, .. }
        | crate::event::Event::CardReassigned { card_id, .. } => match views.cards.get(card_id) {
            Some(row) => Json(CardDto::from_row(row)).into_response(),
            None => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "card not found after apply",
            )
                .into_response(),
        },
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "unexpected event").into_response(),
    }
}

async fn list_cards(
    State(state): State<AppState>,
    Query(q): Query<CardsQuery>,
) -> impl IntoResponse {
    let views = state.views.read().await;
    let filters = CardFilters {
        board_id: q.board_id,
        status: q.status,
    };
    let cards: Vec<CardDto> = views
        .cards
        .list(&filters)
        .into_iter()
        .map(CardDto::from_row)
        .collect();
    let total = cards.len();
    Json(json!({ "cards": cards, "total": total }))
}

async fn get_card(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let views = state.views.read().await;
    match views.cards.get(&id) {
        Some(row) => Json(CardDto::from_row(row)).into_response(),
        None => (StatusCode::NOT_FOUND, "card not found").into_response(),
    }
}

async fn create_card(State(state): State<AppState>, Json(body): Json<CreateCardBody>) -> Response {
    let card_id = format!("card-{}", uuid::Uuid::new_v4());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let event = crate::event::Event::CardCreated {
        card_id: card_id.clone(),
        board_id: body.board_id,
        title: body.title,
        created_at: now,
    };
    append_and_apply(&state, event).await
}

async fn assign_card(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AssignCardBody>,
) -> Response {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let card_id = id.clone();
    let session_id = body.session_id.clone();
    let event = crate::event::Event::CardAssigned {
        card_id: id,
        assigned_id: body.assigned_id,
        assigned_kind: body.assigned_kind,
        session_id: body.session_id,
        attempt_bookmark: body.attempt_bookmark,
        assigned_at: now,
    };
    let response = append_and_apply(&state, event).await;
    // Link the card to the session tree (ADR 0006 §7 footgun 3).
    if !session_id.is_empty() {
        let link_event = crate::event::Event::CardSessionLinked {
            card_id,
            session_id,
            linked_at: now,
        };
        let _ = state.log.append(&link_event);
        let mut views = state.views.write().await;
        views.apply(&link_event);
    }
    response
}

async fn claim_card(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let event = crate::event::Event::CardClaimed {
        card_id: id,
        claimed_at: now,
    };
    append_and_apply(&state, event).await
}

async fn block_card(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<BlockCardBody>,
) -> Response {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let event = crate::event::Event::CardBlocked {
        card_id: id,
        blocked_by: body.blocked_by,
        blocked_at: now,
    };
    append_and_apply(&state, event).await
}

async fn complete_card(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let event = crate::event::Event::CardCompleted {
        card_id: id,
        completed_at: now,
    };
    append_and_apply(&state, event).await
}

async fn reassign_card(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReassignCardBody>,
) -> Response {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let event = crate::event::Event::CardReassigned {
        card_id: id,
        assigned_id: body.assigned_id,
        assigned_kind: body.assigned_kind,
        session_id: body.session_id,
        attempt_bookmark: body.attempt_bookmark,
        previous_session_id: body.previous_session_id,
        reassigned_at: now,
    };
    append_and_apply(&state, event).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::log::Log;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt; // oneshot

    fn test_state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::open(&dir.path().join("log.redb")).unwrap();
        log.append(&Event::SessionCreated {
            session_id: "s1".into(),
            hermes_id: "h1".into(),
            source: "telegram".into(),
            model: Some("glm-5.2".into()),
            title: Some("hello".into()),
            started_at: 100.0,
            message_count: 1,
            input_tokens: 2,
            output_tokens: 3,
            agent: None,
            node: None,
        })
        .unwrap();
        log.append(&Event::MessageAppended {
            session_id: "s1".into(),
            hermes_session_id: "h1".into(),
            message_id: 0,
            role: "user".into(),
            content: Some("hello world".into()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: 101.0,
            token_count: Some(2),
            finish_reason: None,
        })
        .unwrap();

        let mut views = ViewManager::new();
        views.replay(&log).unwrap();

        let mut search = SearchIndex::open(&dir.path().join("idx")).unwrap();
        search.build_from_log(&log).unwrap();

        let (tx, _rx) = broadcast::channel(64);
        let log_arc = Arc::new(log);
        let state = AppState {
            views: Arc::new(RwLock::new(views)),
            search: Arc::new(RwLock::new(search)),
            token: Arc::new("testtoken".to_string()),
            import_state: ImportState::done(),
            hermes_profile: Arc::new("default".into()),
            deltas: tx,
            snapshot_sessions: 1,
            snapshot_messages: 1,
            log: log_arc.clone(),
            bridge: Arc::new(BridgeManager::with_factory(
                log_arc.clone(),
                test_support::mock_factory(),
            )),
            sync_connected: Arc::new(AtomicBool::new(true)),
            irc: crate::irc::IrcBus::new(),
            nodes: crate::node::NodeRegistry::new(),
            envoy_conns: crate::server::envoy_conn::EnvoyConnections::new(),
            proxy: crate::proxy::ProxyTable::new(),
            vaults: Arc::new(crate::vault::VaultStore::with_jj_mode(
                dir.path().join("default"),
                crate::vault::JjMode::Disabled,
            )),
            projects: Arc::new(crate::projects::ProjectStore::new(
                dir.path().join("default"),
            )),
            repos: Arc::new(crate::repos::RepoStore::new(
                dir.path().join("default"),
                "default",
            )),
        };
        (state, dir)
    }

    #[tokio::test]
    async fn sessions_without_token_is_401() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn sessions_with_token_is_200_and_lists() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["total"], 1);
        assert_eq!(v["sessions"][0]["hermesId"], "h1");
        assert_eq!(v["sessions"][0]["source"], "telegram");
    }

    #[tokio::test]
    async fn sort_by_message_count_orders_descending() {
        // Build a 3-session state where started_at order != messageCount order,
        // so a working sort is distinguishable from the view's default.
        let dir = tempfile::tempdir().unwrap();
        let log = Log::open(&dir.path().join("log.redb")).unwrap();
        let mk = |id: &str, started: f64, msgs: u64| Event::SessionCreated {
            session_id: id.into(),
            hermes_id: id.into(),
            source: "cli".into(),
            model: None,
            title: None,
            started_at: started,
            message_count: msgs,
            input_tokens: 0,
            output_tokens: 0,
            agent: None,
            node: None,
        };
        // newest started has FEWEST messages, so startedAt-desc != messageCount-desc.
        log.append(&mk("old_big", 100.0, 500)).unwrap();
        log.append(&mk("mid", 200.0, 50)).unwrap();
        log.append(&mk("new_small", 300.0, 5)).unwrap();
        let mut views = ViewManager::new();
        views.replay(&log).unwrap();
        let mut search = SearchIndex::open(&dir.path().join("idx")).unwrap();
        search.build_from_log(&log).unwrap();
        let (tx, _rx) = broadcast::channel(64);
        let state = AppState {
            views: Arc::new(RwLock::new(views)),
            search: Arc::new(RwLock::new(search)),
            token: Arc::new("testtoken".to_string()),
            import_state: ImportState::done(),
            hermes_profile: Arc::new("default".to_string()),
            deltas: tx,
            snapshot_sessions: 3,
            snapshot_messages: 0,
            log: Arc::new(log),
            bridge: Arc::new(BridgeManager::with_factory(
                Arc::new(Log::open(&dir.path().join("bridge-log.redb")).unwrap()),
                test_support::mock_factory(),
            )),
            sync_connected: Arc::new(AtomicBool::new(true)),
            irc: crate::irc::IrcBus::new(),
            nodes: crate::node::NodeRegistry::new(),
            envoy_conns: crate::server::envoy_conn::EnvoyConnections::new(),
            proxy: crate::proxy::ProxyTable::new(),
            vaults: Arc::new(crate::vault::VaultStore::with_jj_mode(
                dir.path().join("default"),
                crate::vault::JjMode::Disabled,
            )),
            projects: Arc::new(crate::projects::ProjectStore::new(
                dir.path().join("default"),
            )),
            repos: Arc::new(crate::repos::RepoStore::new(
                dir.path().join("default"),
                "default",
            )),
        };
        let app = build_router(state);

        let fetch = |app: axum::Router, q: &str| {
            let uri = format!("/api/sessions?{q}");
            async move {
                let res = app
                    .oneshot(
                        Request::builder()
                            .uri(&uri)
                            .header("authorization", "Bearer testtoken")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let body = axum::body::to_bytes(res.into_body(), usize::MAX)
                    .await
                    .unwrap();
                serde_json::from_slice::<serde_json::Value>(&body).unwrap()
            }
        };

        // sort=messageCount -> 500, 50, 5
        let v = fetch(app.clone(), "sort=messageCount").await;
        let ids: Vec<&str> = v["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec!["old_big", "mid", "new_small"],
            "messageCount desc"
        );

        // sort=startedAt -> 300, 200, 100 (different order, proves sort is applied)
        let v = fetch(app.clone(), "sort=startedAt").await;
        let ids: Vec<&str> = v["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["new_small", "mid", "old_big"], "startedAt desc");
    }

    #[tokio::test]
    async fn wrong_token_is_401() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .header("authorization", "Bearer nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn foreign_origin_is_403() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .header("authorization", "Bearer testtoken")
                    .header("origin", "http://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn health_is_unauthenticated() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["importState"], "done");
        assert_eq!(v["hermesProfile"], "default");
        assert_eq!(v["syncConnected"], true);
    }

    #[tokio::test]
    async fn messages_endpoint_returns_camelcase() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions/s1/messages")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["messages"][0]["messageId"], 0);
        assert_eq!(v["messages"][0]["content"], "hello world");
    }

    #[tokio::test]
    async fn get_unknown_session_is_404() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions/ghost")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn post_message_to_observed_session_is_409() {
        // s1 is a telegram (observed) session — posting must be rejected with 409.
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions/s1/messages")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"text":"hi"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["error"], "observed");
    }

    #[tokio::test]
    async fn post_message_to_unknown_session_is_404() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions/ghost/messages")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"text":"hi"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn post_fork_observed_session_returns_managed_fork_and_leaves_source() {
        let (mut state, _d) = test_state();
        state.bridge = Arc::new(BridgeManager::with_factory(
            state.log.clone(),
            test_support::mock_factory(),
        ));
        let app = build_router(state.clone());

        let source_before = {
            let views = state.views.read().await;
            SessionDto::from_row(views.sessions.get("s1").unwrap())
        };

        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions/s1/fork")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"forkType":"sub"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(v["session"]["source"], "olympus");
        assert_eq!(v["session"]["managed"], true);
        assert_eq!(v["session"]["forkedFrom"], "s1");
        assert_eq!(v["session"]["forkType"], "sub");
        assert!(v["session"]["id"].as_str().unwrap() != "s1");

        let source_after = {
            let views = state.views.read().await;
            SessionDto::from_row(views.sessions.get("s1").unwrap())
        };
        assert_eq!(source_after, source_before);
    }

    #[tokio::test]
    async fn search_finds_indexed_message() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/search?q=hello")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(!v["hits"].as_array().unwrap().is_empty());
        assert_eq!(v["hits"][0]["sessionId"], "s1");
        assert_eq!(v["hits"][0]["source"], "telegram");
    }

    // ---- A2: POST /api/sessions (new managed Olympus chat) ----

    #[tokio::test]
    async fn post_sessions_creates_managed_olympus_session() {
        // POST /api/sessions with no body → creates a new Olympus-managed session
        // OPTIMISTICALLY (no runtime spawned), returns 201 with a Session DTO
        // where source="olympus", managed=true, and an empty hermesId (the real
        // id is backfilled lazily on the first send).
        let (mut state, _d) = test_state();
        state.bridge = Arc::new(BridgeManager::with_factory(
            Arc::new(Log::open(&_d.path().join("bridge-log-a.redb")).unwrap()),
            test_support::mock_factory(),
        ));
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["source"], "olympus");
        assert_eq!(v["managed"], true);
        // Optimistic: a durable id is allocated immediately (`<utc>-<hash>` per
        // ADR 0005 §6 — node is NOT in the id); hermesId is empty until the
        // first send spawns the runtime.
        let id = v["id"].as_str().unwrap();
        assert!(
            id.starts_with("20") || id.starts_with("19"),
            "id should start with a UTC datetime stamp: {id}"
        );
        assert_eq!(
            id.matches('-').count(),
            1,
            "id should be <utc>-<hash> with no node segment: {id}"
        );
        assert_eq!(v["hermesId"], "");
    }

    #[tokio::test]
    async fn post_sessions_with_agent_binds_it_at_creation() {
        // POST /api/sessions {agent, node} → the draft carries the binding.
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"agent":"coding-agent","node":"local"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["agent"], "coding-agent");
        assert_eq!(v["node"], "local");
    }

    #[tokio::test]
    async fn patch_session_assigns_agent_and_model() {
        // PATCH /api/sessions/:id sets agent/model on an existing managed draft.
        let (state, _d) = test_state();
        // Create a draft first.
        let ns = state
            .bridge
            .create_draft(&crate::server::bridge_mgr::RuntimeSpec::default())
            .unwrap();
        {
            let mut views = state.views.write().await;
            if let Ok(events) = state.log.read_all() {
                for (_s, e) in events {
                    views.apply(&e);
                }
            }
            let _ = views.sessions.get(&ns.session_id); // ensure present
        }
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/sessions/{}", ns.session_id))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"agent":"glm52","model":"glm-5.2"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["agent"], "glm52");
        assert_eq!(v["model"], "glm-5.2");
    }

    #[tokio::test]
    async fn patch_session_pins_and_archives() {
        // PATCH /api/sessions/:id with pinned/archived persists both flags.
        let (state, _d) = test_state();
        let ns = state
            .bridge
            .create_draft(&crate::server::bridge_mgr::RuntimeSpec::default())
            .unwrap();
        {
            let mut views = state.views.write().await;
            if let Ok(events) = state.log.read_all() {
                for (_s, e) in events {
                    views.apply(&e);
                }
            }
        }
        let app = build_router(state.clone());
        let res = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/sessions/{}", ns.session_id))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"pinned":true,"archived":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["pinned"], true);
        assert_eq!(v["archived"], true);
        // Unpin only — archived must be left unchanged.
        let app2 = build_router(state);
        let res2 = app2
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/sessions/{}", ns.session_id))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"pinned":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res2.status(), StatusCode::OK);
        let body2 = axum::body::to_bytes(res2.into_body(), usize::MAX)
            .await
            .unwrap();
        let v2: serde_json::Value = serde_json::from_slice(&body2).unwrap();
        assert_eq!(v2["pinned"], false);
        assert_eq!(v2["archived"], true, "archived untouched by pin-only patch");
    }

    #[tokio::test]
    async fn post_message_lazily_spawns_runtime_for_draft_session() {
        // A draft (no runtime, empty hermesId) accepts a send: the handler
        // lazily spawns the runtime via the factory and returns 202 — it does
        // NOT 503 "bridge_unavailable" (the pre-fix regression).
        let (state, _d) = test_state();
        let ns = state
            .bridge
            .create_draft(&crate::server::bridge_mgr::RuntimeSpec::default())
            .unwrap();
        {
            let mut views = state.views.write().await;
            if let Ok(events) = state.log.read_all() {
                for (_s, e) in events {
                    views.apply(&e);
                }
            }
        }
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sessions/{}/messages", ns.session_id))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"text":"hello"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn agent_error_event_is_persisted_as_system_message() {
        let (mut state, _d) = test_state();
        state.bridge = Arc::new(BridgeManager::with_factory(
            state.log.clone(),
            test_support::mock_factory(),
        ));
        let app = build_router(state.clone());

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = v["id"].as_str().unwrap().to_string();

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sessions/{session_id}/messages"))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"content":"trigger agent error"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::ACCEPTED);

        let mut messages = serde_json::Value::Null;
        for _ in 0..50 {
            let res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/sessions/{session_id}/messages"))
                        .header("authorization", "Bearer testtoken")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            let body = axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap();
            messages = serde_json::from_slice(&body).unwrap();
            if messages["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["role"] == "system" && m["content"] == "⚠ agent error: mock failure")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let system = messages["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "system")
            .expect("system error message should be persisted");
        assert_eq!(system["content"], "⚠ agent error: mock failure");
    }

    // ---- card CRUD tests (C1) ----

    #[tokio::test]
    async fn list_cards_empty_by_default() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/cards")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["cards"].is_array());
    }

    #[tokio::test]
    async fn post_message_to_managed_olympus_session_is_202() {
        // A managed olympus session should accept a prompt and return 202
        // (not 503 — the bridge is wired).
        let (mut state, _d) = test_state();
        // The bridge must use the SAME log as the AppState so create_session's
        // SessionCreated event is visible to post_message's view lookup.
        state.bridge = Arc::new(BridgeManager::with_factory(
            state.log.clone(),
            test_support::mock_factory(),
        ));
        // First create a managed session via the API so the bridge knows about it.
        let app = build_router(state.clone());
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = v["id"].as_str().unwrap().to_string();

        // Now POST a message to that session.
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sessions/{session_id}/messages"))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"text":"say PONG"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn post_sessions_without_token_is_401() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn vault_routes_create_write_read_and_list_notes() {
        let (state, _d) = test_state();
        let app = build_router(state);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/vaults")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "Ops Vault"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(created["id"], "ops-vault");

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/vaults/ops-vault/note?path=runbooks/boot.md")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "markdown": "---\ntitle: Boot\n---\n# Ignored\nSee [[Incident Guide]]"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let note: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(note["path"], "runbooks/boot.md");
        assert_eq!(note["title"], "Boot");
        assert_eq!(note["linkedNotes"][0], "Incident Guide");

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/vaults/ops-vault/note?path=runbooks/boot.md")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/vaults/ops-vault/notes")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let tree: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(tree["notes"][0]["kind"], "folder");
        assert_eq!(tree["notes"][0]["children"][0]["path"], "runbooks/boot.md");

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/vaults")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(list["vaults"][0]["noteCount"], 1);
    }

    #[tokio::test]
    async fn create_card_returns_camelcase_dto() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/cards")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"boardId":"b1","title":"Do stuff"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["id"].as_str().unwrap().starts_with("card-"));
        assert_eq!(v["boardId"], "b1");
        assert_eq!(v["title"], "Do stuff");
        assert_eq!(v["status"], "todo");
        // snake_case keys must NOT be present
        assert!(v.get("board_id").is_none());
    }

    #[tokio::test]
    async fn assign_card_transitions_to_assigned() {
        let (state, _d) = test_state();
        let app = build_router(state);

        // Create first
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/cards")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"boardId":"b1","title":"T"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let card_id = v["id"].as_str().unwrap().to_string();

        // Assign
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/cards/{card_id}/assign"))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"assignedId":"zephyr","assignedKind":"agent","sessionId":"s1","attemptBookmark":"bm-1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], "assigned");
        assert_eq!(v["assignedId"], "zephyr");
        assert_eq!(v["currentSessionId"], "s1");
    }

    #[tokio::test]
    async fn complete_card_transitions_to_done() {
        let (state, _d) = test_state();
        let app = build_router(state);

        // Create + claim
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/cards")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"boardId":"b1","title":"T"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let card_id = v["id"].as_str().unwrap().to_string();

        // Complete
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/cards/{card_id}/complete"))
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], "done");
    }

    #[tokio::test]
    async fn get_unknown_card_is_404() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/cards/ghost")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    // ---- setup declaration endpoints (ADR 0006 §3) ----

    #[tokio::test]
    async fn put_setup_then_get_roundtrips() {
        let (state, _d) = test_state();
        let app = build_router(state);
        // PUT an org-scope declaration.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/setup")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"scope":"org:acme","skills":["code-review"],"plugins":["gitnexus"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["scope"], "org:acme");
        assert_eq!(v["skills"][0], "code-review");
        assert_eq!(v["plugins"][0], "gitnexus");
        // mcp/hooks default to empty arrays (camelCase contract).
        assert_eq!(v["mcp"].as_array().unwrap().len(), 0);

        // GET it back by scope.
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/setup?scope=org:acme")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["skills"][0], "code-review");
    }

    #[tokio::test]
    async fn get_setup_effective_merges_org_and_project() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let put = |scope: &str, skills: &str| {
            let body = format!(r#"{{"scope":"{scope}","skills":{skills}}}"#);
            Request::builder()
                .method("PUT")
                .uri("/api/setup")
                .header("authorization", "Bearer testtoken")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap()
        };
        app.clone()
            .oneshot(put("org:acme", r#"["code-review"]"#))
            .await
            .unwrap();
        app.clone()
            .oneshot(put("project:acme/web", r#"["react-doctor"]"#))
            .await
            .unwrap();

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/setup?org=acme&project=web")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let skills: Vec<String> = v["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect();
        assert_eq!(skills, vec!["code-review", "react-doctor"]);
    }

    #[tokio::test]
    async fn put_setup_rejects_invalid_scope() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/setup")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"scope":"nonsense"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_undeclared_setup_is_empty_not_404() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/setup?scope=org:ghost")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["scope"], "org:ghost");
        assert_eq!(v["skills"].as_array().unwrap().len(), 0);
    }

    // ---- restart test: cards survive replay from the log (C1 gate) ----

    #[test]
    fn cards_survive_restart_via_replay() {
        // Simulate the full lifecycle: append card events to a log, replay into
        // a fresh ViewManager, verify the card state is fully reconstructed.
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("log.redb");
        let log = Log::open(&log_path).unwrap();

        // Card 1: create → assign → claim → complete
        log.append(&Event::CardCreated {
            card_id: "c1".into(),
            board_id: "b1".into(),
            title: "First card".into(),
            created_at: 100.0,
        })
        .unwrap();
        log.append(&Event::CardAssigned {
            card_id: "c1".into(),
            assigned_id: "zephyr".into(),
            assigned_kind: "agent".into(),
            session_id: "sess-1".into(),
            attempt_bookmark: "bm-1".into(),
            assigned_at: 101.0,
        })
        .unwrap();
        log.append(&Event::CardClaimed {
            card_id: "c1".into(),
            claimed_at: 102.0,
        })
        .unwrap();
        log.append(&Event::CardCompleted {
            card_id: "c1".into(),
            completed_at: 105.0,
        })
        .unwrap();

        // Card 2: create → assign → reassign (previous attempt forwarded)
        log.append(&Event::CardCreated {
            card_id: "c2".into(),
            board_id: "b1".into(),
            title: "Second card".into(),
            created_at: 200.0,
        })
        .unwrap();
        log.append(&Event::CardAssigned {
            card_id: "c2".into(),
            assigned_id: "zephyr".into(),
            assigned_kind: "agent".into(),
            session_id: "sess-2a".into(),
            attempt_bookmark: "bm-2a".into(),
            assigned_at: 201.0,
        })
        .unwrap();
        log.append(&Event::CardReassigned {
            card_id: "c2".into(),
            assigned_id: "talos".into(),
            assigned_kind: "agent".into(),
            session_id: "sess-2b".into(),
            attempt_bookmark: "bm-2b".into(),
            previous_session_id: "sess-2a".into(),
            reassigned_at: 210.0,
        })
        .unwrap();

        // Card 3: create → blocked
        log.append(&Event::CardCreated {
            card_id: "c3".into(),
            board_id: "b1".into(),
            title: "Third card".into(),
            created_at: 300.0,
        })
        .unwrap();
        log.append(&Event::CardBlocked {
            card_id: "c3".into(),
            blocked_by: vec!["c1".into(), "c2".into()],
            blocked_at: 301.0,
        })
        .unwrap();

        // Drop the log, reopen it (simulating restart), replay.
        drop(log);
        let reopened = Log::open(&log_path).unwrap();
        let mut views = ViewManager::new();
        views.replay(&reopened).unwrap();

        // Card 1: done, one completed attempt
        let c1 = views.cards.get("c1").expect("c1 must exist after replay");
        assert_eq!(c1.status, "done");
        assert_eq!(c1.title, "First card");
        assert_eq!(c1.attempts.len(), 1);
        assert_eq!(c1.attempts[0].outcome, "done");
        assert_eq!(c1.attempts[0].ended_at, Some(105.0));

        // Card 2: assigned (reassigned), two attempts, first closed
        let c2 = views.cards.get("c2").expect("c2 must exist after replay");
        assert_eq!(c2.status, "assigned");
        assert_eq!(c2.assigned_id.as_deref(), Some("talos"));
        assert_eq!(c2.current_session_id.as_deref(), Some("sess-2b"));
        assert_eq!(c2.attempts.len(), 2);
        assert_eq!(c2.attempts[0].session_id, "sess-2a");
        assert_eq!(c2.attempts[0].outcome, "reassigned");
        assert_eq!(c2.attempts[1].session_id, "sess-2b");
        assert!(c2.attempts[1].ended_at.is_none());

        // Card 3: blocked with deps
        let c3 = views.cards.get("c3").expect("c3 must exist after replay");
        assert_eq!(c3.status, "blocked");
        assert_eq!(c3.blocked_by, vec!["c1", "c2"]);

        // The board has 3 cards total
        let all = views.cards.list(&crate::views::CardFilters {
            board_id: Some("b1".into()),
            status: None,
        });
        assert_eq!(all.len(), 3);
    }

    /// B-3 route-contract guard: every expected route must be reachable.
    /// This test exists because a prior manual merge (6549616) silently
    /// dropped the entire /api/repos surface while keeping the store + views
    /// intact — dead code that compiled fine. Walking the route table via
    /// HTTP requests catches that class of regression at build time.
    ///
    /// To add a route: add it here AND to build_router. If you forget
    /// either, this test fails.
    #[tokio::test]
    async fn route_contract_all_expected_routes_exist() {
        let (state, _dir) = test_state();
        let app = build_router(state);

        // (method, path, expected_status_range). We use 400/404 to confirm
        // the route exists (matched) without needing valid bodies.
        // NOTE: session "s1" exists in the test fixture, so /sessions/s1/*
        // routes return 200 for GET/POST.
        let cases: &[(&str, &str, &[u16])] = &[
            ("GET", "/api/sessions", &[200]),
            ("POST", "/api/sessions", &[200, 201, 400, 422]),
            ("GET", "/api/sessions/s1", &[200]),
            ("GET", "/api/sessions/nonexistent", &[404]),
            ("PATCH", "/api/sessions/s1", &[200]),
            ("POST", "/api/sessions/s1/fork", &[200, 409]),
            ("POST", "/api/sessions/s1/cancel", &[200, 409]),
            ("GET", "/api/sessions/s1/messages", &[200]),
            ("GET", "/api/search", &[200]),
            ("GET", "/api/models", &[200]),
            ("GET", "/api/agents", &[200]),
            ("GET", "/api/cards", &[200]),
            ("POST", "/api/cards", &[400, 422]),
            ("GET", "/api/cards/nonexistent", &[404]),
            ("POST", "/api/cards/nonexistent/assign", &[404]),
            ("POST", "/api/cards/nonexistent/claim", &[404, 500]), // TODO: should be 404
            ("POST", "/api/cards/nonexistent/block", &[404]),
            ("POST", "/api/cards/nonexistent/complete", &[404, 500]), // TODO: should be 404
            ("POST", "/api/cards/nonexistent/reassign", &[404]),
            ("GET", "/api/nodes", &[200]),
            ("GET", "/api/nodes/nonexistent/agents", &[200, 404]),
            (
                "POST",
                "/api/nodes/nonexistent/agents/refresh",
                &[200, 404, 501],
            ),
            ("GET", "/api/vaults", &[200]),
            ("POST", "/api/vaults", &[400, 422]),
            ("GET", "/api/vaults/nonexistent/notes", &[404]),
            ("GET", "/api/vaults/nonexistent/note", &[400, 404]),
            ("PUT", "/api/vaults/nonexistent/note", &[400, 404]),
            ("DELETE", "/api/vaults/nonexistent/note", &[400, 404]),
            ("GET", "/api/vaults/nonexistent/graph", &[404]),
            ("GET", "/api/vaults/nonexistent/collections", &[404]),
            ("GET", "/api/vaults/nonexistent/collections/p", &[404]),
            ("GET", "/api/projects", &[200]),
            ("POST", "/api/projects", &[400, 422]),
            ("GET", "/api/projects/nonexistent", &[404]),
            ("PATCH", "/api/projects/nonexistent", &[404]),
            ("DELETE", "/api/projects/nonexistent", &[404]),
            ("POST", "/api/sessions/s1/project", &[400, 404, 422]),
            // ── The regression class: repos were dropped once before ──
            ("GET", "/api/repos", &[200]),
            ("POST", "/api/repos", &[400, 422]),
            ("GET", "/api/repos/nonexistent", &[404]),
            ("DELETE", "/api/repos/nonexistent", &[404]),
            ("POST", "/api/sessions/s1/repos", &[400, 404, 422]),
            // ── Subsessions (B-2) ──
            ("GET", "/api/sessions/s1/subsessions", &[200]),
            (
                "POST",
                "/api/sessions/s1/subsessions",
                &[200, 201, 400, 422],
            ),
            ("POST", "/api/sessions/s1/complete", &[200, 400, 409]),
            ("GET", "/api/health", &[200]),
            ("GET", "/api/setup", &[200]),
            ("PUT", "/api/setup", &[400, 422]),
            ("GET", "/api/registry", &[200]),
        ];

        let mut missing: Vec<String> = Vec::new();
        for (method, path, acceptable) in cases {
            let req_method = match *method {
                "GET" => axum::http::Method::GET,
                "POST" => axum::http::Method::POST,
                "PATCH" => axum::http::Method::PATCH,
                "PUT" => axum::http::Method::PUT,
                "DELETE" => axum::http::Method::DELETE,
                _ => unreachable!(),
            };
            let req = axum::http::Request::builder()
                .method(req_method)
                .uri(*path)
                .header("authorization", "Bearer testtoken")
                .header("x-forwarded-for", "127.0.0.1")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            let status = resp.status().as_u16();
            // 405 = route exists but method not allowed (also confirms match)
            if !acceptable.contains(&status) && status != 405 && status != 400 && status != 415 {
                missing.push(format!(
                    "{} {} → {} (expected {:?})",
                    method, path, status, acceptable
                ));
            }
        }

        assert!(
            missing.is_empty(),
            "route contract violations:\n  {}",
            missing.join("\n  ")
        );
    }
}
