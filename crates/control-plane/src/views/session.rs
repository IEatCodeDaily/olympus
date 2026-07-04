//! Session-list projection — an in-memory materialized view of sessions.
//!
//! A deterministic projection of the event log (ADR 0002 §2.4). On restart it
//! is rebuilt by [`super::ViewManager::replay`]; live events are applied via
//! [`SessionView::apply`]. The log remains the sole source of truth.
//!
//! Ordering: [`SessionView::list`] returns rows ordered by `started_at`
//! descending (most-recent first), matching the session-list UI's recency
//! ordering.
//!
//! Tenancy note (ADR §3.5.3): the foundation `Event::SessionCreated` does not
//! yet carry `orgId`/`ownerId`/`contextId`. When those fields are added to the
//! event enum, they should be carried forward onto [`SessionRow`] here. Do NOT
//! add them to `SessionRow` ahead of the event change — projection fields
//! mirror the event surface.
//! // TODO(tenancy): carry orgId/ownerId/contextId from SessionCreated once the
//! // event enum has them (owned by another concern).

use std::collections::HashMap;

use crate::event::Event;

/// Filters applied to [`SessionView::list`].
///
/// All fields are optional; `None` means "do not filter on this dimension".
/// [`Filters::default`] matches every row.
#[derive(Debug, Clone, Default)]
pub struct Filters {
    /// Restrict to sessions with this source ("cli", "telegram", ...).
    pub source: Option<String>,
    /// Restrict to sessions with this archived flag.
    pub archived: Option<bool>,
    /// Restrict to sessions with this pinned flag.
    pub pinned: Option<bool>,
}

/// A row in the session-list projection.
///
/// Mirrors the fields of `Event::SessionCreated` plus an `archived` flag (from
/// `Event::SessionUpdated`) and a `last_activity` timestamp (advanced by
/// `MessageAppended`).
#[derive(Debug, Clone)]
pub struct SessionRow {
    pub session_id: String,
    pub hermes_id: String,
    pub source: String,
    pub model: Option<String>,
    pub title: Option<String>,
    pub started_at: f64,
    pub message_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub archived: bool,
    /// Manual pin flag (sidebar PINNED section). Never derived from liveness.
    pub pinned: bool,
    /// Most recent activity timestamp known for this session. Seeded from
    /// `started_at` on creation, advanced on each `MessageAppended`.
    pub last_activity: f64,
    /// Agent (Hermes profile) bound to this session, if assigned. Olympus
    /// sessions start unbound and have this set before the first send.
    pub agent: Option<String>,
    /// Node the session's runtime runs on ("local" for now).
    pub node: Option<String>,
    // ---- Session-tree fields (ADR 0006 §7 footgun 3) ----
    /// Parent session if this session was forked/branched. None for roots.
    pub parent_session_id: Option<String>,
    /// Card that owns this session tree, if linked. Inherited by forks.
    pub card_id: Option<String>,
    /// Project attached to this session, if any (from SessionProjectAttached).
    pub project_id: Option<String>,
}

/// In-memory projection of sessions from the event log (ADR §2.4).
///
/// Holds one [`SessionRow`] per known session. `list()` returns rows sorted by
/// `started_at` descending; ties break by session_id for deterministic output.
pub struct SessionView {
    sessions: HashMap<String, SessionRow>,
}

