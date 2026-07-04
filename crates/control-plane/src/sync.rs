//! Live sync from Hermes `state.db` into the Olympus projections.
//!
//! The helpers in this module read from a read-only SQLite connection and turn
//! Hermes mutations into Olympus events. The caller is responsible for applying
//! those events to the log/views/search layers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use tokio::sync::{broadcast, RwLock};

use crate::event::Event;
use crate::log::Log;
use crate::search::SearchIndex;
use crate::server::dto::MessageDto;
use crate::server::ws::ServerFrame;
use crate::views::{MessageView, SessionView, ViewManager};

/// A live-message row pulled from `state.db`.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveMessageRow {
    pub id: u64,
    pub session_id: String,
    pub hermes_session_id: String,
    pub message_id: u64,
    pub role: String,
    pub content: Option<String>,
    pub tool_name: Option<String>,
    pub tool_calls: Option<String>,
    pub reasoning: Option<String>,
    pub timestamp: f64,
    pub token_count: Option<u64>,
    pub finish_reason: Option<String>,
    pub active: bool,
    pub compacted: bool,
}

/// The monotonic tail cursor returned after a poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TailCursor {
    pub last_seen_id: u64,
}

/// Session metadata mirrored from `state.db`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionMeta {
    pub session_id: String,
    pub source: String,
    pub title: Option<String>,
    pub model: Option<String>,
    pub started_at: f64,
    pub archived: bool,
    pub message_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Sync worker state: tail cursors and the Hermes-row→Olympus-message mapping
/// for the sessions currently tracked by the worker.
#[derive(Debug, Clone)]
pub struct SyncState {
    pub last_seen_id: u64,
    pub next_message_id_by_session: HashMap<String, u64>,
    pub message_ids_by_session_db_id: HashMap<String, HashMap<u64, u64>>,
    pub session_signatures: HashMap<String, SessionSignature>,
}

/// A cheap session fingerprint for change detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSignature {
    pub max_id: u64,
    pub row_count: u64,
}

impl SyncState {
    pub fn new() -> Self {
        Self {
            last_seen_id: 0,
            next_message_id_by_session: HashMap::new(),
            message_ids_by_session_db_id: HashMap::new(),
            session_signatures: HashMap::new(),
        }
    }

    pub fn seed_session_from_rows(&mut self, session_id: &str, rows: &[LiveMessageRow]) {
        let mut next = self
            .next_message_id_by_session
            .get(session_id)
            .copied()
            .unwrap_or(0);
        let db_to_olympus = self
            .message_ids_by_session_db_id
            .entry(session_id.to_string())
            .or_default();

        for row in rows {
            if db_to_olympus.contains_key(&row.id) {
                continue;
            }
            db_to_olympus.insert(row.id, next);
            next += 1;
        }

        self.next_message_id_by_session
            .insert(session_id.to_string(), next);
    }

    pub fn knows_session(&self, session_id: &str) -> bool {
        self.session_signatures.contains_key(session_id)
    }
}

impl Default for SyncState {
    fn default() -> Self {
        Self::new()
    }
}

/// List every live session id in `state.db`.
pub fn list_session_ids(conn: &Connection) -> Result<Vec<String>> {
    // Exclude `source='olympus'` sessions: those are created and owned by the
    // Olympus bridge (it writes the event log directly). Hermes ALSO records the
    // same conversation in state.db under the ACP session id, so importing it
    // here would create a phantom duplicate session (keyed by hermes-id) and
    // double-count every message. The bridge is the single writer for olympus
    // sessions; live-sync only mirrors the OTHER channels (cli/telegram/etc.).
    let mut stmt = conn.prepare(
        "SELECT id FROM sessions WHERE source IS NULL OR source != 'olympus' ORDER BY started_at ASC",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row.get::<_, String>("id")?);
    }
    Ok(out)
}

/// Seed a sync state from the current DB snapshot so tail/reconcile can assign
/// stable Olympus message ids from the live Hermes rows.
pub fn seed_state_from_db(conn: &Connection, sync_state: &mut SyncState) -> Result<()> {
    for session_id in list_session_ids(conn)? {
        let rows = load_active_messages(conn, &session_id)?;
        let signature = SessionSignature {
            max_id: rows.iter().map(|row| row.id).max().unwrap_or(0),
            row_count: rows.len() as u64,
        };
        sync_state.last_seen_id = sync_state.last_seen_id.max(signature.max_id);
        sync_state
            .session_signatures
            .insert(session_id.clone(), signature);
        sync_state.seed_session_from_rows(&session_id, &rows);
    }
    Ok(())
}

