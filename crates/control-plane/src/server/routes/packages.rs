use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::event::Event;
use crate::package::{digest_path, validate_install, PackageManifest};
use crate::server::capability::{ids, CapabilityAuthorizer, CapabilityDecision};
use crate::server::dto::PackageDto;
use crate::server::principal::Principal;
use crate::server::AppState;

use super::support::now_epoch;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/packages", get(list).post(install))
        .route("/api/packages/{id}", get(show).delete(remove))
        .route("/api/packages/{id}/grant", post(grant))
        .route("/api/packages/{id}/activate", post(activate))
        .route("/api/packages/{id}/deactivate", post(deactivate))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallBody {
    manifest: Option<String>,
    path: Option<PathBuf>,
    authority_session_id: String,
    #[serde(default)]
    bindings: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthorityBody {
    authority_session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GrantBody {
    authority_session_id: String,
    capabilities: BTreeSet<String>,
}

async fn list(State(state): State<AppState>) -> Response {
    let views = state.views.read().await;
    let packages: Vec<_> = views
        .registry
        .packages()
        .into_iter()
        .map(PackageDto::from_record)
        .collect();
    Json(json!({"packages": packages})).into_response()
}

async fn show(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let views = state.views.read().await;
    match views.registry.package(&id) {
        Some(package) => Json(PackageDto::from_record(package)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn install(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<InstallBody>,
) -> Response {
    if !authorized(
        &state,
        &principal,
        &body.authority_session_id,
        ids::PACKAGE_INSTALL,
    )
    .await
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (manifest_toml, digest, source) = match (body.manifest, body.path) {
        (Some(manifest), None) => {
            let digest = blake3::hash(manifest.as_bytes()).to_hex().to_string();
            (manifest, digest, "inline".to_string())
        }
        (None, Some(path)) => {
            let manifest_path = if path.is_dir() {
                path.join("olympus-package.toml")
            } else {
                path.clone()
            };
            let root = if path.is_dir() {
                path
            } else {
                path.parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .to_path_buf()
            };
            let manifest = match std::fs::read_to_string(&manifest_path) {
                Ok(manifest) => manifest,
                Err(error) => return bad_request("manifest_read", &error.to_string()),
            };
            let digest = match digest_path(&root) {
                Ok(digest) => digest,
                Err(error) => return bad_request("package_digest", &format!("{error:#}")),
            };
            (manifest, digest, root.display().to_string())
        }
        _ => return bad_request("install_source", "provide exactly one of manifest or path"),
    };
    let manifest = match PackageManifest::parse_toml(&manifest_toml) {
        Ok(manifest) => manifest,
        Err(error) => return bad_request("manifest_schema", &format!("{error:#}")),
    };
    // Keep validation, durable append, and projection update in one critical
    // section. Otherwise two concurrent installs can both pass the immutable
    // identity check and append conflicting events before either is projected.
    let mut views = state.views.write().await;
    let report = {
        if let Some(existing) = views.registry.package(&manifest.package.id) {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error":"immutable_package_identity",
                    "message":format!(
                        "package {} is already installed at version {} with digest {}; package ids are immutable",
                        manifest.package.id, existing.manifest.package.version, existing.digest
                    )
                })),
            )
                .into_response();
        }
        match validate_install(
            &manifest,
            &views.registry.active_capabilities(),
            &body.bindings,
        ) {
            Ok(report) => report,
            Err(error) => return bad_request("install_validation", &format!("{error:#}")),
        }
    };
    let event = Event::PackageInstalledV2 {
        manifest: manifest_toml,
        digest,
        source,
        installed_by: principal_id(&principal),
        installed_at: now_epoch(),
        bindings: body.bindings,
    };
    if let Some(response) = append_apply_locked(&state, &mut views, &event) {
        return response;
    }
    let package = views
        .registry
        .package(&manifest.package.id)
        .expect("installed package projected");
    (
        StatusCode::CREATED,
        Json(json!({"package": PackageDto::from_record(package), "validation": report})),
    )
        .into_response()
}

async fn grant(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    Json(body): Json<GrantBody>,
) -> Response {
    if !authorized(
        &state,
        &principal,
        &body.authority_session_id,
        ids::PACKAGE_GRANT,
    )
    .await
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut views = state.views.write().await;
    {
        let Some(package) = views.registry.package(&id) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        if !body
            .capabilities
            .is_subset(&package.manifest.capabilities.required)
        {
            return bad_request(
                "invalid_grant",
                "grants must be a subset of requested capabilities",
            );
        }
    }
    let event = Event::PackageGranted {
        package_id: id,
        capabilities: body.capabilities.into_iter().collect(),
        granted_by: principal_id(&principal),
        granted_at: now_epoch(),
    };
    match append_apply_locked(&state, &mut views, &event) {
        None => StatusCode::NO_CONTENT.into_response(),
        Some(response) => response,
    }
}

async fn activate(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    Json(body): Json<AuthorityBody>,
) -> Response {
    if !authorized(
        &state,
        &principal,
        &body.authority_session_id,
        ids::PACKAGE_ACTIVATE,
    )
    .await
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    // Serialize validation with append+projection so colliding concurrent
    // activations cannot both report success.
    let mut views = state.views.write().await;
    {
        let Some(package) = views.registry.package(&id) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let unsupported = package.manifest.unsupported_classes();
        if !unsupported.is_empty() {
            return bad_request(
                "unsupported_yet",
                &format!(
                    "unsupported contribution classes: {}",
                    unsupported.join(", ")
                ),
            );
        }
        if !package
            .manifest
            .capabilities
            .required
            .is_subset(&package.granted_capabilities)
        {
            return bad_request(
                "capabilities_not_granted",
                "grant every requested capability before activation",
            );
        }
        if let Err(error) = views.registry.validate_activation(&id) {
            return bad_request("activation_validation", &format!("{error:#}"));
        }
    }
    let event = Event::PackageActivated {
        package_id: id,
        activated_by: principal_id(&principal),
        activated_at: now_epoch(),
    };
    match append_apply_locked(&state, &mut views, &event) {
        None => StatusCode::NO_CONTENT.into_response(),
        Some(response) => response,
    }
}

async fn deactivate(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    Json(body): Json<AuthorityBody>,
) -> Response {
    if !authorized(
        &state,
        &principal,
        &body.authority_session_id,
        ids::PACKAGE_ACTIVATE,
    )
    .await
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    mutate(
        &state,
        Event::PackageDeactivated {
            package_id: id,
            deactivated_by: principal_id(&principal),
            deactivated_at: now_epoch(),
        },
    )
    .await
}

async fn remove(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    Json(body): Json<AuthorityBody>,
) -> Response {
    if !authorized(
        &state,
        &principal,
        &body.authority_session_id,
        ids::PACKAGE_INSTALL,
    )
    .await
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    mutate(
        &state,
        Event::PackageRemoved {
            package_id: id,
            removed_by: principal_id(&principal),
            removed_at: now_epoch(),
        },
    )
    .await
}

async fn authorized(
    state: &AppState,
    principal: &Principal,
    session_id: &str,
    capability: &str,
) -> bool {
    let views = state.views.read().await;
    CapabilityAuthorizer::new(&views.sessions, &state.capability_signer)
        .authorize_capability(principal, session_id, capability)
        == CapabilityDecision::Allow
}

async fn mutate(state: &AppState, event: Event) -> Response {
    // Serialize every package lifecycle append with its projection update.
    let mut views = state.views.write().await;
    if let Some(response) = append_apply_locked(state, &mut views, &event) {
        return response;
    }
    StatusCode::NO_CONTENT.into_response()
}

fn append_apply_locked(
    state: &AppState,
    views: &mut crate::views::ViewManager,
    event: &Event,
) -> Option<Response> {
    if let Err(error) = state.log.append(event) {
        return Some(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"log_error","message":format!("{error:#}")})),
            )
                .into_response(),
        );
    }
    views.apply(event);
    None
}

fn principal_id(principal: &Principal) -> String {
    match principal {
        Principal::User { user_id, .. } => user_id.clone(),
        Principal::Operator => "operator".into(),
    }
}

fn bad_request(code: &str, message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":code,"message":message})),
    )
        .into_response()
}
