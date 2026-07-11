//! Project view — in-memory projection of project (context container) events.
//!
//! A Project binds vaults, repo slugs, and kanban board ids. Attaching a session
//! to a project stamps `session.project_id` (handled in `session.rs`).

use std::collections::HashMap;

use crate::event::Event;

/// One project row in the in-memory projection.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectRow {
    pub project_id: String,
    pub org_id: String,
    pub name: String,
    /// Vault ids bound to this project.
    pub vaults: Vec<String>,
    /// Repo slugs (reference only — repo logic lives in a parallel card).
    pub repos: Vec<String>,
    /// Board ids bound to this project.
    pub boards: Vec<String>,
    /// When the project was created (epoch seconds).
    pub created_at: f64,
    /// When the project was deleted, if tombstoned.
    pub deleted_at: Option<f64>,
}

/// In-memory projection of projects.
pub struct ProjectView {
    projects: HashMap<String, ProjectRow>,
}

impl ProjectView {
    pub fn new() -> Self {
        Self {
            projects: HashMap::new(),
        }
    }

    /// Apply a single event. Only project-related events are handled; all others
    /// are silently ignored.
    pub fn apply(&mut self, event: &Event) {
        match event {
            Event::ProjectCreated {
                project_id,
                name,
                created_at,
            } => {
                self.projects.insert(
                    project_id.clone(),
                    ProjectRow {
                        project_id: project_id.clone(),
                        org_id: "personal".into(),
                        name: name.clone(),
                        vaults: vec![],
                        repos: vec![],
                        boards: vec![],
                        created_at: *created_at,
                        deleted_at: None,
                    },
                );
            }
            Event::ProjectOrganizationAssigned {
                project_id,
                organization_id,
            } => {
                if let Some(row) = self.projects.get_mut(project_id) {
                    row.org_id = organization_id.clone();
                }
            }
            Event::ProjectUpdated {
                project_id,
                name,
                vaults,
                repos,
                boards,
            } => {
                if let Some(row) = self.projects.get_mut(project_id) {
                    if let Some(n) = name {
                        row.name = n.clone();
                    }
                    if let Some(v) = vaults {
                        row.vaults = v.clone();
                    }
                    if let Some(r) = repos {
                        row.repos = r.clone();
                    }
                    if let Some(b) = boards {
                        row.boards = b.clone();
                    }
                }
            }
            Event::ProjectDeleted {
                project_id,
                deleted_at,
            } => {
                if let Some(row) = self.projects.get_mut(project_id) {
                    row.deleted_at = Some(*deleted_at);
                }
            }
            _ => {}
        }
    }

    /// List all non-deleted projects, sorted by `created_at` descending.
    pub fn list(&self) -> Vec<&ProjectRow> {
        let mut rows: Vec<&ProjectRow> = self
            .projects
            .values()
            .filter(|r| r.deleted_at.is_none())
            .collect();
        rows.sort_by(|a, b| {
            b.created_at
                .partial_cmp(&a.created_at)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.project_id.cmp(&b.project_id))
        });
        rows
    }

    pub fn list_for_organization(&self, organization_id: &str) -> Vec<&ProjectRow> {
        self.list()
            .into_iter()
            .filter(|row| row.org_id == organization_id)
            .collect()
    }

    /// Get a single project by id (returns None if unknown or deleted).
    pub fn get(&self, project_id: &str) -> Option<&ProjectRow> {
        self.projects
            .get(project_id)
            .filter(|r| r.deleted_at.is_none())
    }

    /// Get a project regardless of deleted state (for admin/tombstone checks).
    pub fn get_any(&self, project_id: &str) -> Option<&ProjectRow> {
        self.projects.get(project_id)
    }
}

