use axum::{
    extract::{FromRef, FromRequestParts},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};

use super::{identity, AppState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Principal {
    User {
        user_id: String,
        username: String,
        memberships: Vec<Membership>,
    },
    Operator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Membership {
    pub organization_id: String,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrgScope {
    pub organization_id: String,
    pub role: String,
    pub admin: bool,
}

impl OrgScope {
    fn member(organization_id: &str, role: &str) -> Self {
        Self {
            organization_id: organization_id.to_string(),
            role: role.to_string(),
            admin: false,
        }
    }

    fn admin(organization_id: &str) -> Self {
        Self {
            organization_id: organization_id.to_string(),
            role: "admin".to_string(),
            admin: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RouteClass<'a> {
    User,
    Organization(Option<&'a str>),
    Operator,
    Admin,
}

pub fn route_class(path: &str) -> RouteClass<'_> {
    if let Some(suffix) = path.strip_prefix("/api/organizations/") {
        let organization_id = suffix
            .split_once('/')
            .filter(|(_, resource)| !resource.is_empty())
            .map(|(organization_id, _)| organization_id)
            .filter(|organization_id| !organization_id.is_empty());
        return RouteClass::Organization(organization_id);
    }
    if path == "/api/enroll" {
        return RouteClass::Admin;
    }
    if path == "/ws"
        || path == "/api/auth/session"
        || path == "/api/auth/logout"
        || path == "/api/organizations"
        || path == "/api/models"
        || path == "/api/agents"
        || (path.starts_with("/api/agents/") && path.ends_with("/models"))
        || path == "/api/nodes/hall-identity"
    {
        return RouteClass::User;
    }
    RouteClass::Operator
}

pub fn authorize(
    principal: &Principal,
    route: RouteClass<'_>,
    organization_exists: bool,
) -> Result<Option<OrgScope>, StatusCode> {
    match route {
        RouteClass::User => Ok(None),
        RouteClass::Operator => match principal {
            Principal::Operator => Ok(None),
            Principal::User { .. } => Err(StatusCode::FORBIDDEN),
        },
        RouteClass::Admin => match principal {
            Principal::Operator => Ok(None),
            Principal::User { memberships, .. }
                if memberships
                    .iter()
                    .any(|membership| membership.role == "owner") =>
            {
                Ok(None)
            }
            Principal::User { .. } => Err(StatusCode::FORBIDDEN),
        },
        RouteClass::Organization(None) => Err(StatusCode::BAD_REQUEST),
        RouteClass::Organization(Some(_)) if !organization_exists => Err(StatusCode::NOT_FOUND),
        RouteClass::Organization(Some(organization_id)) => match principal {
            Principal::Operator => Ok(Some(OrgScope::admin(organization_id))),
            Principal::User { memberships, .. } => memberships
                .iter()
                .find(|membership| membership.organization_id == organization_id)
                .map(|membership| Some(OrgScope::member(organization_id, &membership.role)))
                .ok_or(StatusCode::FORBIDDEN),
        },
    }
}

impl<S> FromRequestParts<S> for Principal
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = AppState::from_ref(state);
        let authorization = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        let bearer = state.allow_installation_token
            && crate::auth::bearer_ok(authorization, state.token.as_str());
        let websocket_bearer = state.allow_installation_token
            && parts.uri.path() == "/ws"
            && parts
                .uri
                .query()
                .and_then(|query| {
                    query
                        .split('&')
                        .find_map(|part| part.strip_prefix("token="))
                })
                .is_some_and(|token| token == state.token.as_str());
        if bearer || websocket_bearer {
            if !crate::auth::request_origin_ok(&parts.headers, true) {
                return Err((StatusCode::FORBIDDEN, "forbidden origin").into_response());
            }
            tracing::info!(path = %parts.uri.path(), "installation-token operator authenticated");
            return Ok(Self::Operator);
        }

        let Some(token) = identity::session_token(&parts.headers) else {
            return Err((StatusCode::UNAUTHORIZED, "unauthorized").into_response());
        };
        if !crate::auth::request_origin_ok(&parts.headers, false) {
            return Err((StatusCode::FORBIDDEN, "forbidden origin").into_response());
        }
        let user = match state
            .auth_store
            .resolve_session(&token, identity::unix_timestamp())
        {
            Ok(Some(user)) => user,
            Ok(None) => return Err((StatusCode::UNAUTHORIZED, "unauthorized").into_response()),
            Err(error) => {
                tracing::error!(%error, "resolving Hall login session");
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "authentication unavailable",
                )
                    .into_response());
            }
        };
        let memberships = state
            .auth_store
            .organizations_for_user(&user.user_id)
            .map_err(|error| {
                tracing::error!(%error, "loading principal memberships");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "authentication unavailable",
                )
                    .into_response()
            })?
            .into_iter()
            .map(|organization| Membership {
                organization_id: organization.id,
                role: organization.role,
            })
            .collect();
        Ok(Self::User {
            user_id: user.user_id,
            username: user.username,
            memberships,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn authorization_matrix_fails_closed_at_one_seam() {
        struct Case {
            name: &'static str,
            principal: Principal,
            route: RouteClass<'static>,
            organization_exists: bool,
            expected: Result<Option<OrgScope>, StatusCode>,
        }

        let member = Membership {
            organization_id: "org-a".into(),
            role: "member".into(),
        };
        let cases = [
            Case {
                name: "member may select own organization",
                principal: Principal::User {
                    user_id: "user-a".into(),
                    username: "user-a".into(),
                    memberships: vec![member.clone()],
                },
                route: RouteClass::Organization(Some("org-a")),
                organization_exists: true,
                expected: Ok(Some(OrgScope::member("org-a", "member"))),
            },
            Case {
                name: "non-member organization selection fails closed",
                principal: Principal::User {
                    user_id: "user-a".into(),
                    username: "user-a".into(),
                    memberships: vec![member],
                },
                route: RouteClass::Organization(Some("org-b")),
                organization_exists: true,
                expected: Err(StatusCode::FORBIDDEN),
            },
            Case {
                name: "unknown organization is hidden",
                principal: Principal::User {
                    user_id: "user-a".into(),
                    username: "user-a".into(),
                    memberships: vec![],
                },
                route: RouteClass::Organization(Some("missing")),
                organization_exists: false,
                expected: Err(StatusCode::NOT_FOUND),
            },
            Case {
                name: "missing organization context is malformed",
                principal: Principal::User {
                    user_id: "user-a".into(),
                    username: "user-a".into(),
                    memberships: vec![],
                },
                route: RouteClass::Organization(None),
                organization_exists: false,
                expected: Err(StatusCode::BAD_REQUEST),
            },
            Case {
                name: "operator reaches organization as explicit admin",
                principal: Principal::Operator,
                route: RouteClass::Organization(Some("org-a")),
                organization_exists: true,
                expected: Ok(Some(OrgScope::admin("org-a"))),
            },
            Case {
                name: "user cannot reach operator surface",
                principal: Principal::User {
                    user_id: "user-a".into(),
                    username: "user-a".into(),
                    memberships: vec![],
                },
                route: RouteClass::Operator,
                organization_exists: false,
                expected: Err(StatusCode::FORBIDDEN),
            },
            Case {
                name: "operator reaches operator surface",
                principal: Principal::Operator,
                route: RouteClass::Operator,
                organization_exists: false,
                expected: Ok(None),
            },
        ];

        for case in cases {
            assert_eq!(
                authorize(&case.principal, case.route, case.organization_exists),
                case.expected,
                "{}",
                case.name
            );
        }
    }
}
