//! axum HTTP server: REST read endpoints + auth gate (ADR 0002 §10.3.1, §3.5.2).
//!
//! The `/ws` delta stream lives in [`crate::server::ws`]. This module owns the
//! router, shared state, the auth middleware, and the read-only REST handlers
//! that back the UI's session list, transcript view, and search.

pub mod bridge_mgr;
pub mod dto;
pub mod ws;

#[cfg(test)]
pub mod test_support;

use std::sync::Arc;

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
use dto::{CardDto, MessageDto, SearchHitDto, SessionDto};
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
    /// The durable event log — sole source of truth. Appended to on new
    /// session creation and message events.
    pub log: Arc<Log>,
    /// Bridge manager: owns agent runtimes for managed (olympus-source) sessions.
    pub bridge: Arc<BridgeManager>,
}

/// Build the full router (REST + WS) with the auth gate applied to `/api/*` and
/// `/ws`. `/api/health` is intentionally left unauthenticated so a client can
/// probe readiness before it has the token.
pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", get(get_session))
        .route(
            "/api/sessions/{id}/messages",
            get(get_messages).post(post_message),
        )
        .route("/api/search", get(search))
        .route("/api/models", get(models))
        .route("/api/cards", get(list_cards).post(create_card))
        .route("/api/cards/{id}", get(get_card))
        .route("/api/cards/{id}/assign", post(assign_card))
        .route("/api/cards/{id}/claim", post(claim_card))
        .route("/api/cards/{id}/block", post(block_card))
        .route("/api/cards/{id}/complete", post(complete_card))
        .route("/api/cards/{id}/reassign", post(reassign_card))
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
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::OPTIONS])
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
    text: String,
    #[serde(default)]
    model: Option<String>,
}

