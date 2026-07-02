//! Setup declaration projection — the replicable agent-setup manifest per scope.
//!
//! A deterministic projection of the event log (ADR 0006 §3). On restart it is
//! rebuilt by [`super::ViewManager::replay`]; live events are applied via
//! [`SetupView::apply`]. The log remains the sole source of truth.
//!
//! A scope (`"org:<org>"` or `"project:<org>/<project>"`) declares the skills,
//! MCP servers, plugins, and hooks the envoy must materialize into every
//! session under it. `SetupDeclared` has PUT (full-replace) semantics — the
//! latest declaration for a scope wins.

use std::collections::HashMap;

use crate::event::Event;

/// A row in the setup projection — one scope's declared agent setup.
#[derive(Debug, Clone, PartialEq)]
pub struct SetupRow {
    /// `"org:<org>"` | `"project:<org>/<project>"`.
    pub scope: String,
    pub skills: Vec<String>,
    pub mcp: Vec<String>,
    pub plugins: Vec<String>,
    pub hooks: Vec<String>,
    pub declared_at: f64,
}

/// In-memory projection of setup declarations from the event log (ADR 0006 §3).
pub struct SetupView {
    by_scope: HashMap<String, SetupRow>,
}

impl SetupView {
    pub fn new() -> Self {
        Self {
            by_scope: HashMap::new(),
        }
    }

    /// Apply an event. Only `SetupDeclared` mutates this view (full replace of
    /// the scope's declaration); all other events are ignored.
    pub fn apply(&mut self, event: &Event) {
        if let Event::SetupDeclared {
            scope,
            skills,
            mcp,
            plugins,
            hooks,
            declared_at,
        } = event
        {
            self.by_scope.insert(
                scope.clone(),
                SetupRow {
                    scope: scope.clone(),
                    skills: skills.clone(),
                    mcp: mcp.clone(),
                    plugins: plugins.clone(),
                    hooks: hooks.clone(),
                    declared_at: *declared_at,
                },
            );
        }
    }

    /// The declared setup for a scope, if any.
    pub fn get(&self, scope: &str) -> Option<&SetupRow> {
        self.by_scope.get(scope)
    }

    /// The effective (merged) setup for a project: the union of its org-level
    /// declaration and its project-level declaration. Org setup is the baseline
    /// (applies to every session in the org); the project layers on top. Each
    /// list is deduped, org-first then project-only additions, preserving order.
    ///
    /// `org` and `project` are slugs; the scopes queried are `"org:<org>"` and
    /// `"project:<org>/<project>"`.
    pub fn effective_for_project(&self, org: &str, project: &str) -> SetupRow {
        let org_scope = format!("org:{org}");
        let proj_scope = format!("project:{org}/{project}");
        let org_row = self.by_scope.get(&org_scope);
        let proj_row = self.by_scope.get(&proj_scope);

        let merge = |pick: fn(&SetupRow) -> &Vec<String>| -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            for row in [org_row, proj_row].into_iter().flatten() {
                for item in pick(row) {
                    if !out.contains(item) {
                        out.push(item.clone());
                    }
                }
            }
            out
        };

        SetupRow {
            scope: proj_scope,
            skills: merge(|r| &r.skills),
            mcp: merge(|r| &r.mcp),
            plugins: merge(|r| &r.plugins),
            hooks: merge(|r| &r.hooks),
            // The more recent of the two declarations.
            declared_at: org_row
                .map(|r| r.declared_at)
                .unwrap_or(0.0)
                .max(proj_row.map(|r| r.declared_at).unwrap_or(0.0)),
        }
    }

    /// All declared scopes (for listing / debugging).
    pub fn list(&self) -> Vec<&SetupRow> {
        let mut rows: Vec<&SetupRow> = self.by_scope.values().collect();
        rows.sort_by(|a, b| a.scope.cmp(&b.scope));
        rows
    }
}

impl Default for SetupView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(scope: &str, skills: &[&str], plugins: &[&str], at: f64) -> Event {
        Event::SetupDeclared {
            scope: scope.into(),
            skills: skills.iter().map(|s| s.to_string()).collect(),
            mcp: vec![],
            plugins: plugins.iter().map(|s| s.to_string()).collect(),
            hooks: vec![],
            declared_at: at,
        }
    }

    #[test]
    fn apply_setup_declared_stores_row() {
        let mut v = SetupView::new();
        v.apply(&declared("org:acme", &["code-review"], &["gitnexus"], 1.0));
        let row = v.get("org:acme").expect("row exists");
        assert_eq!(row.skills, vec!["code-review"]);
        assert_eq!(row.plugins, vec!["gitnexus"]);
    }

    #[test]
    fn setup_declared_is_full_replace() {
        // PUT semantics: the latest declaration wins entirely, not a merge.
        let mut v = SetupView::new();
        v.apply(&declared("org:acme", &["a", "b"], &["p1"], 1.0));
        v.apply(&declared("org:acme", &["c"], &[], 2.0));
        let row = v.get("org:acme").unwrap();
        assert_eq!(row.skills, vec!["c"]);
        assert!(row.plugins.is_empty());
        assert_eq!(row.declared_at, 2.0);
    }

    #[test]
    fn effective_merges_org_and_project_deduped() {
        let mut v = SetupView::new();
        v.apply(&declared("org:acme", &["code-review"], &["gitnexus"], 1.0));
        v.apply(&declared(
            "project:acme/web",
            &["code-review", "react-doctor"],
            &["lsp-typescript"],
            2.0,
        ));
        let eff = v.effective_for_project("acme", "web");
        // org's code-review + project's react-doctor (code-review deduped)
        assert_eq!(eff.skills, vec!["code-review", "react-doctor"]);
        // org's gitnexus + project's lsp-typescript
        assert_eq!(eff.plugins, vec!["gitnexus", "lsp-typescript"]);
        assert_eq!(eff.declared_at, 2.0);
    }

    #[test]
    fn effective_with_only_org_declaration() {
        let mut v = SetupView::new();
        v.apply(&declared("org:acme", &["code-review"], &["gitnexus"], 1.0));
        let eff = v.effective_for_project("acme", "web");
        assert_eq!(eff.skills, vec!["code-review"]);
        assert_eq!(eff.plugins, vec!["gitnexus"]);
    }

    #[test]
    fn effective_with_no_declarations_is_empty() {
        let v = SetupView::new();
        let eff = v.effective_for_project("acme", "web");
        assert!(eff.skills.is_empty());
        assert!(eff.plugins.is_empty());
        assert_eq!(eff.declared_at, 0.0);
    }

    #[test]
    fn get_unknown_scope_is_none() {
        let v = SetupView::new();
        assert!(v.get("org:nope").is_none());
    }

    #[test]
    fn non_setup_events_are_ignored() {
        let mut v = SetupView::new();
        v.apply(&Event::CardCompleted {
            card_id: "c1".into(),
            completed_at: 1.0,
        });
        assert!(v.list().is_empty());
    }
}
