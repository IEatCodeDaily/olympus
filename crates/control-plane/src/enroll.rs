//! Envoy enrollment — one-line node setup (ADR 0008 §5, fleet UX).
//!
//! The flow the operator sees:
//!
//! 1. Fleet view → "Add node" → Hall mints a short-lived **enroll token** and
//!    renders the one-liner:
//!    `curl -fsSL https://<hall>/api/enroll/<token>/install.sh | bash`
//! 2. The target host runs it. The script (served by Hall, token baked in)
//!    downloads the envoy binary from Hall, installs the systemd user unit,
//!    generates the iroh key, and POSTs the envoy's iroh node id back to
//!    `/api/enroll/<token>` — Hall appends it to `hall.toml`'s
//!    `allowed_envoys` (the fail-closed allowlist) and the envoy connects.
//! 3. The node appears in the Fleet view within one heartbeat.
//!
//! Security model: the enroll token is a capability — single-use, expiring
//! (default 15 min), minted only by an authenticated operator. It authorizes
//! exactly two things: fetching the install script/binary and registering ONE
//! iroh node id. It is NOT the API token and grants no other access.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::RngCore;
use tokio::sync::Mutex;

use crate::auth_store::hash_token;

/// How long a minted enroll token stays valid.
pub const ENROLL_TTL: Duration = Duration::from_secs(15 * 60);

/// Length of the enroll token in raw bytes (hex-encoded → 32 chars — short
/// enough to read in a command line, long enough to be unguessable).
const ENROLL_TOKEN_BYTES: usize = 16;

#[derive(Debug, Clone)]
struct EnrollEntry {
    minted_at: Instant,
    /// Set once the token has registered a node id (single-use for
    /// registration; script/binary fetches stay allowed until expiry so a
    /// re-run of the installer can still download).
    used_by: Option<String>,
}

/// In-memory store of active enroll tokens, keyed by BLAKE3(token).
/// Tokens are ephemeral by design — a Hall restart invalidates outstanding
/// invitations (mint a new one).
#[derive(Clone, Default)]
pub struct EnrollStore {
    tokens: Arc<Mutex<HashMap<String, EnrollEntry>>>,
}

/// Outcome of attempting to consume a token for node registration.
#[derive(Debug, PartialEq, Eq)]
pub enum ConsumeOutcome {
    /// Token valid and now bound to this node id.
    Accepted,
    /// Token already registered this same node id (idempotent re-run).
    AlreadyRegistered,
    /// Token unknown, expired, or already used by a different node.
    Rejected,
}

impl EnrollStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint a fresh enroll token. Expired tokens are swept on each mint.
    pub async fn mint(&self) -> String {
        let mut buf = [0u8; ENROLL_TOKEN_BYTES];
        rand::thread_rng().fill_bytes(&mut buf);
        let token: String = buf.iter().map(|b| format!("{b:02x}")).collect();

        let mut tokens = self.tokens.lock().await;
        let now = Instant::now();
        tokens.retain(|_, e| now.duration_since(e.minted_at) < ENROLL_TTL);
        tokens.insert(
            hash_token(&token),
            EnrollEntry {
                minted_at: now,
                used_by: None,
            },
        );
        token
    }

    /// Is this token currently valid for fetching the script/binary?
    /// (Read-only check — does not consume.)
    pub async fn is_valid(&self, token: &str) -> bool {
        let tokens = self.tokens.lock().await;
        tokens
            .get(&hash_token(token))
            .is_some_and(|e| e.minted_at.elapsed() < ENROLL_TTL)
    }

    /// Consume the token to register a node id. Single-use for registration:
    /// a second registration with a DIFFERENT node id is rejected; the same
    /// node id is idempotent (installer re-runs).
    pub async fn consume(&self, token: &str, node_id: &str) -> ConsumeOutcome {
        let mut tokens = self.tokens.lock().await;
        let token_hash = hash_token(token);
        let Some(entry) = tokens.get_mut(&token_hash) else {
            return ConsumeOutcome::Rejected;
        };
        if entry.minted_at.elapsed() >= ENROLL_TTL {
            return ConsumeOutcome::Rejected;
        }
        match &entry.used_by {
            Some(existing) if existing == node_id => ConsumeOutcome::AlreadyRegistered,
            Some(_) => ConsumeOutcome::Rejected,
            None => {
                entry.used_by = Some(node_id.to_string());
                ConsumeOutcome::Accepted
            }
        }
    }
}

