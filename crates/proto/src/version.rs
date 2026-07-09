//! Protocol + build version identity (ADR 0008 §1).
//!
//! Hello carries **two version fields with distinct jobs**:
//! - [`PROTOCOL_VERSION`] — frame-schema compat gate. Unparseable/unknown
//!   version → Hall rejects registration (fail closed). Changes rarely.
//! - [`BuildVersion`] — envoy **build identity**. This is what drain/evict
//!   decisions key on and what the Nodes UI shows.

use serde::{Deserialize, Serialize};

/// Frame-schema compatibility version. v1 was the hello/heartbeat/bye-only
/// node protocol; v2 adds the session frame families (ADR 0008 §1).
pub const PROTOCOL_VERSION: u32 = 2;

/// Build identity: which build of the binary is speaking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildVersion {
    /// Cargo package semver (`CARGO_PKG_VERSION`).
    pub semver: String,
    /// Short git commit hash of the build, or `"unknown"` when git was absent
    /// at build time (source tarball builds).
    #[serde(default = "unknown")]
    pub git_hash: String,
    /// Build timestamp (unix epoch seconds as a string), or `"unknown"`.
    #[serde(default = "unknown")]
    pub built_at: String,
}

fn unknown() -> String {
    "unknown".to_string()
}

impl BuildVersion {
    /// The build identity of this compilation of the proto crate, populated at
    /// build time by `build.rs` (git hash + timestamp; falls back to
    /// `"unknown"` when git is unavailable).
    pub fn current() -> Self {
        Self {
            semver: env!("CARGO_PKG_VERSION").to_string(),
            git_hash: env!("OLYMPUS_GIT_HASH").to_string(),
            built_at: env!("OLYMPUS_BUILT_AT").to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_version_round_trips_camel_case() {
        let v = BuildVersion::current();
        assert!(!v.semver.is_empty());
        assert!(!v.git_hash.is_empty());
        let json = serde_json::to_value(&v).unwrap();
        assert!(json.get("gitHash").is_some(), "camelCase wire naming");
        assert!(json.get("builtAt").is_some(), "camelCase wire naming");
        let back: BuildVersion = serde_json::from_value(json).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn build_version_tolerates_missing_optional_fields() {
        let v: BuildVersion = serde_json::from_str(r#"{"semver":"0.1.0"}"#).unwrap();
        assert_eq!(v.git_hash, "unknown");
        assert_eq!(v.built_at, "unknown");
    }
}
