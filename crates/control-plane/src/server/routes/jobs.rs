//! Remote job dispatch REST surface (ADR 0011 phase 1).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use olympus_proto::frames::{HallFrame, JobStream, NodeRole};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;

use crate::server::AppState;

static JOBS: OnceLock<RwLock<HashMap<String, JobRecord>>> = OnceLock::new();
static NEXT_JOB: AtomicU64 = AtomicU64::new(1);
fn jobs() -> &'static RwLock<HashMap<String, JobRecord>> {
    JOBS.get_or_init(Default::default)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    pub job_id: String,
    pub node_id: String,
    pub argv: Vec<String>,
    pub provider_package: String,
    pub provider_version: String,
    pub provider_digest: String,
    pub status: String,
    pub output: String,
    pub exit_code: Option<i32>,
    pub truncated: bool,
    pub timed_out: bool,
    pub cancelled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DispatchBody {
    node_id: String,
    argv: Vec<String>,
    #[serde(default)]
    privileged: bool,
    #[serde(default)]
    env_allowlist: Vec<String>,
    cwd: Option<String>,
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

async fn dispatch(State(state): State<AppState>, Json(body): Json<DispatchBody>) -> Response {
    if body.argv.first().is_none_or(String::is_empty) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"argv must contain a program"})),
        )
            .into_response();
    }
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
            Json(json!({"error":"selected job.run provider is not JOBS-1-backed"})),
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
    if let Some(error) = privileged_job_error(
        body.privileged,
        state
            .nodes
            .has_role(&body.node_id, NodeRole::SystemEnvoy)
            .await,
    ) {
        return (StatusCode::CONFLICT, Json(json!({"error":error}))).into_response();
    }
    let Some(conn) = state.envoy_conns.get(&body.node_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"node is not connected"})),
        )
            .into_response();
    };
    let job_id = format!(
        "job-{}-{}",
        std::process::id(),
        NEXT_JOB.fetch_add(1, Ordering::Relaxed)
    );
    let record = JobRecord {
        job_id: job_id.clone(),
        node_id: body.node_id.clone(),
        argv: body.argv.clone(),
        provider_package: provider.package_id,
        provider_version: provider.package_version,
        provider_digest: provider.package_digest,
        status: "running".into(),
        output: String::new(),
        exit_code: None,
        truncated: false,
        timed_out: false,
        cancelled: false,
    };
    jobs().write().await.insert(job_id.clone(), record);
    let frame = HallFrame::DispatchJob {
        req_id: 0,
        job_id: job_id.clone(),
        argv: body.argv,
        env_allowlist: body.env_allowlist,
        cwd: body.cwd,
        timeout_secs: body.timeout_secs,
        max_output_bytes: body.max_output_bytes,
    };
    match conn.send_request(frame).await {
        Ok(Some(rx)) => match rx.await {
            Ok(resp) if resp.ok => {
                (StatusCode::ACCEPTED, Json(json!({"jobId":job_id}))).into_response()
            }
            Ok(resp) => {
                jobs().write().await.remove(&job_id);
                (StatusCode::BAD_GATEWAY, Json(json!({"error":resp.error}))).into_response()
            }
            Err(_) => (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":"envoy disconnected"})),
            )
                .into_response(),
        },
        Ok(None) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error":error.to_string()})),
        )
            .into_response(),
    }
}

async fn get_job(Path(id): Path<String>) -> Response {
    match jobs().read().await.get(&id).cloned() {
        Some(job) => Json(job).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn cancel(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(job) = jobs().read().await.get(&id).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(conn) = state.envoy_conns.get(&job.node_id).await else {
        return StatusCode::CONFLICT.into_response();
    };
    let Ok(Some(rx)) = conn
        .send_request(HallFrame::CancelJob {
            req_id: 0,
            job_id: id,
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

pub async fn apply_output(job_id: &str, stream: JobStream, data: String) {
    if let Some(job) = jobs().write().await.get_mut(job_id) {
        if stream == JobStream::Stderr {
            job.output.push_str("[stderr] ");
        }
        job.output.push_str(&data);
        if job.output.len() > 65_536 {
            job.output.drain(..job.output.len() - 65_536);
        }
    }
}

pub async fn apply_result(
    job_id: &str,
    exit_code: Option<i32>,
    truncated: bool,
    timed_out: bool,
    cancelled: bool,
) {
    if let Some(job) = jobs().write().await.get_mut(job_id) {
        job.status = "completed".into();
        job.exit_code = exit_code;
        job.truncated = truncated;
        job.timed_out = timed_out;
        job.cancelled = cancelled;
    }
}

fn privileged_job_error(privileged: bool, system_envoy: bool) -> Option<&'static str> {
    (privileged && !system_envoy).then_some("SYSTEM_ENVOY_REQUIRED")
}

#[cfg(test)]
mod tests {
    use super::privileged_job_error;

    #[test]
    fn privileged_jobs_require_system_envoy() {
        assert_eq!(
            privileged_job_error(true, false),
            Some("SYSTEM_ENVOY_REQUIRED")
        );
        assert_eq!(privileged_job_error(true, true), None);
        assert_eq!(privileged_job_error(false, false), None);
    }
}
