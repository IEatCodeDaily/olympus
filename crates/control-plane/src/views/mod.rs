//! In-memory materialized views — projections of the event log.
//!
//! Phase 2 (ADR 0002 §2.4): `SessionView` and `MessageView` are deterministic
//! projections of the append-only log. `ViewManager` replays the log on startup
//! to rebuild both, and applies events live thereafter. Views are bounded
//! (ADR §11): the message view holds a sliding recent window per session, not
//! the full history.
//!
//! Tests are written FIRST (RED). The `session` and `message` submodules are
//! stubbed only enough to make this file compile-testable; real behavior lands
//! in GREEN.

pub mod card;
pub mod message;
pub mod project;
pub mod registry;
pub mod repo;
pub mod session;
pub mod setup;

pub use card::{CardFilters, CardRow, CardView};
pub use message::{MessageRow, MessageView};
pub use project::{ProjectRow, ProjectView};
pub use registry::{DriftReport, RegistryEntry, RegistryView};
pub use repo::{RepoRow, RepoView};
pub use session::{Filters, SessionRow, SessionView};
pub use setup::{SetupRow, SetupView};

use anyhow::Result;

use crate::log::Log;

/// Replays the log on startup and fans events out to both views.
///
/// Owns the two projections. Call [`ViewManager::replay`] once on startup to
/// rebuild from the durable log, then [`ViewManager::apply`] for each live
/// event. Both views are pure projections: the log remains the sole source of
/// truth (ADR §2.4).
pub struct ViewManager {
    /// Session-list projection.
    pub sessions: SessionView,
    /// Per-session message cache (bounded sliding window).
    pub messages: MessageView,
    /// Card/board projection (C1).
    pub cards: CardView,
    /// Setup declaration projection (ADR 0006 §3 — the replicable manifest).
    pub setup: SetupView,
    /// Registry projection (ADR 0006 §9.4 — slug → definition resolver).
    pub registry: RegistryView,
    /// Project (context container) projection.
    pub projects: ProjectView,
    /// Repo registry projection — managed git repos.
    pub repos: RepoView,
}

impl ViewManager {
    /// Construct an empty manager (no sessions, no messages, no cards).
    pub fn new() -> Self {
        Self {
            sessions: SessionView::new(),
            messages: MessageView::new(),
            cards: CardView::new(),
            setup: SetupView::new(),
            registry: RegistryView::new(),
            projects: ProjectView::new(),
            repos: RepoView::new(),
        }
    }

    /// Rebuild all views by replaying every event in `log` in sequence order.
    ///
    /// Clears any existing in-memory state first, so this is idempotent and
    /// safe to call on restart.
    pub fn replay(&mut self, log: &Log) -> Result<()> {
        self.sessions = SessionView::new();
        self.messages = MessageView::new();
        self.cards = CardView::new();
        self.setup = SetupView::new();
        self.registry = RegistryView::new();
        self.projects = ProjectView::new();
        self.repos = RepoView::new();
        for (_seq, event) in log.read_all()? {
            self.apply(&event);
        }
        Ok(())
    }

    /// Apply a single live event to all views.
    ///
    /// Each view is responsible for ignoring variants it does not care about;
    /// this method never returns an error for an unrecognized event shape.
    pub fn apply(&mut self, event: &crate::event::Event) {
        self.sessions.apply(event);
        self.messages.apply(event);
        self.cards.apply(event);
        self.setup.apply(event);
        self.registry.apply(event);
        self.projects.apply(event);
        self.repos.apply(event);
    }
}

