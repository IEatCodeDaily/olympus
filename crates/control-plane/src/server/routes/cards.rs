//! Kanban card routes (`/api/cards/**`).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use super::support::{append_and_apply, append_and_apply_events};
use crate::server::dto::CardDto;
use crate::server::principal::OrgScope;
use crate::server::AppState;
use crate::views::CardFilters;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/cards", get(list_cards).post(create_card))
        .route("/api/cards/{id}", get(get_card))
        .route("/api/cards/{id}/assign", post(assign_card))
        .route("/api/cards/{id}/claim", post(claim_card))
        .route("/api/cards/{id}/block", post(block_card))
        .route("/api/cards/{id}/complete", post(complete_card))
        .route("/api/cards/{id}/reassign", post(reassign_card))
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CardsQuery {
    board_id: Option<String>,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateCardBody {
    board_id: String,
    title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssignCardBody {
    assigned_id: String,
    assigned_kind: String,
    session_id: String,
    attempt_bookmark: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BlockCardBody {
    blocked_by: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReassignCardBody {
    assigned_id: String,
    assigned_kind: String,
    session_id: String,
    attempt_bookmark: String,
    previous_session_id: String,
}

pub(crate) async fn list_cards(
    State(state): State<AppState>,
    scope: Option<axum::extract::Extension<OrgScope>>,
    Query(q): Query<CardsQuery>,
) -> impl IntoResponse {
    let views = state.views.read().await;
    let filters = CardFilters {
        board_id: q.board_id,
        status: q.status,
        organization_id: scope.map(|scope| scope.0.organization_id),
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

pub(crate) async fn get_card(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let views = state.views.read().await;
    match views.cards.get(&id) {
        Some(row) => Json(CardDto::from_row(row)).into_response(),
        None => (StatusCode::NOT_FOUND, "card not found").into_response(),
    }
}

pub(crate) async fn create_card(
    State(state): State<AppState>,
    scope: Option<axum::extract::Extension<OrgScope>>,
    Json(body): Json<CreateCardBody>,
) -> Response {
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
    if let Some(scope) = scope {
        let events = [
            event,
            crate::event::Event::CardOrganizationAssigned {
                card_id: card_id.clone(),
                organization_id: scope.0.organization_id,
            },
        ];
        if let Err(response) = append_and_apply_events(&state, &events).await {
            return response;
        }
        let views = state.views.read().await;
        return views.cards.get(&card_id).map_or_else(
            || StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            |row| (StatusCode::CREATED, Json(CardDto::from_row(row))).into_response(),
        );
    }
    append_and_apply(&state, event).await
}

pub(crate) async fn assign_card(
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

pub(crate) async fn claim_card(State(state): State<AppState>, Path(id): Path<String>) -> Response {
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

pub(crate) async fn block_card(
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

pub(crate) async fn complete_card(State(state): State<AppState>, Path(id): Path<String>) -> Response {
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

pub(crate) async fn reassign_card(
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
