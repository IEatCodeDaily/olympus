use axum::{
    extract::{Extension, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use super::principal::Principal;
use super::AppState;

const SESSION_COOKIE: &str = "olympus_session";
const SESSION_TTL_SECONDS: i64 = 60 * 60 * 24 * 30;
const LOGIN_WINDOW_SECONDS: i64 = 60;
const LOGIN_GLOBAL_LIMIT: usize = 20;
const LOGIN_USERNAME_LIMIT: usize = 5;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Response {
    if !crate::auth::request_origin_ok(&headers, false) {
        return (StatusCode::FORBIDDEN, "forbidden origin").into_response();
    }
    if !allow_login_attempt(&body.username, unix_timestamp()) {
        return (StatusCode::TOO_MANY_REQUESTS, "too many login attempts").into_response();
    }

    let principal = match state
        .auth_store
        .authenticate(&body.username, &body.password)
    {
        Ok(Some(principal)) => principal,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "invalid username or password").into_response()
        }
        Err(error) => {
            tracing::error!(%error, "authenticating Hall user");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "authentication unavailable",
            )
                .into_response();
        }
    };
    let session = match state.auth_store.create_session(
        &principal.user_id,
        unix_timestamp(),
        SESSION_TTL_SECONDS,
    ) {
        Ok(session) => session,
        Err(error) => {
            tracing::error!(%error, "creating Hall login session");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "authentication unavailable",
            )
                .into_response();
        }
    };

    let mut response = Json(json!({ "user": principal })).into_response();
    let cookie = session_cookie(
        &session.token,
        SESSION_TTL_SECONDS,
        state.session_cookie_secure,
    );
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).expect("valid session cookie"),
    );
    response
}

pub async fn current_session(Extension(principal): Extension<Principal>) -> Response {
    match principal {
        Principal::User {
            user_id, username, ..
        } => Json(json!({
            "user": { "userId": user_id, "username": username, "kind": "user" }
        }))
        .into_response(),
        Principal::Operator => (StatusCode::FORBIDDEN, "user login required").into_response(),
    }
}

pub async fn list_organizations(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    let Principal::User { user_id, .. } = principal else {
        return (StatusCode::FORBIDDEN, "user login required").into_response();
    };
    match state.auth_store.organizations_for_user(&user_id) {
        Ok(organizations) => Json(json!({ "organizations": organizations })).into_response(),
        Err(error) => {
            tracing::error!(%error, "listing user organizations");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "organizations unavailable",
            )
                .into_response()
        }
    }
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = session_token(&headers) {
        if let Err(error) = state.auth_store.revoke_session(&token) {
            tracing::error!(%error, "revoking Hall login session");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "authentication unavailable",
            )
                .into_response();
        }
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    let cookie = session_cookie("", 0, state.session_cookie_secure);
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).expect("valid expired cookie"),
    );
    response
}

pub fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|item| item.trim().split_once('='))
        .find_map(|(name, value)| {
            (name == SESSION_COOKIE && !value.is_empty()).then(|| value.to_string())
        })
}

fn session_cookie(token: &str, max_age: i64, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}"
    )
}

#[derive(Default)]
struct LoginLimiter {
    global: VecDeque<i64>,
    by_username: HashMap<String, VecDeque<i64>>,
}

impl LoginLimiter {
    fn allow(&mut self, username: &str, now: i64) -> bool {
        let cutoff = now - LOGIN_WINDOW_SECONDS;
        self.global.retain(|timestamp| *timestamp > cutoff);
        let username = username.to_ascii_lowercase();
        let attempts = self.by_username.entry(username).or_default();
        attempts.retain(|timestamp| *timestamp > cutoff);
        if self.global.len() >= LOGIN_GLOBAL_LIMIT || attempts.len() >= LOGIN_USERNAME_LIMIT {
            return false;
        }
        self.global.push_back(now);
        attempts.push_back(now);
        true
    }
}

fn allow_login_attempt(username: &str, now: i64) -> bool {
    static LIMITER: OnceLock<Mutex<LoginLimiter>> = OnceLock::new();
    LIMITER
        .get_or_init(|| Mutex::new(LoginLimiter::default()))
        .lock()
        .expect("login limiter mutex poisoned")
        .allow(username, now)
}

pub fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_is_secure_http_only_and_strict() {
        let cookie = session_cookie("secret", 60, true);
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("Max-Age=60"));
    }

    #[test]
    fn cookie_parser_matches_only_named_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("other=x; olympus_session=abc123"),
        );
        assert_eq!(session_token(&headers).as_deref(), Some("abc123"));
    }

    #[test]
    fn login_limiter_caps_username_and_global_argon_work() {
        let mut limiter = LoginLimiter::default();
        for _ in 0..LOGIN_USERNAME_LIMIT {
            assert!(limiter.allow("admin", 100));
        }
        assert!(!limiter.allow("ADMIN", 100));
        for index in 0..(LOGIN_GLOBAL_LIMIT - LOGIN_USERNAME_LIMIT) {
            assert!(limiter.allow(&format!("user-{index}"), 100));
        }
        assert!(!limiter.allow("one-more-user", 100));
        assert!(limiter.allow("admin", 100 + LOGIN_WINDOW_SECONDS + 1));
    }
}