impl Default for ViewManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::log::Log;

    // ---- test helpers (shared event factories) ----

    fn session_created(id: &str, source: &str, started_at: f64) -> Event {
        Event::SessionCreated {
            session_id: id.into(),
            hermes_id: format!("hermes-{id}"),
            source: source.into(),
            model: Some("glm-5.2".into()),
            title: Some(format!("session {id}")),
            started_at,
            message_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            agent: None,
            node: None,
        }
    }

    fn message(session_id: &str, msg_id: u64, role: &str, content: &str, ts: f64) -> Event {
        Event::MessageAppended {
            session_id: session_id.into(),
            hermes_session_id: format!("hermes-{session_id}"),
            message_id: msg_id,
            role: role.into(),
            content: Some(content.into()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: ts,
            token_count: Some(msg_id * 10),
            finish_reason: None,
        }
    }

    fn fresh_log() -> (tempfile::NamedTempFile, Log) {
        let f = tempfile::NamedTempFile::new().unwrap();
        let log = Log::open(f.path()).unwrap();
        (f, log)
    }

    // ---- REQUIRED BEHAVIOR 1: replay rebuilds views from a log ----

    #[test]
    fn replay_rebuilds_views_from_log() {
        let (_f, log) = fresh_log();
        log.append(&session_created("sess-1", "cli", 1_700_000_000.0))
            .unwrap();
        log.append(&message("sess-1", 0, "user", "hello", 1_700_000_001.0))
            .unwrap();
        log.append(&message(
            "sess-1",
            1,
            "assistant",
            "hi back",
            1_700_000_002.0,
        ))
        .unwrap();

        let mut mgr = ViewManager::new();
        // before replay: empty
        assert_eq!(mgr.sessions.list(&Filters::default()).len(), 0);
        assert_eq!(mgr.messages.count("sess-1"), 0);

        mgr.replay(&log).unwrap();

        // after replay: one session, two messages
        assert_eq!(mgr.sessions.list(&Filters::default()).len(), 1);
        assert_eq!(mgr.messages.count("sess-1"), 2);
    }

    #[test]
    fn replay_is_idempotent() {
        // replaying the same log twice must not double-count.
        let (_f, log) = fresh_log();
        log.append(&session_created("sess-1", "cli", 1_700_000_000.0))
            .unwrap();
        log.append(&message("sess-1", 0, "user", "hi", 1_700_000_001.0))
            .unwrap();

        let mut mgr = ViewManager::new();
        mgr.replay(&log).unwrap();
        mgr.replay(&log).unwrap();

        assert_eq!(mgr.sessions.list(&Filters::default()).len(), 1);
        assert_eq!(mgr.messages.count("sess-1"), 1);
    }

    #[test]
    fn replay_empty_log_leaves_views_empty() {
        let (_f, log) = fresh_log();
        let mut mgr = ViewManager::new();
        mgr.replay(&log).unwrap();
        assert_eq!(mgr.sessions.list(&Filters::default()).len(), 0);
    }

    // ---- REQUIRED BEHAVIOR 2: apply(SessionCreated) adds a row ----

    #[test]
    fn apply_session_created_adds_row() {
        let mut mgr = ViewManager::new();
        mgr.apply(&session_created("sess-1", "cli", 1_700_000_000.0));

        let rows = mgr.sessions.list(&Filters::default());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_id, "sess-1");
        assert_eq!(rows[0].hermes_id, "hermes-sess-1");
        assert_eq!(rows[0].source, "cli");
        assert_eq!(rows[0].model.as_deref(), Some("glm-5.2"));
        assert_eq!(rows[0].title.as_deref(), Some("session sess-1"));
        assert_eq!(rows[0].started_at, 1_700_000_000.0);
        assert_eq!(rows[0].message_count, 0);
        assert!(!rows[0].archived);

        // get(id) returns the same row
        let got = mgr.sessions.get("sess-1").expect("sess-1 must be gettable");
        assert_eq!(got.session_id, "sess-1");
    }

    #[test]
    fn get_unknown_session_returns_none() {
        let mgr = ViewManager::new();
        assert!(mgr.sessions.get("nope").is_none());
    }

    // ---- REQUIRED BEHAVIOR 3: apply(MessageAppended) increments count + appears in recent ----

    #[test]
    fn apply_message_appended_increments_count_and_appears_in_recent() {
        let mut mgr = ViewManager::new();
        mgr.apply(&session_created("sess-1", "cli", 1_700_000_000.0));
        mgr.apply(&message("sess-1", 0, "user", "first", 1_700_000_001.0));
        mgr.apply(&message(
            "sess-1",
            1,
            "assistant",
            "second",
            1_700_000_002.0,
        ));

        assert_eq!(mgr.messages.count("sess-1"), 2);

        let recent = mgr.messages.recent("sess-1", 10);
        assert_eq!(recent.len(), 2);
        // ordered by message_id ascending (arrival order)
        assert_eq!(recent[0].message_id, 0);
        assert_eq!(recent[0].content.as_deref(), Some("first"));
        assert_eq!(recent[1].message_id, 1);
        assert_eq!(recent[1].content.as_deref(), Some("second"));
        let last = mgr.messages.recent("sess-1", 1);
        assert_eq!(last.len(), 1);
        assert_eq!(last[0].message_id, 1);
    }

    #[test]
    fn message_count_for_unknown_session_is_zero() {
        let mgr = ViewManager::new();
        assert_eq!(mgr.messages.count("ghost"), 0);
    }

    // ---- ADR 0020 v2 §4.5 — SessionRow.message_count must agree with the
    // durable message rows, for BOTH session kinds, and must not double-count. ----

    #[test]
    fn session_row_message_count_matches_messages_for_managed_and_synced() {
        let mut mgr = ViewManager::new();

        // Managed (olympus): no SessionUpdated{message_count} is ever emitted;
        // the count must come from MessageAppended increments alone.
        mgr.apply(&session_created("managed-1", "olympus", 1.0));
        mgr.apply(&message("managed-1", 0, "user", "hi", 2.0));
        mgr.apply(&message("managed-1", 1, "assistant", "hello", 3.0));

        // Synced (cli): the sync worker sets an ABSOLUTE count via
        // SessionUpdated; MessageAppended must NOT also increment (no double-count).
        mgr.apply(&session_created("synced-1", "cli", 1.0));
        mgr.apply(&message("synced-1", 0, "user", "a", 2.0));
        mgr.apply(&message("synced-1", 1, "assistant", "b", 3.0));
        mgr.apply(&Event::SessionUpdated {
            session_id: "synced-1".into(),
            title: None,
            model: None,
            archived: None,
            message_count: Some(2),
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        });

        let managed = mgr.sessions.get("managed-1").expect("managed row");
        let synced = mgr.sessions.get("synced-1").expect("synced row");

        // SessionRow.message_count (what SessionDto reads) agrees with the
        // MessageView count — the divergence fixed by ADR 0020 v2.
        assert_eq!(managed.message_count, 2, "managed row counts appends");
        assert_eq!(mgr.messages.count("managed-1"), 2);
        assert_eq!(synced.message_count, 2, "synced row uses absolute, no double");
        assert_eq!(mgr.messages.count("synced-1"), 2);
    }

    #[test]
    fn message_count_survives_replay_for_managed_sessions() {
        let (_f, log) = fresh_log();
        log.append(&session_created("m", "olympus", 1.0)).unwrap();
        log.append(&message("m", 0, "user", "x", 2.0)).unwrap();
        log.append(&message("m", 1, "assistant", "y", 3.0)).unwrap();

        let mut mgr = ViewManager::new();
        mgr.replay(&log).unwrap();
        let row = mgr.sessions.get("m").expect("row after replay");
        assert_eq!(row.message_count, 2, "clean replay reproduces the count");
        assert_eq!(mgr.messages.count("m"), 2);
    }

    #[test]
    fn recent_for_unknown_session_is_empty() {
        let mgr = ViewManager::new();
        assert!(mgr.messages.recent("ghost", 10).is_empty());
    }

    #[test]
    fn message_count_tracks_across_sessions_independently() {
        let mut mgr = ViewManager::new();
        mgr.apply(&session_created("a", "cli", 1.0));
        mgr.apply(&session_created("b", "cli", 2.0));
        mgr.apply(&message("a", 0, "user", "x", 3.0));
        mgr.apply(&message("a", 1, "user", "y", 4.0));
        mgr.apply(&message("b", 0, "user", "z", 5.0));

        assert_eq!(mgr.messages.count("a"), 2);
        assert_eq!(mgr.messages.count("b"), 1);
    }

    // ---- REQUIRED BEHAVIOR 4: apply(SessionUpdated) patches title/archived ----

    #[test]
    fn apply_session_updated_patches_title() {
        let mut mgr = ViewManager::new();
        mgr.apply(&session_created("sess-1", "cli", 1_700_000_000.0));

        mgr.apply(&Event::SessionUpdated {
            session_id: "sess-1".into(),
            title: Some("renamed".into()),
            model: None,
            archived: None,
            message_count: None,
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        });

        let row = mgr.sessions.get("sess-1").unwrap();
        assert_eq!(row.title.as_deref(), Some("renamed"));
        // other fields unchanged
        assert_eq!(row.source, "cli");
        assert!(!row.archived);
    }

    #[test]
    fn apply_session_updated_patches_archived() {
        let mut mgr = ViewManager::new();
        mgr.apply(&session_created("sess-1", "cli", 1_700_000_000.0));

        mgr.apply(&Event::SessionUpdated {
            session_id: "sess-1".into(),
            title: None,
            model: None,
            archived: Some(true),
            message_count: None,
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        });

        let row = mgr.sessions.get("sess-1").unwrap();
        assert!(row.archived);
    }

    #[test]
    fn apply_session_updated_patches_message_count() {
        let mut mgr = ViewManager::new();
        mgr.apply(&session_created("sess-1", "cli", 1_700_000_000.0));

        mgr.apply(&Event::SessionUpdated {
            session_id: "sess-1".into(),
            title: None,
            model: None,
            archived: None,
            message_count: Some(42),
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        });

        let row = mgr.sessions.get("sess-1").unwrap();
        assert_eq!(row.message_count, 42);
    }

    #[test]
    fn apply_session_updated_patches_model() {
        let mut mgr = ViewManager::new();
        mgr.apply(&session_created("sess-1", "cli", 1_700_000_000.0));

        mgr.apply(&Event::SessionUpdated {
            session_id: "sess-1".into(),
            title: None,
            model: Some("claude-sonnet-4".into()),
            archived: None,
            message_count: None,
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        });

        let row = mgr.sessions.get("sess-1").unwrap();
        assert_eq!(row.model.as_deref(), Some("claude-sonnet-4"));
    }

    #[test]
    fn apply_session_updated_for_unknown_session_is_noop() {
        // must not panic, must not create a row
        let mut mgr = ViewManager::new();
        mgr.apply(&Event::SessionUpdated {
            session_id: "ghost".into(),
            title: Some("x".into()),
            model: None,
            archived: None,
            message_count: None,
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        });
        assert!(mgr.sessions.get("ghost").is_none());
        assert_eq!(mgr.sessions.list(&Filters::default()).len(), 0);
    }

    // ---- REQUIRED BEHAVIOR 5: list() filters by source ----

    #[test]
    fn list_filters_by_source() {
        let mut mgr = ViewManager::new();
        mgr.apply(&session_created("a", "cli", 1.0));
        mgr.apply(&session_created("b", "telegram", 2.0));
        mgr.apply(&session_created("c", "cli", 3.0));
        mgr.apply(&session_created("d", "discord", 4.0));

        let all = mgr.sessions.list(&Filters::default());
        assert_eq!(all.len(), 4);

        let cli_only = mgr.sessions.list(&Filters {
            source: Some("cli".into()),
            archived: None,
            pinned: None,
        });
        assert_eq!(cli_only.len(), 2);
        let cli_ids: Vec<&str> = cli_only.iter().map(|r| r.session_id.as_str()).collect();
        assert!(cli_ids.contains(&"a"));
        assert!(cli_ids.contains(&"c"));
    }

    #[test]
    fn list_filters_by_archived() {
        let mut mgr = ViewManager::new();
        mgr.apply(&session_created("a", "cli", 1.0));
        mgr.apply(&session_created("b", "cli", 2.0));
        // archive b
        mgr.apply(&Event::SessionUpdated {
            session_id: "b".into(),
            title: None,
            model: None,
            archived: Some(true),
            message_count: None,
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        });

        let active = mgr.sessions.list(&Filters {
            source: None,
            archived: Some(false),
            pinned: None,
        });
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].session_id, "a");

        let archived = mgr.sessions.list(&Filters {
            source: None,
            archived: Some(true),
            pinned: None,
        });
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].session_id, "b");
    }

    #[test]
    fn list_combines_source_and_archived_filters() {
        let mut mgr = ViewManager::new();
        mgr.apply(&session_created("a", "cli", 1.0));
        mgr.apply(&session_created("b", "cli", 2.0));
        mgr.apply(&session_created("c", "telegram", 3.0));
        mgr.apply(&Event::SessionUpdated {
            session_id: "a".into(),
            title: None,
            model: None,
            archived: Some(true),
            message_count: None,
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        });

        let result = mgr.sessions.list(&Filters {
            source: Some("cli".into()),
            archived: Some(false),
            pinned: None,
        });
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].session_id, "b");
    }

    #[test]
    fn list_orders_by_started_at_desc() {
        // most-recent first (ADR §2.4 — session list is recency-ordered)
        let mut mgr = ViewManager::new();
        mgr.apply(&session_created("old", "cli", 1.0));
        mgr.apply(&session_created("newest", "cli", 3.0));
        mgr.apply(&session_created("mid", "cli", 2.0));

        let rows = mgr.sessions.list(&Filters::default());
        let ids: Vec<&str> = rows.iter().map(|r| r.session_id.as_str()).collect();
        assert_eq!(ids, vec!["newest", "mid", "old"]);
    }

    // ---- sliding window bound (ADR §11) ----

    #[test]
    fn message_view_evicts_oldest_beyond_window() {
        // The view holds a bounded recent window; older messages fall out of
        // the hot cache but the COUNT stays accurate (count is durable state
        // derived from the event stream, not the window length).
        let mut mgr = ViewManager::new();
        mgr.apply(&session_created("sess-1", "cli", 1.0));

        // Push more than the default window (50). Use distinct timestamps so
        // ordering is deterministic.
        for i in 0..60u64 {
            mgr.apply(&message(
                "sess-1",
                i,
                "user",
                &format!("msg-{i}"),
                2.0 + i as f64,
            ));
        }

        // count reflects ALL appended messages
        assert_eq!(mgr.messages.count("sess-1"), 60);

        // recent() returns at most the window size, and they are the NEWEST
        let recent = mgr.messages.recent("sess-1", 100);
        assert!(
            recent.len() <= 50,
            "recent must not exceed the bounded window; got {}",
            recent.len()
        );
        // newest first when asked for all (limit >= window): last appended is present
        let last = recent.last().expect("non-empty window");
        assert_eq!(last.message_id, 59);
        // oldest was evicted from the hot window
        let ids: Vec<u64> = recent.iter().map(|m| m.message_id).collect();
        assert!(
            !ids.contains(&0),
            "message_id 0 must be evicted from window"
        );
    }

    // ---- end-to-end through the log + replay (integration) ----

    #[test]
    fn replay_then_live_apply_are_consistent() {
        // A session created + 2 messages via the log, replayed, then a 3rd
        // message applied live: count and recent must be consistent.
        let (_f, log) = fresh_log();
        log.append(&session_created("sess-1", "cli", 1.0)).unwrap();
        log.append(&message("sess-1", 0, "user", "a", 2.0)).unwrap();
        log.append(&message("sess-1", 1, "user", "b", 3.0)).unwrap();

        let mut mgr = ViewManager::new();
        mgr.replay(&log).unwrap();
        assert_eq!(mgr.messages.count("sess-1"), 2);

        // live event (not in the log)
        mgr.apply(&message("sess-1", 2, "user", "c", 4.0));
        assert_eq!(mgr.messages.count("sess-1"), 3);
        let recent = mgr.messages.recent("sess-1", 10);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[2].content.as_deref(), Some("c"));
    }
}