/// Read live message rows with `id > last_seen_id`, returning only rows that
/// should surface as new `MessageAppended` events.
pub fn poll_message_tail(
    conn: &Connection,
    last_seen_id: u64,
    limit: usize,
) -> Result<(Vec<LiveMessageRow>, TailCursor)> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            id,
            session_id,
            role,
            content,
            tool_name,
            tool_calls,
            reasoning,
            timestamp,
            token_count,
            finish_reason,
            active,
            compacted
        FROM messages
        WHERE id > ?1
        ORDER BY id ASC
        LIMIT ?2
        "#,
    )?;
    let mut rows = stmt.query(params![last_seen_id, limit as i64])?;
    let mut out = Vec::new();
    let mut max_seen = last_seen_id;

    while let Some(row) = rows.next()? {
        let id: u64 = row.get("id")?;
        max_seen = max_seen.max(id);

        let active: i64 = row.get("active")?;
        let compacted: i64 = row.get("compacted")?;
        if active == 0 || compacted == 1 {
            continue;
        }

        let session_id: String = row.get("session_id")?;
        out.push(LiveMessageRow {
            id,
            session_id: session_id.clone(),
            hermes_session_id: session_id,
            message_id: 0,
            role: row.get("role")?,
            content: row.get("content")?,
            tool_name: row.get("tool_name")?,
            tool_calls: row.get("tool_calls")?,
            reasoning: row.get("reasoning")?,
            timestamp: row.get("timestamp")?,
            token_count: row
                .get::<_, Option<i64>>("token_count")?
                .map(|value| value as u64),
            finish_reason: row.get("finish_reason")?,
            active: true,
            compacted: false,
        });
    }

    Ok((
        out,
        TailCursor {
            last_seen_id: max_seen,
        },
    ))
}

/// Convert newly seen Hermes rows into Olympus events, allocating stable
/// per-session message ids.
pub fn tail_rows_to_events(sync_state: &mut SyncState, rows: Vec<LiveMessageRow>) -> Vec<Event> {
    let mut out = Vec::new();
    for row in rows {
        if !sync_state.knows_session(&row.session_id) {
            continue;
        }
        let db_to_olympus = sync_state
            .message_ids_by_session_db_id
            .entry(row.session_id.clone())
            .or_default();
        if db_to_olympus.contains_key(&row.id) {
            continue;
        }

        let next = sync_state
            .next_message_id_by_session
            .get(&row.session_id)
            .copied()
            .unwrap_or(0);
        db_to_olympus.insert(row.id, next);
        sync_state
            .next_message_id_by_session
            .insert(row.session_id.clone(), next + 1);

        out.push(Event::MessageAppended {
            session_id: row.session_id.clone(),
            hermes_session_id: row.hermes_session_id.clone(),
            message_id: next,
            role: row.role,
            content: row.content,
            tool_name: row.tool_name,
            tool_calls: row.tool_calls,
            reasoning: row.reasoning,
            timestamp: row.timestamp,
            token_count: row.token_count,
            finish_reason: row.finish_reason,
        });
    }
    out
}

/// Read the authoritative session metadata from `state.db`.
pub fn load_session_meta(conn: &Connection, session_id: &str) -> Result<Option<SessionMeta>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, source, title, model, started_at, archived, message_count, input_tokens, output_tokens
        FROM sessions
        WHERE id = ?1
        "#,
    )?;
    let mut rows = stmt.query(params![session_id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };

    Ok(Some(SessionMeta {
        session_id: row.get("id")?,
        source: row.get("source")?,
        title: row.get("title")?,
        model: row.get("model")?,
        started_at: row.get("started_at")?,
        archived: row.get::<_, i64>("archived")? != 0,
        message_count: row.get::<_, Option<i64>>("message_count")?.unwrap_or(0) as u64,
        input_tokens: row.get::<_, Option<i64>>("input_tokens")?.unwrap_or(0) as u64,
        output_tokens: row.get::<_, Option<i64>>("output_tokens")?.unwrap_or(0) as u64,
    }))
}

