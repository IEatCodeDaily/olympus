//! Agent-setup declaration routes (`/api/setup`, ADR 0006 §3).

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use super::support::now_epoch;
use crate::server::dto::SetupDto;
use crate::server::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/setup", get(get_setup).put(put_setup))
}

/// Query params for `GET /api/setup` — which scope's declaration to fetch.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupQuery {
    /// `"org:<org>"` | `"project:<org>/<project>"`. If `effective` is set with
    /// an org+project, returns the merged org+project setup instead of a single
    /// scope's raw declaration.
    scope: Option<String>,
    /// When both are present, return the merged effective setup for the project
    /// (org baseline + project layer). Overrides `scope`.
    org: Option<String>,
    project: Option<String>,
}

/// GET /api/setup?scope=... OR ?org=..&project=.. — the declared agent setup.
///
/// - `?scope=org:acme` → that scope's raw declaration (or empty setup).
/// - `?org=acme&project=web` → the *effective* (merged org+project) setup the
///   envoy would materialize for a session in that project (ADR 0006 §3.1).
pub(crate) async fn get_setup(State(state): State<AppState>, Query(q): Query<SetupQuery>) -> Response {
    let views = state.views.read().await;
    if let (Some(org), Some(project)) = (q.org.as_deref(), q.project.as_deref()) {
        let row = views.setup.effective_for_project(org, project);
        return Json(serde_json::to_value(SetupDto::from_row(&row)).unwrap()).into_response();
    }
    let Some(scope) = q.scope.as_deref() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "missing_scope", "message": "provide ?scope= or ?org=&project=" }),
            ),
        )
            .into_response();
    };
    match views.setup.get(scope) {
        Some(row) => Json(serde_json::to_value(SetupDto::from_row(row)).unwrap()).into_response(),
        None => {
            // An undeclared scope is a valid empty setup, not a 404.
            let empty = crate::server::dto::SetupDto {
                scope: scope.to_string(),
                skills: vec![],
                mcp: vec![],
                plugins: vec![],
                hooks: vec![],
                declared_at: 0.0,
            };
            Json(serde_json::to_value(empty).unwrap()).into_response()
        }
    }
}

/// Body for `PUT /api/setup` — full-replace a scope's declaration (ADR 0006 §3).
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PutSetupBody {
    scope: String,
    #[serde(default)]
    skills: Vec<String>,
    #[serde(default)]
    mcp: Vec<String>,
    #[serde(default)]
    plugins: Vec<String>,
    #[serde(default)]
    hooks: Vec<String>,
}

/// PUT /api/setup — declare (set/replace) a scope's agent setup. PUT semantics:
/// the body fully replaces the scope's prior declaration (ADR 0006 §3).
pub(crate) async fn put_setup(State(state): State<AppState>, Json(body): Json<PutSetupBody>) -> Response {
    // Validate the scope shape: "org:<slug>" or "project:<org>/<project>".
    let scope = body.scope.trim();
    let valid = scope
        .strip_prefix("org:")
        .map(|s| !s.is_empty() && !s.contains('/'))
        .or_else(|| {
            scope
                .strip_prefix("project:")
                .map(|s| s.split('/').filter(|p| !p.is_empty()).count() == 2)
        })
        .unwrap_or(false);
    if !valid {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_scope",
                "message": "scope must be 'org:<slug>' or 'project:<org>/<project>'",
            })),
        )
            .into_response();
    }

    let event = crate::event::Event::SetupDeclared {
        scope: scope.to_string(),
        skills: body.skills,
        mcp: body.mcp,
        plugins: body.plugins,
        hooks: body.hooks,
        declared_at: now_epoch(),
    };
    if let Err(e) = state.log.append(&event) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "log_error", "message": format!("{e:#}") })),
        )
            .into_response();
    }
    let dto = {
        let mut views = state.views.write().await;
        views.apply(&event);
        views.setup.get(scope).map(SetupDto::from_row)
    };
    match dto {
        Some(dto) => Json(serde_json::to_value(&dto).unwrap()).into_response(),
        None => (StatusCode::INTERNAL_SERVER_ERROR, "apply failed").into_response(),
    }
}
