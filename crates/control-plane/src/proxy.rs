//! Reverse proxy — routes incoming HTTP/WS requests to agent-spawned backends.
//!
//! The proxy is a dumb routing layer. It maps a slug to a target
//! (host:port or Unix socket) and forwards requests. It does NOT manage
//! processes — that's the envoy's job. The envoy reports "plugin-X is up
//! on :3421" and the proxy creates the route.
//!
//! Two routing modes, both supported simultaneously:
//!   • Path-based:   GET /proxy/<slug>/... → forward to target/...
//!   • Subdomain:    <slug>.olympus.domain → forward to target/...
//!
//! Path-based is the default for local dev (zero DNS config). Subdomain is
//! the production mode (requires wildcard DNS).
//!
//! The proxy table is in-memory and populated via the REST API:
//!   POST /api/proxy      { slug, host, port, sessionId?, authMode? }
//!   GET  /api/proxy      → list endpoints
//!   DELETE /api/proxy/:slug
//!
//! WebSocket upgrades are forwarded transparently.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;

/// Authentication mode for a proxy endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProxyAuth {
    /// Anyone can access — no auth check.
    Public,
    /// Bearer token required in the Authorization header.
    Token(String),
    /// Session-scoped: requires the Olympus bearer token (same as /api/*).
    SessionScoped,
}

/// A single proxied endpoint — slug maps to a backend target.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyEndpoint {
    /// The URL slug — used in path routing (`/proxy/<slug>/`) and as the
    /// subdomain label (`<slug>.olympus.*`).
    pub slug: String,
    /// Backend host (usually 127.0.0.1 for local processes).
    pub host: String,
    /// Backend port.
    pub port: u16,
    /// Owning session, if any. None = standalone endpoint.
    pub session_id: Option<String>,
    /// Owning project, if any.
    pub project_id: Option<String>,
    /// Auth mode for this endpoint.
    pub auth_mode: ProxyAuth,
    /// Endpoint status.
    pub status: ProxyStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProxyStatus {
    Active,
    Stopped,
}

/// Internal tracking entry.
struct ProxyEntry {
    endpoint: ProxyEndpoint,
    /// When the endpoint was registered (epoch seconds).
    registered_at: f64,
}

/// Thread-safe proxy routing table.
#[derive(Clone)]
pub struct ProxyTable {
    routes: Arc<RwLock<HashMap<String, ProxyEntry>>>,
}

impl ProxyTable {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register or update an endpoint. If the slug exists, it's overwritten.
    pub async fn upsert(&self, endpoint: ProxyEndpoint) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        self.routes.write().await.insert(
            endpoint.slug.clone(),
            ProxyEntry {
                endpoint,
                registered_at: now,
            },
        );
    }

    /// Remove an endpoint. Returns true if it existed.
    pub async fn remove(&self, slug: &str) -> bool {
        self.routes.write().await.remove(slug).is_some()
    }

    /// Look up an endpoint by slug.
    pub async fn get(&self, slug: &str) -> Option<ProxyEndpoint> {
        self.routes.read().await.get(slug).map(|e| e.endpoint.clone())
    }

    /// List all endpoints.
    pub async fn list(&self) -> Vec<ProxyEndpoint> {
        self.routes
            .read()
            .await
            .values()
            .map(|e| e.endpoint.clone())
            .collect()
    }

    /// Resolve a slug from the request Host header (subdomain routing).
    /// E.g. `myapp.olympus.localhost` → slug = `myapp`.
    /// Returns None if the host doesn't match the subdomain pattern.
    pub fn extract_slug_from_host(host: &str, base_domain: &str) -> Option<String> {
        // Strip port if present.
        let host = host.split(':').next()?;
        if !host.ends_with(base_domain) {
            return None;
        }
        let prefix = &host[..host.len() - base_domain.len()];
        // prefix should be "slug." — strip the trailing dot.
        let slug = prefix.strip_suffix('.')?;
        if slug.is_empty() || slug.contains('.') {
            return None; // multi-level subdomain, not our pattern
        }
        Some(slug.to_string())
    }
}

