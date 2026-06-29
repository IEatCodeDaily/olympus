//! Read-only bulk import from Hermes `state.db` into the Olympus event log.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OpenFlags};

use crate::event::Event;
use crate::log::Log;

const MESSAGE_BATCH_SIZE: i64 = 1_000;

/// Counts and elapsed time reported by an import pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportStats {
    pub session_count: u64,
    pub message_count: u64,
    pub duration_ms: u128,
}

/// Import Hermes sessions from `state.db` as `Event::SessionCreated` events.
pub fn import_sessions(state_db: &Path, log: &Log) -> Result<ImportStats> {
    let started = Instant::now();
    let conn = open_read_only(state_db)?;
    let mut stmt = conn.prepare(
        "SELECT id, source, model, title, started_at, message_count, input_tokens, output_tokens, archived, parent_session_id
         FROM sessions
         ORDER BY started_at",
    )?;
    let mut rows = stmt.query([])?;
    let mut session_count = 0;
    let mut batch: Vec<Event> = Vec::with_capacity(MESSAGE_BATCH_SIZE as usize);
    while let Some(row) = rows.next()? {
        let id: String = row.get("id")?;
        let message_count = nullable_count_or_zero(row.get("message_count")?, "message_count")?;
        let input_tokens = nullable_count_or_zero(row.get("input_tokens")?, "input_tokens")?;
        let output_tokens = nullable_count_or_zero(row.get("output_tokens")?, "output_tokens")?;
        batch.push(Event::SessionCreated {
            session_id: id.clone(),
            hermes_id: id,
            source: row.get("source")?,
            model: row.get("model")?,
            title: row.get("title")?,
            started_at: row.get("started_at")?,
            message_count,
            input_tokens,
            output_tokens,
        });
        session_count += 1;
        if batch.len() >= MESSAGE_BATCH_SIZE as usize {
            log.append_batch(&batch)
                .context("appending imported session batch")?;
            batch.clear();
        }
    }
    log.append_batch(&batch)
        .context("appending final imported session batch")?;

    Ok(ImportStats {
        session_count,
        message_count: 0,
        duration_ms: started.elapsed().as_millis(),
    })
}

/// Import active Hermes messages from `state.db` as `Event::MessageAppended` events.
pub fn import_messages(state_db: &Path, log: &Log) -> Result<ImportStats> {
    let started = Instant::now();
    let conn = open_read_only(state_db)?;
    let mut offset = 0;
    let mut message_count = 0;
    let mut next_message_id_by_session = HashMap::<String, u64>::new();

    loop {
        let mut stmt = conn.prepare(
            "SELECT session_id, role, content, tool_name, tool_calls, reasoning, timestamp, token_count, finish_reason
             FROM messages
             WHERE active = 1
             ORDER BY session_id, timestamp
             LIMIT ?1 OFFSET ?2",
        )?;
        let mut rows = stmt.query(params![MESSAGE_BATCH_SIZE, offset])?;
        let mut rows_in_batch = 0;
        let mut batch: Vec<Event> = Vec::with_capacity(MESSAGE_BATCH_SIZE as usize);

        while let Some(row) = rows.next()? {
            let session_id: String = row.get("session_id")?;
            let message_id = next_message_id_by_session
                .entry(session_id.clone())
                .and_modify(|next| *next += 1)
                .or_insert(0);
            batch.push(Event::MessageAppended {
                session_id: session_id.clone(),
                hermes_session_id: session_id,
                message_id: *message_id,
                role: row.get("role")?,
                content: row.get("content")?,
                tool_name: row.get("tool_name")?,
                tool_calls: row.get("tool_calls")?,
                reasoning: row.get("reasoning")?,
                timestamp: row.get("timestamp")?,
                token_count: nullable_token_count(row.get("token_count")?, "token_count")?,
                finish_reason: row.get("finish_reason")?,
            });
            rows_in_batch += 1;
            message_count += 1;
        }

        log.append_batch(&batch)
            .context("appending imported message batch")?;

        if rows_in_batch == 0 {
            break;
        }
        offset += MESSAGE_BATCH_SIZE;
    }

    Ok(ImportStats {
        session_count: 0,
        message_count,
        duration_ms: started.elapsed().as_millis(),
    })
}

fn open_read_only(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("opening {} read-only", path.display()))
}

fn nullable_count_or_zero(value: Option<i64>, field: &str) -> Result<u64> {
    match value {
        Some(value) => i64_to_u64(value, field),
        None => Ok(0),
    }
}

