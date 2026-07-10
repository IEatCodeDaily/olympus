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
}
