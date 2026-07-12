use axum::extract::{Extension, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::path::{Component, Path};

use crate::edge::AuthPolicy;
use crate::server::capability::{CapabilityAuthorizer, CapabilityDecision};
use crate::server::principal::{Membership, Principal};
use crate::server::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/edge/static", post(publish_static))
}

pub(crate) async fn forward_auth(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(route_id) = header(&headers, "x-olympus-route-id") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(route) = state.edge.route(route_id) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let uri = header(&headers, "x-forwarded-uri").unwrap_or_default();
    if !uri
        .split('?')
        .next()
        .is_some_and(|path| path.starts_with(&route.path_prefix))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if route.auth_policy == AuthPolicy::Public {
        return StatusCode::NO_CONTENT.into_response();
    }
    let Some(token) = super::super::identity::session_token(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let user = match state
        .auth_store
        .resolve_session(&token, super::super::identity::unix_timestamp())
    {
        Ok(Some(user)) => user,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(%error, "edge authentication unavailable");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let memberships = match state.auth_store.organizations_for_user(&user.user_id) {
        Ok(memberships) => memberships,
        Err(error) => {
            tracing::error!(%error, "edge authorization unavailable");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let endpoint = state.proxy.get(route_id).await;
    let views = state.views.read().await;
    let (organization_id, session_id) = if let Some(endpoint) = &endpoint {
        let Some(session_id) = endpoint.session_id.as_deref() else {
            return StatusCode::FORBIDDEN.into_response();
        };
        let Some(session) = views.sessions.get(session_id) else {
            return StatusCode::FORBIDDEN.into_response();
        };
        (session.org_id.as_str(), Some(session_id))
    } else if route.path_prefix.starts_with("/artifacts/") {
        let Some(organization_id) = route.path_prefix.split('/').nth(2) else {
            return StatusCode::FORBIDDEN.into_response();
        };
        (organization_id, None)
    } else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if !memberships
        .iter()
        .any(|membership| membership.id == organization_id)
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let principal = Principal::User {
        user_id: user.user_id.clone(),
        username: user.username,
        memberships: memberships
            .into_iter()
            .map(|membership| Membership {
                organization_id: membership.id,
                role: membership.role,
            })
            .collect(),
    };
    if let Some(session_id) = session_id {
        let capability = format!("proxy.route.access:{route_id}");
        if CapabilityAuthorizer::new(&views.sessions, &state.capability_signer)
            .authorize_capability(&principal, session_id, &capability)
            == CapabilityDecision::Deny
        {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    insert_header(&mut response, "x-olympus-user", &user.user_id);
    insert_header(&mut response, "x-olympus-org", organization_id);
    if let Some(session_id) = session_id {
        insert_header(&mut response, "x-olympus-session", session_id);
    }
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("private, no-store"),
    );
    response
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)?
        .to_str()
        .ok()
        .filter(|value| !value.contains(|c: char| c.is_control()))
}

fn insert_header(response: &mut Response, name: &'static str, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        response.headers_mut().insert(name, value);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishStatic {
    organization_id: String,
    session_id: String,
    slug: String,
    relative_path: String,
    content: String,
    #[serde(default)]
    public: bool,
}

async fn publish_static(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<PublishStatic>,
) -> Response {
    if !safe_segment(&body.organization_id) || !safe_segment(&body.slug) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid organization or slug"})),
        )
            .into_response();
    }
    let views = state.views.read().await;
    let Some(session) = views.sessions.get(&body.session_id) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if session.org_id != body.organization_id
        || CapabilityAuthorizer::new(&views.sessions, &state.capability_signer)
            .authorize_capability(
                &principal,
                &body.session_id,
                crate::server::capability::ids::STATIC_PUBLISH,
            )
            == CapabilityDecision::Deny
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    drop(views);
    let relative = Path::new(&body.relative_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid artifact path"})),
        )
            .into_response();
    }
    let root = state
        .home
        .join(&body.organization_id)
        .join("artifacts")
        .join(&body.slug);
    let target = root.join(relative);
    if let Err(error) = publish_artifact(&root, relative, body.content.as_bytes()) {
        tracing::error!(%error, "publishing artifact");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"artifact path is not symlink-safe"})),
        )
            .into_response();
    }
    let route = crate::edge::Route {
        id: format!("static-{}-{}", body.organization_id, body.slug),
        path_prefix: format!("/artifacts/{}/{}/", body.organization_id, body.slug),
        upstream: None,
        artifact_root: Some(root),
        auth_policy: if body.public {
            AuthPolicy::Public
        } else {
            AuthPolicy::SessionScoped
        },
        websocket: false,
    };
    if let Err(error) = state.edge.upsert(route.clone()) {
        let _ = std::fs::remove_file(&target);
        tracing::warn!(%error, "static route registration refused");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"edge unavailable"})),
        )
            .into_response();
    }
    (
        StatusCode::CREATED,
        Json(json!({"path": target, "pathPrefix": route.path_prefix})),
    )
        .into_response()
}

fn safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn publish_artifact(root: &Path, relative: &Path, content: &[u8]) -> anyhow::Result<()> {
    anyhow::ensure!(
        !relative.is_absolute()
            && relative
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "invalid artifact path"
    );
    let mut directory = root.to_path_buf();
    std::fs::create_dir_all(&directory)?;
    reject_symlink(&directory)?;
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            let Component::Normal(segment) = component else {
                anyhow::bail!("invalid artifact path");
            };
            directory.push(segment);
            match std::fs::create_dir(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            reject_symlink(&directory)?;
            anyhow::ensure!(
                std::fs::symlink_metadata(&directory)?.is_dir(),
                "artifact parent is not a directory"
            );
        }
    }
    let target = root.join(relative);
    if target.exists() || std::fs::symlink_metadata(&target).is_ok() {
        reject_symlink(&target)?;
    }
    std::fs::write(&target, content)?;
    reject_symlink(&target)?;
    crate::edge::validate_artifact_root(root)
}

fn reject_symlink(path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        !std::fs::symlink_metadata(path)?.file_type().is_symlink(),
        "artifact path contains a symlink: {}",
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_publish_rejects_parent_traversal() {
        let temporary = tempfile::tempdir().unwrap();
        assert!(publish_artifact(temporary.path(), Path::new("../escape"), b"no").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn artifact_publish_rejects_symlink_parent() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("artifacts");
        let outside = temporary.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        assert!(publish_artifact(&root, Path::new("escape/file.txt"), b"no").is_err());
        assert!(!outside.join("file.txt").exists());
    }
}
