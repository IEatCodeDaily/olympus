//! Tail-able event-log route (`/api/events`).

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::server::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/events", get(tail_events))
}

/// Query params for `GET /api/events` (the tail-able event stream).
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EventsQuery {
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
pub(crate) async fn tail_events(State(state): State<AppState>, Query(q): Query<EventsQuery>) -> Response {
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