impl Default for ProxyTable {
    fn default() -> Self {
        Self::new()
    }
}

// ── REST API handlers ──────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProxyBody {
    pub slug: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default = "default_auth")]
    pub auth_mode: ProxyAuth,
}

fn default_auth() -> ProxyAuth {
    ProxyAuth::SessionScoped
}

pub async fn list_proxy_endpoints(State(state): State<crate::server::AppState>) -> impl IntoResponse {
    let endpoints = state.proxy.list().await;
    Json(json!({ "endpoints": endpoints }))
}

pub async fn create_proxy_endpoint(
    State(state): State<crate::server::AppState>,
    Json(body): Json<CreateProxyBody>,
) -> Response {
    // Validate slug — alphanumeric + dashes only.
    let slug = body.slug.trim().to_lowercase();
    if slug.is_empty()
        || slug.len() > 63
        || !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "slug must be 1-63 chars, alphanumeric or dashes" })),
        )
            .into_response();
    }

    let endpoint = ProxyEndpoint {
        slug: slug.clone(),
        host: body.host.clone(),
        port: body.port,
        session_id: body.session_id,
        project_id: body.project_id,
        auth_mode: body.auth_mode,
        status: ProxyStatus::Active,
    };

    state.proxy.upsert(endpoint.clone()).await;

    tracing::info!(
        slug = %slug,
        host = %body.host,
        port = body.port,
        "proxy endpoint registered"
    );

    Json(json!({ "endpoint": endpoint })).into_response()
}

pub async fn delete_proxy_endpoint(
    State(state): State<crate::server::AppState>,
    Path(slug): Path<String>,
) -> Response {
    if state.proxy.remove(&slug).await {
        tracing::info!(slug = %slug, "proxy endpoint removed");
        Json(json!({ "ok": true })).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "endpoint not found" })),
        )
            .into_response()
    }
}

// NOTE: proxy management routes are wired directly in server::build_router.

// ── Reverse proxy forwarding ───────────────────────

/// Handler for `/proxy/{slug}` (no sub-path — forwards to `/`).
pub async fn proxy_forward_root(
    state: State<crate::server::AppState>,
    Path(slug): Path<String>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    req: Request<Body>,
) -> Response {
    proxy_forward(state, Path((slug, String::new())), method, headers, Query(query), req).await
}

/// The catch-all handler for `/proxy/{slug}/{path:.*}`.
/// Forwards the request to the backend, streaming the body.
pub async fn proxy_forward(
    State(state): State<crate::server::AppState>,
    Path((slug, rest)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    req: Request<Body>,
) -> Response {
    let Some(endpoint) = state.proxy.get(&slug).await else {
        return proxy_not_found(&slug);
    };

    if endpoint.status == ProxyStatus::Stopped {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "endpoint stopped",
        )
            .into_response();
    }

    // Auth check.
    match &endpoint.auth_mode {
        ProxyAuth::Public => {}
        ProxyAuth::Token(expected) => {
            let provided = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "));
            if provided != Some(expected.as_str()) {
                return (
                    StatusCode::UNAUTHORIZED,
                    "invalid or missing proxy token",
                )
                    .into_response();
            }
        }
        ProxyAuth::SessionScoped => {
            // Reuse the main Olympus token.
            let provided = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "));
            if provided != Some(state.token.as_str()) {
                return (
                    StatusCode::UNAUTHORIZED,
                    "olympus token required",
                )
                    .into_response();
            }
        }
    }

    forward_to_backend(&endpoint, &rest, method, headers, &query, req).await
}