/// POST /api/sessions — create a new Olympus-managed chat session.
///
/// Spawns a bridge runtime, performs the ACP handshake (session/new), appends a
/// SessionCreated event to the log, broadcasts session.added, and returns the
/// new Session DTO with 201.
async fn create_session(State(state): State<AppState>) -> Response {
    match state.bridge.create_session().await {
        Ok(ns) => {
            // Rebuild the views to pick up the new session from the log.
            let dto = {
                let mut views = state.views.write().await;
                if let Ok(events) = state.log.read_all() {
                    for (_seq, event) in events.iter().rev() {
                        if let crate::event::Event::SessionCreated { session_id, .. } = event {
                            if session_id == &ns.session_id {
                                views.apply(event);
                                break;
                            }
                        }
                    }
                }
                // Build the DTO from the view row while we hold the lock.
                match views.sessions.get(&ns.session_id) {
                    Some(r) => SessionDto::from_row(r),
                    None => SessionDto {
                        id: ns.session_id.clone(),
                        hermes_id: ns.hermes_id.clone(),
                        org_id: "personal".into(),
                        owner_id: "rpw".into(),
                        context_id: None,
                        source: "olympus".into(),
                        model: None,
                        title: None,
                        started_at: 0.0,
                        last_activity: 0.0,
                        message_count: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                        archived: false,
                        forked_from: None,
                        fork_point: None,
                        fork_type: None,
                        managed: true,
                    },
                }
            };

            // Broadcast session.added to WS subscribers.
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
            tracing::error!(error = %e, "bridge create_session failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "bridge_error",
                    "message": format!("Failed to create agent session: {e}"),
                })),
            )
                .into_response()
        }
    }
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
    let views = state.views.read().await;
    let Some(session) = views.sessions.get(&id) else {
        return (StatusCode::NOT_FOUND, "session not found").into_response();
    };
    let managed = session.source == "olympus" || session.source == "acp";
    let hermes_id = session.hermes_id.clone();
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

    // Look up the runtime for this session.
    let Some(runtime) = state.bridge.get_runtime(&id).await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "bridge_unavailable",
                "message": "No active agent runtime for this session.",
            })),
        )
            .into_response();
    };

    // Record the user message in the log.
    if let Err(e) = state
        .bridge
        .append_user_message(&id, &hermes_id, &body.text)
    {
        tracing::warn!(error = %e, "failed to append user message");
    }

    // Send the prompt.
    let cmd = AgentCommand::Prompt {
        text: body.text,
        model: body.model,
    };
    if let Err(e) = runtime.send(cmd).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "send_failed", "message": format!("{e}") })),
        )
            .into_response();
    }

    // Spawn a task to drain the runtime event stream and broadcast WS frames.
    let session_id = id.clone();
    let deltas = state.deltas.clone();
    let bridge = state.bridge.clone();
    let hermes_id_clone = hermes_id.clone();
    tokio::spawn(async move {
        use futures::stream::StreamExt;
        let mut stream = runtime.events();
        let mut assistant_text = String::new();
        let mut assistant_msg_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        while let Some(event) = stream.next().await {
            match event {
                AgentEvent::Text(chunk) => {
                    assistant_text.push_str(&chunk);
                    let _ = deltas.send(ServerFrame::MessageDelta {
                        session_id: session_id.clone(),
                        message_id: assistant_msg_id,
                        text_delta: chunk,
                    });
                }
                AgentEvent::Done { finish_reason } => {
                    let _ = deltas.send(ServerFrame::MessageDone {
                        session_id: session_id.clone(),
                        message_id: assistant_msg_id,
                        finish_reason: finish_reason.clone(),
                    });
                    // Persist the final assistant message.
                    let _ = bridge.append_assistant_message(
                        &session_id,
                        &hermes_id_clone,
                        assistant_msg_id,
                        &assistant_text,
                        finish_reason.as_deref(),
                    );
                    break;
                }
                AgentEvent::Error(e) => {
                    tracing::warn!(error = %e, session = %session_id, "agent error event");
                    let _ = deltas.send(ServerFrame::MessageDone {
                        session_id: session_id.clone(),
                        message_id: assistant_msg_id,
                        finish_reason: Some(format!("error: {e}")),
                    });
                    break;
                }
                AgentEvent::ToolCall { .. } | AgentEvent::Reasoning(_) => {
                    // Forward tool calls / reasoning in a future iteration.
                    // For now, accumulate silently.
                }
            }
            assistant_msg_id += 1;
        }
    });

    (StatusCode::ACCEPTED, Json(json!({ "accepted": true }))).into_response()
}

// ---- card handlers (C1) ----

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
    let event = crate::event::Event::CardAssigned {
        card_id: id,
        assigned_id: body.assigned_id,
        assigned_kind: body.assigned_kind,
        session_id: body.session_id,
        attempt_bookmark: body.attempt_bookmark,
        assigned_at: now,
    };
    append_and_apply(&state, event).await
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
            import_state: ImportState::Done,
            hermes_profile: Arc::new("default".to_string()),
            deltas: tx,
            snapshot_sessions: 1,
            snapshot_messages: 1,
            log: log_arc,
            bridge: Arc::new(BridgeManager::with_factory(
                Arc::new(Log::open(&dir.path().join("bridge-log.redb")).unwrap()),
                test_support::mock_factory(),
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
            log: Arc::new(log),
            bridge: Arc::new(BridgeManager::with_factory(
                Arc::new(Log::open(&dir.path().join("bridge-log.redb")).unwrap()),
                test_support::mock_factory(),
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

    // ---- A2: POST /api/sessions (new managed Olympus chat) ----

    #[tokio::test]
    async fn post_sessions_creates_managed_olympus_session() {
        // POST /api/sessions with no body → creates a new Olympus-managed session
        // via the bridge, returns 201 with a Session DTO where source="olympus"
        // and managed=true.
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
        assert!(v["hermesId"].is_string());
        assert!(v["id"].is_string());
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
}
