//! Wire DTOs: camelCase JSON shapes the UI consumes (see `docs/api-contract.md`).
//!
//! The in-memory view rows (`SessionRow`, `MessageRow`) are internal,
//! snake_case, and not `Serialize`. These DTOs are the *contract* boundary: they
//! map view rows → the exact JSON the TypeScript client expects. Keeping the
//! mapping in one module means a contract change touches one file.

use serde::Serialize;

use crate::search::SearchHit as IndexHit;
use crate::views::{CardRow, MessageRow, SessionRow};

/// `Session` as the UI consumes it (api-contract.md §Session).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub id: String,
    pub hermes_id: String,
    pub org_id: String,
    pub owner_id: String,
    pub context_id: Option<String>,
    pub source: String,
    pub model: Option<String>,
    pub title: Option<String>,
    pub started_at: f64,
    pub last_activity: f64,
    pub message_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub archived: bool,
    pub forked_from: Option<String>,
    pub fork_point: Option<u64>,
    pub fork_type: Option<String>,
    /// true = Olympus-driven (steerable); false = observed/read-only.
    pub managed: bool,
    /// Agent (Hermes profile) bound to this session, if assigned.
    pub agent: Option<String>,
    /// Node the session's runtime runs on ("local" for now).
    pub node: Option<String>,
    /// Derived liveness: "active" (a turn is in-flight, or activity within the
    /// recency window) or "idle". Honest by construction — for observed sessions
    /// this reflects *recent activity*, NOT a confirmed live process (a crashed
    /// agent that never wrote `ended_at` looks idle, not dead). Set by the
    /// handler, which has `now` + the bridge in-flight set; `from_row` defaults
    /// it to "idle".
    pub liveness: String,
}

/// Recency window (seconds) within which a session with no in-flight turn is
/// still considered "active" because something wrote to it recently.
pub const ACTIVE_WINDOW_SECS: f64 = 90.0;

/// Derive liveness from the authoritative in-flight flag (a turn is actively
/// streaming) and activity recency. `in_flight` short-circuits to active; this
/// is the accurate signal for Olympus-managed sessions the bridge drives.
pub fn compute_liveness(last_activity: f64, now: f64, in_flight: bool) -> &'static str {
    if in_flight || (now - last_activity) <= ACTIVE_WINDOW_SECS {
        "active"
    } else {
        "idle"
    }
}

impl SessionDto {
    /// Build the wire DTO from an internal view row.
    ///
    /// MVP tenancy is single-org/single-owner; fork lineage is not yet tracked
    /// in `SessionRow`, so those fields are `None`. `managed` is false for
    /// imported/observed sessions — only Olympus-created/forked sessions are
    /// steerable (the POST mutation gate keys off this).
    pub fn from_row(row: &SessionRow) -> Self {
        Self {
            id: row.session_id.clone(),
            hermes_id: row.hermes_id.clone(),
            org_id: "personal".to_string(),
            owner_id: "rpw".to_string(),
            context_id: None,
            source: row.source.clone(),
            model: row.model.clone(),
            title: row.title.clone(),
            started_at: row.started_at,
            last_activity: row.last_activity,
            message_count: row.message_count,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            archived: row.archived,
            forked_from: None,
            fork_point: None,
            fork_type: None,
            managed: row.source == "acp" || row.source == "olympus",
            agent: row.agent.clone(),
            node: row.node.clone(),
            liveness: "idle".to_string(),
        }
    }
}

/// `Message` as the UI consumes it (api-contract.md §Message).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MessageDto {
    pub message_id: u64,
    pub session_id: String,
    pub role: String,
    pub content: Option<String>,
    pub tool_name: Option<String>,
    /// Tool calls are not yet projected into the message view; always null for now.
    pub tool_calls: Option<serde_json::Value>,
    pub reasoning: Option<String>,
    pub timestamp: f64,
    pub token_count: Option<u64>,
    pub finish_reason: Option<String>,
}

impl MessageDto {
    pub fn from_row(session_id: &str, row: &MessageRow) -> Self {
        Self {
            message_id: row.message_id,
            session_id: session_id.to_string(),
            role: row.role.clone(),
            content: row.content.clone(),
            tool_name: row.tool_name.clone(),
            tool_calls: None,
            reasoning: None,
            timestamp: row.timestamp,
            token_count: row.token_count,
            finish_reason: None,
        }
    }
}