fn load_active_messages(conn: &Connection, session_id: &str) -> Result<Vec<LiveMessageRow>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            id,
            session_id,
            role,
            content,
            tool_name,
            tool_calls,
            reasoning,
            timestamp,
            token_count,
            finish_reason,
            active,
            compacted
        FROM messages
        WHERE session_id = ?1
        ORDER BY id ASC
        "#,
    )?;

    let mut rows = stmt.query(params![session_id])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let active: i64 = row.get("active")?;
        let compacted: i64 = row.get("compacted")?;
        if active == 0 || compacted == 1 {
            continue;
        }
        let session_id: String = row.get("session_id")?;
        out.push(LiveMessageRow {
            id: row.get("id")?,
            session_id: session_id.clone(),
            hermes_session_id: session_id,
            message_id: 0,
            role: row.get("role")?,
            content: row.get("content")?,
            tool_name: row.get("tool_name")?,
            tool_calls: row.get("tool_calls")?,
            reasoning: row.get("reasoning")?,
            timestamp: row.get("timestamp")?,
            token_count: row
                .get::<_, Option<i64>>("token_count")?
                .map(|value| value as u64),
            finish_reason: row.get("finish_reason")?,
            active: true,
            compacted: false,
        });
    }
    Ok(out)
}

/// Reconcile one session's current hot window against the live DB state.
///
/// The returned events are ordered so callers can remove stale rows before
/// appending the current active window, then patch session metadata.
pub fn reconcile_session(
    conn: &Connection,
    sync_state: &mut SyncState,
    session_view: &SessionView,
    message_view: &MessageView,
    session_id: &str,
) -> Result<Vec<Event>> {
    let Some(meta) = load_session_meta(conn, session_id)? else {
        return Ok(vec![]);
    };
    let session_exists = session_view.get(session_id).is_some();

    let current = load_active_messages(conn, session_id)?;
    let signature = SessionSignature {
        max_id: current.iter().map(|row| row.id).max().unwrap_or(0),
        row_count: current.len() as u64,
    };
    if sync_state.session_signatures.get(session_id).copied() == Some(signature) {
        let current_row = session_view.get(session_id);
        let meta_changed = match current_row {
            Some(row) => {
                row.title != meta.title
                    || row.model != meta.model
                    || row.archived != meta.archived
                    || row.message_count != meta.message_count
            }
            None => true,
        };
        if !meta_changed {
            return Ok(vec![]);
        }
    }
    sync_state
        .session_signatures
        .insert(session_id.to_string(), signature);

    let window_size = message_view.window_size();
    let desired_window: Vec<_> = current
        .iter()
        .skip(current.len().saturating_sub(window_size))
        .cloned()
        .collect();

    let current_map = sync_state
        .message_ids_by_session_db_id
        .entry(session_id.to_string())
        .or_default();

    let desired_db_ids: std::collections::HashSet<u64> =
        desired_window.iter().map(|row| row.id).collect();

    let mut events = Vec::new();
    if !session_exists {
        events.push(Event::SessionCreated {
            session_id: meta.session_id.clone(),
            hermes_id: meta.session_id.clone(),
            source: meta.source.clone(),
            model: meta.model.clone(),
            title: meta.title.clone(),
            started_at: meta.started_at,
            message_count: meta.message_count,
            input_tokens: meta.input_tokens,
            output_tokens: meta.output_tokens,
            agent: None,
            node: None,
        });
    }
    let mut stale_db_ids: Vec<u64> = current_map
        .keys()
        .copied()
        .filter(|db_id| !desired_db_ids.contains(db_id))
        .collect();
    stale_db_ids.sort_unstable();
    for db_id in stale_db_ids {
        if let Some(message_id) = current_map.remove(&db_id) {
            events.push(Event::MessageRemoved {
                session_id: session_id.to_string(),
                hermes_session_id: session_id.to_string(),
                message_id,
            });
        }
    }

    let mut next_message_id = sync_state
        .next_message_id_by_session
        .get(session_id)
        .copied()
        .unwrap_or(0);

    for row in desired_window {
        if current_map.contains_key(&row.id) {
            continue;
        }
        let message_id = next_message_id;
        next_message_id += 1;
        current_map.insert(row.id, message_id);
        events.push(Event::MessageAppended {
            session_id: session_id.to_string(),
            hermes_session_id: session_id.to_string(),
            message_id,
            role: row.role.clone(),
            content: row.content.clone(),
            tool_name: row.tool_name.clone(),
            tool_calls: row.tool_calls.clone(),
            reasoning: row.reasoning.clone(),
            timestamp: row.timestamp,
            token_count: row.token_count,
            finish_reason: row.finish_reason.clone(),
        });
    }

    sync_state
        .next_message_id_by_session
        .insert(session_id.to_string(), next_message_id);

    let current_row = session_view.get(session_id);
    let meta_changed = match current_row {
        Some(row) => {
            row.title != meta.title
                || row.model != meta.model
                || row.archived != meta.archived
                || row.message_count != meta.message_count
        }
        None => true,
    };

    let window_changed = events.iter().any(|event| {
        matches!(
            event,
            Event::MessageRemoved { .. } | Event::MessageAppended { .. }
        )
    });
    let has_refresh = events
        .iter()
        .any(|event| matches!(event, Event::SessionUpdated { .. }));
    if session_exists && (meta_changed || window_changed) && !has_refresh {
        events.push(Event::SessionUpdated {
            session_id: session_id.to_string(),
            title: meta.title,
            model: meta.model,
            archived: Some(meta.archived),
            message_count: Some(meta.message_count),
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        });
    }

    Ok(events)
}

