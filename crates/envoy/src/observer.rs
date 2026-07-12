//! Read-only Hermes `state.db` host observer.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use olympus_proto::frames::ObservedEvent;
use rusqlite::{params, Connection, OpenFlags};

/// Stateful tailer. Existing rows are treated as Hall cold-boot history; only
/// sessions/messages created after construction are emitted.
pub struct StateDbObserver {
    conn: Connection,
    last_seen_id: u64,
    known_sessions: HashSet<String>,
}

impl StateDbObserver {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening {} read-only", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let last_seen_id = conn
            .query_row("SELECT COALESCE(MAX(id),0) FROM messages", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);
        let known_sessions = session_ids(&conn)?.into_iter().collect();
        Ok(Self {
            conn,
            last_seen_id,
            known_sessions,
        })
    }

    /// Poll once, preserving session-before-message ordering and excluding the
    /// bridge-owned `source='olympus'` channel.
    pub fn poll(&mut self, limit: usize) -> Result<Vec<ObservedEvent>> {
        let mut out = Vec::new();
        for id in session_ids(&self.conn)? {
            if self.known_sessions.insert(id.clone()) {
                if let Some(session) = load_session(&self.conn, &id)? {
                    out.push(session);
                }
            }
        }

        let mut stmt = self.conn.prepare(
            r#"
            SELECT m.id, m.session_id,
                   (SELECT COUNT(*) - 1 FROM messages prior
                    WHERE prior.session_id=m.session_id AND prior.id<=m.id
                      AND prior.active=1 AND prior.compacted=0) AS message_id,
                   m.role,m.content,m.tool_name,m.tool_calls,m.reasoning,
                   m.timestamp,m.token_count,m.finish_reason,m.active,m.compacted
            FROM messages m JOIN sessions s ON s.id=m.session_id
            WHERE m.id>?1 AND (s.source IS NULL OR s.source!='olympus')
            ORDER BY m.id LIMIT ?2
            "#,
        )?;
        let mut rows = stmt.query(params![self.last_seen_id, limit as i64])?;
        while let Some(row) = rows.next()? {
            let row_id: u64 = row.get(0)?;
            self.last_seen_id = self.last_seen_id.max(row_id);
            let active: i64 = row.get(11)?;
            let compacted: i64 = row.get(12)?;
            if active == 0 || compacted != 0 {
                continue;
            }
            out.push(ObservedEvent::Message {
                hermes_id: row.get(1)?,
                message_id: row.get::<_, i64>(2)? as u64,
                role: row.get(3)?,
                content: row.get(4)?,
                tool_name: row.get(5)?,
                tool_calls: row.get(6)?,
                reasoning: row.get(7)?,
                timestamp: row.get(8)?,
                token_count: row.get::<_, Option<i64>>(9)?.map(|v| v as u64),
                finish_reason: row.get(10)?,
            });
        }
        Ok(out)
    }
}

fn session_ids(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM sessions WHERE source IS NULL OR source!='olympus' ORDER BY started_at,id",
    )?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn load_session(conn: &Connection, id: &str) -> Result<Option<ObservedEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id,COALESCE(source,''),model,title,started_at,COALESCE(message_count,0),COALESCE(input_tokens,0),COALESCE(output_tokens,0),COALESCE(archived,0) FROM sessions WHERE id=?1 AND (source IS NULL OR source!='olympus')",
    )?;
    let mut rows = stmt.query([id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(ObservedEvent::Session {
        hermes_id: row.get(0)?,
        source: row.get(1)?,
        model: row.get(2)?,
        title: row.get(3)?,
        started_at: row.get(4)?,
        message_count: row.get::<_, i64>(5)? as u64,
        input_tokens: row.get::<_, i64>(6)? as u64,
        output_tokens: row.get::<_, i64>(7)? as u64,
        archived: row.get::<_, i64>(8)? != 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tails_new_non_olympus_rows_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE sessions(id TEXT PRIMARY KEY,source TEXT,model TEXT,title TEXT,started_at REAL,message_count INTEGER,input_tokens INTEGER,output_tokens INTEGER,archived INTEGER); CREATE TABLE messages(id INTEGER PRIMARY KEY AUTOINCREMENT,session_id TEXT,role TEXT,content TEXT,tool_name TEXT,tool_calls TEXT,reasoning TEXT,timestamp REAL,token_count INTEGER,finish_reason TEXT,active INTEGER,compacted INTEGER); INSERT INTO sessions VALUES('old','cli',NULL,NULL,1,0,0,0,0);").unwrap();
        drop(conn);
        let mut observer = StateDbObserver::open(&path).unwrap();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("INSERT INTO sessions VALUES('new','telegram','m','title',2,1,3,4,0); INSERT INTO sessions VALUES('owned','olympus',NULL,NULL,3,1,0,0,0); INSERT INTO messages(session_id,role,content,timestamp,active,compacted) VALUES('new','user','hello',4,1,0),('owned','user','hidden',5,1,0);").unwrap();
        drop(conn);
        let events = observer.poll(100).unwrap();
        assert!(
            matches!(&events[0], ObservedEvent::Session { hermes_id, .. } if hermes_id == "new")
        );
        assert!(
            matches!(&events[1], ObservedEvent::Message { hermes_id, message_id: 0, .. } if hermes_id == "new")
        );
        assert_eq!(events.len(), 2);
    }
}
