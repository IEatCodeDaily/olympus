//! Wire DTOs: camelCase JSON shapes the UI consumes (see `docs/api-contract.md`).
//!
//! The in-memory view rows (`SessionRow`, `MessageRow`) are internal,
//! snake_case, and not `Serialize`. These DTOs are the *contract* boundary: they
//! map view rows → the exact JSON the TypeScript client expects. Keeping the
//! mapping in one module means a contract change touches one file.

use serde::Serialize;

use crate::search::SearchHit as IndexHit;
use crate::vault::{NoteDocument, NoteTreeEntry, NoteTreeEntryKind, VaultSummary};
use crate::views::{CardRow, MessageRow, ProjectRow, RegistryEntry, RepoRow, SessionRow, SetupRow};

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
    /// Manual pin flag (sidebar PINNED section) — user-set, never derived.
    pub pinned: bool,
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
    /// Parent session if this was forked/branched (ADR 0006 §7 footgun 3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    /// Card that owns this session tree, if linked (ADR 0006 §7 footgun 3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card_id: Option<String>,
}

/// Recency window (seconds) within which an OBSERVED session (one Olympus does
/// not drive) is still considered "active" because something wrote to it
/// recently. Managed sessions ignore this — they use the authoritative
/// in-flight flag instead (see `compute_liveness`).
pub const ACTIVE_WINDOW_SECS: f64 = 90.0;

/// Derive a session's live state, honest by construction:
///
/// - **Managed** sessions (Olympus drives the turn): `awaiting` (blocked on a
///   permission decision) → "input-required"; else `in_flight` → "running";
///   else "idle". No recency window — a completed turn is idle the instant it
///   finishes.
/// - **Observed** sessions: recency-based "active"/"idle" (no in-flight signal).
///
/// Returns "input-required" | "running" | "active" | "idle".
pub fn compute_liveness(
    last_activity: f64,
    now: f64,
    in_flight: bool,
    managed: bool,
    awaiting: bool,
) -> &'static str {
    if managed {
        if awaiting {
            "input-required"
        } else if in_flight {
            "running"
        } else {
            "idle"
        }
    } else if (now - last_activity) <= ACTIVE_WINDOW_SECS {
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
            pinned: row.pinned,
            forked_from: None,
            fork_point: None,
            fork_type: None,
            managed: row.source == "acp" || row.source == "olympus",
            agent: row.agent.clone(),
            node: row.node.clone(),
            liveness: "idle".into(),
            parent_session_id: row.parent_session_id.clone(),
            card_id: row.card_id.clone(),
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

/// One tool invocation as the UI consumes it. Borrows the Vercel AI SDK tool-
/// "part" shape (de-facto standard) so the UI can render args/result uniformly
/// and detect edit/patch-shaped results for a diff view.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallDto {
    /// Tool invocation id (from the model; may be absent for some providers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Tool/function name, e.g. "terminal", "patch", "web_search".
    pub name: String,
    /// Parsed arguments object (already JSON), kept opaque for the UI to render.
    pub args: serde_json::Value,
    /// Display label when the provider gives one (title), else the name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Parse the OpenAI-style `tool_calls` JSON string stored on an assistant
/// message into the UI array. Accepts both the function-call envelope
/// (`[{"id":..,"function":{"name":..,"arguments":..}}]`) and a bare array of
/// `{name, args/arguments}` objects. Returns None for empty/unparseable input.
pub fn parse_tool_calls(raw: &str) -> Option<serde_json::Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    let items = match &v {
        serde_json::Value::Array(a) => a,
        // A single object — wrap it.
        serde_json::Value::Object(_) => {
            return Some(serde_json::Value::Array(vec![normalize_tool_call(&v)]));
        }
        _ => return None,
    };
    let out: Vec<serde_json::Value> = items.iter().map(normalize_tool_call).collect();
    if out.is_empty() {
        None
    } else {
        Some(serde_json::Value::Array(out))
    }
}

