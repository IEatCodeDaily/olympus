//! axum HTTP server: REST read endpoints + auth gate (ADR 0002 §10.3.1, §3.5.2).
//!
//! The `/ws` delta stream lives in [`crate::server::ws`]. This module owns the
//! router, shared state, the auth middleware, and the read-only REST handlers
//! that back the UI's session list, transcript view, and search.
pub mod bridge_mgr;
pub mod capability;
pub mod dto;
pub mod envoy_conn;
mod identity;
pub mod principal;
pub(crate) mod routes;
pub mod terminal_ws;
pub mod ws;

// Agent discovery moved to `olympus-envoy` (ADR 0008 S2) — probing the host
// for Hermes profiles + CLI harnesses is the envoy's job. Re-exported so
// existing `server::agents::…` call sites keep working unchanged.
pub use olympus_envoy::discovery as agents;

#[cfg(test)]
pub mod test_support;
#[cfg(test)]
mod tests;

use std::sync::{
    atomic::{AtomicBool, AtomicU8, Ordering},
    Arc,
};

use axum::{
    extract::State,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::log::Log;
use crate::search::SearchIndex;
use crate::state_db_reader::StateDbReader;
use crate::views::ViewManager;
use bridge_mgr::BridgeManager;
use principal::{Principal, RouteClass};
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
    pub capability_signer: Arc<capability::CapabilitySigner>,
    pub auth_store: Arc<crate::auth_store::AuthStore>,
    pub allow_installation_token: bool,
    pub session_cookie_secure: bool,
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
    /// Event-backed durable job records and reconciliation state.
    pub jobs: Arc<crate::jobs::JobService>,
    /// Bridge manager: owns agent runtimes for managed (olympus-source) sessions.
    pub bridge: Arc<BridgeManager>,
    /// Whether the live `state.db` sync worker has successfully connected.
    pub sync_connected: Arc<AtomicBool>,
    /// In-process IRC bus for inter-agent messaging (ADR 0006 §2).
    pub irc: crate::irc::IrcBus,
    /// Fleet node registry — tracks connected envoys (UDS) + the local node.
    pub nodes: crate::node::NodeRegistry,
    /// Remote envoy connections (UDS or iroh write halves for RemoteRuntime).
    pub envoy_conns: crate::server::envoy_conn::EnvoyConnections,
    /// Hall-local operator terminal (PTY) manager — the "hall" node target in
    /// the cockpit picker (ADR 0021). Hall has no EnvoyConnection to itself, so
    /// it runs shells directly via the same node-agnostic PtyManager the envoy
    /// uses. Operator-only; reachable solely from the operator terminal WS.
    pub hall_pty: Arc<crate::server::terminal_ws::HallTerminals>,
    /// Hall's iroh node id (public key, z-base-32). `None` when iroh is not
    /// enabled (no listener bound). Exposed via GET /api/nodes/hall-identity
    /// so the installer can fetch it without scraping logs (ADR 0008 §1 S7).
    pub hall_iroh_id: Option<Arc<String>>,
    /// Reverse proxy routing table — slug → backend target.
    pub proxy: crate::proxy::ProxyTable,
    /// Desired external-edge state and its sole serialized driver.
    pub edge: crate::edge::EdgeManager,
    /// Markdown-first knowledge vault storage (ADR 0004).
    pub vaults: Arc<crate::vault::VaultStore>,
    /// Read-only connection to the Hermes `state.db` for on-demand message
    /// reads and full-text search (ADR 0009 lazy-history). `None` when no
    /// state.db exists (fresh install). Replaces the 1.4 GB in-memory message
    /// mirror + tantivy index.
    pub state_db: Option<Arc<StateDbReader>>,
    /// Project (context container) storage — dir/manifest/symlink.
    pub projects: Arc<crate::projects::ProjectStore>,
    /// Managed repo store — clone/sync/attach jj workspaces.
    pub repos: Arc<crate::repos::RepoStore>,
    /// Envoy enrollment tokens (one-line node setup). Ephemeral by design.
    pub enroll: crate::enroll::EnrollStore,
    /// The olympus home dir (`~/.olympus`) — hall.toml allowlist + bin/ live here.
    pub home: Arc<std::path::PathBuf>,
}