/// Run the live Hermes→Olympus sync loop until the process exits.
///
/// This is intended to run on a detached worker thread. It polls the state DB
/// for new rows, appends them to the event log + views, and periodically
/// reconciles each session's hot window when compaction/tombstones happen.
pub fn run_live_sync(
    state_db: PathBuf,
    log: Arc<Log>,
    views: Arc<RwLock<ViewManager>>,
    search: Arc<RwLock<SearchIndex>>,
    deltas: broadcast::Sender<ServerFrame>,
    sync_connected: Arc<AtomicBool>,
) -> Result<()> {
    let conn = Connection::open_with_flags(state_db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("opening read-only Hermes state.db")?;

    let mut sync_state = SyncState::new();
    seed_state_from_db(&conn, &mut sync_state)?;
    sync_connected.store(true, Ordering::SeqCst);

    // Adaptive polling: when the tail is empty (idle — no new Hermes activity),
    // back off to a slow interval so the worker burns ~no CPU; when new rows
    // arrive, poll fast to keep latency low. This was the #1 CPU consumer at
    // idle (59% of lifetime CPU) because it polled every 2s and reconciled
    // every 30s unconditionally over ~1700 sessions.
    let fast_interval = Duration::from_secs(2);
    let idle_interval = Duration::from_secs(30);
    let reconcile_interval = Duration::from_secs(60);
    let mut last_reconcile = Instant::now();

    loop {
        let (rows, cursor) = poll_message_tail(&conn, sync_state.last_seen_id, 1_000)?;
        sync_state.last_seen_id = cursor.last_seen_id;

        let events = tail_rows_to_events(&mut sync_state, rows);
        let had_events = !events.is_empty();
        if had_events {
            apply_events(&log, &views, &search, &deltas, &events)?;
        }

        // Only run the (expensive) full reconcile sweep when new messages
        // actually arrived since the last reconcile — otherwise nothing
        // changed, so re-checking 1700 sessions is pure waste.
        if had_events && last_reconcile.elapsed() >= reconcile_interval {
            let session_ids = list_session_ids(&conn)?;
            for session_id in session_ids {
                let events = {
                    let snapshot = views.blocking_read();
                    reconcile_session(
                        &conn,
                        &mut sync_state,
                        &snapshot.sessions,
                        &snapshot.messages,
                        &session_id,
                    )?
                };
                if !events.is_empty() {
                    apply_events(&log, &views, &search, &deltas, &events)?;
                }
            }
            last_reconcile = Instant::now();
        }

        // Back off when idle; stay fast right after activity.
        std::thread::sleep(if had_events {
            fast_interval
        } else {
            idle_interval
        });
    }
}

fn apply_events(
    log: &Log,
    views: &Arc<RwLock<ViewManager>>,
    search: &Arc<RwLock<SearchIndex>>,
    deltas: &broadcast::Sender<ServerFrame>,
    events: &[Event],
) -> Result<()> {
    let mut needs_search_rebuild = false;
    let mut views_guard = views.blocking_write();
    let mut search_guard = search.blocking_write();

    for event in events {
        log.append(event)?;
        views_guard.apply(event);

        match event {
            Event::MessageAppended {
                session_id,
                message_id,
                content: Some(content),
                tool_name,
                timestamp,
                ..
            } => {
                search_guard.index_message(
                    session_id,
                    *message_id,
                    content,
                    event_role(event),
                    tool_name.as_deref(),
                    *timestamp,
                )?;
                let _ = deltas.send(ServerFrame::MessageAppended {
                    session_id: session_id.clone(),
                    message: MessageDto {
                        message_id: *message_id,
                        session_id: session_id.clone(),
                        role: event_role(event).to_string(),
                        content: Some(content.clone()),
                        tool_name: tool_name.clone(),
                        tool_calls: None,
                        reasoning: None,
                        timestamp: *timestamp,
                        token_count: event_token_count(event),
                        finish_reason: event_finish_reason(event),
                    },
                });
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
                pinned: _,
            } => {
                let mut changes = serde_json::Map::new();
                if let Some(title) = title {
                    changes.insert("title".into(), serde_json::Value::String(title.clone()));
                }
                if let Some(model) = model {
                    changes.insert("model".into(), serde_json::Value::String(model.clone()));
                }
                if let Some(archived) = archived {
                    changes.insert("archived".into(), serde_json::Value::Bool(*archived));
                }
                if let Some(message_count) = message_count {
                    changes.insert(
                        "messageCount".into(),
                        serde_json::Value::Number((*message_count).into()),
                    );
                }
                if let Some(agent) = agent {
                    changes.insert("agent".into(), serde_json::Value::String(agent.clone()));
                }
                if let Some(node) = node {
                    changes.insert("node".into(), serde_json::Value::String(node.clone()));
                }
                if let Some(hermes_id) = hermes_id {
                    changes.insert(
                        "hermesId".into(),
                        serde_json::Value::String(hermes_id.clone()),
                    );
                }
                let _ = deltas.send(ServerFrame::SessionUpdated {
                    session_id: session_id.clone(),
                    changes: serde_json::Value::Object(changes),
                });
            }
            Event::MessageRemoved { .. } => {
                needs_search_rebuild = true;
            }
            _ => {}
        }
    }

    if needs_search_rebuild {
        search_guard.build_from_log(log)?;
    }

    Ok(())
}

fn event_role(event: &Event) -> &str {
    match event {
        Event::MessageAppended { role, .. } => role.as_str(),
        _ => "assistant",
    }
}

fn event_token_count(event: &Event) -> Option<u64> {
    match event {
        Event::MessageAppended { token_count, .. } => *token_count,
        _ => None,
    }
}

fn event_finish_reason(event: &Event) -> Option<String> {
    match event {
        Event::MessageAppended { finish_reason, .. } => finish_reason.clone(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::views::ViewManager;
    use rusqlite::params;
    use tempfile::NamedTempFile;

    fn create_state_db() -> NamedTempFile {
        let file = NamedTempFile::new().unwrap();
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                model TEXT,
                title TEXT,
                started_at REAL NOT NULL,
                message_count INTEGER,
                input_tokens INTEGER,
                output_tokens INTEGER,
                archived INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT,
                tool_name TEXT,
                tool_calls TEXT,
                reasoning TEXT,
                timestamp REAL NOT NULL,
                token_count INTEGER,
                finish_reason TEXT,
                active INTEGER NOT NULL DEFAULT 1,
                compacted INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .unwrap();
        drop(conn);
        file
    }

    fn seed_session(conn: &Connection, id: &str, title: &str, model: &str, message_count: u64) {
        conn.execute(
            "INSERT INTO sessions (id, source, model, title, started_at, message_count, input_tokens, output_tokens, archived) VALUES (?1, 'cli', ?2, ?3, 1.0, ?4, 11, 22, 0)",
            params![id, model, title, message_count as i64],
        )
        .unwrap();
    }

    fn seed_message(
        conn: &Connection,
        session_id: &str,
        role: &str,
        content: &str,
        active: i64,
        compacted: i64,
    ) {
        conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_name, tool_calls, reasoning, timestamp, token_count, finish_reason, active, compacted) VALUES (?1, ?2, ?3, NULL, NULL, NULL, 1.0, NULL, NULL, ?4, ?5)",
            params![session_id, role, content, active, compacted],
        )
        .unwrap();
    }

    fn read_only_conn(db: &NamedTempFile) -> Connection {
        Connection::open_with_flags(db.path(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }

    #[test]
    fn poll_message_tail_skips_tombstoned_and_compacted_rows() {
        let db = create_state_db();
        let conn = Connection::open(db.path()).unwrap();
        seed_session(&conn, "sess-1", "title", "glm-5.2", 4);
        seed_message(&conn, "sess-1", "user", "one", 1, 0);
        seed_message(&conn, "sess-1", "user", "two", 0, 0);
        seed_message(&conn, "sess-1", "assistant", "three", 1, 1);
        seed_message(&conn, "sess-1", "assistant", "four", 1, 0);
        drop(conn);

        let conn = read_only_conn(&db);
        let (rows, cursor) = poll_message_tail(&conn, 0, 100).unwrap();
        assert_eq!(cursor.last_seen_id, 4);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].content.as_deref(), Some("one"));
        assert_eq!(rows[1].content.as_deref(), Some("four"));
        assert!(rows.iter().all(|row| row.active && !row.compacted));
    }

    #[test]
    fn seed_state_from_db_advances_tail_cursor_to_snapshot_max_message_id() {
        let db = create_state_db();
        let conn = Connection::open(db.path()).unwrap();
        seed_session(&conn, "sess-1", "title", "glm-5.2", 2);
        seed_message(&conn, "sess-1", "user", "old one", 1, 0);
        seed_message(&conn, "sess-1", "assistant", "old two", 1, 0);
        drop(conn);

        let conn = read_only_conn(&db);
        let mut sync = SyncState::new();
        seed_state_from_db(&conn, &mut sync).unwrap();

        assert_eq!(sync.last_seen_id, 2);

        drop(conn);
        let conn = Connection::open(db.path()).unwrap();
        seed_message(&conn, "sess-1", "user", "new", 1, 0);
        drop(conn);

        let conn = read_only_conn(&db);
        let (rows, cursor) = poll_message_tail(&conn, sync.last_seen_id, 100).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].content.as_deref(), Some("new"));
        assert_eq!(cursor.last_seen_id, 3);
    }

    #[test]
    fn reconcile_unknown_live_session_creates_session_before_messages() {
        let db = create_state_db();
        let conn = Connection::open(db.path()).unwrap();
        seed_session(&conn, "sess-new", "new title", "glm-5.2", 1);
        seed_message(&conn, "sess-new", "user", "hello from live db", 1, 0);
        drop(conn);

        let views = ViewManager::new();
        let mut sync = SyncState::new();
        let conn = read_only_conn(&db);
        let events = reconcile_session(
            &conn,
            &mut sync,
            &views.sessions,
            &views.messages,
            "sess-new",
        )
        .unwrap();

        assert!(
            matches!(events.first(), Some(Event::SessionCreated { session_id, hermes_id, source, model, title, message_count, input_tokens, output_tokens, .. })
            if session_id == "sess-new"
                && hermes_id == "sess-new"
                && source == "cli"
                && model.as_deref() == Some("glm-5.2")
                && title.as_deref() == Some("new title")
                && *message_count == 1
                && *input_tokens == 11
                && *output_tokens == 22)
        );
        assert!(
            matches!(events.get(1), Some(Event::MessageAppended { session_id, content, message_id, .. })
            if session_id == "sess-new" && content.as_deref() == Some("hello from live db") && *message_id == 0)
        );
    }

    #[test]
    fn live_state_db_new_session_reconciles_into_views_log_and_search() {
        let db = create_state_db();
        let conn = read_only_conn(&db);
        let mut sync = SyncState::new();
        seed_state_from_db(&conn, &mut sync).unwrap();
        drop(conn);

        let log_file = NamedTempFile::new().unwrap();
        let log = Log::open(log_file.path()).unwrap();
        let views = Arc::new(RwLock::new(ViewManager::new()));
        let search_dir = tempfile::tempdir().unwrap();
        let search = Arc::new(RwLock::new(SearchIndex::open(search_dir.path()).unwrap()));
        let (deltas, _rx) = broadcast::channel(16);

        let conn = Connection::open(db.path()).unwrap();
        seed_session(&conn, "sess-live", "live title", "glm-5.2", 1);
        seed_message(&conn, "sess-live", "user", "live hello", 1, 0);
        drop(conn);

        let conn = read_only_conn(&db);
        let (rows, cursor) = poll_message_tail(&conn, sync.last_seen_id, 100).unwrap();
        sync.last_seen_id = cursor.last_seen_id;
        assert_eq!(rows.len(), 1);
        assert!(
            tail_rows_to_events(&mut sync, rows).is_empty(),
            "messages for an unknown session wait for reconciliation so the log stays ordered"
        );

        for session_id in list_session_ids(&conn).unwrap() {
            let events = {
                let snapshot = views.blocking_read();
                reconcile_session(
                    &conn,
                    &mut sync,
                    &snapshot.sessions,
                    &snapshot.messages,
                    &session_id,
                )
                .unwrap()
            };
            apply_events(&log, &views, &search, &deltas, &events).unwrap();
        }

        let snapshot = views.blocking_read();
        assert!(snapshot.sessions.get("sess-live").is_some());
        let recent = snapshot.messages.recent("sess-live", 10);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].content.as_deref(), Some("live hello"));
        drop(snapshot);

        let events = log.read_all().unwrap();
        assert!(
            matches!(events.first().map(|(_, event)| event), Some(Event::SessionCreated { session_id, .. }) if session_id == "sess-live")
        );
        assert!(
            matches!(events.get(1).map(|(_, event)| event), Some(Event::MessageAppended { session_id, content, .. }) if session_id == "sess-live" && content.as_deref() == Some("live hello"))
        );
    }

    #[test]
    fn reconcile_session_emits_removals_appends_and_meta_updates() {
        let db = create_state_db();
        let conn = Connection::open(db.path()).unwrap();
        seed_session(&conn, "sess-1", "old title", "glm-5.2", 2);
        seed_message(&conn, "sess-1", "user", "keep", 1, 0);
        seed_message(&conn, "sess-1", "assistant", "drop", 1, 0);

        let mut views = ViewManager::new();
        views.apply(&Event::SessionCreated {
            session_id: "sess-1".into(),
            hermes_id: "sess-1".into(),
            source: "cli".into(),
            model: Some("glm-5.2".into()),
            title: Some("old title".into()),
            started_at: 1.0,
            message_count: 2,
            input_tokens: 0,
            output_tokens: 0,
            agent: None,
            node: None,
        });
        views.apply(&Event::MessageAppended {
            session_id: "sess-1".into(),
            hermes_session_id: "sess-1".into(),
            message_id: 0,
            role: "user".into(),
            content: Some("keep".into()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: 1.0,
            token_count: None,
            finish_reason: None,
        });
        views.apply(&Event::MessageAppended {
            session_id: "sess-1".into(),
            hermes_session_id: "sess-1".into(),
            message_id: 1,
            role: "assistant".into(),
            content: Some("drop".into()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: 2.0,
            token_count: None,
            finish_reason: None,
        });

        let mut sync = SyncState::new();
        sync.seed_session_from_rows(
            "sess-1",
            &[
                LiveMessageRow {
                    id: 1,
                    session_id: "sess-1".into(),
                    hermes_session_id: "sess-1".into(),
                    message_id: 0,
                    role: "user".into(),
                    content: Some("keep".into()),
                    tool_name: None,
                    tool_calls: None,
                    reasoning: None,
                    timestamp: 1.0,
                    token_count: None,
                    finish_reason: None,
                    active: true,
                    compacted: false,
                },
                LiveMessageRow {
                    id: 2,
                    session_id: "sess-1".into(),
                    hermes_session_id: "sess-1".into(),
                    message_id: 1,
                    role: "assistant".into(),
                    content: Some("drop".into()),
                    tool_name: None,
                    tool_calls: None,
                    reasoning: None,
                    timestamp: 2.0,
                    token_count: None,
                    finish_reason: None,
                    active: true,
                    compacted: false,
                },
            ],
        );

        conn.execute(
            "UPDATE messages SET active = 0, compacted = 1 WHERE id = 2",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE sessions SET title = 'new title', model = 'claude-sonnet-4', message_count = 1, archived = 1 WHERE id = 'sess-1'",
            [],
        )
        .unwrap();
        drop(conn);

        let conn = read_only_conn(&db);
        let events =
            reconcile_session(&conn, &mut sync, &views.sessions, &views.messages, "sess-1")
                .unwrap();

        assert!(matches!(
            events.first(),
            Some(Event::MessageRemoved { message_id: 1, .. })
        ));
        assert!(events.iter().any(|event| matches!(
            event,
            Event::SessionUpdated {
                title,
                model,
                archived: Some(true),
                message_count: Some(1),
                ..
            } if title.as_deref() == Some("new title") && model.as_deref() == Some("claude-sonnet-4")
        )));
    }
}
