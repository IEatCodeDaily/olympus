//! Single-Hall identity and bounded backup/recovery contracts (ADR 0016).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use iroh::SecretKey;

pub fn identity_path(home: &Path) -> PathBuf {
    home.join("iroh.key")
}

/// Load the Hall identity, creating it only for a genuinely new Hall.
pub fn load_identity(home: &Path, database_exists: bool) -> Result<SecretKey> {
    let path = identity_path(home);
    anyhow::ensure!(
        path.exists() || !database_exists,
        "Hall database exists but {} is missing; restore the matching identity key from backup",
        path.display()
    );
    olympus_envoy::transport::load_or_create_secret(home)
        .context("loading Hall identity private key")
}

pub fn hall_id(identity: &SecretKey) -> String {
    identity.public().to_string()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn identity_persists_as_a_0600_file_and_public_key_is_hall_id() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_identity(dir.path(), false).unwrap();
        let second = load_identity(dir.path(), true).unwrap();

        assert_eq!(first.to_bytes(), second.to_bytes());
        assert_eq!(hall_id(&first), first.public().to_string());
        assert_eq!(
            std::fs::metadata(identity_path(dir.path()))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn existing_database_without_identity_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let error = load_identity(dir.path(), true).unwrap_err();
        assert!(error.to_string().contains("restore the matching identity"));
        assert!(!identity_path(dir.path()).exists());
    }
}
