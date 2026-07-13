//! Registry projection — resolves (kind, slug) → definition.
//!
//! A deterministic projection of the event log (ADR 0006 §9.4). On restart it
//! is rebuilt by [`super::ViewManager::replay`]; live events are applied via
//! [`RegistryView::apply`]. The log remains the sole source of truth.
//!
//! The registry is the authority: a slug → definition record that the adapter
//! resolves before materializing. `EntryRegistered` has PUT semantics (full
//! replace). Drift detection compares the registry against a node's discovered
//! configs and surfaces entries that exist on the node but NOT in the registry.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{anyhow, ensure, Result};

use crate::event::Event;
use crate::package::{Contribution, Contributions, PackageManifest};

#[derive(Debug, Clone)]
pub struct PackageRecord {
    pub manifest: PackageManifest,
    pub manifest_toml: String,
    pub digest: String,
    pub source: String,
    pub installed_by: String,
    pub installed_at: f64,
    pub granted_capabilities: BTreeSet<String>,
    pub bindings: BTreeMap<String, String>,
    pub active: bool,
    pub activated_at: Option<f64>,
    pub trust: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityProviderRecord {
    pub package_id: String,
    pub package_version: String,
    pub package_digest: String,
    pub contribution_id: String,
    pub definition: toml::Table,
}

// RegistryEntry moved to `olympus-envoy` (ADR 0008 S2) — the adapter consumes
// resolved entries envoy-side. Re-exported so existing call sites keep working.
pub use olympus_envoy::adapter::RegistryEntry;

/// In-memory projection of the registry from the event log (ADR 0006 §9.4).
pub struct RegistryView {
    /// (kind, slug) → active adapter entry.
    entries: HashMap<String, RegistryEntry>,
    entry_owners: HashMap<String, String>,
    packages: HashMap<String, PackageRecord>,
}

impl RegistryView {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            entry_owners: HashMap::new(),
            packages: HashMap::new(),
        }
    }

    fn key(kind: &str, slug: &str) -> String {
        format!("{kind}/{slug}")
    }

    /// Apply registry-v1 and package-v2 events deterministically.
    pub fn apply(&mut self, event: &Event) {
        match event {
            Event::EntryRegistered {
                kind,
                slug,
                definition,
                registered_at,
            } => {
                let package_id = format!("legacy.{kind}.{slug}");
                let key = Self::key(kind, slug);
                self.entries.insert(
                    key.clone(),
                    RegistryEntry {
                        kind: kind.clone(),
                        slug: slug.clone(),
                        definition: definition.clone(),
                        registered_at: *registered_at,
                    },
                );
                self.entry_owners.insert(key, package_id.clone());
                self.packages.insert(
                    package_id,
                    PackageRecord {
                        manifest: legacy_manifest(kind, slug, definition),
                        manifest_toml: legacy_manifest_toml(kind, slug, definition),
                        digest: blake3::hash(definition.as_bytes()).to_hex().to_string(),
                        source: "legacy-registry".into(),
                        installed_by: "legacy".into(),
                        installed_at: *registered_at,
                        granted_capabilities: BTreeSet::new(),
                        bindings: BTreeMap::new(),
                        active: true,
                        activated_at: Some(*registered_at),
                        trust: crate::package::DEV_UNSIGNED.into(),
                    },
                );
            }
            Event::PackageInstalled {
                manifest,
                digest,
                source,
                installed_by,
                installed_at,
            } => self.install_package(
                manifest,
                digest,
                source,
                installed_by,
                *installed_at,
                BTreeMap::new(),
            ),
            Event::PackageInstalledV2 {
                manifest,
                digest,
                source,
                installed_by,
                installed_at,
                bindings,
            } => self.install_package(
                manifest,
                digest,
                source,
                installed_by,
                *installed_at,
                bindings.clone(),
            ),
            Event::PackageGranted {
                package_id,
                capabilities,
                ..
            } => {
                if let Some(package) = self.packages.get_mut(package_id) {
                    package.granted_capabilities = capabilities.iter().cloned().collect();
                }
            }
            Event::PackageActivated {
                package_id,
                activated_at,
                ..
            } => {
                if self.validate_activation(package_id).is_ok() {
                    let package = self
                        .packages
                        .get_mut(package_id)
                        .expect("validated package exists");
                    package.active = true;
                    package.activated_at = Some(*activated_at);
                    self.rebuild_entries();
                }
            }
            Event::PackageDeactivated { package_id, .. } => {
                if let Some(package) = self.packages.get_mut(package_id) {
                    package.active = false;
                    package.activated_at = None;
                }
                self.rebuild_entries();
            }
            Event::PackageRemoved { package_id, .. } => {
                self.packages.remove(package_id);
                self.rebuild_entries();
            }
            _ => {}
        }
    }

    fn install_package(
        &mut self,
        manifest: &str,
        digest: &str,
        source: &str,
        installed_by: &str,
        installed_at: f64,
        bindings: BTreeMap<String, String>,
    ) {
        let Ok(parsed) = PackageManifest::parse_toml(manifest) else {
            return;
        };
        let id = parsed.package.id.clone();
        let record = PackageRecord {
            manifest: parsed,
            manifest_toml: manifest.into(),
            digest: digest.into(),
            source: source.into(),
            installed_by: installed_by.into(),
            installed_at,
            granted_capabilities: BTreeSet::new(),
            bindings,
            active: false,
            activated_at: None,
            trust: crate::package::DEV_UNSIGNED.into(),
        };
        if record.manifest.package.publisher == "legacy"
            && self
                .packages
                .get(&id)
                .is_some_and(|existing| existing.manifest.package.publisher == "legacy")
        {
            self.packages.insert(id, record);
        } else {
            self.packages.entry(id).or_insert(record);
        }
    }

    fn rebuild_entries(&mut self) {
        self.entries.clear();
        self.entry_owners.clear();
        let owners = self.active_capabilities();
        let mut active: Vec<_> = self
            .packages
            .iter()
            .filter(|(_, package)| package.active)
            .collect();
        active.sort_by(|(id_a, a), (id_b, b)| {
            a.activated_at
                .partial_cmp(&b.activated_at)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(id_a.cmp(id_b))
        });
        for (package_id, package) in active {
            let at = package.activated_at.unwrap_or(package.installed_at);
            for (kind, contribution) in adapter_contributions(&package.manifest.contributions) {
                if !contribution.provides.is_empty()
                    && !contribution
                        .provides
                        .iter()
                        .all(|capability| owners.get(capability) == Some(package_id))
                {
                    continue;
                }
                let definition =
                    serde_json::to_string(&contribution.definition).unwrap_or_else(|_| "{}".into());
                let key = Self::key(kind, &contribution.id);
                if contribution.provides.is_empty() && self.entries.contains_key(&key) {
                    continue;
                }
                self.entries.insert(
                    key.clone(),
                    RegistryEntry {
                        kind: kind.into(),
                        slug: contribution.id.clone(),
                        definition,
                        registered_at: at,
                    },
                );
                self.entry_owners.insert(key, package_id.clone());
            }
        }
    }

    pub fn package(&self, id: &str) -> Option<&PackageRecord> {
        self.packages.get(id)
    }

    pub fn packages(&self) -> Vec<&PackageRecord> {
        let mut values: Vec<_> = self.packages.values().collect();
        values.sort_by(|a, b| a.manifest.package.id.cmp(&b.manifest.package.id));
        values
    }

    pub fn active_capabilities(&self) -> BTreeMap<String, String> {
        let mut owners = BTreeMap::from([("job.run".into(), "core.jobs".into())]);
        let mut active: Vec<_> = self
            .packages
            .values()
            .filter(|package| package.active)
            .collect();
        active.sort_by(|a, b| {
            a.activated_at
                .partial_cmp(&b.activated_at)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.manifest.package.id.cmp(&b.manifest.package.id))
        });
        for package in active {
            for capability in package.manifest.provided_capabilities() {
                match package.bindings.get(&capability) {
                    Some(selected) if selected == &package.manifest.package.id => {
                        owners.insert(capability, selected.clone());
                    }
                    Some(_) => {}
                    None if !owners.contains_key(&capability) => {
                        owners.insert(capability, package.manifest.package.id.clone());
                    }
                    None => {}
                }
            }
        }
        owners
    }

    pub fn validate_activation(&self, id: &str) -> Result<()> {
        let package = self
            .packages
            .get(id)
            .ok_or_else(|| anyhow!("package not found"))?;
        for contribution in &package.manifest.contributions.activity_provider {
            ensure!(
                contribution
                    .definition
                    .get("backend")
                    .and_then(toml::Value::as_str)
                    == Some("jobs"),
                "activity provider {} has unsupported backend; v1 requires backend=jobs",
                contribution.id
            );
        }
        let active = self.active_capabilities();
        for capability in package.manifest.provided_capabilities() {
            if let Some(owner) = active.get(&capability) {
                if owner != id {
                    let selected = package.bindings.get(&capability);
                    ensure!(
                        selected == Some(owner) || selected.is_some_and(|value| value == id),
                        "capability collision: {capability} is provided by {owner}; persist a binding selecting {owner} or {id}"
                    );
                }
            }
        }
        Ok(())
    }

    pub fn resolve_activity(&self, capability: &str) -> Option<ActivityProviderRecord> {
        let owner = self.active_capabilities().get(capability)?.clone();
        if owner == "core.jobs" && capability == "job.run" {
            return Some(ActivityProviderRecord {
                package_id: owner,
                package_version: crate::package::OLYMPUS_API_VERSION.into(),
                package_digest: "builtin:jobs-v1".into(),
                contribution_id: "job-runner".into(),
                definition: toml::Table::from_iter([(
                    "backend".into(),
                    toml::Value::String("jobs".into()),
                )]),
            });
        }
        let package = self.packages.get(&owner)?;
        let contribution = package
            .manifest
            .contributions
            .activity_provider
            .iter()
            .find(|item| item.provides.contains(capability))?;
        Some(ActivityProviderRecord {
            package_id: owner,
            package_version: package.manifest.package.version.clone(),
            package_digest: package.digest.clone(),
            contribution_id: contribution.id.clone(),
            definition: contribution.definition.clone(),
        })
    }

    /// Resolve a (kind, slug) pair to its definition, if registered.
    pub fn get(&self, kind: &str, slug: &str) -> Option<&RegistryEntry> {
        self.entries.get(&Self::key(kind, slug))
    }

    /// Resolve a batch of slugs for a kind. Returns (found, missing) where
    /// missing is the list of unregistered slugs (so the adapter can warn).
    pub fn resolve_batch(
        &self,
        kind: &str,
        slugs: &[String],
    ) -> (Vec<&RegistryEntry>, Vec<String>) {
        let mut found = Vec::new();
        let mut missing = Vec::new();
        for slug in slugs {
            match self.get(kind, slug) {
                Some(e) => found.push(e),
                None => missing.push(slug.clone()),
            }
        }
        (found, missing)
    }

    /// All entries of a given kind.
    pub fn list_kind(&self, kind: &str) -> Vec<&RegistryEntry> {
        let mut out: Vec<&RegistryEntry> =
            self.entries.values().filter(|e| e.kind == kind).collect();
        out.sort_by(|a, b| a.slug.cmp(&b.slug));
        out
    }

    /// All entries (for listing / debugging).
    pub fn list(&self) -> Vec<&RegistryEntry> {
        let mut out: Vec<&RegistryEntry> = self.entries.values().collect();
        out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.slug.cmp(&b.slug)));
        out
    }

    /// Drift detection: given a set of (kind, slug) pairs discovered on the
    /// node, return (unregistered, orphaned):
    /// - `unregistered`: on the node AND in the registry — fine, expected.
    ///   (returned as a count for info, not a warning)
    /// - `orphaned`: on the node but NOT in the registry — the warning the
    ///   operator asked for ("a non-recorded config exists; remove it or
    ///   register it").
    /// - `missing`: in the registry but NOT on the node — the declared setup
    ///   can't be materialized until the node installs it.
    ///
    /// `discovered` is a list of (kind, slug) the envoy found by scanning the
    /// node's actual installed configs.
    pub fn drift(&self, discovered: &[(String, String)]) -> DriftReport {
        let registered: std::collections::HashSet<String> = self.entries.keys().cloned().collect();
        let discovered_set: std::collections::HashSet<String> =
            discovered.iter().map(|(k, s)| Self::key(k, s)).collect();

        let orphaned: Vec<(String, String)> = discovered
            .iter()
            .filter(|(k, s)| !registered.contains(&Self::key(k, s)))
            .cloned()
            .collect();
        let missing: Vec<(String, String)> = self
            .entries
            .values()
            .filter(|e| !discovered_set.contains(&Self::key(&e.kind, &e.slug)))
            .map(|e| (e.kind.clone(), e.slug.clone()))
            .collect();

        DriftReport {
            matched: registered.intersection(&discovered_set).count() as u64,
            orphaned,
            missing,
        }
    }
}

