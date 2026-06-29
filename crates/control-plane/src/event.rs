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
}