/// Normalize one raw tool-call item to the UI shape `{id,name,args,label}`.
fn normalize_tool_call(item: &serde_json::Value) -> serde_json::Value {
    // function-call envelope: {"function":{"name":..,"arguments":..}}
    let func = item.get("function");
    let name = func
        .and_then(|f| f.get("name"))
        .or_else(|| item.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("(unknown tool)")
        .to_string();
    // arguments may be a JSON string (OpenAI) or an object (Hermes/Anthropic).
    let args_raw = func
        .and_then(|f| f.get("arguments"))
        .or_else(|| item.get("args"))
        .or_else(|| item.get("arguments"));
    let args = match args_raw {
        Some(serde_json::Value::String(s)) => {
            serde_json::from_str(s).unwrap_or(serde_json::Value::String(s.clone()))
        }
        Some(v) => v.clone(),
        None => serde_json::Value::Object(serde_json::Map::new()),
    };
    serde_json::json!({
        "id": item.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()),
        "name": name,
        "args": args,
        "label": item.get("title").and_then(|t| t.as_str()).map(|s| s.to_string()),
    })
}

impl MessageDto {
    pub fn from_row(session_id: &str, row: &MessageRow) -> Self {
        // Parse the stored tool_calls JSON string into a structured array; fall
        // back to the raw string as a single-element array if it isn't valid
        // JSON so the UI never sees malformed/empty data silently.
        let tool_calls = row.tool_calls.as_deref().and_then(parse_tool_calls);
        Self {
            message_id: row.message_id,
            session_id: session_id.to_string(),
            role: row.role.clone(),
            content: row.content.clone(),
            tool_name: row.tool_name.clone(),
            tool_calls,
            reasoning: row.reasoning.clone(),
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

/// `Setup` as the UI consumes it — a scope's declared agent setup (ADR 0006 §3).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SetupDto {
    pub scope: String,
    pub skills: Vec<String>,
    pub mcp: Vec<String>,
    pub plugins: Vec<String>,
    pub hooks: Vec<String>,
    pub declared_at: f64,
}

impl SetupDto {
    pub fn from_row(row: &SetupRow) -> Self {
        Self {
            scope: row.scope.clone(),
            skills: row.skills.clone(),
            mcp: row.mcp.clone(),
            plugins: row.plugins.clone(),
            hooks: row.hooks.clone(),
            declared_at: row.declared_at,
        }
    }
}

/// `RegistryEntry` as the UI consumes it (ADR 0006 §9.4).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RegistryEntryDto {
    pub kind: String,
    pub slug: String,
    /// Harness-agnostic definition (JSON string — the UI parses or displays it).
    pub definition: String,
    pub registered_at: f64,
}

impl RegistryEntryDto {
    pub fn from_entry(row: &RegistryEntry) -> Self {
        Self {
            kind: row.kind.clone(),
            slug: row.slug.clone(),
            definition: row.definition.clone(),
            registered_at: row.registered_at,
        }
    }
}

/// `Vault` summary as the UI consumes it (api-contract.md §Vaults).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VaultSummaryDto {
    pub id: String,
    pub name: String,
    pub note_count: usize,
    pub updated_at: f64,
}

impl From<VaultSummary> for VaultSummaryDto {
    fn from(vault: VaultSummary) -> Self {
        Self {
            id: vault.id,
            name: vault.name,
            note_count: vault.note_count,
            updated_at: vault.updated_at,
        }
    }
}

/// Recursive note tree entry. Folders have `kind="folder"`; files have `kind="note"`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NoteTreeEntryDto {
    pub path: String,
    pub title: String,
    pub updated_at: f64,
    pub kind: String,
    pub children: Vec<NoteTreeEntryDto>,
}

impl From<NoteTreeEntry> for NoteTreeEntryDto {
    fn from(entry: NoteTreeEntry) -> Self {
        Self {
            path: entry.path,
            title: entry.title,
            updated_at: entry.updated_at,
            kind: match entry.kind {
                NoteTreeEntryKind::Folder => "folder".to_string(),
                NoteTreeEntryKind::Note => "note".to_string(),
            },
            children: entry.children.into_iter().map(Into::into).collect(),
        }
    }
}

/// Full note document.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NoteDocumentDto {
    pub path: String,
    pub title: String,
    pub markdown: String,
    pub frontmatter: serde_json::Value,
    pub linked_notes: Vec<String>,
    pub cid: Option<String>,
}

impl From<NoteDocument> for NoteDocumentDto {
    fn from(note: NoteDocument) -> Self {
        Self {
            path: note.path,
            title: note.title,
            markdown: note.markdown,
            frontmatter: note.frontmatter,
            linked_notes: note.linked_notes,
            cid: note.cid,
        }
    }
}

