//! Envoy enrollment routes (one-line node setup). The public token routes are
//! served by `router()`; the operator-authed mint route (`POST /api/enroll`) is
//! registered in the protected group by `build_router`.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::server::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/enroll/{token}/install.sh", get(enroll_install_script))
        .route("/api/enroll/{token}/binary", get(enroll_binary))
        .route("/api/enroll/{token}/status", get(enroll_status))
        .route("/api/enroll/{token}", post(enroll_register))
}

/// Derive the externally-reachable base URL from reverse-proxy headers,
/// validating that the host is a safe hostname (no shell metacharacters).
/// Returns `None` if the host is missing or contains characters that could
/// break out of the baked-in shell command in the install script.
pub(crate) fn derive_base_url(headers: &HeaderMap) -> Option<String> {
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get("host"))?
        .to_str()
        .ok()?;
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    // Hostname + port only: letters, digits, dot, dash, colon, [ ] (for IPv6).
    // Reject anything that could inject shell metacharacters into the install
    // script's curl URL (spaces, $, backticks, |, ;, etc.).
    let safe = host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'));
    if !safe || host.is_empty() {
        return None;
    }
    // Proto is also interpolated; restrict to the known-good set.
    let proto = match proto {
        "http" | "https" => proto,
        _ => "http",
    };
    Some(format!("{proto}://{host}"))
}

/// POST /api/enroll (operator-authed) — mint a short-lived enroll token and
/// return the ready-to-paste one-liner. The public URL is derived from the
/// request's Host header (works for localhost AND the CF-tunnel domain).
pub(crate) async fn mint_enroll(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Remote envoys connect over iroh; without a hall identity the installer
    // can't configure the envoy's --hall target. Fail honestly.
    let Some(hall_iroh) = state.hall_iroh_id.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "hall iroh endpoint is not bound — remote enrollment unavailable" })),
        )
            .into_response();
    };

    let Some(base) = derive_base_url(&headers) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "could not derive a safe base URL from the Host header" })),
        )
            .into_response();
    };
    let token = state.enroll.mint().await;
    // Enrollment endpoints must be reached directly. Refuse redirects so an
    // edge login/error page can never be piped into bash as if it were the
    // installer.
    let command = format!("curl -fsSL --max-redirs 0 {base}/api/enroll/{token}/install.sh | bash");

    Json(json!({
        "token": token,
        "command": command,
        "expiresInSecs": crate::enroll::ENROLL_TTL.as_secs(),
        "hallIrohId": hall_iroh.as_str(),
    }))
    .into_response()
}

/// GET /api/enroll/:token/install.sh — the bootstrap script with the Hall
/// URL, enroll token, and Hall iroh id baked in. Token-gated.
pub(crate) async fn enroll_install_script(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !state.enroll.is_valid(&token).await {
        return (StatusCode::FORBIDDEN, "enroll token invalid or expired").into_response();
    }
    let Some(hall_iroh) = state.hall_iroh_id.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "hall iroh endpoint is not bound",
        )
            .into_response();
    };
    let Some(base) = derive_base_url(&headers) else {
        return (
            StatusCode::BAD_REQUEST,
            "could not derive a safe base URL from the Host header",
        )
            .into_response();
    };

    let script = include_str!("../../../../../scripts/envoy-bootstrap.sh")
        .replace("{{HALL_URL}}", &base)
        .replace("{{ENROLL_TOKEN}}", &token)
        .replace("{{HALL_IROH_ID}}", hall_iroh.as_str());

    (
        StatusCode::OK,
        [("content-type", "text/x-shellscript; charset=utf-8")],
        script,
    )
        .into_response()
}

/// GET /api/enroll/:token/binary — serve the olympus-envoy binary from
/// `<home>/bin/olympus-envoy` (the deployed symlink). Token-gated. Streaming
/// is unnecessary at ~20MB; read + send is fine for an enrollment path.
pub(crate) async fn enroll_binary(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Response {
    if !state.enroll.is_valid(&token).await {
        return (StatusCode::FORBIDDEN, "enroll token invalid or expired").into_response();
    }
    let path = state.home.join("bin").join("olympus-envoy");
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [("content-type", "application/octet-stream")],
            bytes,
        )
            .into_response(),
        Err(e) => {
            tracing::error!(path = %path.display(), error = %e, "envoy binary missing for enrollment");
            (
                StatusCode::NOT_FOUND,
                "olympus-envoy binary not found on the hall — run scripts/deploy.sh envoy first",
            )
                .into_response()
        }
    }
}

/// Body for POST /api/enroll/:token — the installer reporting the new envoy's
/// iroh node id.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EnrollRegisterBody {
    iroh_node_id: String,
    /// Informational (the node's chosen id); logged, not trusted for auth.
    #[serde(default)]
    node_id: Option<String>,
}

/// POST /api/enroll/:token — register the envoy's iroh node id: consumes the
/// token (single registration) and appends the id to the hall.toml allowlist.
pub(crate) async fn enroll_register(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(body): Json<EnrollRegisterBody>,
) -> Response {
    use crate::enroll::ConsumeOutcome;
    match state.enroll.consume(&token, &body.iroh_node_id).await {
        ConsumeOutcome::Accepted | ConsumeOutcome::AlreadyRegistered => {}
        ConsumeOutcome::Rejected => {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "enroll token invalid, expired, or already used" })),
            )
                .into_response();
        }
    }
    match crate::enroll::allowlist_add(&state.home, &body.iroh_node_id) {
        Ok(added) => {
            let node_id = body.node_id.as_deref().unwrap_or(&body.iroh_node_id);
            if let Err(e) = state.nodes.enroll(node_id, &body.iroh_node_id).await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
            tracing::info!(
                iroh = %body.iroh_node_id,
                node = %node_id,
                added,
                "envoy enrolled via one-line installer"
            );
            Json(json!({ "ok": true, "added": added })).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/enroll/:token/status — is the enrolled node online yet? Token-gated
/// polling endpoint for the installer's final verification step.
pub(crate) async fn enroll_status(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Response {
    if !state.enroll.is_valid(&token).await {
        return (StatusCode::FORBIDDEN, "enroll token invalid or expired").into_response();
    }
    // A freshly-enrolled envoy is the newest iroh-transport node; report all
    // online iroh nodes so the installer can grep for its own.
    let nodes = state.nodes.list().await;
    let online = nodes.iter().any(|n| {
        n.transport == crate::node::NodeTransport::Iroh
            && n.status == crate::node::NodeStatus::Online
    });
    Json(json!({
        "online": online,
        "nodes": nodes
            .iter()
            .map(|n| json!({ "nodeId": n.node_id, "status": n.status, "transport": n.transport }))
            .collect::<Vec<_>>(),
    }))
    .into_response()
}
