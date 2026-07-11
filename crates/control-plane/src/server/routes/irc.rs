//! In-process IRC bus routes (`/api/irc/*`, ADR 0006 §2).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::server::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/irc/peers", get(list_irc_peers))
        .route("/api/irc/send", post(irc_send))
}

#[derive(Debug, Deserialize)]
pub(crate) struct IrcSendBody {
    from: String,
    to: String,
    content: String,
}

/// GET /api/irc/peers — list registered IRC peers.
pub(crate) async fn list_irc_peers(State(state): State<AppState>) -> Response {
    let peers = state.irc.list_peers().await;
    Json(json!({ "peers": peers })).into_response()
}

/// POST /api/irc/send — send a DM from one peer to another.
pub(crate) async fn irc_send(
    State(state): State<AppState>,
    Json(body): Json<IrcSendBody>,
) -> Response {
    match state.irc.send(&body.from, &body.to, &body.content).await {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