/// Vault graph node.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeDto {
    pub id: String,
    pub title: String,
    pub path: String,
    pub cid: Option<String>,
    pub link_count: usize,
}

/// Vault graph edge.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdgeDto {
    pub source: String,
    pub target: String,
}

/// Vault graph response.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VaultGraphDto {
    pub nodes: Vec<GraphNodeDto>,
    pub edges: Vec<GraphEdgeDto>,
}

/// Collection summary.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSummaryDto {
    pub name: String,
    pub path: String,
    pub row_count: usize,
}

/// Collection data (rows + columns).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CollectionDataDto {
    pub columns: Vec<String>,
    pub rows: Vec<serde_json::Value>,
}

/// `Project` as the UI consumes it — camelCase wire DTO.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub vaults: Vec<String>,
    pub repos: Vec<String>,
    pub boards: Vec<String>,
    pub created_at: f64,
}

impl ProjectDto {
    pub fn from_row(row: &ProjectRow) -> Self {
        Self {
            id: row.project_id.clone(),
            name: row.name.clone(),
            vaults: row.vaults.clone(),
            repos: row.repos.clone(),
            boards: row.boards.clone(),
            created_at: row.created_at,
        }
    }
}

/// `Repo` as the UI consumes it (api-contract.md §Repos).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepoDto {
    pub slug: String,
    pub url: String,
    pub default_branch: String,
    pub registered_at: f64,
}

impl RepoDto {
    /// Map a view row → wire DTO.
    pub fn from_row(row: &RepoRow) -> Self {
        Self {
            slug: row.slug.clone(),
            url: row.url.clone(),
            default_branch: row.default_branch.clone(),
            registered_at: row.registered_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::SessionRow;

    #[test]
    fn liveness_managed_in_flight_is_running() {
        // A managed turn streaming right now is "running" regardless of age.
        assert_eq!(
            compute_liveness(0.0, 1_000_000.0, true, true, false),
            "running"
        );
    }

    #[test]
    fn liveness_managed_awaiting_is_input_required() {
        // Awaiting a permission decision beats in-flight → "input-required".
        assert_eq!(
            compute_liveness(0.0, 1_000_000.0, true, true, true),
            "input-required"
        );
    }

    #[test]
    fn liveness_managed_not_in_flight_is_idle() {
        // A managed session with no in-flight turn is idle the instant it
        // finishes — no 90s recency fudge (fixes "active" lingering after reply).
        let now = 1_000_000.0;
        assert_eq!(compute_liveness(now - 5.0, now, false, true, false), "idle");
    }

    #[test]
    fn liveness_observed_recent_activity_is_active() {
        let now = 1_000_000.0;
        assert_eq!(
            compute_liveness(now - 10.0, now, false, false, false),
            "active"
        );
    }

    #[test]
    fn liveness_observed_stale_is_idle() {
        let now = 1_000_000.0;
        // Older than the recency window and nothing in-flight → idle (honest:
        // could be walked-away or crashed; we don't claim "dead").
        assert_eq!(
            compute_liveness(now - (ACTIVE_WINDOW_SECS + 30.0), now, false, false, false),
            "idle"
        );
    }

    #[test]
    fn parse_tool_calls_openai_function_envelope() {
        // The OpenAI shape stored in state.db.
        let raw = r#"[{"id":"call_1","function":{"name":"terminal","arguments":"{\"command\":\"ls\"}"}}]"#;
        let v = parse_tool_calls(raw).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "terminal");
        // arguments JSON-string is parsed into an object.
        assert_eq!(arr[0]["args"]["command"], "ls");
    }

    #[test]
    fn parse_tool_calls_bare_array() {
        // Hermes/Anthropic-style: {name, args} objects, args already an object.
        let raw = r#"[{"name":"patch","args":{"path":"x.rs"}},{"name":"ls"}]"#;
        let v = parse_tool_calls(raw).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["args"]["path"], "x.rs");
        assert_eq!(arr[1]["name"], "ls");
    }

    #[test]
    fn parse_tool_calls_empty_and_garbage_is_none() {
        assert_eq!(parse_tool_calls(""), None);
        assert_eq!(parse_tool_calls("   "), None);
        assert_eq!(parse_tool_calls("not json"), None);
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
            pinned: false,
            last_activity: 200.0,
            agent: None,
            node: None,
            parent_session_id: None,
            card_id: None,
            project_id: None,
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
