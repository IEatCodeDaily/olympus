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
    /// Most recent activity timestamp known for this session. Seeded from
    /// `started_at` on creation, advanced on each `MessageAppended`.
    pub last_activity: f64,
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
                        last_activity: *started_at,
                    },
                );
            }
            Event::SessionUpdated {
                session_id,
                title,
                model,
                archived,
                message_count,
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
            Event::MessageRemoved { .. } => {}
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
            .filter(|row| match (&filters.source, filters.archived) {
                (Some(src), _) if row.source != *src => false,
                (_, Some(a)) if row.archived != a => false,
                _ => true,
            })
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
}
