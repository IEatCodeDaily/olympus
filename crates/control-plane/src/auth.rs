//! Auth gate for the control-plane HTTP/WS surface (ADR 0002 §3.5.2 MVP form).
//!
//! state.db holds secrets, system prompts, tool output, and corporate context,
//! so even the single-operator MVP must not expose an unauthenticated local
//! server that can read all history and drive Hermes. This module provides:
//!
//! - a per-install random token (generated on first run, stored mode-0600 under
//!   `~/.olympus/token`), required on every `/api/*` request and the `/ws`
//!   upgrade;
//! - strict `Origin` checks so a hostile local web page cannot reach the port
//!   ("localhost == trusted" is explicitly rejected).
//!
//! This is the `can(user, action, resource)` seam in its MVP form: one operator,
//! one token; the call sites exist so real RBAC is a later flip, not a rewrite.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rand::RngCore;

/// Length of the per-install token in raw bytes (hex-encoded → 64 chars).
const TOKEN_BYTES: usize = 32;

/// Resolve `~/.olympus/token`, honoring `OLYMPUS_HOME` for tests/overrides.
fn token_path() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("OLYMPUS_HOME") {
        return Ok(PathBuf::from(dir).join("token"));
    }
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".olympus").join("token"))
}

/// Load the per-install token, generating and persisting it on first run.
///
/// The file is created mode-0600 (owner read/write only). Idempotent: a second
/// call returns the same token read from disk.
pub fn load_or_create_token() -> Result<String> {
    load_or_create_token_at(&token_path()?)
}

/// Like [`load_or_create_token`] but against an explicit path (testable).
pub fn load_or_create_token_at(path: &Path) -> Result<String> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating token dir {}", parent.display()))?;
    }

    let mut buf = [0u8; TOKEN_BYTES];
    rand::thread_rng().fill_bytes(&mut buf);
    let token = hex_encode(&buf);

    write_secret(path, &token).with_context(|| format!("writing token file {}", path.display()))?;
    Ok(token)
}

/// Write `contents` to `path` with mode 0600 on unix.
fn write_secret(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Does the presented `Authorization` header value carry the right Bearer token?
///
/// Accepts exactly `Bearer <token>` (case-insensitive scheme). Constant-time-ish
/// compare is not required for a local single-operator token, but we still avoid
/// early-exit on length to keep the surface boring.
pub fn bearer_ok(header: Option<&str>, expected: &str) -> bool {
    let Some(value) = header else { return false };
    let value = value.trim();
    let Some(rest) = split_bearer(value) else {
        return false;
    };
    rest == expected
}

fn split_bearer(value: &str) -> Option<&str> {
    let mut parts = value.splitn(2, ' ');
    let scheme = parts.next()?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    parts.next().map(str::trim)
}

/// Validate browser provenance against the exact Hall origin. Native clients
/// may omit Origin only when they present a valid non-cookie credential.
pub fn request_origin_ok(headers: &axum::http::HeaderMap, native_credential: bool) -> bool {
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if let Some(origin) = origin {
        let host = headers
            .get(axum::http::header::HOST)
            .and_then(|value| value.to_str().ok());
        return browser_origin_allowed(origin, host);
    }
    native_credential
        || headers
            .get("sec-fetch-site")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("same-origin"))
}

pub fn browser_origin_allowed(origin: &str, host: Option<&str>) -> bool {
    origin_allowed(origin, host, allowed_extra_origins())
}

fn origin_allowed(origin: &str, host: Option<&str>, allowed: &[String]) -> bool {
    let Ok(uri) = origin.parse::<axum::http::Uri>() else {
        return false;
    };
    if !matches!(uri.scheme_str(), Some("http" | "https")) || uri.path() != "/" {
        return false;
    }
    let Some(authority) = uri.authority().map(|authority| authority.as_str()) else {
        return false;
    };
    host.is_some_and(|host| authority.eq_ignore_ascii_case(host))
        || allowed.iter().any(|allowed| origin == allowed)
}

/// Exact origins from OLYMPUS_ALLOWED_ORIGINS, for the Vite development server
/// or an explicitly configured reverse-proxy origin.
fn allowed_extra_origins() -> &'static Vec<String> {
    use std::sync::OnceLock;
    static ORIGINS: OnceLock<Vec<String>> = OnceLock::new();
    ORIGINS.get_or_init(|| {
        std::env::var("OLYMPUS_ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty())
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_or_create_is_idempotent_and_0600() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("token");

        let first = load_or_create_token_at(&path).unwrap();
        assert_eq!(first.len(), TOKEN_BYTES * 2, "token is hex of 32 bytes");
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));

        let second = load_or_create_token_at(&path).unwrap();
        assert_eq!(first, second, "second call returns the same token");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "token file must be 0600");
        }
    }

    #[test]
    fn bearer_ok_accepts_correct_token_only() {
        assert!(bearer_ok(Some("Bearer secret123"), "secret123"));
        assert!(bearer_ok(Some("bearer secret123"), "secret123")); // scheme case-insensitive
        assert!(!bearer_ok(Some("Bearer wrong"), "secret123"));
        assert!(!bearer_ok(Some("secret123"), "secret123")); // missing scheme
        assert!(!bearer_ok(None, "secret123"));
        assert!(!bearer_ok(Some("Basic secret123"), "secret123"));
    }

    #[test]
    fn origin_must_match_exact_host_or_explicit_origin() {
        let allowed = vec!["https://olympus.entelechia.cloud".to_string()];
        assert!(origin_allowed(
            "http://127.0.0.1:5173",
            Some("127.0.0.1:5173"),
            &[]
        ));
        assert!(!origin_allowed(
            "http://127.0.0.1:9999",
            Some("127.0.0.1:5173"),
            &[]
        ));
        assert!(origin_allowed(
            "https://olympus.entelechia.cloud",
            Some("127.0.0.1:8787"),
            &allowed
        ));
        assert!(!origin_allowed(
            "http://olympus.entelechia.cloud",
            Some("127.0.0.1:8787"),
            &allowed
        ));
        assert!(!origin_allowed("null", None, &allowed));
        assert!(!origin_allowed("ftp://127.0.0.1", None, &allowed));
    }

    #[test]
    fn absent_origin_requires_native_credential_or_same_origin_fetch_metadata() {
        let mut headers = axum::http::HeaderMap::new();
        assert!(!request_origin_ok(&headers, false));
        assert!(request_origin_ok(&headers, true));
        headers.insert("sec-fetch-site", "same-origin".parse().unwrap());
        assert!(request_origin_ok(&headers, false));
        headers.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert!(!request_origin_ok(&headers, false));
    }
}