impl SessionView {
    /// Construct an empty view.
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Apply an event, mutating the projection.
    ///
    /// - `SessionCreated` inserts a new row (or replaces any existing row with
    ///   the same id — a replay must produce the final state, not stack rows).
    /// - `SessionUpdated` patches the named fields of an existing row; unknown
    ///   sessions are silently ignored (the update may arrive before creation
    ///   during partial replays, and must not create a phantom row).
    /// - `MessageAppended` advances `last_activity` and is a no-op for unknown
    ///   sessions (the message view owns message storage; the session view only
    ///   tracks the activity timestamp).
    pub fn apply(&mut self, event: &Event) {
        match event {
            Event::SessionCreated {
                session_id,
                hermes_id,
                source,
                model,
                title,
                started_at,
                message_count,
                input_tokens,
                output_tokens,
                agent,
                node,
            } => {
                // TODO(tenancy): carry orgId/ownerId/contextId from the event
                // once the Event enum has them. Project what exists for now.
                self.sessions.insert(
                    session_id.clone(),
                    SessionRow {
                        session_id: session_id.clone(),
                        hermes_id: hermes_id.clone(),
                        source: source.clone(),
                        model: model.clone(),
                        title: title.clone(),
                        started_at: *started_at,
                        message_count: *message_count,
                        input_tokens: *input_tokens,
                        output_tokens: *output_tokens,
                        archived: false,
                        pinned: false,
                        last_activity: *started_at,
                        agent: agent.clone(),
                        node: node.clone(),
                        parent_session_id: None,
                        card_id: None,
                        project_id: None,
                    },
                );
            }
            Event::SessionUpdated {
                session_id,
                title,
                model,
                archived,
                message_count,
                agent,
                node,
                hermes_id,
                pinned,
            } => {
                // Patch in place; ignore unknown sessions (no phantom rows).
                if let Some(row) = self.sessions.get_mut(session_id) {
                    if let Some(t) = title {
                        row.title = Some(t.clone());
                    }
                    if let Some(m) = model {
                        row.model = Some(m.clone());
                    }
                    if let Some(a) = archived {
                        row.archived = *a;
                    }
                    if let Some(c) = message_count {
                        row.message_count = *c;
                    }
                    if let Some(ag) = agent {
                        row.agent = Some(ag.clone());
                    }
                    if let Some(n) = node {
                        row.node = Some(n.clone());
                    }
                    if let Some(h) = hermes_id {
                        row.hermes_id = h.clone();
                    }
                    if let Some(p) = pinned {
                        row.pinned = *p;
                    }
                }
            }
            Event::MessageAppended {
                session_id,
                timestamp,
                ..
            } => {
                if let Some(row) = self.sessions.get_mut(session_id) {
                    if *timestamp > row.last_activity {
                        row.last_activity = *timestamp;
                    }
                }
            }
            Event::SessionProjectAttached {
                session_id,
                project_id,
                ..
            } => {
                if let Some(row) = self.sessions.get_mut(session_id) {
                    row.project_id = Some(project_id.clone());
                }
            }
            Event::MessageRemoved { .. } => {}
            // ---- Session-tree events (ADR 0006 §7 footgun 3) ----
            Event::SessionForked {
                parent_session_id,
                child_session_id,
                fork_point: _,
                ..
            } => {
                // The child session was already created (SessionCreated fires
                // before SessionForked). We stamp the parent link + inherit
                // the parent's card_id (a card owns the whole tree).
                if let Some(child) = self.sessions.get_mut(child_session_id) {
                    child.parent_session_id = Some(parent_session_id.clone());
                }
                // Inherit card_id from parent.
                let parent_card = self
                    .sessions
                    .get(parent_session_id)
                    .and_then(|p| p.card_id.clone());
                if let Some(card_id) = parent_card {
                    if let Some(child) = self.sessions.get_mut(child_session_id) {
                        child.card_id = Some(card_id);
                    }
                }
            }
            Event::CardSessionLinked {
                card_id,
                session_id,
                ..
            } => {
                // Link the card to the session (the tree root). Existing forks
                // of this session also get the card_id retroactively.
                if let Some(row) = self.sessions.get_mut(session_id) {
                    row.card_id = Some(card_id.clone());
                }
                // Propagate to existing children (forks that happened before
                // the link). Tree is parent→child; walk one level — deeper
                // propagation happens naturally on fork if the parent already
                // has the card_id.
                let children: Vec<String> = self
                    .sessions
                    .iter()
                    .filter_map(|(id, r)| {
                        if r.parent_session_id.as_deref() == Some(session_id.as_str()) {
                            Some(id.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                for child_id in children {
                    if let Some(child) = self.sessions.get_mut(&child_id) {
                        child.card_id = Some(card_id.clone());
                    }
                }
            }
            Event::SessionHandover {
                source_session_id,
                target_session_id,
                ..
            } => {
                // The target session already exists (SessionCreated fired).
                // Stamp the parent link (target is a child of source for tree
                // purposes) and inherit the card_id.
                if let Some(target) = self.sessions.get_mut(target_session_id) {
                    target.parent_session_id = Some(source_session_id.clone());
                }
                let source_card = self
                    .sessions
                    .get(source_session_id)
                    .and_then(|s| s.card_id.clone());
                if let Some(card_id) = source_card {
                    if let Some(target) = self.sessions.get_mut(target_session_id) {
                        target.card_id = Some(card_id);
                    }
                }
            }
            // Card events (and any other variant) do not affect the
            // session-list projection.
            _ => {}
        }
    }

    /// Return rows matching `filters`, ordered by `started_at` descending.
    ///
    /// Ties (identical `started_at`) break by `session_id` ascending so the
    /// output is deterministic.
    pub fn list(&self, filters: &Filters) -> Vec<&SessionRow> {
        let mut rows: Vec<&SessionRow> = self
            .sessions
            .values()
            .filter(
                |row| match (&filters.source, filters.archived, filters.pinned) {
                    (Some(src), _, _) if row.source != *src => false,
                    (_, Some(a), _) if row.archived != a => false,
                    (_, _, Some(p)) if row.pinned != p => false,
                    _ => true,
                },
            )
            .collect();
        // started_at desc; session_id asc as a stable tiebreaker.
        rows.sort_by(|a, b| {
            b.started_at
                .partial_cmp(&a.started_at)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.session_id.cmp(&b.session_id))
        });
        rows
    }

    /// Look up a single session by id.
    pub fn get(&self, session_id: &str) -> Option<&SessionRow> {
        self.sessions.get(session_id)
    }
}

impl Default for SessionView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created(id: &str, source: &str, started_at: f64) -> Event {
        Event::SessionCreated {
            session_id: id.into(),
            hermes_id: format!("hermes-{id}"),
            source: source.into(),
            model: Some("glm-5.2".into()),
            title: Some(format!("t-{id}")),
            started_at,
            message_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            agent: None,
            node: None,
        }
    }

    #[test]
    fn list_tiebreak_is_deterministic_on_equal_started_at() {
        let mut v = SessionView::new();
        v.apply(&created("b", "cli", 5.0));
        v.apply(&created("a", "cli", 5.0));
        v.apply(&created("c", "cli", 5.0));
        let ids: Vec<&str> = v
            .list(&Filters::default())
            .into_iter()
            .map(|r| r.session_id.as_str())
            .collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    // ---- SessionHandover ----

    #[test]
    fn handover_stamps_parent_link_on_target() {
        let mut v = SessionView::new();
        v.apply(&created("src", "cli", 1.0));
        v.apply(&created("tgt", "cli", 2.0));
        v.apply(&Event::SessionHandover {
            source_session_id: "src".into(),
            target_session_id: "tgt".into(),
            from_agent_kind: "Hermes".into(),
            to_agent_kind: "ClaudeCode".into(),
            translated_message_count: 5,
            handed_over_at: 3.0,
        });
        let tgt = v.get("tgt").unwrap();
        assert_eq!(tgt.parent_session_id.as_deref(), Some("src"));
    }

    #[test]
    fn handover_inherits_card_id_from_source() {
        let mut v = SessionView::new();
        v.apply(&created("src", "cli", 1.0));
        v.apply(&created("tgt", "cli", 2.0));
        // Link a card to the source before the handover.
        v.apply(&Event::CardSessionLinked {
            card_id: "card-1".into(),
            session_id: "src".into(),
            linked_at: 1.5,
        });
        v.apply(&Event::SessionHandover {
            source_session_id: "src".into(),
            target_session_id: "tgt".into(),
            from_agent_kind: "Hermes".into(),
            to_agent_kind: "ClaudeCode".into(),
            translated_message_count: 3,
            handed_over_at: 3.0,
        });
        let tgt = v.get("tgt").unwrap();
        assert_eq!(tgt.card_id.as_deref(), Some("card-1"));
    }

    #[test]
    fn handover_to_unknown_target_is_silent() {
        // Should not panic or create a phantom row.
        let mut v = SessionView::new();
        v.apply(&created("src", "cli", 1.0));
        v.apply(&Event::SessionHandover {
            source_session_id: "src".into(),
            target_session_id: "ghost".into(),
            from_agent_kind: "Hermes".into(),
            to_agent_kind: "ClaudeCode".into(),
            translated_message_count: 0,
            handed_over_at: 2.0,
        });
        assert!(v.get("ghost").is_none());
    }

    // ---- SessionForked ----

    #[test]
    fn fork_stamps_parent_link_on_child() {
        let mut v = SessionView::new();
        v.apply(&created("parent", "cli", 1.0));
        v.apply(&created("child", "cli", 2.0));
        v.apply(&Event::SessionForked {
            parent_session_id: "parent".into(),
            child_session_id: "child".into(),
            fork_type: "sub".into(),
            fork_point: Some(7),
            forked_at: 2.5,
        });
        let child = v.get("child").unwrap();
        assert_eq!(child.parent_session_id.as_deref(), Some("parent"));
    }

    #[test]
    fn fork_inherits_card_id_from_parent() {
        let mut v = SessionView::new();
        v.apply(&created("parent", "cli", 1.0));
        v.apply(&created("child", "cli", 2.0));
        v.apply(&Event::CardSessionLinked {
            card_id: "card-99".into(),
            session_id: "parent".into(),
            linked_at: 1.5,
        });
        v.apply(&Event::SessionForked {
            parent_session_id: "parent".into(),
            child_session_id: "child".into(),
            fork_type: "fork".into(),
            fork_point: None,
            forked_at: 3.0,
        });
        let child = v.get("child").unwrap();
        assert_eq!(child.card_id.as_deref(), Some("card-99"));
    }

    // ---- CardSessionLinked ----

    #[test]
    fn card_link_propagates_to_existing_forks() {
        let mut v = SessionView::new();
        v.apply(&created("root", "cli", 1.0));
        v.apply(&created("fork1", "cli", 2.0));
        // Fork happened before the card was linked.
        v.apply(&Event::SessionForked {
            parent_session_id: "root".into(),
            child_session_id: "fork1".into(),
            fork_type: "sub".into(),
            fork_point: None,
            forked_at: 2.5,
        });
        // Linking the card now — fork1 should receive it retroactively.
        v.apply(&Event::CardSessionLinked {
            card_id: "card-42".into(),
            session_id: "root".into(),
            linked_at: 5.0,
        });
        assert_eq!(v.get("root").unwrap().card_id.as_deref(), Some("card-42"));
        assert_eq!(v.get("fork1").unwrap().card_id.as_deref(), Some("card-42"));
    }

    // ---- SessionUpdated ----

    #[test]
    fn session_updated_patches_in_place() {
        let mut v = SessionView::new();
        v.apply(&created("s1", "cli", 1.0));
        v.apply(&Event::SessionUpdated {
            session_id: "s1".into(),
            title: Some("new title".into()),
            model: None,
            archived: Some(true),
            message_count: Some(10),
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        });
        let row = v.get("s1").unwrap();
        assert_eq!(row.title.as_deref(), Some("new title"));
        assert!(row.archived);
        assert_eq!(row.message_count, 10);
        // Fields not included in the update must remain unchanged.
        assert_eq!(row.model.as_deref(), Some("glm-5.2"));
    }

    #[test]
    fn session_updated_for_unknown_session_does_not_create_phantom() {
        let mut v = SessionView::new();
        v.apply(&Event::SessionUpdated {
            session_id: "ghost".into(),
            title: Some("oops".into()),
            model: None,
            archived: None,
            message_count: None,
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        });
        assert!(v.get("ghost").is_none());
    }

    // ---- Filters ----

    #[test]
    fn filters_by_source() {
        let mut v = SessionView::new();
        v.apply(&created("a", "cli", 1.0));
        v.apply(&created("b", "telegram", 2.0));
        let rows = v.list(&Filters {
            source: Some("cli".into()),
            archived: None,
            pinned: None,
        });
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_id, "a");
    }

    #[test]
    fn filters_by_archived() {
        let mut v = SessionView::new();
        v.apply(&created("live", "cli", 1.0));
        v.apply(&created("dead", "cli", 2.0));
        v.apply(&Event::SessionUpdated {
            session_id: "dead".into(),
            title: None,
            model: None,
            archived: Some(true),
            message_count: None,
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        });
        let active = v.list(&Filters {
            source: None,
            archived: Some(false),
            pinned: None,
        });
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].session_id, "live");
    }
}
