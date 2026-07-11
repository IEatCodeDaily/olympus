//! Project (context container) routes (`/api/projects/**`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use super::support::{append_and_apply, append_and_apply_events};
use crate::server::dto::ProjectDto;
use crate::server::principal::OrgScope;
use crate::server::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route(
            "/api/projects/{id}",
            get(get_project).patch(patch_project).delete(delete_project),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateProjectBody {
    name: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PatchProjectBody {
    name: Option<String>,
    vaults: Option<Vec<String>>,
    repos: Option<Vec<String>>,
    boards: Option<Vec<String>>,
}

pub(crate) async fn list_projects(
    State(state): State<AppState>,
    scope: Option<axum::extract::Extension<OrgScope>>,
) -> Response {
    let views = state.views.read().await;
    let rows: Vec<ProjectDto> = views
        .projects
        .list()
        .into_iter()
        .filter(|row| {
            scope
                .as_ref()
                .is_none_or(|scope| row.org_id == scope.0.organization_id)
        })
        .map(ProjectDto::from_row)
        .collect();
    Json(json!({ "projects": rows, "total": rows.len() })).into_response()
}

pub(crate) async fn get_project(
    State(state): State<AppState>,
    scope: Option<axum::extract::Extension<OrgScope>>,
    Path(id): Path<String>,
) -> Response {
    let views = state.views.read().await;
    match views.projects.get(&id) {
        Some(row)
            if scope
                .as_ref()
                .is_none_or(|scope| row.org_id == scope.0.organization_id) =>
        {
            Json(ProjectDto::from_row(row)).into_response()
        }
        None | Some(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "not_found", "message": "project not found" })),
        )
            .into_response(),
    }
}

pub(crate) async fn create_project(
    State(state): State<AppState>,
    scope: Option<axum::extract::Extension<OrgScope>>,
    Json(body): Json<CreateProjectBody>,
) -> Response {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid", "message": "name is required" })),
        )
            .into_response();
    }
    let project_id = uuid::Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let event = crate::event::Event::ProjectCreated {
        project_id: project_id.clone(),
        name: name.clone(),
        created_at: now,
    };
    // Persist manifest to disk (best-effort; event is the source of truth).
    let _ = state.projects.create(&project_id, &name, now);
    let mut events = vec![event];
    if let Some(scope) = scope {
        events.push(crate::event::Event::ProjectOrganizationAssigned {
            project_id: project_id.clone(),
            organization_id: scope.0.organization_id,
        });
    }
    if let Err(response) = append_and_apply_events(&state, &events).await {
        return response;
    }
    let views = state.views.read().await;
    match views.projects.get(&project_id) {
        Some(row) => (StatusCode::CREATED, Json(ProjectDto::from_row(row))).into_response(),
        None => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn patch_project(
    State(state): State<AppState>,
    scope: Option<axum::extract::Extension<OrgScope>>,
    Path(id): Path<String>,
    Json(body): Json<PatchProjectBody>,
) -> Response {
    {
        let views = state.views.read().await;
        if views.projects.get(&id).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "not_found", "message": "project not found" })),
            )
                .into_response();
        }
    }
    if state
        .views
        .read()
        .await
        .projects
        .get(&id)
        .is_none_or(|project| {
            scope
                .as_ref()
                .is_some_and(|scope| project.org_id != scope.0.organization_id)
        })
    {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "not_found", "message": "project not found" })),
        )
            .into_response();
    }
    // Update on-disk manifest (best-effort).
    let _ = state.projects.update(
        &id,
        body.name.as_deref(),
        body.vaults.as_deref(),
        body.repos.as_deref(),
        body.boards.as_deref(),
    );
    let event = crate::event::Event::ProjectUpdated {
        project_id: id.clone(),
        name: body.name,
        vaults: body.vaults,
        repos: body.repos,
        boards: body.boards,
    };
    append_and_apply(&state, event).await;
    let views = state.views.read().await;
    match views.projects.get(&id) {
        Some(row) => Json(ProjectDto::from_row(row)).into_response(),
        None => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn delete_project(
    State(state): State<AppState>,
    scope: Option<axum::extract::Extension<OrgScope>>,
    Path(id): Path<String>,
) -> Response {
    {
        let views = state.views.read().await;
        if views.projects.get(&id).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "not_found", "message": "project not found" })),
            )
                .into_response();
        }
    }
    if state
        .views
        .read()
        .await
        .projects
        .get(&id)
        .is_none_or(|project| {
            scope
                .as_ref()
                .is_some_and(|scope| project.org_id != scope.0.organization_id)
        })
    {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "not_found", "message": "project not found" })),
        )
            .into_response();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let event = crate::event::Event::ProjectDeleted {
        project_id: id.clone(),
        deleted_at: now,
    };
    append_and_apply(&state, event).await;
    StatusCode::NO_CONTENT.into_response()
}