/// Forward a request to the backend target.
async fn forward_to_backend(
    endpoint: &ProxyEndpoint,
    rest: &str,
    method: Method,
    headers: HeaderMap,
    query: &HashMap<String, String>,
    req: Request<Body>,
) -> Response {
    use hyper_util::rt::TokioIo;

    let target_addr = format!("{}:{}", endpoint.host, endpoint.port);

    // Build the upstream path.
    let path = if rest.is_empty() {
        "/".to_string()
    } else if !rest.starts_with('/') {
        format!("/{rest}")
    } else {
        rest.to_string()
    };

    // Append query string.
    let qs = if query.is_empty() {
        String::new()
    } else {
        let pairs: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("?{}", pairs.join("&"))
    };

    let upstream = format!("{path}{qs}");

    // Connect to the backend.
    let stream = match tokio::net::TcpStream::connect(&target_addr).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target = %target_addr, error = %e, "proxy: backend unreachable");
            return proxy_bad_gateway(&target_addr, &e.to_string());
        }
    };
    let io = TokioIo::new(stream);

    let (mut sender, conn) = match hyper::client::conn::http1::handshake(io).await {
        Ok(c) => c,
        Err(e) => {
            return proxy_bad_gateway(&target_addr, &e.to_string());
        }
    };

    // Drive the connection in background.
    tokio::task::spawn(async move {
        if let Err(e) = conn.await {
            tracing::debug!(error = %e, "proxy connection closed");
        }
    });

    // Build the upstream request.
    let (parts, body) = req.into_parts();
    let mut upstream_req = Request::builder()
        .method(method)
        .uri(&upstream);

    // Copy headers, fixing Host.
    for (key, value) in headers.iter() {
        if key == "host" {
            continue;
        }
        upstream_req = upstream_req.header(key, value);
    }
    upstream_req = upstream_req.header("host", &target_addr);

    let upstream_req = match upstream_req.body(body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to build upstream request: {e}"),
            )
                .into_response();
        }
    };

    // Send and relay response.
    match sender.send_request(upstream_req).await {
        Ok(resp) => {
            let (parts, body) = resp.into_parts();
            // Collect the body bytes and re-wrap — streaming via MapErr
            // doesn't satisfy axum's Body trait bounds directly.
            let bytes = match body.collect().await {
                Ok(b) => b.to_bytes(),
                Err(e) => {
                    return proxy_bad_gateway(&target_addr, &e.to_string());
                }
            };
            Response::from_parts(parts, Body::from(bytes))
        }
        Err(e) => proxy_bad_gateway(&target_addr, &e.to_string()),
    }
}

fn proxy_not_found(slug: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        [
            ("content-type", "text/html; charset=utf-8"),
        ],
        format!(
            "<html><body><h1>404 — No proxy endpoint '{slug}'</h1>\
             <p>This endpoint has not been registered. \
             Register it via <code>POST /api/proxy</code>.</p></body></html>"
        ),
    )
        .into_response()
}