impl Default for ProjectView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created(id: &str, name: &str, ts: f64) -> Event {
        Event::ProjectCreated {
            project_id: id.into(),
            name: name.into(),
            created_at: ts,
        }
    }

    #[test]
    fn create_then_list_and_get() {
        let mut v = ProjectView::new();
        v.apply(&created("p1", "Alpha", 1.0));
        v.apply(&created("p2", "Beta", 2.0));

        let rows = v.list();
        assert_eq!(rows.len(), 2);
        // Most-recent first.
        assert_eq!(rows[0].project_id, "p2");
        assert_eq!(rows[1].project_id, "p1");

        let p = v.get("p1").unwrap();
        assert_eq!(p.name, "Alpha");
        assert!(p.vaults.is_empty());
    }

    #[test]
    fn update_patches_fields() {
        let mut v = ProjectView::new();
        v.apply(&created("p1", "Alpha", 1.0));
        v.apply(&Event::ProjectUpdated {
            project_id: "p1".into(),
            name: Some("Alpha-2".into()),
            vaults: Some(vec!["vault-a".into()]),
            repos: Some(vec!["my-repo".into()]),
            boards: Some(vec!["board-1".into()]),
        });

        let p = v.get("p1").unwrap();
        assert_eq!(p.name, "Alpha-2");
        assert_eq!(p.vaults, vec!["vault-a"]);
        assert_eq!(p.repos, vec!["my-repo"]);
        assert_eq!(p.boards, vec!["board-1"]);
    }

    #[test]
    fn update_null_fields_are_noop() {
        let mut v = ProjectView::new();
        v.apply(&created("p1", "Alpha", 1.0));
        v.apply(&Event::ProjectUpdated {
            project_id: "p1".into(),
            name: None,
            vaults: Some(vec!["vault-a".into()]),
            repos: None,
            boards: None,
        });

        let p = v.get("p1").unwrap();
        assert_eq!(p.name, "Alpha"); // unchanged
        assert_eq!(p.vaults, vec!["vault-a"]);
        assert!(p.repos.is_empty());
    }

    #[test]
    fn delete_hides_project_from_list_and_get() {
        let mut v = ProjectView::new();
        v.apply(&created("p1", "Alpha", 1.0));
        v.apply(&Event::ProjectDeleted {
            project_id: "p1".into(),
            deleted_at: 99.0,
        });

        assert_eq!(v.list().len(), 0);
        assert!(v.get("p1").is_none());
        // But get_any still surfaces it.
        assert!(v.get_any("p1").is_some());
    }

    #[test]
    fn update_unknown_project_is_noop() {
        let mut v = ProjectView::new();
        v.apply(&Event::ProjectUpdated {
            project_id: "ghost".into(),
            name: Some("X".into()),
            vaults: None,
            repos: None,
            boards: None,
        });
        assert_eq!(v.list().len(), 0);
    }

    #[test]
    fn get_unknown_project_is_none() {
        let v = ProjectView::new();
        assert!(v.get("nope").is_none());
    }

    #[test]
    fn organization_assignment_is_projected_and_filterable() {
        let mut view = ProjectView::new();
        view.apply(&created("a", "A", 1.0));
        view.apply(&created("b", "B", 2.0));
        view.apply(&Event::ProjectOrganizationAssigned {
            project_id: "a".into(),
            organization_id: "org-a".into(),
        });
        view.apply(&Event::ProjectOrganizationAssigned {
            project_id: "b".into(),
            organization_id: "org-b".into(),
        });

        assert_eq!(view.get("a").unwrap().org_id, "org-a");
        let rows = view.list_for_organization("org-b");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project_id, "b");
    }

    #[test]
    fn project_created_postcard_roundtrips() {
        use crate::log::Log;
        let f = tempfile::NamedTempFile::new().unwrap();
        let log = Log::open(f.path()).unwrap();
        log.append(&created("p1", "Test", 1.0)).unwrap();
        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].1,
            Event::ProjectCreated { project_id, name, .. }
            if project_id == "p1" && name == "Test"
        ));
    }

    #[test]
    fn project_updated_postcard_roundtrips() {
        use crate::log::Log;
        let f = tempfile::NamedTempFile::new().unwrap();
        let log = Log::open(f.path()).unwrap();
        let e = Event::ProjectUpdated {
            project_id: "p1".into(),
            name: Some("New".into()),
            vaults: Some(vec!["v1".into()]),
            repos: None,
            boards: Some(vec!["b1".into()]),
        };
        log.append(&e).unwrap();
        let ev = &log.read_all().unwrap()[0].1;
        assert!(matches!(ev, Event::ProjectUpdated { project_id, .. } if project_id == "p1"));
    }

    #[test]
    fn session_project_attached_postcard_roundtrips() {
        use crate::log::Log;
        let f = tempfile::NamedTempFile::new().unwrap();
        let log = Log::open(f.path()).unwrap();
        let e = Event::SessionProjectAttached {
            session_id: "sess-1".into(),
            project_id: "p1".into(),
            attached_at: 42.0,
        };
        log.append(&e).unwrap();
        let ev = &log.read_all().unwrap()[0].1;
        assert!(matches!(
            ev,
            Event::SessionProjectAttached { session_id, project_id, .. }
            if session_id == "sess-1" && project_id == "p1"
        ));
    }
}
