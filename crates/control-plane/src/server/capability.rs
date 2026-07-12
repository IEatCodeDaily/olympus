//! Per-session capability envelopes, narrowing, signing, and evaluation.
//!
//! This is the single capability-evaluation seam. Callers (including future
//! workflow step dispatch) must use [`CapabilityAuthorizer::authorize_capability`]
//! rather than inspecting projected capability records directly.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use super::principal::Principal;
use crate::views::SessionView;

pub const CAPABILITY_ENVELOPE_VERSION: u16 = 1;

/// Reserved authority IDs from ADR 0012. They are vocabulary only in CAPS-1.
pub mod ids {
    pub const SESSION_FORK: &str = "session.fork";
    pub const WORKFLOW_LIST: &str = "workflow.list";
    pub const WORKFLOW_EXECUTE: &str = "workflow.execute";
    pub const WORKFLOW_DRAFT_CREATE: &str = "workflow.draft.create";
    pub const WORKFLOW_PUBLISH: &str = "workflow.publish";
    pub const PACKAGE_AUTHOR: &str = "package.author";
    pub const PACKAGE_BUILD: &str = "package.build";
    pub const PACKAGE_SIGN: &str = "package.sign";
    pub const PACKAGE_INSTALL: &str = "package.install";
    pub const PACKAGE_GRANT: &str = "package.grant";
    pub const PACKAGE_ACTIVATE: &str = "package.activate";
    pub const PROXY_ROUTE_REGISTER: &str = "proxy.route.register";
    pub const STATIC_PUBLISH: &str = "static.publish";
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceLimits {
    pub max_cpu_seconds: Option<u64>,
    pub max_memory_bytes: Option<u64>,
    pub max_wall_seconds: Option<u64>,
    pub max_concurrent_jobs: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilitySet {
    pub version: u16,
    #[serde(default)]
    pub ids: BTreeSet<String>,
    #[serde(default)]
    pub readable_paths: BTreeSet<String>,
    #[serde(default)]
    pub writable_paths: BTreeSet<String>,
    #[serde(default)]
    pub linked_repos: BTreeSet<String>,
    #[serde(default)]
    pub linked_vaults: BTreeSet<String>,
    #[serde(default)]
    pub resource_limits: ResourceLimits,
    #[serde(default)]
    pub can_fork: bool,
    /// HMAC-SHA256 over the canonical envelope with this field empty.
    #[serde(default)]
    pub signature: String,
}

impl Default for CapabilitySet {
    fn default() -> Self {
        Self {
            version: CAPABILITY_ENVELOPE_VERSION,
            ids: BTreeSet::new(),
            readable_paths: BTreeSet::new(),
            writable_paths: BTreeSet::new(),
            linked_repos: BTreeSet::new(),
            linked_vaults: BTreeSet::new(),
            resource_limits: ResourceLimits::default(),
            can_fork: false,
            signature: String::new(),
        }
    }
}

impl CapabilitySet {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.version == CAPABILITY_ENVELOPE_VERSION,
            "unsupported capability envelope version {}",
            self.version
        );
        for id in &self.ids {
            validate_capability_id(id)?;
        }
        anyhow::ensure!(
            self.writable_paths
                .iter()
                .all(|path| self.readable_paths.contains(path)),
            "every writable path must also be readable"
        );
        Ok(())
    }

