//! Durable remote job dispatch REST surface (ADR 0017 §5).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use olympus_proto::frames::{HallFrame, NodeRole};
use serde::Deserialize;
use serde_json::json;

use crate::event::Event;
use crate::server::principal::Principal;
use crate::server::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DispatchBody {
    node_id: String,
    argv: Vec<String>,
    #[serde(default)]
    env_allowlist: Vec<String>,
    cwd: Option<String>,
    initiating_session: Option<String>,
    organization_id: Option<String>,
    #[serde(default = "default_timeout")]
    timeout_secs: u64,
    #[serde(default = "default_output")]
    max_output_bytes: u64,
}

fn default_timeout() -> u64 {
    3600
}
fn default_output() -> u64 {
    16 * 1024 * 1024
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/jobs", post(dispatch))
        .route("/api/jobs/{id}", get(get_job).delete(cancel))
}

async fn dispatch(
    State(state): State<AppState>,
    principal: Principal,
    Json(body): Json<DispatchBody>,
) -> Response {
    if body.argv.first().is_none_or(String::is_empty) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"argv must contain a program"})),
        )
            .into_response();
    }
    let Principal::Operator = principal else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let principal = "operator".to_string();
    let organization_id = body
        .organization_id
        .clone()
        .unwrap_or_else(|| "default".into());
    let provider = {
        let views = state.views.read().await;
        views.registry.resolve_activity("job.run")
    };
    let Some(provider) = provider else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"no active provider for job.run"})),
        )
            .into_response();
    };
    if provider
        .definition
        .get("backend")
        .and_then(toml::Value::as_str)
        != Some("jobs")
    {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({"error":"selected job.run provider is not jobs-backed"})),
        )
            .into_response();
    }
    if !state
        .nodes
        .has_role(&body.node_id, NodeRole::JobRunner)
        .await
    {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"node does not advertise job_runner"})),
        )
            .into_response();
    }
    let Some(conn) = state.envoy_conns.get(&body.node_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"node is not connected"})),
        )
            .into_response();
    };

    let job_id = format!("job-{}", uuid::Uuid::new_v4());
    let attempt_epoch = 1;
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64());
    let intent = Event::JobDispatchIntent {
        job_id: job_id.clone(),
        attempt_epoch,
        organization_id,
        initiating_principal: principal,
        initiating_session: body.initiating_session,
        node_id: body.node_id,
        package_id: provider.package_id.clone(),
        package_version: provider.package_version.clone(),
        package_digest: provider.package_digest.clone(),
        activity: "job.run".into(),
        argv: body.argv.clone(),
        cwd: body.cwd.clone(),
        env_allowlist: body.env_allowlist.clone(),
        timeout_secs: body.timeout_secs,
        max_output_bytes: body.max_output_bytes,
        created_at,
    };
    if let Err(error) = state.jobs.create(intent) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string()})),
        )
            .into_response();
    }

    let frame = HallFrame::DispatchJob {
        req_id: 0,
        job_id: job_id.clone(),
        attempt_epoch,
        package_id: provider.package_id,
        package_version: provider.package_version,
        package_digest: provider.package_digest,
        activity: "job.run".into(),
        argv: body.argv,
        env_allowlist: body.env_allowlist,
        cwd: body.cwd,
        timeout_secs: body.timeout_secs,
        max_output_bytes: body.max_output_bytes,
    };
    let response = match conn.send_request(frame).await {
        Ok(Some(rx)) => rx.await.ok(),
        _ => None,
    };
    if response.as_ref().is_some_and(|response| response.ok) {
        (
            StatusCode::ACCEPTED,
            Json(json!({"jobId":job_id, "attemptEpoch":attempt_epoch})),
        )
            .into_response()
    } else {
        let acknowledgement_unknown = response.is_none();
        let reason = response
            .and_then(|response| response.error)
            .unwrap_or_else(|| "envoy disconnected before dispatch acknowledgement".into());
        let _ = if acknowledgement_unknown {
            state.jobs.dispatch_indeterminate(
                &job_id,
                attempt_epoch,
                "dispatch_acknowledgement_unknown".into(),
            )
        } else {
            state
                .jobs
                .dispatch_failed(&job_id, attempt_epoch, reason.clone())
        };
        (StatusCode::BAD_GATEWAY, Json(json!({"error":reason}))).into_response()
    }
}

async fn get_job(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
) -> Response {
    if !matches!(principal, Principal::Operator) {
        return StatusCode::FORBIDDEN.into_response();
    }
    state.jobs.get(&id).map_or_else(
        || StatusCode::NOT_FOUND.into_response(),
        |job| Json(job).into_response(),
    )
}

async fn cancel(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
) -> Response {
    if !matches!(principal, Principal::Operator) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(job) = state.jobs.get(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(conn) = state.envoy_conns.get(&job.node_id).await else {
        return StatusCode::CONFLICT.into_response();
    };
    let Ok(Some(rx)) = conn
        .send_request(HallFrame::CancelJob {
            req_id: 0,
            job_id: id,
            attempt_epoch: job.attempt_epoch,
        })
        .await
    else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    if rx.await.is_ok_and(|response| response.ok) {
        StatusCode::ACCEPTED.into_response()
    } else {
        StatusCode::BAD_GATEWAY.into_response()
    }
}
