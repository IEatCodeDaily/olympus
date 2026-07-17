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
        .route("/api/terminal/targets", get(terminal_targets))
}

/// GET /api/terminal/targets — nodes that can host an operator terminal (ADR
/// 0021 cockpit picker). Always includes the Hall host first (the default
/// target), then every online node advertising `TerminalHost`.
pub(crate) async fn terminal_targets(State(state): State<AppState>) -> Response {
    let mut targets = vec![json!({
        "id": "hall",
        "label": "Hall",
        "kind": "hall",
        "default": true,
    })];
    for node in state.nodes.list().await {
        // Only online nodes that advertise the TerminalHost role are selectable.
        let online = matches!(node.status, crate::node::NodeStatus::Online);
        if !online {
            continue;
        }
        if state
            .nodes
            .has_role(&node.node_id, olympus_proto::frames::NodeRole::TerminalHost)
            .await
        {
            targets.push(json!({
                "id": node.node_id,
                "label": node.hostname,
                "kind": "node",
                "default": false,
            }));
        }
    }
    Json(json!({ "targets": targets })).into_response()
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
/// automatic).
///
/// - `local` (the in-process control-plane host, no envoy connection): re-run
///   discovery in-process.
/// - any node with a connected envoy: send a `Probe` frame over its existing
///   connection and store what it reports. This is what makes "Detect agents"
///   work for the real fleet nodes (the dev node registers as its hostname,
///   not `local`) — the older blanket 501 predated the Probe frame.
/// - a known-but-disconnected node: honest 503, not a silent success.
pub(crate) async fn refresh_node_agents(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    // The in-process host has no envoy connection to itself.
    if id == "local" {
        let fresh = agents::discover_local_agents();
        return match state.nodes.set_agents(&id, fresh).await {
            Ok(agents) => Json(json!({ "agents": agents })).into_response(),
            Err(e) => (StatusCode::NOT_FOUND, Json(json!({ "error": e.to_string() }))).into_response(),
        };
    }

    // Remote node: ask its envoy to re-probe over the live connection.
    let Some(conn) = state.envoy_conns.get(&id).await else {
        // Distinguish "unknown node" from "known but its envoy isn't connected".
        let status = if state.nodes.get(&id).await.is_some() {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::NOT_FOUND
        };
        return (
            status,
            Json(json!({ "error": format!("node '{id}' has no connected envoy to probe") })),
        )
            .into_response();
    };

    let fresh = match probe_agents(&conn).await {
        Ok(agents) => agents,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("probe failed: {e}") })),
            )
                .into_response()
        }
    };
    match state.nodes.set_agents(&id, fresh).await {
        Ok(agents) => Json(json!({ "agents": agents })).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Send a `Probe` frame to an envoy and parse its reported agents. Bounded by a
/// timeout so a wedged envoy can't hang the request.
async fn probe_agents(
    conn: &crate::server::envoy_conn::EnvoyConnection,
) -> Result<Vec<agents::AgentInfo>, String> {
    let rx = conn
        .send_request(olympus_proto::frames::HallFrame::Probe { req_id: 0 })
        .await
        .map_err(|e| e.to_string())?
        .ok_or("probe is a request frame but no response slot was created")?;
    let resp = tokio::time::timeout(std::time::Duration::from_secs(5), rx)
        .await
        .map_err(|_| "envoy did not respond within 5s".to_string())?
        .map_err(|_| "envoy disconnected before responding".to_string())?;
    if !resp.ok {
        return Err(resp.error.unwrap_or_else(|| "envoy reported failure".into()));
    }
    let result = resp.result.ok_or("probe response had no result payload")?;
    let agents = result
        .get("agents")
        .cloned()
        .ok_or("probe response missing 'agents'")?;
    serde_json::from_value(agents).map_err(|e| format!("decoding agents: {e}"))
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
        tokio::spawn(async move { conn.close().await });
    }
    Json(json!({ "ok": true, "allowlistRevoked": revoked })).into_response()
}
