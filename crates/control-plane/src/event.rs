//! The core event types stored in the append-only log.
//!
//! See `docs/plans/2026-06-28-olympus-mvp.md` Task 1.1 for the exact spec.

use serde::{Deserialize, Serialize};

/// Events (v1 — MVP-scoped) that the log stores.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Event {
    /// A session was imported or created.
    SessionCreated {
        session_id: String,
        /// Hermes's session ID.
        hermes_id: String,
        /// "cli"|"telegram"|"discord"|"webui"|"cron"|"subagent"|"api_server"
        source: String,
        model: Option<String>,
        title: Option<String>,
        started_at: f64,
        message_count: u64,
        input_tokens: u64,
        output_tokens: u64,
        /// Agent (Hermes profile) bound to this session, if any. Olympus-created
        /// sessions can be created without one and have it assigned later.
        #[serde(default)]
        agent: Option<String>,
        /// Node the session's runtime runs on ("local" for now).
        #[serde(default)]
        node: Option<String>,
    },
    /// A message was appended to a session.
    MessageAppended {
        /// Olympus session ID.
        session_id: String,
        /// Hermes session ID.
        hermes_session_id: String,
        /// Monotonic within session.
        message_id: u64,
        /// "user"|"assistant"|"tool"|"system"
        role: String,
        /// Stored zstd-compressed in the log (decompressed by the log layer).
        content: Option<String>,
        tool_name: Option<String>,
        tool_calls: Option<String>,
        reasoning: Option<String>,
        timestamp: f64,
        token_count: Option<u64>,
        finish_reason: Option<String>,
    },
    /// A message was removed or tombstoned in Hermes.
    MessageRemoved {
        session_id: String,
        hermes_session_id: String,
        message_id: u64,
    },
    /// A session's metadata was updated (title, archived, model, etc).
    SessionUpdated {
        session_id: String,
        title: Option<String>,
        model: Option<String>,
        archived: Option<bool>,
        message_count: Option<u64>,
        /// Agent (Hermes profile) bound to this session. `None` = leave unchanged.
        agent: Option<String>,
        /// Node the session's runtime runs on ("local" for now). `None` = unchanged.
        node: Option<String>,
        /// Backfill the real Hermes session id once a lazily-spawned runtime
        /// captures it from `session/new`. `None` = leave unchanged.
        hermes_id: Option<String>,
    },
    // ---- Card lifecycle events (C1, ADR §6) ----
    /// A card was created on a board.
    CardCreated {
        card_id: String,
        board_id: String,
        title: String,
        created_at: f64,
    },
    /// A card was assigned to an agent or human, starting a session attempt.
    CardAssigned {
        card_id: String,
        assigned_id: String,
        /// "agent" | "user"
        assigned_kind: String,
        session_id: String,
        attempt_bookmark: String,
        assigned_at: f64,
    },
    /// A card was claimed (the assigned agent accepted it and began work).
    CardClaimed { card_id: String, claimed_at: f64 },
    /// A card was blocked by one or more dependencies.
    CardBlocked {
        card_id: String,
        blocked_by: Vec<String>,
        blocked_at: f64,
    },
    /// A card reached the done state.
    CardCompleted { card_id: String, completed_at: f64 },
    /// A card was reassigned to a new agent/session (previous attempt forwarded
    /// as a "previous attempt" block per ADR §6.2).
    CardReassigned {
        card_id: String,
        assigned_id: String,
        assigned_kind: String,
        session_id: String,
        attempt_bookmark: String,
        previous_session_id: String,
        reassigned_at: f64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_created_postcard_roundtrips() {
        let e = Event::SessionCreated {
            session_id: "sess-1".into(),
            hermes_id: "h-1".into(),
            source: "cli".into(),
            model: Some("glm-5.2".into()),
            title: Some("hello".into()),
            started_at: 1_700_000_000.0,
            message_count: 0,
            input_tokens: 10,
            output_tokens: 20,
            agent: None,
            node: None,
        };
        let bytes = postcard::to_allocvec(&e).unwrap();
        let back: Event = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn message_appended_postcard_roundtrips_with_none_fields() {
        let e = Event::MessageAppended {
            session_id: "sess-1".into(),
            hermes_session_id: "h-1".into(),
            message_id: 5,
            role: "user".into(),
            content: Some("hi there".into()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: 1_700_000_001.0,
            token_count: Some(3),
            finish_reason: None,
        };
        let bytes = postcard::to_allocvec(&e).unwrap();
        let back: Event = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn session_updated_postcard_roundtrips() {
        let e = Event::SessionUpdated {
            session_id: "sess-1".into(),
            title: Some("renamed".into()),
            model: None,
            archived: Some(true),
            message_count: Some(42),
            agent: None,
            node: None,
            hermes_id: None,
        };
        let bytes = postcard::to_allocvec(&e).unwrap();
        let back: Event = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn message_removed_postcard_roundtrips() {
        let e = Event::MessageRemoved {
            session_id: "sess-1".into(),
            hermes_session_id: "h-1".into(),
            message_id: 5,
        };
        let bytes = postcard::to_allocvec(&e).unwrap();
        let back: Event = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(e, back);
    }

    // ---- Card event roundtrips (C1) ----

    #[test]
    fn card_created_roundtrips() {
        let e = Event::CardCreated {
            card_id: "card-1".into(),
            board_id: "board-1".into(),
            title: "Implement cards".into(),
            created_at: 1_700_000_000.0,
        };
        let bytes = postcard::to_allocvec(&e).unwrap();
        let back: Event = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn card_assigned_roundtrips() {
        let e = Event::CardAssigned {
            card_id: "card-1".into(),
            assigned_id: "agent-zephyr".into(),
            assigned_kind: "agent".into(),
            session_id: "sess-1".into(),
            attempt_bookmark: "attempt-1".into(),
            assigned_at: 1_700_000_001.0,
        };
        let bytes = postcard::to_allocvec(&e).unwrap();
        let back: Event = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn card_claimed_roundtrips() {
        let e = Event::CardClaimed {
            card_id: "card-1".into(),
            claimed_at: 1_700_000_002.0,
        };
        let bytes = postcard::to_allocvec(&e).unwrap();
        let back: Event = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn card_blocked_roundtrips() {
        let e = Event::CardBlocked {
            card_id: "card-1".into(),
            blocked_by: vec!["card-0".into(), "card-2".into()],
            blocked_at: 1_700_000_003.0,
        };
        let bytes = postcard::to_allocvec(&e).unwrap();
        let back: Event = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn card_completed_roundtrips() {
        let e = Event::CardCompleted {
            card_id: "card-1".into(),
            completed_at: 1_700_000_004.0,
        };
        let bytes = postcard::to_allocvec(&e).unwrap();
        let back: Event = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn card_reassigned_roundtrips() {
        let e = Event::CardReassigned {
            card_id: "card-1".into(),
            assigned_id: "agent-talos".into(),
            assigned_kind: "agent".into(),
            session_id: "sess-2".into(),
            attempt_bookmark: "attempt-2".into(),
            previous_session_id: "sess-1".into(),
            reassigned_at: 1_700_000_005.0,
        };
        let bytes = postcard::to_allocvec(&e).unwrap();
        let back: Event = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(e, back);
    }
}