fn nullable_token_count(value: Option<i64>, field: &str) -> Result<Option<u64>> {
    value.map(|value| i64_to_u64(value, field)).transpose()
}

fn i64_to_u64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("{field} must be non-negative"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use rusqlite::Connection;
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
                archived INTEGER,
                parent_session_id TEXT
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
                active INTEGER NOT NULL DEFAULT 1
            );

            INSERT INTO sessions
                (id, source, model, title, started_at, message_count, input_tokens, output_tokens, archived, parent_session_id)
            VALUES
                ('sess-b', 'telegram', 'claude-sonnet-4', 'Later session', 20.0, 2, 11, 13, 1, 'parent-1'),
                ('sess-a', 'cli',      'glm-5.2',          'Earlier session', 10.0, 1, 3,  5,  0, NULL);

            INSERT INTO messages
                (session_id, role, content, tool_name, tool_calls, reasoning, timestamp, token_count, finish_reason, active)
            VALUES
                ('sess-b', 'user',      'deleted message', NULL, NULL, NULL, 19.0, 9, NULL, 0),
                ('sess-b', 'user',      'hello b',         NULL, NULL, NULL, 21.0, 2, NULL, 1),
                ('sess-b', 'assistant', 'answer b',        NULL, '{"x":1}', 'thought', 22.0, 7, 'stop', 1),
                ('sess-a', 'user',      'hello a',         NULL, NULL, NULL, 11.0, 1, NULL, 1);
            "#,
        )
        .unwrap();
        drop(conn);
        file
    }

    fn fresh_log() -> (NamedTempFile, Log) {
        let file = NamedTempFile::new().unwrap();
        let log = Log::open(file.path()).unwrap();
        (file, log)
    }

    #[test]
    fn import_sessions_appends_one_session_created_event_per_session_in_started_order() {
        let state_db = create_state_db();
        let (_log_file, log) = fresh_log();

        let stats = import_sessions(state_db.path(), &log).unwrap();

        assert_eq!(stats.session_count, 2);
        assert_eq!(stats.message_count, 0);
        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events.iter().map(|(_, event)| event).collect::<Vec<_>>(),
            vec![
                &Event::SessionCreated {
                    session_id: "sess-a".into(),
                    hermes_id: "sess-a".into(),
                    source: "cli".into(),
                    model: Some("glm-5.2".into()),
                    title: Some("Earlier session".into()),
                    started_at: 10.0,
                    message_count: 1,
                    input_tokens: 3,
                    output_tokens: 5,
                },
                &Event::SessionCreated {
                    session_id: "sess-b".into(),
                    hermes_id: "sess-b".into(),
                    source: "telegram".into(),
                    model: Some("claude-sonnet-4".into()),
                    title: Some("Later session".into()),
                    started_at: 20.0,
                    message_count: 2,
                    input_tokens: 11,
                    output_tokens: 13,
                },
            ]
        );
    }

    #[test]
    fn import_messages_appends_only_active_messages_in_session_timestamp_order() {
        let state_db = create_state_db();
        let (_log_file, log) = fresh_log();

        let stats = import_messages(state_db.path(), &log).unwrap();

        assert_eq!(stats.session_count, 0);
        assert_eq!(stats.message_count, 3);
        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(
            events.iter().map(|(_, event)| event).collect::<Vec<_>>(),
            vec![
                &Event::MessageAppended {
                    session_id: "sess-a".into(),
                    hermes_session_id: "sess-a".into(),
                    message_id: 0,
                    role: "user".into(),
                    content: Some("hello a".into()),
                    tool_name: None,
                    tool_calls: None,
                    reasoning: None,
                    timestamp: 11.0,
                    token_count: Some(1),
                    finish_reason: None,
                },
                &Event::MessageAppended {
                    session_id: "sess-b".into(),
                    hermes_session_id: "sess-b".into(),
                    message_id: 0,
                    role: "user".into(),
                    content: Some("hello b".into()),
                    tool_name: None,
                    tool_calls: None,
                    reasoning: None,
                    timestamp: 21.0,
                    token_count: Some(2),
                    finish_reason: None,
                },
                &Event::MessageAppended {
                    session_id: "sess-b".into(),
                    hermes_session_id: "sess-b".into(),
                    message_id: 1,
                    role: "assistant".into(),
                    content: Some("answer b".into()),
                    tool_name: None,
                    tool_calls: Some("{\"x\":1}".into()),
                    reasoning: Some("thought".into()),
                    timestamp: 22.0,
                    token_count: Some(7),
                    finish_reason: Some("stop".into()),
                },
            ]
        );
    }
}
