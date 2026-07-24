//! Iroh transport for the Hall↔Envoy wire protocol (ADR 0008 §1, milestone S7).
//!
//! The JSON-lines EnvoyFrame/HallFrame protocol is transport-agnostic: locally
//! it runs over UDS, remotely over an iroh QUIC connection (public n0 relays,
//! e2e-encrypted, NAT-traversing, keyed by node ids). One envoy connection =
//! one bidirectional QUIC stream carrying the same newline-delimited JSON both
//! ways — so the Hall/envoy read-loops don't fork per transport.
//!
//! Key handling: the envoy persists its ed25519 secret key at
//! `<state_dir>/iroh.key` (32 raw bytes, 0600) so its node id is stable across
//! restarts — the node id IS the allowlist identity on the Hall side.

use std::path::Path;

use anyhow::{Context, Result};
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, PublicKey, SecretKey};

/// ALPN for the Olympus Hall↔Envoy protocol.
pub const OLYMPUS_ALPN: &[u8] = b"olympus/envoy/1";

/// Load the persisted iroh secret key from `dir/iroh.key`, generating and
/// persisting a fresh one on first run (0600 perms).
pub fn load_or_create_secret(dir: &Path) -> Result<SecretKey> {
    let path = dir.join("iroh.key");
    if path.exists() {
        let bytes =
            std::fs::read(&path).with_context(|| format!("reading iroh key {}", path.display()))?;
        let arr: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("iroh.key must be exactly 32 bytes"))?;
        return Ok(SecretKey::from_bytes(&arr));
    }
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let key = SecretKey::generate();
    std::fs::write(&path, key.to_bytes())
        .with_context(|| format!("writing iroh key {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 600 {}", path.display()))?;
    }
    Ok(key)
}

/// Bind an iroh endpoint with the given secret key using the public n0 relay
/// preset, accepting the Olympus ALPN.
pub async fn bind_endpoint(secret: SecretKey) -> Result<Endpoint> {
    let ep = Endpoint::builder(presets::N0)
        .secret_key(secret)
        .alpns(vec![OLYMPUS_ALPN.to_vec()])
        .bind()
        .await
        .context("binding iroh endpoint")?;
    Ok(ep)
}

/// Connect to a Hall by its iroh node id (public key, z-base-32 or hex as
/// printed by the hall at boot). Returns the QUIC connection's bi-stream
/// halves, which speak the same JSON-lines protocol as the UDS path.
pub async fn connect_to_hall(
    endpoint: &Endpoint,
    hall_node_id: &str,
) -> Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream)> {
    let key: PublicKey = hall_node_id
        .parse()
        .with_context(|| format!("parsing hall node id {hall_node_id:?}"))?;
    let addr = EndpointAddr::from(key);
    let conn = endpoint
        .connect(addr, OLYMPUS_ALPN)
        .await
        .with_context(|| format!("connecting to hall {hall_node_id} via iroh"))?;
    let (send, recv) = conn
        .open_bi()
        .await
        .context("opening bidirectional stream to hall")?;
    Ok((send, recv))
}

/// Persistent Hall pin status stored beside the envoy's own iroh identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HallPinStatus {
    PinnedFresh,
    AlreadyPinned,
    Replaced,
}

const HALL_PIN_FILE: &str = "hall.key";

fn hall_pin_path(dir: &Path) -> std::path::PathBuf {
    dir.join(HALL_PIN_FILE)
}

/// Load the pinned Hall public key, if any.
pub fn load_pinned_hall(dir: &Path) -> Result<Option<PublicKey>> {
    let path = hall_pin_path(dir);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading pinned Hall key {}", path.display()));
        }
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("pinned Hall key file {} is empty", path.display());
    }
    let key = trimmed
        .parse::<PublicKey>()
        .with_context(|| format!("parsing pinned Hall key {}", path.display()))?;
    Ok(Some(key))
}

/// Persist a Hall public key atomically at `<state_dir>/hall.key`.
fn store_pinned_hall(dir: &Path, hall: PublicKey) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = hall_pin_path(dir);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, hall.to_string())
        .with_context(|| format!("writing temporary Hall key {}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 600 {}", tmp.display()))?;
    }
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("atomically pinning Hall key at {}", path.display()))?;
    Ok(())
}

/// Pin the first Hall or confirm the existing one matches.
pub fn pin_hall(dir: &Path, hall: PublicKey) -> Result<HallPinStatus> {
    match load_pinned_hall(dir)? {
        None => {
            store_pinned_hall(dir, hall)?;
            Ok(HallPinStatus::PinnedFresh)
        }
        Some(current) if current == hall => Ok(HallPinStatus::AlreadyPinned),
        Some(current) => anyhow::bail!(
            "different Hall key already pinned at {}; use --replace-hall --expected-old-hall {} to re-pin locally",
            hall_pin_path(dir).display(),
            current
        ),
    }
}

