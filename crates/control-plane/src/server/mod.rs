//! axum HTTP server: REST read endpoints + auth gate (ADR 0002 §10.3.1, §3.5.2).
//!
//! The `/ws` delta stream lives in [`crate::server::ws`]. This module owns the
//! router, shared state, the auth middleware, and the read-only REST handlers
//! that back the UI's session list, transcript view, and search.

pub mod dto;
pub mod ws;

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::search::SearchIndex;
use crate::views::{Filters, ViewManager};
use dto::{MessageDto, SearchHitDto, SessionDto};
use ws::ServerFrame;

/// Import progress, surfaced on `/api/health`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportState {
    Idle,
    Running,
    Done,
}

impl ImportState {
    fn as_str(&self) -> &'static str {
        match self {
            ImportState::Idle => "idle",
            ImportState::Running => "running",
            ImportState::Done => "done",
        }
    }
}

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
}

/// Build the full router (REST + WS) with the auth gate applied to `/api/*` and
/// `/ws`. `/api/health` is intentionally left unauthenticated so a client can
/// probe readiness before it has the token.
pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/{id}", get(get_session))
        .route(
            "/api/sessions/{id}/messages",
            get(get_messages).post(post_message),
        )
        .route("/api/search", get(search))
        .route("/api/models", get(models))
        .route("/ws", get(ws::ws_handler))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth_gate));

    Router::new()
        .route("/api/health", get(health))
        .merge(protected)
        .layer(cors_layer())
        .with_state(state)
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
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
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

// ---- handlers ----

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "importState": state.import_state.as_str(),
        "snapshot": { "sessions": state.snapshot_sessions, "messages": state.snapshot_messages },
        "syncConnected": false,
        "hermesProfile": state.hermes_profile.as_str(),
    }))
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
        .collect();

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
        Some(row) => Json(SessionDto::from_row(row)).into_response(),
        None => (StatusCode::NOT_FOUND, "session not found").into_response(),
    }
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
    // MVP: model discovery from Hermes config is post-MVP; return an empty list
    // so the UI's model picker degrades gracefully.
    Json(json!({ "models": [] }))
}

#[derive(Debug, Deserialize)]
struct PostMessageBody {
    #[allow(dead_code)]
    text: String,
    #[serde(default)]
    #[allow(dead_code)]
    model: Option<String>,
}

/// POST a message to drive a session.
///
/// Only MANAGED (acp-source) sessions are steerable. Observed sessions
/// (imported telegram/cli/etc.) return 409 — the UI must FORK them into an
/// acp-owned session first (cross-channel continuation, ADR §6.6). The actual
/// prompt delivery + streaming lands with the ACP bridge (Phase 4); until then
/// even managed sessions return 503 so the contract shape is honest and the UI
/// can render the right affordance.
async fn post_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(_body): Json<PostMessageBody>,
) -> Response {
    let views = state.views.read().await;
    let Some(session) = views.sessions.get(&id) else {
        return (StatusCode::NOT_FOUND, "session not found").into_response();
    };
    let managed = session.source == "acp";
    drop(views);

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

    // Managed path: the ACP bridge (Phase 4) is not wired yet.
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "bridge_unavailable",
            "message": "The Hermes ACP bridge is not connected yet — driving managed sessions is coming next.",
        })),
    )
        .into_response()
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
        let state = AppState {
            views: Arc::new(RwLock::new(views)),
            search: Arc::new(RwLock::new(search)),
            token: Arc::new("testtoken".to_string()),
            import_state: ImportState::Done,
            hermes_profile: Arc::new("default".to_string()),
            deltas: tx,
            snapshot_sessions: 1,
            snapshot_messages: 1,
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
            import_state: ImportState::Done,
            hermes_profile: Arc::new("default".to_string()),
            deltas: tx,
            snapshot_sessions: 3,
            snapshot_messages: 0,
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
}
