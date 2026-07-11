//! Authenticated-client auth + organization-scoped resource routes.
//!
//! Hosts the org-scoped aliases (`organization_resource_routes`) that reuse the
//! per-resource handlers, plus the rewriting proxy that enforces org ownership
//! before dispatching into the unscoped router.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::Router;
use tower::ServiceExt;

use super::cards::*;
use super::projects::*;
use super::sessions::*;
use super::vaults::*;
use crate::server::identity;
use crate::server::principal::OrgScope;
use crate::server::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/auth/session", get(identity::current_session))
        .route("/api/auth/logout", post(identity::logout))
        .route("/api/organizations", get(identity::list_organizations))
        .route(
            "/api/organizations/{organization_id}/{*resource}",
            any(organization_resource_proxy),
        )
        .route(
            "/api/organizations/{organization_id}",
            any(organization_resource_proxy),
        )
}

/// Organization-scoped aliases for resource APIs used by authenticated UI clients.
/// Legacy unscoped routes remain temporarily available for installation-token clients.
pub(crate) fn organization_resource_routes() -> Router<AppState> {
    Router::new()
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{id}", get(get_session).patch(patch_session))
        .route("/sessions/{id}/fork", post(fork_session))
        .route("/sessions/{id}/handover", post(handover_session))
        .route(
            "/sessions/{id}/messages",
            get(get_messages).post(post_message),
        )
        .route("/sessions/{id}/cancel", post(cancel_session))
        .route("/sessions/{id}/steer", post(steer_session))
        .route(
            "/sessions/{id}/permission",
            post(respond_permission_handler),
        )
        .route(
            "/sessions/{id}/subsessions",
            get(list_subsessions).post(create_subsession),
        )
        .route("/sessions/{id}/complete", post(complete_session))
        .route("/cards", get(list_cards).post(create_card))
        .route("/cards/{id}", get(get_card))
        .route("/cards/{id}/assign", post(assign_card))
        .route("/cards/{id}/claim", post(claim_card))
        .route("/cards/{id}/block", post(block_card))
        .route("/cards/{id}/complete", post(complete_card))
        .route("/cards/{id}/reassign", post(reassign_card))
        .route("/projects", get(list_projects).post(create_project))
        .route(
            "/projects/{id}",
            get(get_project).patch(patch_project).delete(delete_project),
        )
        .route("/sessions/{id}/project", post(attach_session_project))
        .route("/vaults", get(list_vaults).post(create_vault))
        .route("/vaults/{id}/notes", get(list_vault_notes))
        .route("/vaults/{id}/documents", get(list_vault_documents))
        .route(
            "/vaults/{id}/note",
            get(get_vault_note)
                .put(put_vault_note)
                .delete(delete_vault_note),
        )
        .route("/vaults/{id}/graph", get(get_vault_graph))
        .route("/vaults/{id}/collections", get(list_vault_collections))
        .route("/vaults/{id}/collections/{path}", get(get_collection_rows))
}

pub(crate) async fn organization_resource_proxy(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> Response {
    let original = request.uri();
    let Some(suffix) = original
        .path()
        .strip_prefix("/api/organizations/")
        .and_then(|path| {
            path.split_once('/')
                .map(|(_, resource)| resource.to_string())
        })
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let organization_id = original
        .path()
        .strip_prefix("/api/organizations/")
        .and_then(|path| path.split_once('/').map(|(id, _)| id.to_string()))
        .expect("organization route matched");
    if let Some(session_id) = suffix
        .strip_prefix("sessions/")
        .and_then(|path| path.split('/').next())
    {
        let views = state.views.read().await;
        if views
            .sessions
            .get(session_id)
            .is_none_or(|session| session.org_id != organization_id)
        {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        }
    }
    if let Some(card_id) = suffix
        .strip_prefix("cards/")
        .and_then(|path| path.split('/').next())
    {
        let views = state.views.read().await;
        if views
            .cards
            .get(card_id)
            .is_none_or(|card| card.org_id != organization_id)
        {
            return (StatusCode::NOT_FOUND, "card not found").into_response();
        }
    }
    if let Some(project_id) = suffix
        .strip_prefix("projects/")
        .and_then(|path| path.split('/').next())
    {
        let views = state.views.read().await;
        if views
            .projects
            .get(project_id)
            .is_none_or(|project| project.org_id != organization_id)
        {
            return (StatusCode::NOT_FOUND, "project not found").into_response();
        }
    }
    let rewritten = match original.query() {
        Some(query) => format!("/{suffix}?{query}"),
        None => format!("/{suffix}"),
    };
    let Ok(uri) = rewritten.parse() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    // Rebuild the request rather than only changing its URI. Axum stores
    // matched path parameters in request extensions; forwarding those would
    // leak `organization_id` into the inner router and break its existing
    // `Path<String>` extractors.
    let (parts, body) = request.into_parts();
    let mut request = axum::extract::Request::new(body);
    *request.method_mut() = parts.method;
    *request.uri_mut() = uri;
    *request.version_mut() = parts.version;
    *request.headers_mut() = parts.headers;
    request.extensions_mut().insert(OrgScope {
        organization_id: organization_id.clone(),
        role: "validated".to_string(),
        admin: false,
    });
    let scoped_state = if suffix == "vaults" || suffix.starts_with("vaults/") {
        let vaults = match state.vaults.for_organization(&organization_id) {
            Ok(vaults) => Arc::new(vaults),
            Err(_) => return StatusCode::BAD_REQUEST.into_response(),
        };
        AppState { vaults, ..state }
    } else if suffix == "projects" || suffix.starts_with("projects/") {
        let projects = Arc::new(crate::projects::ProjectStore::new(
            state.home.join(&organization_id),
        ));
        AppState { projects, ..state }
    } else {
        state
    };
    organization_resource_routes()
        .with_state(scoped_state)
        .oneshot(request)
        .await
        .expect("organization resource router is infallible")
}