fn proxy_bad_gateway(target: &str, error: &str) -> Response {
    (
        StatusCode::BAD_GATEWAY,
        [
            ("content-type", "text/html; charset=utf-8"),
        ],
        format!(
            "<html><body><h1>502 — Backend unreachable</h1>\
             <p>Target <code>{target}</code> did not respond.</p>\
             <pre>{error}</pre></body></html>"
        ),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_slug_from_subdomain() {
        assert_eq!(
            ProxyTable::extract_slug_from_host("myapp.olympus.localhost", "olympus.localhost"),
            Some("myapp".to_string())
        );
    }

    #[test]
    fn extract_slug_with_port() {
        assert_eq!(
            ProxyTable::extract_slug_from_host("myapp.olympus.localhost:8799", "olympus.localhost"),
            Some("myapp".to_string())
        );
    }

    #[test]
    fn extract_slug_wrong_domain() {
        assert_eq!(
            ProxyTable::extract_slug_from_host("myapp.other.com", "olympus.localhost"),
            None
        );
    }

    #[test]
    fn extract_slug_base_domain_itself() {
        // olympus.localhost itself is not a slug — no prefix.
        assert_eq!(
            ProxyTable::extract_slug_from_host("olympus.localhost", "olympus.localhost"),
            None
        );
    }

    #[test]
    fn extract_slug_multi_level() {
        // a.b.olympus.localhost — multi-level, skip.
        assert_eq!(
            ProxyTable::extract_slug_from_host("a.b.olympus.localhost", "olympus.localhost"),
            None
        );
    }

    #[tokio::test]
    async fn upsert_and_get() {
        let table = ProxyTable::new();
        let ep = ProxyEndpoint {
            slug: "test-app".into(),
            host: "127.0.0.1".into(),
            port: 3000,
            session_id: Some("sess-1".into()),
            project_id: None,
            auth_mode: ProxyAuth::Public,
            status: ProxyStatus::Active,
        };
        table.upsert(ep.clone()).await;

        let got = table.get("test-app").await.unwrap();
        assert_eq!(got.slug, "test-app");
        assert_eq!(got.port, 3000);
    }

    #[tokio::test]
    async fn upsert_overwrites() {
        let table = ProxyTable::new();
        let ep1 = ProxyEndpoint {
            slug: "app".into(),
            host: "127.0.0.1".into(),
            port: 3000,
            session_id: None,
            project_id: None,
            auth_mode: ProxyAuth::Public,
            status: ProxyStatus::Active,
        };
        table.upsert(ep1).await;

        let ep2 = ProxyEndpoint {
            slug: "app".into(),
            host: "127.0.0.1".into(),
            port: 4000, // port changed
            session_id: None,
            project_id: None,
            auth_mode: ProxyAuth::Public,
            status: ProxyStatus::Active,
        };
        table.upsert(ep2).await;

        let got = table.get("app").await.unwrap();
        assert_eq!(got.port, 4000);
    }

    #[tokio::test]
    async fn remove_endpoint() {
        let table = ProxyTable::new();
        let ep = ProxyEndpoint {
            slug: "tmp".into(),
            host: "127.0.0.1".into(),
            port: 3000,
            session_id: None,
            project_id: None,
            auth_mode: ProxyAuth::Public,
            status: ProxyStatus::Active,
        };
        table.upsert(ep).await;
        assert!(table.get("tmp").await.is_some());
        assert!(table.remove("tmp").await);
        assert!(table.get("tmp").await.is_none());
        assert!(!table.remove("tmp").await); // already removed
    }

    #[tokio::test]
    async fn list_endpoints() {
        let table = ProxyTable::new();
        for i in 0..3 {
            let ep = ProxyEndpoint {
                slug: format!("app-{i}"),
                host: "127.0.0.1".into(),
                port: 3000 + i as u16,
                session_id: None,
                project_id: None,
                auth_mode: ProxyAuth::Public,
                status: ProxyStatus::Active,
            };
            table.upsert(ep).await;
        }
        let list = table.list().await;
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn auth_mode_serialize() {
        let public = ProxyAuth::Public;
        assert_eq!(serde_json::to_string(&public).unwrap(), "\"public\"");

        let scoped = ProxyAuth::SessionScoped;
        assert_eq!(serde_json::to_string(&scoped).unwrap(), "\"sessionscoped\"");
    }

    #[test]
    fn auth_mode_deserialize_token() {
        // Token variant carries a string value.
        let json = r#"{"token":"my-secret"}"#;
        let auth: ProxyAuth = serde_json::from_str(json).unwrap();
        match auth {
            ProxyAuth::Token(s) => assert_eq!(s, "my-secret"),
            _ => panic!("expected Token"),
        }
    }

    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct TokenWrapper {
        #[serde(rename = "authMode")]
        auth_mode: ProxyAuth,
    }

    #[test]
    fn create_proxy_body_deserialize() {
        let json = r#"{"slug":"myapp","host":"127.0.0.1","port":3000,"authMode":"public"}"#;
        let body: CreateProxyBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.slug, "myapp");
        assert_eq!(body.port, 3000);
        assert_eq!(body.auth_mode, ProxyAuth::Public);
    }

    #[test]
    fn create_proxy_body_defaults_to_session_scoped() {
        let json = r#"{"slug":"x","host":"127.0.0.1","port":1}"#;
        let body: CreateProxyBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.auth_mode, ProxyAuth::SessionScoped);
    }
}
