//! Full-text search route (`/api/search`).

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::server::dto::SearchHitDto;
use crate::server::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/search", get(search))
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct SearchQuery {
    q: Option<String>,
    limit: Option<usize>,
}

pub(crate) async fn search(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Response {
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
