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
    pub active: bool,
    pub trust: String,
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
                self.packages
                    .entry(package_id)
                    .or_insert_with(|| PackageRecord {
                        manifest: legacy_manifest(kind, slug, definition),
                        manifest_toml: legacy_manifest_toml(kind, slug, definition),
                        digest: blake3::hash(definition.as_bytes()).to_hex().to_string(),
                        source: "legacy-registry".into(),
                        installed_by: "legacy".into(),
                        installed_at: *registered_at,
                        granted_capabilities: BTreeSet::new(),
                        active: true,
                        trust: crate::package::DEV_UNSIGNED.into(),
                    });
            }
            Event::PackageInstalled {
                manifest,
                digest,
                source,
                installed_by,
                installed_at,
            } => {
                if let Ok(parsed) = PackageManifest::parse_toml(manifest) {
                    let id = parsed.package.id.clone();
                    self.remove_package_entries(&id);
                    self.packages.insert(
                        id,
                        PackageRecord {
                            manifest: parsed,
                            manifest_toml: manifest.clone(),
                            digest: digest.clone(),
                            source: source.clone(),
                            installed_by: installed_by.clone(),
                            installed_at: *installed_at,
                            granted_capabilities: BTreeSet::new(),
                            active: false,
                            trust: crate::package::DEV_UNSIGNED.into(),
                        },
                    );
                }
            }
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
                if let Some(package) = self.packages.get_mut(package_id) {
                    package.active = true;
                    let manifest = package.manifest.clone();
                    self.add_package_entries(package_id, &manifest, *activated_at);
                }
            }
            Event::PackageDeactivated { package_id, .. } => {
                self.remove_package_entries(package_id);
                if let Some(package) = self.packages.get_mut(package_id) {
                    package.active = false;
                }
            }
            Event::PackageRemoved { package_id, .. } => {
                self.remove_package_entries(package_id);
                self.packages.remove(package_id);
            }
            _ => {}
        }
    }

    fn add_package_entries(&mut self, package_id: &str, manifest: &PackageManifest, at: f64) {
        for (kind, contribution) in adapter_contributions(&manifest.contributions) {
            let definition =
                serde_json::to_string(&contribution.definition).unwrap_or_else(|_| "{}".into());
            let key = Self::key(kind, &contribution.id);
            self.entries.insert(
                key.clone(),
                RegistryEntry {
                    kind: kind.into(),
                    slug: contribution.id.clone(),
                    definition,
                    registered_at: at,
                },
            );
            self.entry_owners.insert(key, package_id.into());
        }
    }

    fn remove_package_entries(&mut self, package_id: &str) {
        let keys: Vec<_> = self
            .entry_owners
            .iter()
            .filter_map(|(key, owner)| (owner == package_id).then_some(key.clone()))
            .collect();
        for key in keys {
            self.entries.remove(&key);
            self.entry_owners.remove(&key);
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
        self.packages
            .values()
            .filter(|package| package.active)
            .flat_map(|package| {
                package
                    .manifest
                    .provided_capabilities()
                    .into_iter()
                    .map(move |capability| (capability, package.manifest.package.id.clone()))
            })
            .collect()
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
}
