//! Registry entry routes (`/api/registry`, ADR 0006 §9.4).

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use super::support::now_epoch;
use crate::server::dto::RegistryEntryDto;
use crate::server::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/registry", get(list_registry).put(put_registry_entry))
}

/// Query for `GET /api/registry` — filter by kind.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegistryQuery {
    /// Filter to one kind: "skill" | "mcp" | "plugin" | "hook". Absent → all.
    kind: Option<String>,
}

/// GET /api/registry?kind=mcp — list registry entries (ADR 0006 §9.4).
pub(crate) async fn list_registry(State(state): State<AppState>, Query(q): Query<RegistryQuery>) -> Response {
    let views = state.views.read().await;
    let entries: Vec<RegistryEntryDto> = match q.kind.as_deref() {
        Some(kind) => views.registry.list_kind(kind),
        None => views.registry.list(),
    }
    .iter()
    .map(|e| RegistryEntryDto::from_entry(e))
    .collect();
    Json(json!({ "entries": entries })).into_response()
}

/// Body for `PUT /api/registry` — register (or replace) an entry.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PutRegistryBody {
    kind: String,
    slug: String,
    definition: String,
}

/// PUT /api/registry — register a (kind, slug) → definition entry. PUT semantics
/// (full replace). Validates kind is one of skill/mcp/plugin/hook.
pub(crate) async fn put_registry_entry(
    State(state): State<AppState>,
    Json(body): Json<PutRegistryBody>,
) -> Response {
    let kind = body.kind.trim().to_string();
    if !matches!(kind.as_str(), "skill" | "mcp" | "plugin" | "hook") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_kind",
                "message": "kind must be skill | mcp | plugin | hook",
            })),
        )
            .into_response();
    }
    let slug = body.slug.trim().to_string();
    if slug.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "empty_slug", "message": "slug must be non-empty" })),
        )
            .into_response();
    }

    let event = crate::event::Event::EntryRegistered {
        kind: kind.clone(),
        slug: slug.clone(),
        definition: body.definition,
        registered_at: now_epoch(),
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
        views
            .registry
            .get(&kind, &slug)
            .map(RegistryEntryDto::from_entry)
    };
    match dto {
        Some(dto) => Json(serde_json::to_value(&dto).unwrap()).into_response(),
        None => (StatusCode::INTERNAL_SERVER_ERROR, "apply failed").into_response(),
    }
}