    /// Narrow `requested` against `parent`. The result can never exceed parent.
    pub fn intersect(parent: &Self, requested: &Self) -> Self {
        Self {
            version: CAPABILITY_ENVELOPE_VERSION,
            ids: requested
                .ids
                .iter()
                .filter(|id| parent.allows_id(id))
                .cloned()
                .collect(),
            readable_paths: requested
                .readable_paths
                .intersection(&parent.readable_paths)
                .cloned()
                .collect(),
            writable_paths: requested
                .writable_paths
                .intersection(&parent.writable_paths)
                .cloned()
                .collect(),
            linked_repos: requested
                .linked_repos
                .intersection(&parent.linked_repos)
                .cloned()
                .collect(),
            linked_vaults: requested
                .linked_vaults
                .intersection(&parent.linked_vaults)
                .cloned()
                .collect(),
            resource_limits: ResourceLimits {
                max_cpu_seconds: narrow_limit(
                    parent.resource_limits.max_cpu_seconds,
                    requested.resource_limits.max_cpu_seconds,
                ),
                max_memory_bytes: narrow_limit(
                    parent.resource_limits.max_memory_bytes,
                    requested.resource_limits.max_memory_bytes,
                ),
                max_wall_seconds: narrow_limit(
                    parent.resource_limits.max_wall_seconds,
                    requested.resource_limits.max_wall_seconds,
                ),
                max_concurrent_jobs: narrow_limit(
                    parent.resource_limits.max_concurrent_jobs,
                    requested.resource_limits.max_concurrent_jobs,
                ),
            },
            can_fork: parent.can_fork && requested.can_fork,
            signature: String::new(),
        }
    }

    pub fn is_narrower_than(&self, parent: &Self) -> bool {
        let mut unsigned = self.clone();
        unsigned.signature.clear();
        Self::intersect(parent, &unsigned) == unsigned
    }

    pub fn allows_id(&self, requested: &str) -> bool {
        self.ids
            .iter()
            .any(|granted| capability_matches(granted, requested))
    }
}

fn narrow_limit<T: Ord + Copy>(parent: Option<T>, requested: Option<T>) -> Option<T> {
    match (parent, requested) {
        (Some(parent), Some(requested)) => Some(parent.min(requested)),
        (Some(parent), None) => Some(parent),
        (None, requested) => requested,
    }
}

fn capability_matches(granted: &str, requested: &str) -> bool {
    if granted == requested {
        return true;
    }
    let (Some((granted_id, granted_resource)), Some((requested_id, requested_resource))) =
        (granted.split_once(':'), requested.split_once(':'))
    else {
        return false;
    };
    granted_id == requested_id && requested_resource.starts_with(granted_resource)
}

fn validate_capability_id(id: &str) -> Result<()> {
    let (authority, resource) = id.split_once(':').map_or((id, None), |(a, r)| (a, Some(r)));
    anyhow::ensure!(!authority.is_empty(), "capability id is empty");
    anyhow::ensure!(
        authority.split('.').all(|part| {
            !part.is_empty()
                && part.chars().all(|character| {
                    character.is_ascii_alphanumeric() || character == '-' || character == '_'
                })
        }),
        "invalid capability id: {id}"
    );
    anyhow::ensure!(
        resource.is_none_or(|resource| !resource.is_empty()),
        "empty capability resource: {id}"
    );
    Ok(())
}

#[derive(Clone)]
pub struct CapabilitySigner {
    key: [u8; 32],
}

impl CapabilitySigner {
    pub fn load_or_create(home: &Path) -> Result<Self> {
        Self::load_or_create_at(&home.join("capability.key"))
    }

    pub fn load_or_create_at(path: &Path) -> Result<Self> {
        let key = match std::fs::read(path) {
            Ok(bytes) if bytes.len() == 32 => bytes.try_into().expect("length checked"),
            Ok(_) => anyhow::bail!(
                "invalid capability signing key length at {}",
                path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("creating {}", parent.display()))?;
                }
                let mut key = [0_u8; 32];
                rand::thread_rng().fill_bytes(&mut key);
                write_secret(path, &key)?;
                key
            }
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        Ok(Self { key })
    }

    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self { key: [7; 32] }
    }

    pub fn sign(&self, capabilities: &mut CapabilitySet) -> Result<()> {
        capabilities.validate()?;
        capabilities.signature.clear();
        let payload = serde_json::to_vec(capabilities)?;
        capabilities.signature = hex_encode(&hmac_blake3(&self.key, &payload));
        Ok(())
    }

    pub fn verify(&self, capabilities: &CapabilitySet) -> bool {
        if capabilities.validate().is_err() || capabilities.signature.is_empty() {
            return false;
        }
        let Ok(signature) = hex_decode(&capabilities.signature) else {
            return false;
        };
        let mut unsigned = capabilities.clone();
        unsigned.signature.clear();
        let Ok(payload) = serde_json::to_vec(&unsigned) else {
            return false;
        };
        constant_time_eq(&hmac_blake3(&self.key, &payload), &signature)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityDecision {
    Allow,
    Deny,
}

pub struct CapabilityAuthorizer<'a> {
    sessions: &'a SessionView,
    signer: &'a CapabilitySigner,
}