/// Build the full router (REST + WS) with the auth gate applied to `/api/*` and
/// `/ws`. `/api/health` is intentionally left unauthenticated so a client can
/// probe readiness before it has the token.
pub fn build_router(state: AppState) -> Router {
    // Protected surface: every `/api/*` resource router + `/ws`, behind the
    // auth gate. Resource routers are state-generic and merged here; state is
    // applied once at the end via `.with_state(state)`.
    let protected = Router::new()
        .merge(routes::sessions::router())
        .merge(routes::irc::router())
        .merge(routes::jobs::router())
        .merge(routes::search::router())
        .merge(routes::agents::router())
        .merge(routes::cards::router())
        .merge(routes::events::router())
        .merge(routes::setup::router())
        .merge(routes::registry::router())
        .merge(routes::nodes::router())
        .merge(routes::vaults::router())
        .merge(routes::projects::router())
        .merge(routes::repos::router())
        .merge(routes::organizations::router())
        .merge(routes::packages::router())
        .merge(routes::edge::router())
        .route("/api/enroll", post(routes::enroll::mint_enroll))
        .route(
            "/api/proxy",
            get(crate::proxy::list_proxy_endpoints).post(crate::proxy::create_proxy_endpoint),
        )
        .route(
            "/api/proxy/{slug}",
            axum::routing::delete(crate::proxy::delete_proxy_endpoint),
        )
        .route("/ws", get(ws::ws_handler))
        .route(
            "/ws/operator/terminals/{terminal_id}",
            get(terminal_ws::terminal_ws_handler),
        )
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
        .route("/api/auth/login", post(identity::login))
        .route("/api/edge/auth", get(routes::edge::forward_auth))
        .merge(protected)
        .merge(routes::enroll::router())
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
/// cross-origin requests + preflights. Mirror the exact Hall-origin policy and
/// reflect only explicitly accepted origins. `tower-http`
/// answers `OPTIONS` preflight automatically (before the auth middleware), so
/// the token isn't required on the preflight itself.
fn cors_layer() -> CorsLayer {
    use axum::http::{header, Method};
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _parts| {
            origin
                .to_str()
                .map(|origin| {
                    let host = _parts
                        .headers
                        .get(axum::http::header::HOST)
                        .and_then(|value| value.to_str().ok());
                    crate::auth::browser_origin_allowed(origin, host)
                })
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
        .allow_credentials(true)
}

/// Auth middleware: enforce the Origin policy on every request, and the Bearer
/// token on REST. The `/ws` upgrade carries its token as a `?token=` query
/// param (browsers can't set headers on WS), so that route validates the token
/// itself; here we still enforce Origin for it.
async fn auth_gate(
    State(state): State<AppState>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    use axum::extract::FromRequestParts;

    let (mut parts, body) = request.into_parts();
    let principal = match Principal::from_request_parts(&mut parts, &state).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    request = axum::extract::Request::from_parts(parts, body);

    let route = principal::route_class(request.uri().path());
    let organization_exists = match route {
        RouteClass::Organization(Some(organization_id)) => {
            match state.auth_store.organization_exists(organization_id) {
                Ok(exists) => exists,
                Err(error) => {
                    tracing::error!(%error, "checking organization existence");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "authorization unavailable",
                    )
                        .into_response();
                }
            }
        }
        _ => false,
    };
    match principal::authorize(&principal, route, organization_exists) {
        Ok(scope) => {
            request.extensions_mut().insert(principal);
            if let Some(scope) = scope {
                request.extensions_mut().insert(scope);
            }
            next.run(request).await
        }
        Err(status) => (
            status,
            status.canonical_reason().unwrap_or("authorization denied"),
        )
            .into_response(),
    }
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "importState": state.import_state.as_str(),
        "snapshot": { "sessions": state.snapshot_sessions, "messages": state.snapshot_messages },
        "syncConnected": state.sync_connected.load(Ordering::SeqCst),
        "hermesProfile": state.hermes_profile.as_str(),
        "edge": if state.edge.healthy() { "ready" } else { "missing" },
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
