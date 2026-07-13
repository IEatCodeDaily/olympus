//! Declarative package manifests and pre-execution validation (ADR 0012).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const OLYMPUS_API_VERSION: &str = "0.1";
pub const DEV_UNSIGNED: &str = "dev-unsigned";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PackageManifest {
    pub package: PackageMetadata,
    pub compatibility: Compatibility,
    #[serde(default)]
    pub capabilities: CapabilityReview,
    #[serde(default)]
    pub contributions: Contributions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PackageMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub license: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Compatibility {
    pub olympus_api: String,
    #[serde(default)]
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityReview {
    #[serde(default)]
    pub required: BTreeSet<String>,
}

/// Typed contribution tables. Definitions remain schema/protocol data, never a
/// Rust ABI. `definition` is class-specific TOML converted losslessly to JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Contributions {
    #[serde(default)]
    pub activity_provider: Vec<Contribution>,
    #[serde(default)]
    pub trigger_provider: Vec<Contribution>,
    #[serde(default)]
    pub resource_provider: Vec<Contribution>,
    #[serde(default)]
    pub session_tool_provider: Vec<Contribution>,
    #[serde(default)]
    pub runtime_adapter: Vec<Contribution>,
    #[serde(default)]
    pub embedded_app: Vec<Contribution>,
    #[serde(default)]
    pub indexer_extractor: Vec<Contribution>,
    #[serde(default)]
    pub policy_provider: Vec<Contribution>,
    #[serde(default)]
    pub view_provider: Vec<Contribution>,
    #[serde(default)]
    pub storage_provider: Vec<Contribution>,
    #[serde(default)]
    pub skill: Vec<Contribution>,
    #[serde(default)]
    pub workflow_template: Vec<Contribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Contribution {
    pub id: String,
    #[serde(default)]
    pub provides: BTreeSet<String>,
    #[serde(default)]
    pub state_namespaces: Vec<String>,
    #[serde(default)]
    pub definition: toml::Table,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStage {
    Schema,
    Compatibility,
    CapabilityReview,
    Signature,
    Collision,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub stages: Vec<ValidationStage>,
    pub requested_capabilities: BTreeSet<String>,
    pub trust: String,
}

impl PackageManifest {
    pub fn parse_toml(source: &str) -> Result<Self> {
        toml::from_str(source).context("parsing package manifest TOML")
    }

    pub fn validate_schema(&self) -> Result<()> {
        for (label, value) in [
            ("package.id", self.package.id.as_str()),
            ("package.name", self.package.name.as_str()),
            ("package.version", self.package.version.as_str()),
            ("package.publisher", self.package.publisher.as_str()),
            ("package.license", self.package.license.as_str()),
        ] {
            anyhow::ensure!(!value.trim().is_empty(), "{label} must be non-empty");
        }
        anyhow::ensure!(valid_id(&self.package.id), "invalid package.id");
        anyhow::ensure!(
            valid_version(&self.package.version),
            "invalid package.version"
        );
        let mut ids = BTreeSet::new();
        for (_, contribution) in self.contributions.all() {
            anyhow::ensure!(
                valid_id(&contribution.id),
                "invalid contribution id {}",
                contribution.id
            );
            anyhow::ensure!(
                ids.insert(contribution.id.clone()),
                "duplicate contribution id {}",
                contribution.id
            );
            for capability in &contribution.provides {
                validate_capability(capability)?;
            }
            for namespace in &contribution.state_namespaces {
                anyhow::ensure!(
                    namespace.starts_with("plugin-state://"),
                    "invalid plugin state namespace {namespace}"
                );
            }
        }
        for capability in &self.capabilities.required {
            validate_capability(capability)?;
        }
        Ok(())
    }

    pub fn validate_compatibility(&self, olympus_api: &str, platform: &str) -> Result<()> {
        let requested = self.compatibility.olympus_api.trim();
        anyhow::ensure!(
            requested == "*"
                || requested == olympus_api
                || requested
                    .strip_prefix('^')
                    .is_some_and(|v| v == olympus_api),
            "package requires Olympus API {requested}, host is {olympus_api}"
        );
        anyhow::ensure!(
            self.compatibility.platforms.is_empty()
                || self
                    .compatibility
                    .platforms
                    .iter()
                    .any(|p| p == "*" || p == platform),
            "package does not support platform {platform}"
        );
        Ok(())
    }

    pub fn unsupported_classes(&self) -> Vec<&'static str> {
        let c = &self.contributions;
        [
            ("trigger_provider", !c.trigger_provider.is_empty()),
            ("resource_provider", !c.resource_provider.is_empty()),
            ("runtime_adapter", !c.runtime_adapter.is_empty()),
            ("embedded_app", !c.embedded_app.is_empty()),
            ("indexer_extractor", !c.indexer_extractor.is_empty()),
            ("policy_provider", !c.policy_provider.is_empty()),
            ("view_provider", !c.view_provider.is_empty()),
            ("storage_provider", !c.storage_provider.is_empty()),
        ]
        .into_iter()
        .filter_map(|(name, present)| present.then_some(name))
        .collect()
    }

    pub fn provided_capabilities(&self) -> BTreeSet<String> {
        self.contributions
            .all()
            .into_iter()
            .flat_map(|(_, c)| c.provides.iter().cloned())
            .collect()
    }
}

impl Contributions {
    pub fn all(&self) -> Vec<(&'static str, &Contribution)> {
        let groups: [(&str, &Vec<Contribution>); 12] = [
            ("activity_provider", &self.activity_provider),
            ("trigger_provider", &self.trigger_provider),
            ("resource_provider", &self.resource_provider),
            ("session_tool_provider", &self.session_tool_provider),
            ("runtime_adapter", &self.runtime_adapter),
            ("embedded_app", &self.embedded_app),
            ("indexer_extractor", &self.indexer_extractor),
            ("policy_provider", &self.policy_provider),
            ("view_provider", &self.view_provider),
            ("storage_provider", &self.storage_provider),
            ("skill", &self.skill),
            ("workflow_template", &self.workflow_template),
        ];
        groups
            .into_iter()
            .flat_map(|(kind, values)| values.iter().map(move |value| (kind, value)))
            .collect()
    }
}

pub fn validate_install(
    manifest: &PackageManifest,
    active_capabilities: &BTreeMap<String, String>,
    bindings: &BTreeMap<String, String>,
) -> Result<ValidationReport> {
    manifest.validate_schema()?;
    manifest.validate_compatibility(OLYMPUS_API_VERSION, std::env::consts::OS)?;
    let requested_capabilities = manifest.capabilities.required.clone();
    let provided_capabilities = manifest.provided_capabilities();
    for (capability, provider) in bindings {
        anyhow::ensure!(
            provided_capabilities.contains(capability),
            "binding {capability} does not name a capability provided by this package"
        );
        anyhow::ensure!(
            provider == &manifest.package.id
                || active_capabilities.get(capability) == Some(provider),
            "binding {capability} selects unknown provider {provider}"
        );
    }
    for capability in provided_capabilities {
        if let Some(owner) = active_capabilities.get(&capability) {
            anyhow::ensure!(
                owner == &manifest.package.id
                    || bindings
                        .get(&capability)
                        .is_some_and(|bound| { bound == &manifest.package.id || bound == owner }),
                "capability collision: {capability} is already provided by {owner}"
            );
        }
    }
    Ok(ValidationReport {
        stages: vec![
            ValidationStage::Schema,
            ValidationStage::Compatibility,
            ValidationStage::CapabilityReview,
            ValidationStage::Signature,
            ValidationStage::Collision,
        ],
        requested_capabilities,
        trust: DEV_UNSIGNED.into(),
    })
}

pub fn digest_path(path: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect_files(path, path, &mut files)?;
    files.sort();
    let mut hasher = blake3::Hasher::new();
    for relative in files {
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update(&[0]);
        hasher.update(
            &std::fs::read(path.join(&relative))
                .with_context(|| format!("reading {}", relative.display()))?,
        );
        hasher.update(&[0]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn collect_files(root: &Path, path: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        output.push(path.strip_prefix(root)?.to_path_buf());
        return Ok(());
    }
    for entry in std::fs::read_dir(path)
        .with_context(|| format!("reading package directory {}", path.display()))?
    {
        let entry = entry?;
        let metadata = entry.file_type()?;
        anyhow::ensure!(
            !metadata.is_symlink(),
            "package directory contains symlink: {}",
            entry.path().display()
        );
        collect_files(root, &entry.path(), output)?;
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        })
}

fn valid_version(value: &str) -> bool {
    let core = value.split_once('-').map_or(value, |(core, _)| core);
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn validate_capability(value: &str) -> Result<()> {
    let authority = value
        .split_once(':')
        .map_or(value, |(authority, _)| authority);
    anyhow::ensure!(valid_id(authority), "invalid capability id {value}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(extra: &str) -> PackageManifest {
        PackageManifest::parse_toml(&format!(
            r#"
[package]
id = "acme.tools"
name = "Acme tools"
version = "1.2.3"
publisher = "acme"
license = "MIT"
[compatibility]
olympus_api = "0.1"
platforms = ["*"]
[capabilities]
required = ["job.run"]
{extra}
"#
        ))
        .unwrap()
    }

    #[test]
    fn parses_all_typed_classes() {
        let parsed = manifest(
            r#"
[[contributions.session_tool_provider]]
id = "git"
provides = ["git.query"]
[contributions.session_tool_provider.definition]
command = "git-mcp"
[[contributions.skill]]
id = "review"
[contributions.skill.definition]
dir = "skills/review"
[[contributions.activity_provider]]
id = "build"
[[contributions.workflow_template]]
id = "release"
[[contributions.storage_provider]]
id = "db"
"#,
        );
        assert_eq!(parsed.contributions.all().len(), 5);
        assert_eq!(parsed.unsupported_classes(), vec!["storage_provider"]);
    }

    #[test]
    fn validation_pipeline_rejects_compat_and_collisions() {
        let package =
            manifest("[[contributions.activity_provider]]\nid='runner'\nprovides=['job.run']");
        let mut active = BTreeMap::new();
        active.insert("job.run".into(), "core.jobs".into());
        assert!(validate_install(&package, &active, &BTreeMap::new())
            .unwrap_err()
            .to_string()
            .contains("collision"));
        let mut bindings = BTreeMap::new();
        bindings.insert("job.run".into(), "acme.tools".into());
        assert_eq!(
            validate_install(&package, &active, &bindings)
                .unwrap()
                .trust,
            DEV_UNSIGNED
        );

        let unique =
            manifest("[[contributions.activity_provider]]\nid='runner'\nprovides=['ci.run']");
        let invalid_binding = BTreeMap::from([("ci.run".into(), "ghost.provider".into())]);
        assert!(validate_install(&unique, &active, &invalid_binding)
            .unwrap_err()
            .to_string()
            .contains("binding"));
    }

    #[test]
    fn directory_digest_is_order_independent_and_content_sensitive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b"), "two").unwrap();
        std::fs::write(dir.path().join("a"), "one").unwrap();
        let first = digest_path(dir.path()).unwrap();
        std::fs::write(dir.path().join("a"), "changed").unwrap();
        assert_ne!(first, digest_path(dir.path()).unwrap());
    }
}
