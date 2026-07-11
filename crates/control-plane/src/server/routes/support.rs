//! Shared mutation/utility helpers used across the resource route modules.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::server::dto::CardDto;
use crate::server::ws::ServerFrame;
use crate::server::AppState;

/// Current epoch seconds as f64 (for liveness recency math).
pub(crate) fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Append a card event to the log, apply it to views, broadcast a
/// `CardsChanged` delta, and return the updated card DTO. Shared
/// mutation path for all card write ops.
pub(crate) async fn append_and_apply(state: &AppState, event: crate::event::Event) -> Response {
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

pub(crate) async fn append_and_apply_events(
    state: &AppState,
    events: &[crate::event::Event],
) -> Result<(), Response> {
    if let Err(error) = state.log.append_batch(events) {
        tracing::error!(%error, "failed to append organization-owned events");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to persist events",
        )
            .into_response());
    }
    let mut views = state.views.write().await;
    for event in events {
        views.apply(event);
    }
    Ok(())
}
