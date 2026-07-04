//! Repo registry projection — in-memory materialized view of registered repos.
//!
//! A deterministic projection of the event log. On restart rebuilt by
//! [`super::ViewManager::replay`]; live events applied via
//! [`RepoView::apply`]. The log remains the sole source of truth.
//!
//! Also projects `attached_repos: Vec<String>` onto sessions — when a
//! `SessionRepoAttached` event fires, the slug is appended to that session's
//! attached list in the session view.

use std::collections::HashMap;

use crate::event::Event;

/// A row in the repo-registry projection.
#[derive(Debug, Clone)]
pub struct RepoRow {
    pub slug: String,
    pub url: String,
    pub default_branch: String,
    pub registered_at: f64,
}

/// In-memory projection of the repo registry from the event log.
pub struct RepoView {
    repos: HashMap<String, RepoRow>,
    /// session_id → sorted vec of attached slugs (carried on SessionRow too).
    session_repos: HashMap<String, Vec<String>>,
}

impl RepoView {
    /// Construct an empty view.
    pub fn new() -> Self {
        Self {
            repos: HashMap::new(),
            session_repos: HashMap::new(),
        }
    }

    /// Apply an event, mutating the projection.
    pub fn apply(&mut self, event: &Event) {
        match event {
            Event::RepoRegistered {
                slug,
                url,
                default_branch,
                registered_at,
            } => {
                self.repos.insert(
                    slug.clone(),
                    RepoRow {
                        slug: slug.clone(),
                        url: url.clone(),
                        default_branch: default_branch.clone(),
                        registered_at: *registered_at,
                    },
                );
            }
            Event::RepoRemoved { slug, .. } => {
                self.repos.remove(slug);
            }
            Event::SessionRepoAttached {
                session_id, slug, ..
            } => {
                let entry = self.session_repos.entry(session_id.clone()).or_default();
                if !entry.contains(slug) {
                    entry.push(slug.clone());
                }
            }
            _ => {}
        }
    }

    /// List all registered repos.
    pub fn list(&self) -> Vec<&RepoRow> {
        self.repos.values().collect()
    }

    /// Look up a single repo by slug.
    pub fn get(&self, slug: &str) -> Option<&RepoRow> {
        self.repos.get(slug)
    }

    /// Return the attached repo slugs for a session.
    pub fn attached_slugs(&self, session_id: &str) -> &[String] {
        self.session_repos
            .get(session_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

impl Default for RepoView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_then_list_shows_repo() {
        let mut v = RepoView::new();
        v.apply(&Event::RepoRegistered {
            slug: "olympus".into(),
            url: "https://github.com/user/olympus".into(),
            default_branch: "main".into(),
            registered_at: 1.0,
        });
        assert_eq!(v.list().len(), 1);
        let r = v.get("olympus").unwrap();
        assert_eq!(r.url, "https://github.com/user/olympus");
        assert_eq!(r.default_branch, "main");
    }

    #[test]
    fn remove_deletes_repo() {
        let mut v = RepoView::new();
        v.apply(&Event::RepoRegistered {
            slug: "old".into(),
            url: "https://example.com/old".into(),
            default_branch: "main".into(),
            registered_at: 1.0,
        });
        assert!(v.get("old").is_some());
        v.apply(&Event::RepoRemoved {
            slug: "old".into(),
            removed_at: 2.0,
        });
        assert!(v.get("old").is_none());
        assert_eq!(v.list().len(), 0);
    }

    #[test]
    fn attach_adds_slug_to_session() {
        let mut v = RepoView::new();
        v.apply(&Event::SessionRepoAttached {
            session_id: "s1".into(),
            slug: "olympus".into(),
            attached_at: 1.0,
        });
        let slugs = v.attached_slugs("s1");
        assert_eq!(slugs, &["olympus"]);
    }

    #[test]
    fn attach_deduplicates() {
        let mut v = RepoView::new();
        v.apply(&Event::SessionRepoAttached {
            session_id: "s1".into(),
            slug: "olympus".into(),
            attached_at: 1.0,
        });
        v.apply(&Event::SessionRepoAttached {
            session_id: "s1".into(),
            slug: "olympus".into(),
            attached_at: 2.0,
        });
        assert_eq!(v.attached_slugs("s1"), &["olympus"]);
    }

    #[test]
    fn multiple_repos_multiple_sessions() {
        let mut v = RepoView::new();
        v.apply(&Event::RepoRegistered {
            slug: "a".into(),
            url: "url-a".into(),
            default_branch: "main".into(),
            registered_at: 1.0,
        });
        v.apply(&Event::RepoRegistered {
            slug: "b".into(),
            url: "url-b".into(),
            default_branch: "dev".into(),
            registered_at: 2.0,
        });
        v.apply(&Event::SessionRepoAttached {
            session_id: "s1".into(),
            slug: "a".into(),
            attached_at: 3.0,
        });
        v.apply(&Event::SessionRepoAttached {
            session_id: "s1".into(),
            slug: "b".into(),
            attached_at: 4.0,
        });
        v.apply(&Event::SessionRepoAttached {
            session_id: "s2".into(),
            slug: "a".into(),
            attached_at: 5.0,
        });

        assert_eq!(v.list().len(), 2);
        assert_eq!(v.attached_slugs("s1"), &["a", "b"]);
        assert_eq!(v.attached_slugs("s2"), &["a"]);
    }

    #[test]
    fn unknown_session_returns_empty_attached() {
        let v = RepoView::new();
        assert_eq!(v.attached_slugs("ghost").len(), 0);
    }
}
