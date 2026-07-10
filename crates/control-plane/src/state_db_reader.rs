//! Read-only query helpers for the Hermes `state.db`.
//!
//! Hall is a lightweight routing layer — it keeps a session metadata index
//! (session id, source, title, model, timestamps) for cross-node/cross-agent
//! session listing, but message bodies and full-text search are queried
//! on-demand from the source `state.db` instead of being mirrored into Hall's
//! own storage.
//!
//! This eliminates the 1.4 GB RSS caused by importing 137K decompressed message
//! bodies into in-memory views.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::views::message::MessageRow;

/// A raw search hit — session_id + snippet, before enrichment.
pub struct RawSearchHit {
    pub session_id: String,
    pub message_id: u64,
    pub timestamp: f64,
    pub snippet: String,
}

/// A read-only connection to the Hermes `state.db`.
pub struct StateDbReader {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl StateDbReader {
    /// Open a read-only connection. Returns `None` if the file doesn't exist
    /// (Hall starts without history — only live sessions show up).
    pub fn open(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            tracing::info!(db = %path.display(), "state.db not found — history reads disabled");
            return Ok(None);
        }
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening read-only {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Some(Self {
            path: path.to_path_buf(),
            conn: Mutex::new(conn),
        }))
    }

    /// Path of the underlying state.db (for diagnostics).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Fetch the most recent `limit` active, non-compacted messages for a
    /// session, returned oldest-first for display. Message IDs are assigned
    /// as a 0-based sequence over active rows (same semantics as the sync
    /// worker's id mapping).
    pub fn recent_messages(&self, session_id: &str, limit: usize) -> Result<Vec<MessageRow>> {
        let conn = self.conn.lock().expect("state.db mutex poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT role, content, tool_name, tool_calls, reasoning,
                   timestamp, token_count, finish_reason
            FROM (
                SELECT
                    ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY id ASC) - 1 AS seq,
                    role, content, tool_name, tool_calls, reasoning,
                    timestamp, token_count, finish_reason
                FROM messages
                WHERE session_id = ?1 AND active = 1 AND compacted = 0
                ORDER BY id DESC
                LIMIT ?2
            )
            ORDER BY seq ASC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id, limit as i64], |row| {
            let token_count: Option<i64> = row.get(6)?;
            Ok(MessageRow {
                message_id: row.get::<_, i64>(0)? as u64, // not used — assigned below
                role: row.get(1)?,
                content: row.get(2)?,
                tool_name: row.get(3)?,
                tool_calls: row.get(4)?,
                reasoning: row.get(5)?,
                timestamp: row.get(6)?,
                token_count: token_count.map(|v| v as u64),
            })
        })?;
        // Re-assign sequential message IDs over the returned window so the UI
        // gets stable, contiguous IDs regardless of the window size.
        let mut out = Vec::new();
        let mut seq = 0u64;
        for row in rows {
            let mut r: MessageRow = row?;
            r.message_id = seq;
            seq += 1;
            out.push(r);
        }
        Ok(out)
    }

    /// Total active message count for a session.
    pub fn message_count(&self, session_id: &str) -> Result<u64> {
        let conn = self.conn.lock().expect("state.db mutex poisoned");
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id = ?1 AND active = 1 AND compacted = 0",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    /// Full-text search across all message content. Uses FTS5 if the Hermes
    /// state.db has it; falls back to LIKE.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<RawSearchHit>> {
        let conn = self.conn.lock().expect("state.db mutex poisoned");
        if conn
            .prepare(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='messages_fts' LIMIT 1",
            )?
            .exists([])?
        {
            let mut stmt = conn.prepare(
                r#"
                SELECT m.session_id,
                       ROW_NUMBER() OVER (PARTITION BY m.session_id ORDER BY m.id ASC) - 1,
                       m.timestamp,
                       snippet(messages_fts, 2, '<mark>', '</mark>', '…', 32)
                FROM messages_fts fts
                JOIN messages m ON m.rowid = fts.rowid
                WHERE messages_fts MATCH ?1 AND m.active = 1
                ORDER BY rank
                LIMIT ?2
                "#,
            )?;
            let rows = stmt.query_map(params![query, limit as i64], |row| {
                Ok(RawSearchHit {
                    session_id: row.get(0)?,
                    message_id: row.get::<_, i64>(1)? as u64,
                    timestamp: row.get(2)?,
                    snippet: row.get(3)?,
                })
            })?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            return Ok(out);
        }
        // LIKE fallback
        let pattern = format!("%{query}%");
        let mut stmt = conn.prepare(
            r#"
            SELECT session_id,
                   ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY id ASC) - 1,
                   timestamp,
                   substr(content, 1, 200)
            FROM messages
            WHERE content LIKE ?1 AND active = 1
            ORDER BY timestamp DESC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![pattern, limit as i64], |row| {
            let snippet: String = row.get(3).unwrap_or_default();
            Ok(RawSearchHit {
                session_id: row.get(0)?,
                message_id: row.get::<_, i64>(1)? as u64,
                timestamp: row.get(2)?,
                snippet,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}
