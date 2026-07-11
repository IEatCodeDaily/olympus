//! Fleet node registry routes (`/api/nodes/**`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::server::agents;
use crate::server::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/nodes", get(list_nodes))
        .route("/api/nodes/hall-identity", get(hall_identity))
        .route("/api/nodes/{id}/agents", get(node_agents))
        .route("/api/nodes/{id}/agents/refresh", post(refresh_node_agents))
        .route("/api/nodes/{id}/drain", post(drain_node))
        .route("/api/nodes/{id}", delete(remove_node))
}

/// GET /api/nodes/:id/agents — the agents a specific node's envoy discovered.
pub(crate) async fn node_agents(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.nodes.agents(&id).await {
        Ok(agents) => Json(json!({ "agents": agents })).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/nodes/:id/agents/refresh — re-detect agents on a node (manual, as
/// requested: refreshing to detect newly-installed agents is explicit, not
/// automatic). For the local node the control plane re-runs discovery in-process;
/// a remote node would forward a detect request to its envoy (TODO when the
/// standalone envoy binary lands).
pub(crate) async fn refresh_node_agents(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if id != "local" {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "error": "remote agent refresh requires the node's envoy; only 'local' is supported in-process for now"
            })),
        )
            .into_response();
    }
    let fresh = agents::discover_local_agents();
    match state.nodes.set_agents(&id, fresh).await {
        Ok(agents) => Json(json!({ "agents": agents })).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub(crate) async fn list_nodes(State(state): State<AppState>) -> impl IntoResponse {
    let nodes = state.nodes.list().await;
    Json(json!({ "nodes": nodes }))
}

/// GET /api/nodes/hall-identity — returns the hall's iroh node id (public key,
/// z-base-32) so the installer can fetch it without scraping boot logs (ADR
/// 0008 §1 S7). Returns `{"irohNodeId": null}` when iroh is not enabled.
pub(crate) async fn hall_identity(State(state): State<AppState>) -> impl IntoResponse {
    let id = state.hall_iroh_id.as_ref().map(|s| s.as_str().to_string());
    Json(json!({ "irohNodeId": id }))
}

/// POST /api/nodes/:id/drain — mark a node draining (no new sessions routed
/// to it). Full ADR 0008 §5 drain choreography (resume-then-flip) is S5; this
/// sets the registry state the Fleet view + scheduler already respect.
pub(crate) async fn drain_node(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.nodes.set_draining(&id).await {
        Ok(()) => Json(json!({ "ok": true, "status": "draining" })).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /api/nodes/:id — remove a node from the fleet. Deregisters it,
/// drops its envoy connection, and — for iroh nodes — revokes its key from
/// the hall.toml allowlist so it cannot silently reconnect.
pub(crate) async fn remove_node(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(node) = state.nodes.get(&id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("unknown node: {id}") })),
        )
            .into_response();
    };
    if node.local {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "the local node cannot be removed" })),
        )
            .into_response();
    }

    // Revoke from the allowlist FIRST (fail-closed: if this errors we still
    // deregister, but report the revocation failure so the operator knows the
    // node could reconnect).
    let mut revoked = false;
    if let Some(iroh_id) = &node.iroh_node_id {
        match crate::enroll::allowlist_remove(&state.home, iroh_id) {
            Ok(r) => revoked = r,
            Err(e) => {
                tracing::warn!(node = %id, error = %e, "allowlist revocation failed on node remove");
            }
        }
    }

    state.nodes.deregister(&id).await;
    if let Some(conn) = state.envoy_conns.remove(&id).await {
        conn.fail_all().await;
    }
    Json(json!({ "ok": true, "allowlistRevoked": revoked })).into_response()
}