/// `SearchHit` as the UI consumes it (api-contract.md §SearchHit).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHitDto {
    pub session_id: String,
    pub message_id: u64,
    pub source: String,
    pub snippet: String,
    pub score: f32,
    pub timestamp: f64,
}

impl SearchHitDto {
    /// Build from a tantivy hit, enriching `source` (from the session view) and
    /// `timestamp` (resolved by the handler) which the index does not store.
    pub fn from_index_hit(hit: &IndexHit, source: String, timestamp: f64) -> Self {
        Self {
            session_id: hit.session_id.clone(),
            message_id: hit.message_id,
            source,
            snippet: hit.snippet.clone(),
            score: hit.score,
            timestamp,
        }
    }
}

/// `Card` as the UI consumes it (api-contract.md §Card, ADR §6.3).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CardDto {
    pub id: String,
    pub board_id: String,
    pub title: String,
    pub status: String,
    pub assigned_id: Option<String>,
    pub assigned_kind: Option<String>,
    pub current_session_id: Option<String>,
    pub current_bookmark: Option<String>,
    pub blocked_by: Vec<String>,
    pub priority: i64,
    pub created_at: f64,
    pub status_changed_at: f64,
}

impl CardDto {
    pub fn from_row(row: &CardRow) -> Self {
        Self {
            id: row.card_id.clone(),
            board_id: row.board_id.clone(),
            title: row.title.clone(),
            status: row.status.clone(),
            assigned_id: row.assigned_id.clone(),
            assigned_kind: row.assigned_kind.clone(),
            current_session_id: row.current_session_id.clone(),
            current_bookmark: row.current_bookmark.clone(),
            blocked_by: row.blocked_by.clone(),
            priority: row.priority,
            created_at: row.created_at,
            status_changed_at: row.status_changed_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::SessionRow;

    #[test]
    fn liveness_in_flight_is_active_even_when_stale() {
        // A turn streaming right now is active regardless of last-activity age.
        assert_eq!(compute_liveness(0.0, 1_000_000.0, true), "active");
    }

    #[test]
    fn liveness_recent_activity_is_active() {
        let now = 1_000_000.0;
        assert_eq!(compute_liveness(now - 10.0, now, false), "active");
    }

    #[test]
    fn liveness_stale_no_inflight_is_idle() {
        let now = 1_000_000.0;
        // Older than the recency window and nothing in-flight → idle (honest:
        // could be walked-away or crashed; we don't claim "dead").
        assert_eq!(
            compute_liveness(now - (ACTIVE_WINDOW_SECS + 30.0), now, false),
            "idle"
        );
    }

    fn sample_row() -> SessionRow {
        SessionRow {
            session_id: "s1".into(),
            hermes_id: "h1".into(),
            source: "telegram".into(),
            model: Some("glm-5.2".into()),
            title: Some("hi".into()),
            started_at: 100.0,
            message_count: 3,
            input_tokens: 5,
            output_tokens: 7,
            archived: false,
            last_activity: 200.0,
            agent: None,
            node: None,
        }
    }

    #[test]
    fn session_dto_serializes_camelcase() {
        let dto = SessionDto::from_row(&sample_row());
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["hermesId"], "h1");
        assert_eq!(json["orgId"], "personal");
        assert_eq!(json["ownerId"], "rpw");
        assert_eq!(json["lastActivity"], 200.0);
        assert_eq!(json["messageCount"], 3);
        assert_eq!(json["forkedFrom"], serde_json::Value::Null);
        // imported telegram session is observed, not managed
        assert_eq!(json["managed"], false);
        // snake_case keys must NOT be present
        assert!(json.get("hermes_id").is_none());
        assert!(json.get("last_activity").is_none());
    }

    #[test]
    fn acp_session_is_managed() {
        let mut row = sample_row();
        row.source = "acp".into();
        let dto = SessionDto::from_row(&row);
        assert!(dto.managed);
    }

    #[test]
    fn olympus_session_is_managed() {
        let mut row = sample_row();
        row.source = "olympus".into();
        let dto = SessionDto::from_row(&row);
        assert!(dto.managed);
    }
}
