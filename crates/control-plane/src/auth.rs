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

/// Is this `Origin` header acceptable?
///
/// Policy (fail-closed): absent Origin (curl, native clients) is allowed; a
/// present Origin must be a loopback http(s) origin. Any other Origin — a
/// hostile page on `http://evil.example`, or even a non-loopback host — is
/// rejected. "localhost == trusted" alone is NOT sufficient; we require the
/// loopback scheme+host shape explicitly.
pub fn origin_ok(origin: Option<&str>) -> bool {
    let Some(origin) = origin else { return true };
    let origin = origin.trim();
    if origin.is_empty() || origin.eq_ignore_ascii_case("null") {
        // "null" origin (sandboxed iframe, file://) is explicitly untrusted.
        return origin.is_empty();
    }
    let rest = match origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    {
        Some(r) => r,
        None => return false,
    };
    // Strip an optional path (Origin normally has none), then extract the host,
    // handling bracketed IPv6 like `[::1]:5173`.
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = if let Some(after) = authority.strip_prefix('[') {
        // `[::1]:port` or `[::1]` → take up to the closing bracket.
        after.split(']').next().unwrap_or(after)
    } else {
        // `host:port` or `host` → strip the port.
        authority.split(':').next().unwrap_or(authority)
    };
    host == "127.0.0.1" || host == "localhost" || host == "::1"
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
    fn origin_ok_allows_loopback_and_absent_rejects_foreign() {
        assert!(origin_ok(None), "absent Origin (curl) allowed");
        assert!(origin_ok(Some("http://127.0.0.1:5173")));
        assert!(origin_ok(Some("http://localhost:8787")));
        assert!(origin_ok(Some("https://localhost")));
        assert!(origin_ok(Some("http://[::1]:5173")));

        assert!(
            !origin_ok(Some("http://evil.example")),
            "foreign origin rejected"
        );
        assert!(!origin_ok(Some("https://attacker.test:443")));
        assert!(!origin_ok(Some("null")), "null origin rejected");
        assert!(
            !origin_ok(Some("ftp://127.0.0.1")),
            "non-http scheme rejected"
        );
    }
}