/// Result of drift detection between the registry and a node's discovered configs.
#[derive(Debug, Clone, PartialEq)]
pub struct DriftReport {
    /// Count of entries both registered and discovered (healthy).
    pub matched: u64,
    /// (kind, slug) on the node but NOT in the registry → the warning.
    pub orphaned: Vec<(String, String)>,
    /// (kind, slug) registered but NOT on the node → can't materialize yet.
    pub missing: Vec<(String, String)>,
}

fn adapter_contributions(contributions: &Contributions) -> Vec<(&'static str, &Contribution)> {
    contributions
        .session_tool_provider
        .iter()
        .map(|item| ("mcp", item))
        .chain(contributions.skill.iter().map(|item| ("skill", item)))
        .chain(
            contributions
                .runtime_adapter
                .iter()
                .map(|item| ("plugin", item)),
        )
        .chain(
            contributions
                .policy_provider
                .iter()
                .map(|item| ("hook", item)),
        )
        .collect()
}

fn legacy_manifest(kind: &str, slug: &str, definition: &str) -> PackageManifest {
    PackageManifest::parse_toml(&legacy_manifest_toml(kind, slug, definition))
        .expect("generated legacy package manifest is valid")
}

pub fn legacy_manifest_toml(kind: &str, slug: &str, definition: &str) -> String {
    let class = match kind {
        "mcp" => "session_tool_provider",
        "skill" => "skill",
        "plugin" => "runtime_adapter",
        "hook" => "policy_provider",
        _ => "resource_provider",
    };
    let table: toml::Table = serde_json::from_str::<serde_json::Value>(definition)
        .ok()
        .and_then(|value| toml::Value::try_from(value).ok())
        .and_then(|value| value.as_table().cloned())
        .unwrap_or_default();
    let definition_toml = toml::to_string(&table).unwrap_or_default();
    format!(
        "[package]\nid = {id:?}\nname = {name:?}\nversion = \"0.0.0\"\npublisher = \"legacy\"\nlicense = \"unknown\"\n\n[compatibility]\nolympus_api = \"*\"\nplatforms = [\"*\"]\n\n[[contributions.{class}]]\nid = {slug:?}\n\n[contributions.{class}.definition]\n{definition_toml}",
        id = format!("legacy.{kind}.{slug}"),
        name = format!("Legacy {kind} {slug}"),
    )
}

