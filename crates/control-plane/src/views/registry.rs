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

use std::collections::HashMap;

use crate::event::Event;

/// A registry entry — one definition for one (kind, slug) pair.
#[derive(Debug, Clone, PartialEq)]
pub struct RegistryEntry {
    pub kind: String, // "skill" | "mcp" | "plugin" | "hook"
    pub slug: String,
    /// Harness-agnostic definition (JSON string):
    /// - MCP → `{"command":"...","args":[...],"env":{...}}`
    /// - skill → `{"dir":"/path/to/skill"}` or a content-addressed ref
    /// - plugin → `{"kind":"install|service", ...}`
    /// - hook → harness-specific JSON
    pub definition: String,
    pub registered_at: f64,
}

/// In-memory projection of the registry from the event log (ADR 0006 §9.4).
pub struct RegistryView {
    /// (kind, slug) → entry. Key is `(kind, slug)` as a composite string.
    entries: HashMap<String, RegistryEntry>,
}

impl RegistryView {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    fn key(kind: &str, slug: &str) -> String {
        format!("{kind}/{slug}")
    }

    /// Apply an event. Only `EntryRegistered` mutates this view; all others
    /// are ignored.
    pub fn apply(&mut self, event: &Event) {
        if let Event::EntryRegistered {
            kind,
            slug,
            definition,
            registered_at,
        } = event
        {
            self.entries.insert(
                Self::key(kind, slug),
                RegistryEntry {
                    kind: kind.clone(),
                    slug: slug.clone(),
                    definition: definition.clone(),
                    registered_at: *registered_at,
                },
            );
        }
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

impl Default for RegistryView {
    fn default() -> Self {
        Self::new()
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
}