impl<'a> CapabilityAuthorizer<'a> {
    pub fn new(sessions: &'a SessionView, signer: &'a CapabilitySigner) -> Self {
        Self { sessions, signer }
    }

    /// The one ADR 0012 authority-intersection seam.
    ///
    /// A missing capability record preserves the legacy full grant. Once any
    /// session in a lineage carries a record, every signed ancestor record is
    /// intersected dynamically so parent revocation takes effect at the next call.
    pub fn authorize_capability(
        &self,
        _principal: &Principal,
        session_id: &str,
        capability: &str,
    ) -> CapabilityDecision {
        let Some(mut current) = self.sessions.get(session_id) else {
            return CapabilityDecision::Deny;
        };
        loop {
            if let Some(record) = &current.capabilities {
                if !self.signer.verify(record) || !record.allows_id(capability) {
                    return CapabilityDecision::Deny;
                }
            }
            let Some(parent_id) = current.parent_session_id.as_deref() else {
                break;
            };
            let Some(parent) = self.sessions.get(parent_id) else {
                return CapabilityDecision::Deny;
            };
            current = parent;
        }
        CapabilityDecision::Allow
    }
}

fn write_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(value: &str) -> Result<Vec<u8>> {
    anyhow::ensure!(value.len().is_multiple_of(2), "odd hex length");
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).map_err(Into::into))
        .collect()
}

fn hmac_blake3(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut padded = [0_u8; BLOCK];
    if key.len() > BLOCK {
        padded[..32].copy_from_slice(blake3::hash(key).as_bytes());
    } else {
        padded[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = padded;
    let mut outer_pad = padded;
    for byte in &mut inner_pad {
        *byte ^= 0x36;
    }
    for byte in &mut outer_pad {
        *byte ^= 0x5c;
    }
    let mut inner_hasher = blake3::Hasher::new();
    inner_hasher.update(&inner_pad);
    inner_hasher.update(message);
    let inner = inner_hasher.finalize();
    let mut outer_hasher = blake3::Hasher::new();
    outer_hasher.update(&outer_pad);
    outer_hasher.update(inner.as_bytes());
    *outer_hasher.finalize().as_bytes()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(ids: &[&str]) -> CapabilitySet {
        CapabilitySet {
            ids: ids.iter().map(|id| (*id).to_string()).collect(),
            can_fork: true,
            ..CapabilitySet::default()
        }
    }

    #[test]
    fn intersection_never_expands_parent_property() {
        let universe = [
            "tool.terminal",
            "session.fork",
            "vault.read:a",
            "vault.read:b",
        ];
        for parent_mask in 0_u8..16 {
            for requested_mask in 0_u8..16 {
                let parent = caps(
                    &universe
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| parent_mask & (1 << index) != 0)
                        .map(|(_, id)| *id)
                        .collect::<Vec<_>>(),
                );
                let requested = caps(
                    &universe
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| requested_mask & (1 << index) != 0)
                        .map(|(_, id)| *id)
                        .collect::<Vec<_>>(),
                );
                let effective = CapabilitySet::intersect(&parent, &requested);
                assert!(effective.is_narrower_than(&parent));
            }
        }
    }

    #[test]
    fn signer_detects_tampering_and_uses_0600_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("capability.key");
        let signer = CapabilitySigner::load_or_create_at(&path).unwrap();
        let mut set = caps(&["tool.terminal"]);
        signer.sign(&mut set).unwrap();
        assert!(signer.verify(&set));
        set.ids.insert("package.install".into());
        assert!(!signer.verify(&set));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
