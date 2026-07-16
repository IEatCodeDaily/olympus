//! Agent + model discovery routes (`/api/models`, `/api/agents/**`).

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::server::agents;
use crate::server::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/models", get(models))
        .route("/api/agents", get(list_agents_handler))
        .route("/api/agents/catalog", get(agent_catalog_handler))
        .route("/api/agents/{id}/models", get(agent_models))
}

pub(crate) async fn models(State(_state): State<AppState>) -> impl IntoResponse {
    // All models across every configured agent (deduped). For an agent-specific
    // list use GET /api/agents/:id/models.
    Json(json!({ "models": agents::list_models() }))
}

/// GET /api/agents/:id/models — models the given agent can actually run
/// (scoped to that agent's provider). This is what keeps the composer's model
/// selector agent-specific — a Codex agent is never offered Claude models.
pub(crate) async fn agent_models(
    State(_state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let provider = agents::list_agents()
        .into_iter()
        .find(|a| a.id == id)
        .and_then(|a| a.provider);
    Json(json!({ "models": agents::list_models_for(provider.as_deref()) }))
}

/// GET /api/agents — flat list of agents across all fleet nodes (deduped by id).
/// Sourced from the node registry (each node's envoy-reported agents), NOT a
/// live control-plane probe. For per-node scoping use /api/agents/catalog.
pub(crate) async fn list_agents_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({ "agents": state.nodes.all_agents().await }))
}

/// GET /api/agents/catalog — per-node agent availability for node-aware session creation.
pub(crate) async fn agent_catalog_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({ "nodes": state.nodes.agent_catalog().await }))
}