// ── hall.toml allowlist mutation ────────────────────────────────────────
//
// The allowlist file (`<home>/hall.toml`, `allowed_envoys = [...]`) is read
// per-connection by the iroh accept loop, so appending here takes effect on
// the envoy's next connect attempt — no Hall restart. Writes are atomic
// (tmp + rename) and preserve unknown keys by rewriting only via the parsed
// struct — hall.toml currently has exactly one key, so full-rewrite is safe.

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct HallConfigFile {
    #[serde(default)]
    allowed_envoys: Vec<String>,
}

fn read_config(home: &std::path::Path) -> HallConfigFile {
    let path = home.join("hall.toml");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| toml::from_str(&raw).ok())
        .unwrap_or_default()
}

fn write_config(home: &std::path::Path, cfg: &HallConfigFile) -> anyhow::Result<()> {
    let path = home.join("hall.toml");
    let tmp = home.join("hall.toml.tmp");
    let raw = toml::to_string_pretty(cfg)?;
    std::fs::create_dir_all(home)?;
    std::fs::write(&tmp, raw)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Append an iroh node id to the allowlist (idempotent). Validates the id
/// parses as an iroh public key first — garbage never lands in the file.
pub fn allowlist_add(home: &std::path::Path, node_id: &str) -> anyhow::Result<bool> {
    node_id
        .parse::<iroh::PublicKey>()
        .map_err(|e| anyhow::anyhow!("invalid iroh node id {node_id:?}: {e}"))?;
    let mut cfg = read_config(home);
    if cfg.allowed_envoys.iter().any(|s| s == node_id) {
        return Ok(false); // already present
    }
    cfg.allowed_envoys.push(node_id.to_string());
    write_config(home, &cfg)?;
    Ok(true)
}

/// Remove an iroh node id from the allowlist. Returns whether it was present.
pub fn allowlist_remove(home: &std::path::Path, node_id: &str) -> anyhow::Result<bool> {
    let mut cfg = read_config(home);
    let before = cfg.allowed_envoys.len();
    cfg.allowed_envoys.retain(|s| s != node_id);
    let removed = cfg.allowed_envoys.len() != before;
    if removed {
        write_config(home, &cfg)?;
    }
    Ok(removed)
}

/// List the current allowlist (raw strings as stored).
pub fn allowlist_list(home: &std::path::Path) -> Vec<String> {
    read_config(home).allowed_envoys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mint_and_validate() {
        let store = EnrollStore::new();
        let t = store.mint().await;
        assert_eq!(t.len(), ENROLL_TOKEN_BYTES * 2);
        assert!(store.is_valid(&t).await);
        assert!(!store.is_valid("nonsense").await);
    }

    #[tokio::test]
    async fn consume_is_single_use_but_idempotent_per_node() {
        let store = EnrollStore::new();
        let t = store.mint().await;
        assert_eq!(store.consume(&t, "node-a").await, ConsumeOutcome::Accepted);
        // Same node re-registering (installer re-run) is fine.
        assert_eq!(
            store.consume(&t, "node-a").await,
            ConsumeOutcome::AlreadyRegistered
        );
        // A different node on the same token is rejected.
        assert_eq!(store.consume(&t, "node-b").await, ConsumeOutcome::Rejected);
        // Unknown token rejected.
        assert_eq!(
            store.consume("nope", "node-a").await,
            ConsumeOutcome::Rejected
        );
    }

    #[test]
    fn allowlist_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        // A real 32-byte hex key shape (iroh parses 64-char hex).
        let id = "83141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499320";

        assert!(allowlist_list(home).is_empty());
        assert!(allowlist_add(home, id).unwrap());
        assert!(!allowlist_add(home, id).unwrap(), "idempotent add");
        assert_eq!(allowlist_list(home), vec![id.to_string()]);

        // Garbage id never lands in the file.
        assert!(allowlist_add(home, "not-a-key").is_err());
        assert_eq!(allowlist_list(home).len(), 1);

        assert!(allowlist_remove(home, id).unwrap());
        assert!(!allowlist_remove(home, id).unwrap());
        assert!(allowlist_list(home).is_empty());
    }
}