/// Replace the pinned Hall, but only when the expected old key matches.
pub fn replace_pinned_hall(
    dir: &Path,
    expected_old: PublicKey,
    hall: PublicKey,
) -> Result<HallPinStatus> {
    match load_pinned_hall(dir)? {
        None => anyhow::bail!("expected old Hall {expected_old} but no Hall is pinned yet"),
        Some(current) if current != expected_old => {
            anyhow::bail!("expected old Hall {expected_old}, found {current}")
        }
        Some(current) if current == hall => Ok(HallPinStatus::AlreadyPinned),
        Some(_) => {
            store_pinned_hall(dir, hall)?;
            Ok(HallPinStatus::Replaced)
        }
    }
}

/// A local re-pin is authorized when the caller is the owner of the state dir
/// or root (system envoy tier).
pub fn local_repin_authorized_uid(owner_uid: u32, current_uid: u32) -> bool {
    current_uid == 0 || current_uid == owner_uid
}

#[cfg(unix)]
fn current_euid() -> u32 {
    unsafe { geteuid() }
}

#[cfg(unix)]
pub fn local_repin_authorized(dir: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let owner_uid = std::fs::metadata(dir)
        .with_context(|| format!("reading ownership of {}", dir.display()))?
        .uid();
    Ok(local_repin_authorized_uid(owner_uid, current_euid()))
}

#[cfg(not(unix))]
pub fn local_repin_authorized(_dir: &Path) -> Result<bool> {
    Ok(true)
}

#[cfg(unix)]
unsafe extern "C" {
    fn geteuid() -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_persists_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let k1 = load_or_create_secret(dir.path()).unwrap();
        let k2 = load_or_create_secret(dir.path()).unwrap();
        assert_eq!(
            k1.to_bytes(),
            k2.to_bytes(),
            "key must be stable across loads"
        );
        assert_eq!(k1.public(), k2.public());
    }

    #[test]
    fn key_file_is_0600() {
        let dir = tempfile::tempdir().unwrap();
        load_or_create_secret(dir.path()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("iroh.key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn corrupt_key_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("iroh.key"), b"short").unwrap();
        assert!(load_or_create_secret(dir.path()).is_err());
    }

    #[test]
    fn hall_pin_persists_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let old_hall: PublicKey =
            "83141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499320"
                .parse()
                .unwrap();

        assert_eq!(load_pinned_hall(dir.path()).unwrap(), None);
        assert_eq!(
            pin_hall(dir.path(), old_hall).unwrap(),
            HallPinStatus::PinnedFresh
        );
        assert_eq!(load_pinned_hall(dir.path()).unwrap(), Some(old_hall));
        assert_eq!(
            pin_hall(dir.path(), old_hall).unwrap(),
            HallPinStatus::AlreadyPinned
        );
    }

    #[test]
    fn hall_pin_rejects_remote_takeover_until_locally_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let old_hall: PublicKey =
            "83141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499320"
                .parse()
                .unwrap();
        let new_hall: PublicKey =
            "93141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499321"
                .parse()
                .unwrap();

        pin_hall(dir.path(), old_hall).unwrap();
        let error = pin_hall(dir.path(), new_hall).unwrap_err();
        assert!(error.to_string().contains("different Hall key"));
        assert_eq!(load_pinned_hall(dir.path()).unwrap(), Some(old_hall));

        replace_pinned_hall(dir.path(), old_hall, new_hall).unwrap();
        assert_eq!(load_pinned_hall(dir.path()).unwrap(), Some(new_hall));
    }

    #[test]
    fn hall_pin_replace_requires_expected_old_key() {
        let dir = tempfile::tempdir().unwrap();
        let old_hall: PublicKey =
            "83141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499320"
                .parse()
                .unwrap();
        let wrong_old_hall: PublicKey =
            "73141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499320"
                .parse()
                .unwrap();
        let new_hall: PublicKey =
            "93141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499321"
                .parse()
                .unwrap();

        pin_hall(dir.path(), old_hall).unwrap();
        let error = replace_pinned_hall(dir.path(), wrong_old_hall, new_hall).unwrap_err();
        assert!(error.to_string().contains("expected old Hall"));
        assert_eq!(load_pinned_hall(dir.path()).unwrap(), Some(old_hall));
    }

    #[cfg(unix)]
    #[test]
    fn local_repin_authorization_matches_owner_or_root() {
        let dir = tempfile::tempdir().unwrap();
        assert!(local_repin_authorized(dir.path()).unwrap());
        assert!(local_repin_authorized_uid(1000, 1000));
        assert!(local_repin_authorized_uid(1000, 0));
        assert!(!local_repin_authorized_uid(1000, 1001));
    }
}
