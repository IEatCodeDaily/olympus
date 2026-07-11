//! Managed repo routes (`/api/repos/**`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use super::support::{append_and_apply, now_epoch};
use crate::server::dto;
use crate::server::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/repos", get(list_repos).post(register_repo))
        .route("/api/repos/{slug}", get(get_repo).delete(remove_repo))
}

pub(crate) async fn list_repos(State(state): State<AppState>) -> Response {
    let views = state.views.read().await;
    let dtos: Vec<dto::RepoDto> = views
        .repos
        .list()
        .iter()
        .map(|r| dto::RepoDto::from_row(r))
        .collect();
    Json(dtos).into_response()
}

pub(crate) async fn get_repo(State(state): State<AppState>, Path(slug): Path<String>) -> Response {
    match state.views.read().await.repos.get(&slug) {
        Some(row) => Json(dto::RepoDto::from_row(row)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct RegisterRepoBody {
    slug: String,
    url: String,
    default_branch: String,
}

pub(crate) async fn register_repo(
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

pub(crate) async fn remove_repo(State(state): State<AppState>, Path(slug): Path<String>) -> Response {
    if state.views.read().await.repos.get(&slug).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let event = crate::event::Event::RepoRemoved {
        slug,
        removed_at: now_epoch(),
    };
    append_and_apply(&state, event).await
}