impl Default for RegistryView {
    fn default() -> Self {
        Self::new()
    }
}

/// The registry view is the hall-side implementation of the envoy adapter's
/// resolver seam: it resolves declared slugs to concrete definitions that the
/// setup adapter then materializes into the session space.
impl olympus_envoy::adapter::SlugResolver for RegistryView {
    fn resolve_batch_owned(
        &self,
        kind: &str,
        slugs: &[String],
    ) -> (Vec<RegistryEntry>, Vec<String>) {
        let (found, missing) = self.resolve_batch(kind, slugs);
        (found.into_iter().cloned().collect(), missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package_manifest(id: &str, version: &str, contribution: &str) -> String {
        format!(
            r#"[package]
id = "{id}"
name = "Test package"
version = "{version}"
publisher = "test"
license = "MIT"

[compatibility]
olympus_api = "*"
platforms = ["*"]

{contribution}
"#
        )
    }

    fn installed_v2(
        id: &str,
        version: &str,
        digest: &str,
        contribution: &str,
        bindings: BTreeMap<String, String>,
    ) -> Event {
        Event::PackageInstalledV2 {
            manifest: package_manifest(id, version, contribution),
            digest: digest.into(),
            source: "inline".into(),
            installed_by: "operator".into(),
            installed_at: 1.0,
            bindings,
        }
    }

    fn registered(kind: &str, slug: &str, def: &str, at: f64) -> Event {
        Event::EntryRegistered {
            kind: kind.into(),
            slug: slug.into(),
            definition: def.into(),
            registered_at: at,
        }
    }

    #[test]
    fn apply_registers_entry() {
        let mut v = RegistryView::new();
        v.apply(&registered(
            "mcp",
            "gitnexus",
            r#"{"command":"gitnexus"}"#,
            1.0,
        ));
        let e = v.get("mcp", "gitnexus").expect("entry exists");
        assert_eq!(e.definition, r#"{"command":"gitnexus"}"#);
    }

    #[test]
    fn entry_registered_is_replace() {
        let mut v = RegistryView::new();
        v.apply(&registered("mcp", "gitnexus", r#"{"v":1}"#, 1.0));
        v.apply(&registered("mcp", "gitnexus", r#"{"v":2}"#, 2.0));
        let e = v.get("mcp", "gitnexus").unwrap();
        assert_eq!(e.definition, r#"{"v":2}"#);
        assert_eq!(e.registered_at, 2.0);
    }

    #[test]
    fn resolve_batch_splits_found_and_missing() {
        let mut v = RegistryView::new();
        v.apply(&registered("mcp", "gitnexus", "{}", 1.0));
        v.apply(&registered("mcp", "grafana", "{}", 2.0));
        let (found, missing) = v.resolve_batch(
            "mcp",
            &["gitnexus".into(), "unknown".into(), "grafana".into()],
        );
        assert_eq!(found.len(), 2);
        assert_eq!(missing, vec!["unknown"]);
    }

    #[test]
    fn get_unknown_is_none() {
        let v = RegistryView::new();
        assert!(v.get("mcp", "ghost").is_none());
    }

    #[test]
    fn list_kind_filters() {
        let mut v = RegistryView::new();
        v.apply(&registered("mcp", "gitnexus", "{}", 1.0));
        v.apply(&registered("skill", "code-review", "{}", 2.0));
        v.apply(&registered("mcp", "grafana", "{}", 3.0));
        let mcp = v.list_kind("mcp");
        assert_eq!(mcp.len(), 2);
        assert!(mcp.iter().any(|e| e.slug == "gitnexus"));
        assert!(mcp.iter().any(|e| e.slug == "grafana"));
    }

    #[test]
    fn non_registry_events_ignored() {
        let mut v = RegistryView::new();
        v.apply(&Event::CardCompleted {
            card_id: "c1".into(),
            completed_at: 1.0,
        });
        assert!(v.list().is_empty());
    }

    #[test]
    fn drift_finds_orphaned_and_missing() {
        let mut v = RegistryView::new();
        // Registry has: mcp/gitnexus, mcp/grafana, skill/code-review
        v.apply(&registered("mcp", "gitnexus", "{}", 1.0));
        v.apply(&registered("mcp", "grafana", "{}", 2.0));
        v.apply(&registered("skill", "code-review", "{}", 3.0));

        // Node discovered: mcp/gitnexus (matched), mcp/bytebase (orphaned!)
        // Missing from node: mcp/grafana, skill/code-review
        let report = v.drift(&[
            ("mcp".into(), "gitnexus".into()),
            ("mcp".into(), "bytebase".into()),
        ]);
        assert_eq!(report.matched, 1); // gitnexus matched
        assert!(report.orphaned.contains(&("mcp".into(), "bytebase".into())));
        assert!(report.missing.contains(&("mcp".into(), "grafana".into())));
        assert!(report
            .missing
            .contains(&("skill".into(), "code-review".into())));
    }

    #[test]
    fn drift_clean_when_no_difference() {
        let mut v = RegistryView::new();
        v.apply(&registered("mcp", "gitnexus", "{}", 1.0));
        let report = v.drift(&[("mcp".into(), "gitnexus".into())]);
        assert_eq!(report.matched, 1);
        assert!(report.orphaned.is_empty());
        assert!(report.missing.is_empty());
    }

    #[test]
    fn legacy_migration_is_idempotent_across_double_replay() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let log = crate::log::Log::open(file.path()).unwrap();
        log.append(&registered(
            "mcp",
            "gitnexus",
            r#"{"command":"gitnexus"}"#,
            1.0,
        ))
        .unwrap();
        let mut views = crate::views::ViewManager::new();
        views.replay(&log).unwrap();
        views.replay(&log).unwrap();
        assert_eq!(views.registry.packages().len(), 1);
        let package = views.registry.package("legacy.mcp.gitnexus").unwrap();
        assert!(package.active);
        assert_eq!(views.registry.list_kind("mcp").len(), 1);
    }

    #[test]
    fn package_identity_is_first_write_immutable() {
        let mut view = RegistryView::new();
        view.apply(&installed_v2(
            "acme.runner",
            "1.0.0",
            "digest-v1",
            "",
            BTreeMap::new(),
        ));
        view.apply(&installed_v2(
            "acme.runner",
            "2.0.0",
            "digest-v2",
            "",
            BTreeMap::new(),
        ));

        let package = view.package("acme.runner").unwrap();
        assert_eq!(package.manifest.package.version, "1.0.0");
        assert_eq!(package.digest, "digest-v1");
    }

    #[test]
    fn activation_fails_closed_on_unbound_provider_collision() {
        let mut view = RegistryView::new();
        view.apply(&installed_v2(
            "acme.runner",
            "1.0.0",
            "digest",
            r#"[[contributions.activity_provider]]
id = "runner"
provides = ["job.run"]
[contributions.activity_provider.definition]
backend = "jobs""#,
            BTreeMap::new(),
        ));

        assert!(view.validate_activation("acme.runner").is_err());
        view.apply(&Event::PackageActivated {
            package_id: "acme.runner".into(),
            activated_by: "operator".into(),
            activated_at: 2.0,
        });
        assert!(!view.package("acme.runner").unwrap().active);
        assert_eq!(
            view.resolve_activity("job.run").unwrap().package_id,
            "core.jobs"
        );
    }

    #[test]
    fn durable_binding_selects_and_pins_jobs_activity_provider() {
        let mut view = RegistryView::new();
        let mut bindings = BTreeMap::new();
        bindings.insert("job.run".into(), "acme.runner".into());
        view.apply(&installed_v2(
            "acme.runner",
            "1.2.3",
            "digest-123",
            r#"[[contributions.activity_provider]]
id = "runner"
provides = ["job.run"]
[contributions.activity_provider.definition]
backend = "jobs""#,
            bindings,
        ));

        assert!(view.validate_activation("acme.runner").is_ok());
        view.apply(&Event::PackageActivated {
            package_id: "acme.runner".into(),
            activated_by: "operator".into(),
            activated_at: 2.0,
        });

        let provider = view.resolve_activity("job.run").unwrap();
        assert_eq!(provider.package_id, "acme.runner");
        assert_eq!(provider.package_version, "1.2.3");
        assert_eq!(provider.package_digest, "digest-123");
        assert_eq!(
            provider
                .definition
                .get("backend")
                .and_then(toml::Value::as_str),
            Some("jobs")
        );
    }

    /// Full lifecycle replay: install → grant → activate → check visible →
    /// deactivate → check invisible → remove → check gone.
    /// Re-applying the same event sequence twice must produce the same result
    /// (deterministic projection).
    #[test]
    fn full_lifecycle_replay_is_deterministic() {
        let mcp_contribution = r#"[[contributions.session_tool_provider]]
id = "my-mcp"
[contributions.session_tool_provider.definition]
command = "my-mcp-server""#;

        let skill_contribution = r#"[[contributions.skill]]
id = "my-skill"
[contributions.skill.definition]
dir = "skills/my-skill""#;

        let contribution = format!("{mcp_contribution}\n{skill_contribution}");

        let events = vec![
            installed_v2("acme.pkg", "1.0.0", "d1", &contribution, BTreeMap::new()),
            Event::PackageGranted {
                package_id: "acme.pkg".into(),
                capabilities: vec![],
                granted_by: "operator".into(),
                granted_at: 2.0,
            },
            Event::PackageActivated {
                package_id: "acme.pkg".into(),
                activated_by: "operator".into(),
                activated_at: 3.0,
            },
        ];

        let snapshot_active = {
            let mut view = RegistryView::new();
            for e in &events {
                view.apply(e);
            }
            let pkg = view.package("acme.pkg").expect("package exists");
            assert!(pkg.active, "package must be active after PackageActivated");
            // contributions are visible in the adapter entries
            assert!(
                view.get("mcp", "my-mcp").is_some(),
                "mcp contribution visible"
            );
            assert!(
                view.get("skill", "my-skill").is_some(),
                "skill contribution visible"
            );
            (pkg.active, pkg.digest.clone())
        };

        // Deactivate — contributions must disappear.
        let mut view = RegistryView::new();
        for e in &events {
            view.apply(e);
        }
        view.apply(&Event::PackageDeactivated {
            package_id: "acme.pkg".into(),
            deactivated_by: "operator".into(),
            deactivated_at: 4.0,
        });
        assert!(!view.package("acme.pkg").unwrap().active);
        assert!(
            view.get("mcp", "my-mcp").is_none(),
            "mcp gone after deactivate"
        );

        // Remove — package disappears entirely.
        view.apply(&Event::PackageRemoved {
            package_id: "acme.pkg".into(),
            removed_by: "operator".into(),
            removed_at: 5.0,
        });
        assert!(view.package("acme.pkg").is_none(), "package removed");

        // Re-replay the activate sequence and verify same projection as first time.
        let mut view2 = RegistryView::new();
        for e in &events {
            view2.apply(e);
        }
        let pkg2 = view2.package("acme.pkg").unwrap();
        assert_eq!(pkg2.active, snapshot_active.0);
        assert_eq!(pkg2.digest, snapshot_active.1);
    }
}
