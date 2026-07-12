//! SQLite-backed event log and durable projections (ADR 0009).
//!
//! Every append writes the immutable event and applies its projection in the
//! same WAL transaction. Message history and FTS stay on disk; callers page it
//! on demand instead of retaining decompressed messages in process memory.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::event::Event;
use crate::views::card::CardAttempt;
use crate::views::{CardRow, MessageRow, ProjectRow, RegistryEntry, RepoRow, SessionRow, SetupRow};

pub struct Log {
    conn: Mutex<Connection>,
}

impl Log {
    pub fn open(path: &Path) -> Result<Self> {
        let mut conn =
            Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)
            .context("initializing Olympus SQLite schema")?;
        migrate_event_payloads_to_json(&mut conn)?;
        // Migrate pre-session_id databases (ADR 0009 incremental migration).
        let has_sid: bool = conn
            .prepare("PRAGMA table_info(events)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .any(|col| col == "session_id");
        if !has_sid {
            conn.execute_batch(
                "ALTER TABLE events ADD COLUMN session_id TEXT;
                 CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id);",
            )
            .context("migrating events table to add session_id column")?;
        }
        let has_org_id = conn
            .prepare("PRAGMA table_info(sessions)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(Result::ok)
            .any(|column| column == "org_id");
        if !has_org_id {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN org_id TEXT NOT NULL DEFAULT 'personal';",
            )
            .context("migrating sessions table to add org_id column")?;
        }
        conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_sessions_org ON sessions(org_id);")
            .context("indexing sessions by organization")?;
        for table in ["cards", "projects"] {
            let has_org_id = conn
                .prepare(&format!("PRAGMA table_info({table})"))?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(Result::ok)
                .any(|column| column == "org_id");
            if !has_org_id {
                conn.execute_batch(&format!(
                    "ALTER TABLE {table} ADD COLUMN org_id TEXT NOT NULL DEFAULT 'personal';"
                ))
                .with_context(|| format!("migrating {table} table to add org_id column"))?;
            }
            conn.execute_batch(&format!(
                "CREATE INDEX IF NOT EXISTS idx_{table}_org ON {table}(org_id);"
            ))
            .with_context(|| format!("indexing {table} by organization"))?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn append(&self, event: &Event) -> Result<u64> {
        let mut conn = self.conn.lock().expect("SQLite mutex poisoned");
        let tx = conn.transaction()?;
        let seq = append_in_tx(&tx, event)?;
        tx.commit()?;
        Ok(seq)
    }

    pub fn append_batch(&self, events: &[Event]) -> Result<Option<u64>> {
        if events.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn.lock().expect("SQLite mutex poisoned");
        let tx = conn.transaction()?;
        let mut first = None;
        for event in events {
            let seq = append_in_tx(&tx, event)?;
            first.get_or_insert(seq);
        }
        tx.commit()?;
        Ok(first)
    }

    pub fn read_from(&self, seq: u64, limit: usize) -> Result<Vec<(u64, Event)>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        let mut stmt =
            conn.prepare("SELECT seq, payload FROM events WHERE seq >= ?1 ORDER BY seq LIMIT ?2")?;
        let rows = stmt.query_map(params![seq as i64, limit as i64], decode_event_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn read_all(&self) -> Result<Vec<(u64, Event)>> {
        self.read_from(0, usize::MAX)
    }

    pub fn event_count(&self) -> Result<usize> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        Ok(conn.query_row("SELECT COUNT(*) FROM events", [], |row| {
            row.get::<_, i64>(0)
        })? as usize)
    }

    /// Durably advance the per-session Envoy transport watermark. Returns
    /// false for a duplicate and fails closed if delivery has a gap.
    pub fn accept_envoy_seq(&self, session_id: &str, seq: u64) -> Result<bool> {
        let mut conn = self.conn.lock().expect("SQLite mutex poisoned");
        let tx = conn.transaction()?;
        let current: Option<i64> = tx
            .query_row(
                "SELECT seq FROM envoy_watermarks WHERE session_id=?1",
                [session_id],
                |row| row.get(0),
            )
            .optional()?;
        if current.is_some_and(|watermark| seq <= watermark as u64) {
            return Ok(false);
        }
        let expected = current.map_or(0, |watermark| watermark as u64 + 1);
        if seq != expected {
            anyhow::bail!(
                "envoy event sequence gap for {session_id}: expected {expected}, got {seq}"
            );
        }
        tx.execute(
            "INSERT INTO envoy_watermarks(session_id,seq) VALUES(?1,?2)
             ON CONFLICT(session_id) DO UPDATE SET seq=excluded.seq",
            params![session_id, seq as i64],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn accept_observed(
        &self,
        transport_session_id: &str,
        seq: u64,
        hermes_id: &str,
        message_id: Option<u64>,
        event: &Event,
    ) -> Result<bool> {
        let mut conn = self.conn.lock().expect("SQLite mutex poisoned");
        let tx = conn.transaction()?;
        let current: Option<i64> = tx
            .query_row(
                "SELECT seq FROM envoy_watermarks WHERE session_id=?1",
                [transport_session_id],
                |row| row.get(0),
            )
            .optional()?;
        if current.is_some_and(|watermark| seq <= watermark as u64) {
            return Ok(false);
        }
        let expected = current.map_or(0, |watermark| watermark as u64 + 1);
        anyhow::ensure!(
            seq == expected,
            "envoy observation sequence gap for {transport_session_id}: expected {expected}, got {seq}"
        );
        tx.execute(
            "INSERT OR IGNORE INTO observed_sessions(hermes_id) VALUES(?1)",
            [hermes_id],
        )?;
        let is_new = if let Some(message_id) = message_id {
            tx.execute(
                "INSERT OR IGNORE INTO observed_messages(hermes_id,message_id) VALUES(?1,?2)",
                params![hermes_id, message_id as i64],
            )? == 1
        } else {
            true
        };
        if is_new {
            append_in_tx(&tx, event)?;
        }
        tx.execute(
            "INSERT INTO envoy_watermarks(session_id,seq) VALUES(?1,?2)
             ON CONFLICT(session_id) DO UPDATE SET seq=excluded.seq",
            params![transport_session_id, seq as i64],
        )?;
        tx.commit()?;
        Ok(is_new)
    }

    pub fn envoy_watermark(&self, session_id: &str) -> Result<Option<u64>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        conn.query_row(
            "SELECT seq FROM envoy_watermarks WHERE session_id=?1",
            [session_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map(|value| value.map(|seq| seq as u64))
        .map_err(Into::into)
    }

    /// Keep Olympus-native events only (setup declarations, cards, registry,
    /// olympus sessions + their messages). The state.db mirror is rebuilt on
    /// every boot so we must purge it between boots.
    ///
    /// Pure SQL using the `events.session_id` column: deletes non-native
    /// events, messages, and session rows in a single transaction without
    /// deserializing any `Event` into RAM. The old approach materialized
    /// 143K events + 137K message payloads (~1.8 GB RSS) and caused OOM.
    pub fn retain_native(&self) -> Result<()> {
        let mut conn = self.conn.lock().expect("SQLite mutex poisoned");
        let tx = conn.transaction()?;

        // Build native session set once (temp table = index-friendly).
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS _native AS
             SELECT session_id FROM sessions WHERE source = 'olympus'
             UNION SELECT hermes_id FROM observed_sessions;",
        )?;

        // Delete non-native events. session_id IS NULL events are
        // unconditionally-native (setup, registry, card, project, repo) and
        // survive. Session-scoped events survive iff their session is native.
        tx.execute(
            "DELETE FROM events
             WHERE session_id IS NOT NULL
               AND session_id NOT IN (SELECT session_id FROM _native)",
            [],
        )?;
        // Delete non-native message rows.
        tx.execute(
            "DELETE FROM messages
             WHERE session_id NOT IN (SELECT session_id FROM _native)",
            [],
        )?;
        // Delete non-native session rows.
        tx.execute("DELETE FROM sessions WHERE source != 'olympus'", [])?;

        tx.execute_batch("DROP TABLE _native;")?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionRow>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT session_id, hermes_id, source, model, title, started_at,
                    message_count, input_tokens, output_tokens, archived, pinned,
                    last_activity, agent, node, parent_session_id, card_id, project_id, org_id
             FROM sessions ORDER BY started_at DESC, session_id",
        )?;
        let rows = stmt.query_map([], session_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_session(&self, id: &str) -> Result<Option<SessionRow>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        conn.query_row(
            "SELECT session_id, hermes_id, source, model, title, started_at,
                    message_count, input_tokens, output_tokens, archived, pinned,
                    last_activity, agent, node, parent_session_id, card_id, project_id, org_id
             FROM sessions WHERE session_id = ?1",
            [id],
            session_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn recent_messages(&self, session_id: &str, limit: usize) -> Result<Vec<MessageRow>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT message_id, role, content, tool_name, timestamp, token_count,
                    tool_calls, reasoning
             FROM (SELECT message_id, role, content, tool_name, timestamp, token_count,
                          tool_calls, reasoning
                   FROM messages WHERE session_id = ?1
                   ORDER BY message_id DESC LIMIT ?2)
             ORDER BY message_id",
        )?;
        let rows = stmt.query_map(params![session_id, limit as i64], message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn next_message_id(&self, session_id: &str) -> Result<u64> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        let next: i64 = conn.query_row(
            "SELECT COALESCE(MAX(message_id) + 1, 0) FROM messages WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )?;
        Ok(next as u64)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT f.session_id, CAST(f.message_id AS INTEGER),
                    snippet(messages_fts, 2, '<mark>', '</mark>', '…', 32),
                    -bm25(messages_fts), m.timestamp, COALESCE(s.source, '')
             FROM messages_fts f
             JOIN messages m ON m.session_id = f.session_id AND m.message_id = CAST(f.message_id AS INTEGER)
             LEFT JOIN sessions s ON s.session_id = f.session_id
             WHERE messages_fts MATCH ?1 ORDER BY bm25(messages_fts) LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit as i64], |row| {
            Ok(SearchHit {
                session_id: row.get(0)?,
                message_id: row.get::<_, i64>(1)? as u64,
                snippet: row.get(2)?,
                score: row.get::<_, f64>(3)? as f32,
                timestamp: row.get(4)?,
                source: row.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_setup(&self, scope: &str) -> Result<Option<SetupRow>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        conn.query_row(
            "SELECT scope, skills, mcp, plugins, hooks, declared_at FROM setup WHERE scope=?1",
            [scope],
            |row| {
                Ok(SetupRow {
                    scope: row.get(0)?,
                    skills: json_vec(row.get::<_, String>(1)?),
                    mcp: json_vec(row.get::<_, String>(2)?),
                    plugins: json_vec(row.get::<_, String>(3)?),
                    hooks: json_vec(row.get::<_, String>(4)?),
                    declared_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn effective_setup(&self, org: &str, project: &str) -> Result<SetupRow> {
        let org_scope = format!("org:{org}");
        let project_scope = format!("project:{org}/{project}");
        let org_row = self.get_setup(&org_scope)?;
        let project_row = self.get_setup(&project_scope)?;
        let merge = |pick: fn(&SetupRow) -> &Vec<String>| {
            let mut out = Vec::new();
            for row in [&org_row, &project_row]
                .into_iter()
                .filter_map(|r| r.as_ref())
            {
                for value in pick(row) {
                    if !out.contains(value) {
                        out.push(value.clone());
                    }
                }
            }
            out
        };
        Ok(SetupRow {
            scope: project_scope,
            skills: merge(|r| &r.skills),
            mcp: merge(|r| &r.mcp),
            plugins: merge(|r| &r.plugins),
            hooks: merge(|r| &r.hooks),
            declared_at: org_row
                .as_ref()
                .map_or(0.0, |r| r.declared_at)
                .max(project_row.as_ref().map_or(0.0, |r| r.declared_at)),
        })
    }

    pub fn list_registry(&self, kind: Option<&str>) -> Result<Vec<RegistryEntry>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        let sql = if kind.is_some() {
            "SELECT kind, slug, definition, registered_at FROM registry WHERE kind=?1 ORDER BY slug"
        } else {
            "SELECT kind, slug, definition, registered_at FROM registry ORDER BY kind, slug"
        };
        let mut stmt = conn.prepare(sql)?;
        let map = |row: &rusqlite::Row<'_>| {
            Ok(RegistryEntry {
                kind: row.get(0)?,
                slug: row.get(1)?,
                definition: row.get(2)?,
                registered_at: row.get(3)?,
            })
        };
        let rows = if let Some(kind) = kind {
            stmt.query_map([kind], map)?
        } else {
            stmt.query_map([], map)?
        };
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_registry(&self, kind: &str, slug: &str) -> Result<Option<RegistryEntry>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        conn.query_row(
            "SELECT kind, slug, definition, registered_at FROM registry WHERE kind=?1 AND slug=?2",
            params![kind, slug],
            |row| {
                Ok(RegistryEntry {
                    kind: row.get(0)?,
                    slug: row.get(1)?,
                    definition: row.get(2)?,
                    registered_at: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectRow>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        let mut stmt = conn.prepare("SELECT project_id,org_id,name,vaults,repos,boards,created_at,deleted_at FROM projects WHERE deleted_at IS NULL ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], project_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_project(&self, id: &str) -> Result<Option<ProjectRow>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        conn.query_row("SELECT project_id,org_id,name,vaults,repos,boards,created_at,deleted_at FROM projects WHERE project_id=?1 AND deleted_at IS NULL", [id], project_row).optional().map_err(Into::into)
    }

    pub fn list_repos(&self) -> Result<Vec<RepoRow>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        let mut stmt =
            conn.prepare("SELECT slug,url,default_branch,registered_at FROM repos ORDER BY slug")?;
        let rows = stmt.query_map([], repo_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_repo(&self, slug: &str) -> Result<Option<RepoRow>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        conn.query_row(
            "SELECT slug,url,default_branch,registered_at FROM repos WHERE slug=?1",
            [slug],
            repo_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_cards(&self) -> Result<Vec<CardRow>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        let mut stmt = conn.prepare("SELECT card_id,org_id,board_id,title,status,assigned_id,assigned_kind,current_session_id,current_bookmark,blocked_by,priority,attempts,created_at,status_changed_at FROM cards ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], card_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_card(&self, id: &str) -> Result<Option<CardRow>> {
        let conn = self.conn.lock().expect("SQLite mutex poisoned");
        conn.query_row("SELECT card_id,org_id,board_id,title,status,assigned_id,assigned_kind,current_session_id,current_bookmark,blocked_by,priority,attempts,created_at,status_changed_at FROM cards WHERE card_id=?1", [id], card_row).optional().map_err(Into::into)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub session_id: String,
    pub message_id: u64,
    pub snippet: String,
    pub score: f32,
    pub timestamp: f64,
    pub source: String,
}

fn append_in_tx(tx: &Transaction<'_>, event: &Event) -> Result<u64> {
    let encoded = serde_json::to_vec(event).context("encoding event as JSON")?;
    let payload = zstd::stream::encode_all(encoded.as_slice(), 3).context("compressing event")?;
    let sid = event_session_id(event);
    tx.execute(
        "INSERT INTO events(event_type,payload,created_at,session_id) VALUES(?1,?2,?3,?4)",
        params![event_type(event), payload, event_time(event), sid],
    )?;
    let seq = tx.last_insert_rowid() as u64;
    apply_projection(tx, event)?;
    Ok(seq)
}

fn decode_event_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(u64, Event)> {
    let seq = row.get::<_, i64>(0)? as u64;
    let payload: Vec<u8> = row.get(1)?;
    let decoded = zstd::stream::decode_all(payload.as_slice()).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            payload.len(),
            rusqlite::types::Type::Blob,
            Box::new(e),
        )
    })?;
    let event = serde_json::from_slice(&decoded).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            decoded.len(),
            rusqlite::types::Type::Blob,
            Box::new(e),
        )
    })?;
    Ok((seq, event))
}

/// One-shot storage migration from the original positional postcard payloads
/// to the self-describing JSON codec. This is encoding maintenance only: every
/// decoded event is compared with its JSON round-trip before its row is updated.
fn migrate_event_payloads_to_json(conn: &mut Connection) -> Result<()> {
    const CODEC_KEY: &str = "event_payload_codec";
    const JSON_CODEC: &str = "json+zstd-v1";

    let current: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key=?1", [CODEC_KEY], |row| {
            row.get(0)
        })
        .optional()?;
    if current.as_deref() == Some(JSON_CODEC) {
        return Ok(());
    }

    let tx = conn.transaction()?;
    let count_before: i64 = tx.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
    let mut last_seq = 0_i64;
    loop {
        let rows = {
            let mut stmt =
                tx.prepare("SELECT seq,payload FROM events WHERE seq>?1 ORDER BY seq LIMIT 512")?;
            let mapped = stmt.query_map([last_seq], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };
        if rows.is_empty() {
            break;
        }
        for (seq, payload) in rows {
            let legacy = zstd::stream::decode_all(payload.as_slice())
                .with_context(|| format!("decompressing legacy event {seq}"))?;
            let event: Event = postcard::from_bytes(&legacy)
                .with_context(|| format!("decoding legacy postcard event {seq}"))?;
            let json = serde_json::to_vec(&event)
                .with_context(|| format!("encoding migrated JSON event {seq}"))?;
            let decoded: Event = serde_json::from_slice(&json)
                .with_context(|| format!("verifying migrated JSON event {seq}"))?;
            anyhow::ensure!(
                event == decoded,
                "event {seq} changed during codec migration"
            );
            let migrated = zstd::stream::encode_all(json.as_slice(), 3)
                .with_context(|| format!("compressing migrated event {seq}"))?;
            tx.execute(
                "UPDATE events SET payload=?1 WHERE seq=?2",
                params![migrated, seq],
            )?;
            last_seq = seq;
        }
    }

    let count_after: i64 = tx.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
    anyhow::ensure!(
        count_before == count_after,
        "event count changed during codec migration: {count_before} -> {count_after}"
    );
    tx.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![CODEC_KEY, JSON_CODEC],
    )?;
    tx.commit().context("committing event payload migration")?;
    Ok(())
}

fn apply_projection(tx: &Transaction<'_>, event: &Event) -> Result<()> {
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
            agent,
            node,
        } => {
            tx.execute("INSERT OR REPLACE INTO sessions(session_id,hermes_id,source,model,title,started_at,message_count,input_tokens,output_tokens,archived,pinned,last_activity,agent,node,parent_session_id,card_id,project_id,org_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,0,?6,?10,?11,NULL,NULL,NULL,'personal')", params![session_id,hermes_id,source,model,title,started_at,*message_count as i64,*input_tokens as i64,*output_tokens as i64,agent,node])?;
        }
        Event::SessionOrganizationAssigned {
            session_id,
            organization_id,
        } => {
            tx.execute(
                "UPDATE sessions SET org_id=?2 WHERE session_id=?1",
                params![session_id, organization_id],
            )?;
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
            pinned,
        } => {
            tx.execute("UPDATE sessions SET title=COALESCE(?2,title),model=COALESCE(?3,model),archived=COALESCE(?4,archived),message_count=COALESCE(?5,message_count),agent=COALESCE(?6,agent),node=COALESCE(?7,node),hermes_id=COALESCE(?8,hermes_id),pinned=COALESCE(?9,pinned) WHERE session_id=?1", params![session_id,title,model,archived.map(i64::from),message_count.map(|v| v as i64),agent,node,hermes_id,pinned.map(i64::from)])?;
        }
        Event::MessageAppended {
            session_id,
            message_id,
            role,
            content,
            tool_name,
            tool_calls,
            reasoning,
            timestamp,
            token_count,
            finish_reason,
            ..
        } => {
            tx.execute("INSERT OR REPLACE INTO messages(session_id,message_id,role,content,tool_name,tool_calls,reasoning,timestamp,token_count,finish_reason) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![session_id,*message_id as i64,role,content,tool_name,tool_calls,reasoning,timestamp,token_count.map(|v| v as i64),finish_reason])?;
            tx.execute(
                "UPDATE sessions SET last_activity=MAX(last_activity,?2) WHERE session_id=?1",
                params![session_id, timestamp],
            )?;
        }
        Event::MessageRemoved {
            session_id,
            message_id,
            ..
        } => {
            tx.execute(
                "DELETE FROM messages WHERE session_id=?1 AND message_id=?2",
                params![session_id, *message_id as i64],
            )?;
        }
        Event::SetupDeclared {
            scope,
            skills,
            mcp,
            plugins,
            hooks,
            declared_at,
        } => {
            tx.execute(
                "INSERT OR REPLACE INTO setup VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    scope,
                    json(skills),
                    json(mcp),
                    json(plugins),
                    json(hooks),
                    declared_at
                ],
            )?;
        }
        Event::EntryRegistered {
            kind,
            slug,
            definition,
            registered_at,
        } => {
            tx.execute(
                "INSERT OR REPLACE INTO registry VALUES(?1,?2,?3,?4)",
                params![kind, slug, definition, registered_at],
            )?;
        }
        Event::RepoRegistered {
            slug,
            url,
            default_branch,
            registered_at,
        } => {
            tx.execute(
                "INSERT OR REPLACE INTO repos VALUES(?1,?2,?3,?4)",
                params![slug, url, default_branch, registered_at],
            )?;
        }
        Event::RepoRemoved { slug, .. } => {
            tx.execute("DELETE FROM repos WHERE slug=?1", [slug])?;
        }
        Event::SessionRepoAttached {
            session_id,
            slug,
            attached_at,
        } => {
            tx.execute(
                "INSERT OR IGNORE INTO session_repos VALUES(?1,?2,?3)",
                params![session_id, slug, attached_at],
            )?;
        }
        Event::ProjectCreated {
            project_id,
            name,
            created_at,
        } => {
            tx.execute(
                "INSERT OR REPLACE INTO projects(project_id,name,vaults,repos,boards,created_at,deleted_at,org_id) VALUES(?1,?2,'[]','[]','[]',?3,NULL,'personal')",
                params![project_id, name, created_at],
            )?;
        }
        Event::ProjectOrganizationAssigned {
            project_id,
            organization_id,
        } => {
            tx.execute(
                "UPDATE projects SET org_id=?2 WHERE project_id=?1",
                params![project_id, organization_id],
            )?;
        }
        Event::ProjectUpdated {
            project_id,
            name,
            vaults,
            repos,
            boards,
        } => {
            tx.execute("UPDATE projects SET name=COALESCE(?2,name),vaults=COALESCE(?3,vaults),repos=COALESCE(?4,repos),boards=COALESCE(?5,boards) WHERE project_id=?1", params![project_id,name,vaults.as_ref().map(json),repos.as_ref().map(json),boards.as_ref().map(json)])?;
        }
        Event::ProjectDeleted {
            project_id,
            deleted_at,
        } => {
            tx.execute(
                "UPDATE projects SET deleted_at=?2 WHERE project_id=?1",
                params![project_id, deleted_at],
            )?;
        }
        Event::SessionProjectAttached {
            session_id,
            project_id,
            ..
        } => {
            tx.execute(
                "UPDATE sessions SET project_id=?2 WHERE session_id=?1",
                params![session_id, project_id],
            )?;
        }
        Event::SessionForked {
            parent_session_id,
            child_session_id,
            ..
        } => {
            tx.execute("UPDATE sessions SET parent_session_id=?2, card_id=(SELECT card_id FROM sessions WHERE session_id=?2) WHERE session_id=?1", params![child_session_id,parent_session_id])?;
        }
        Event::CardSessionLinked {
            card_id,
            session_id,
            ..
        } => {
            tx.execute(
                "UPDATE sessions SET card_id=?2 WHERE session_id=?1 OR parent_session_id=?1",
                params![session_id, card_id],
            )?;
        }
        Event::SessionHandover {
            source_session_id,
            target_session_id,
            ..
        } => {
            tx.execute("UPDATE sessions SET parent_session_id=?2, card_id=(SELECT card_id FROM sessions WHERE session_id=?2) WHERE session_id=?1", params![target_session_id,source_session_id])?;
        }
        Event::CardCreated {
            card_id,
            board_id,
            title,
            created_at,
        } => {
            tx.execute("INSERT OR REPLACE INTO cards(card_id,board_id,title,status,assigned_id,assigned_kind,current_session_id,current_bookmark,blocked_by,priority,attempts,created_at,status_changed_at,org_id) VALUES(?1,?2,?3,'todo',NULL,NULL,NULL,NULL,'[]',0,'[]',?4,?4,'personal')", params![card_id,board_id,title,created_at])?;
        }
        Event::CardOrganizationAssigned {
            card_id,
            organization_id,
        } => {
            tx.execute(
                "UPDATE cards SET org_id=?2 WHERE card_id=?1",
                params![card_id, organization_id],
            )?;
        }
        Event::CardAssigned {
            card_id,
            assigned_id,
            assigned_kind,
            session_id,
            attempt_bookmark,
            assigned_at,
        } => update_card_attempt(
            tx,
            card_id,
            assigned_id,
            assigned_kind,
            session_id,
            attempt_bookmark,
            *assigned_at,
            None,
        )?,
        Event::CardClaimed {
            card_id,
            claimed_at,
        } => {
            tx.execute(
                "UPDATE cards SET status='claimed',status_changed_at=?2 WHERE card_id=?1",
                params![card_id, claimed_at],
            )?;
        }
        Event::CardBlocked {
            card_id,
            blocked_by,
            blocked_at,
        } => {
            tx.execute("UPDATE cards SET status='blocked',blocked_by=?2,status_changed_at=?3 WHERE card_id=?1", params![card_id,json(blocked_by),blocked_at])?;
        }
        Event::CardCompleted {
            card_id,
            completed_at,
        } => {
            tx.execute(
                "UPDATE cards SET status='done',status_changed_at=?2 WHERE card_id=?1",
                params![card_id, completed_at],
            )?;
        }
        Event::CardReassigned {
            card_id,
            assigned_id,
            assigned_kind,
            session_id,
            attempt_bookmark,
            previous_session_id,
            reassigned_at,
        } => update_card_attempt(
            tx,
            card_id,
            assigned_id,
            assigned_kind,
            session_id,
            attempt_bookmark,
            *reassigned_at,
            Some(previous_session_id),
        )?,
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn update_card_attempt(
    tx: &Transaction<'_>,
    card_id: &str,
    assigned_id: &str,
    assigned_kind: &str,
    session_id: &str,
    bookmark: &str,
    at: f64,
    previous: Option<&String>,
) -> Result<()> {
    let raw: String = tx
        .query_row(
            "SELECT attempts FROM cards WHERE card_id=?1",
            [card_id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_else(|| "[]".into());
    let mut attempts: Vec<CardAttempt> = serde_json::from_str(&raw).unwrap_or_default();
    if let Some(previous) = previous {
        for attempt in &mut attempts {
            if &attempt.session_id == previous && attempt.ended_at.is_none() {
                attempt.ended_at = Some(at);
                attempt.outcome = "reassigned".into();
            }
        }
    }
    attempts.push(CardAttempt {
        session_id: session_id.into(),
        assigned_id: assigned_id.into(),
        bookmark: bookmark.into(),
        started_at: at,
        ended_at: None,
        outcome: "running".into(),
    });
    tx.execute("UPDATE cards SET assigned_id=?2,assigned_kind=?3,current_session_id=?4,current_bookmark=?5,status='assigned',attempts=?6,status_changed_at=?7 WHERE card_id=?1", params![card_id,assigned_id,assigned_kind,session_id,bookmark,serde_json::to_string(&attempts)?,at])?;
    Ok(())
}

fn json<T: serde::Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".into())
}
fn json_vec(raw: String) -> Vec<String> {
    serde_json::from_str(&raw).unwrap_or_default()
}
fn event_type(event: &Event) -> &'static str {
    match event {
        Event::SessionCreated { .. } => "session.created",
        Event::SessionUpdated { .. } => "session.updated",
        Event::MessageAppended { .. } => "message.appended",
        Event::MessageRemoved { .. } => "message.removed",
        Event::CardCreated { .. } => "card.created",
        Event::CardAssigned { .. } => "card.assigned",
        Event::CardClaimed { .. } => "card.claimed",
        Event::CardBlocked { .. } => "card.blocked",
        Event::CardCompleted { .. } => "card.completed",
        Event::CardReassigned { .. } => "card.reassigned",
        Event::SessionForked { .. } => "session.forked",
        Event::CardSessionLinked { .. } => "card.linked",
        Event::SessionHandover { .. } => "session.handover",
        Event::SetupDeclared { .. } => "setup.declared",
        Event::EntryRegistered { .. } => "registry.registered",
        Event::RepoRegistered { .. } => "repo.registered",
        Event::RepoRemoved { .. } => "repo.removed",
        Event::SessionRepoAttached { .. } => "session.repo_attached",
        Event::ProjectCreated { .. } => "project.created",
        Event::ProjectUpdated { .. } => "project.updated",
        Event::ProjectDeleted { .. } => "project.deleted",
        Event::SessionProjectAttached { .. } => "session.project_attached",
        Event::SessionOrganizationAssigned { .. } => "session.organization_assigned",
        Event::ProjectOrganizationAssigned { .. } => "project.organization_assigned",
        Event::CardOrganizationAssigned { .. } => "card.organization_assigned",
    }
}
fn event_time(event: &Event) -> f64 {
    match event {
        Event::SessionCreated { started_at, .. } => *started_at,
        Event::MessageAppended { timestamp, .. } => *timestamp,
        Event::CardCreated { created_at, .. } => *created_at,
        Event::CardAssigned { assigned_at, .. } => *assigned_at,
        Event::CardClaimed { claimed_at, .. } => *claimed_at,
        Event::CardBlocked { blocked_at, .. } => *blocked_at,
        Event::CardCompleted { completed_at, .. } => *completed_at,
        Event::CardReassigned { reassigned_at, .. } => *reassigned_at,
        Event::SessionForked { forked_at, .. } => *forked_at,
        Event::CardSessionLinked { linked_at, .. } => *linked_at,
        Event::SessionHandover { handed_over_at, .. } => *handed_over_at,
        Event::SetupDeclared { declared_at, .. } => *declared_at,
        Event::EntryRegistered { registered_at, .. } => *registered_at,
        Event::RepoRegistered { registered_at, .. } => *registered_at,
        Event::RepoRemoved { removed_at, .. } => *removed_at,
        Event::SessionRepoAttached { attached_at, .. } => *attached_at,
        Event::ProjectCreated { created_at, .. } => *created_at,
        Event::ProjectDeleted { deleted_at, .. } => *deleted_at,
        Event::SessionProjectAttached { attached_at, .. } => *attached_at,
        _ => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64()),
    }
}

/// Extract the session_id from session-scoped events, or `None` for
/// unconditionally-native events (setup, registry, card, project, repo).
/// Used to populate the `events.session_id` column so `retain_native` can
/// be a pure SQL DELETE without deserializing any event payload into RAM.
fn event_session_id(event: &Event) -> Option<&str> {
    match event {
        Event::SessionCreated { session_id, .. }
        | Event::SessionUpdated { session_id, .. }
        | Event::MessageAppended { session_id, .. }
        | Event::MessageRemoved { session_id, .. }
        | Event::CardSessionLinked { session_id, .. }
        | Event::SessionRepoAttached { session_id, .. }
        | Event::SessionProjectAttached { session_id, .. }
        | Event::SessionOrganizationAssigned { session_id, .. } => Some(session_id),
        Event::CardAssigned { session_id, .. } => Some(session_id),
        Event::SessionForked {
            child_session_id, ..
        } => Some(child_session_id),
        Event::SessionHandover {
            target_session_id, ..
        } => Some(target_session_id),
        // Unconditionally native — no session_id column.
        _ => None,
    }
}

fn session_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        session_id: r.get(0)?,
        hermes_id: r.get(1)?,
        source: r.get(2)?,
        model: r.get(3)?,
        title: r.get(4)?,
        started_at: r.get(5)?,
        message_count: r.get::<_, i64>(6)? as u64,
        input_tokens: r.get::<_, i64>(7)? as u64,
        output_tokens: r.get::<_, i64>(8)? as u64,
        archived: r.get::<_, i64>(9)? != 0,
        pinned: r.get::<_, i64>(10)? != 0,
        last_activity: r.get(11)?,
        agent: r.get(12)?,
        node: r.get(13)?,
        parent_session_id: r.get(14)?,
        card_id: r.get(15)?,
        project_id: r.get(16)?,
        org_id: r.get(17)?,
    })
}
fn message_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRow> {
    Ok(MessageRow {
        message_id: r.get::<_, i64>(0)? as u64,
        role: r.get(1)?,
        content: r.get(2)?,
        tool_name: r.get(3)?,
        timestamp: r.get(4)?,
        token_count: r.get::<_, Option<i64>>(5)?.map(|v| v as u64),
        tool_calls: r.get(6)?,
        reasoning: r.get(7)?,
    })
}
fn project_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectRow> {
    Ok(ProjectRow {
        project_id: r.get(0)?,
        org_id: r.get(1)?,
        name: r.get(2)?,
        vaults: json_vec(r.get(3)?),
        repos: json_vec(r.get(4)?),
        boards: json_vec(r.get(5)?),
        created_at: r.get(6)?,
        deleted_at: r.get(7)?,
    })
}
fn repo_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RepoRow> {
    Ok(RepoRow {
        slug: r.get(0)?,
        url: r.get(1)?,
        default_branch: r.get(2)?,
        registered_at: r.get(3)?,
    })
}
fn card_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CardRow> {
    Ok(CardRow {
        card_id: r.get(0)?,
        org_id: r.get(1)?,
        board_id: r.get(2)?,
        title: r.get(3)?,
        status: r.get(4)?,
        assigned_id: r.get(5)?,
        assigned_kind: r.get(6)?,
        current_session_id: r.get(7)?,
        current_bookmark: r.get(8)?,
        blocked_by: json_vec(r.get(9)?),
        priority: r.get(10)?,
        attempts: serde_json::from_str(&r.get::<_, String>(11)?).unwrap_or_default(),
        created_at: r.get(12)?,
        status_changed_at: r.get(13)?,
    })
}

const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA cache_size=-4096; PRAGMA temp_store=MEMORY;
CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,event_type TEXT NOT NULL,payload BLOB NOT NULL,created_at REAL NOT NULL,session_id TEXT);
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id);
CREATE TABLE IF NOT EXISTS sessions(session_id TEXT PRIMARY KEY,hermes_id TEXT NOT NULL DEFAULT '',source TEXT NOT NULL DEFAULT '',model TEXT,title TEXT,started_at REAL NOT NULL,message_count INTEGER NOT NULL DEFAULT 0,input_tokens INTEGER NOT NULL DEFAULT 0,output_tokens INTEGER NOT NULL DEFAULT 0,archived INTEGER NOT NULL DEFAULT 0,pinned INTEGER NOT NULL DEFAULT 0,last_activity REAL NOT NULL DEFAULT 0,agent TEXT,node TEXT,parent_session_id TEXT,card_id TEXT,project_id TEXT,org_id TEXT NOT NULL DEFAULT 'personal');
CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at DESC); CREATE INDEX IF NOT EXISTS idx_sessions_source ON sessions(source); CREATE INDEX IF NOT EXISTS idx_sessions_archived ON sessions(archived); CREATE INDEX IF NOT EXISTS idx_sessions_pinned ON sessions(pinned);
CREATE TABLE IF NOT EXISTS messages(session_id TEXT NOT NULL,message_id INTEGER NOT NULL,role TEXT NOT NULL,content TEXT,tool_name TEXT,tool_calls TEXT,reasoning TEXT,timestamp REAL NOT NULL,token_count INTEGER,finish_reason TEXT,PRIMARY KEY(session_id,message_id)) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS idx_messages_ts ON messages(session_id,timestamp);
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(session_id UNINDEXED,message_id UNINDEXED,content,role UNINDEXED,tool_name UNINDEXED,timestamp UNINDEXED,tokenize='porter unicode61');
CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages BEGIN INSERT INTO messages_fts(session_id,message_id,content,role,tool_name,timestamp) VALUES(new.session_id,new.message_id,new.content,new.role,new.tool_name,new.timestamp); END;
CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages BEGIN DELETE FROM messages_fts WHERE session_id=old.session_id AND message_id=old.message_id; END;
CREATE TRIGGER IF NOT EXISTS messages_fts_update AFTER UPDATE ON messages BEGIN DELETE FROM messages_fts WHERE session_id=old.session_id AND message_id=old.message_id; INSERT INTO messages_fts(session_id,message_id,content,role,tool_name,timestamp) VALUES(new.session_id,new.message_id,new.content,new.role,new.tool_name,new.timestamp); END;
CREATE TABLE IF NOT EXISTS cards(card_id TEXT PRIMARY KEY,board_id TEXT NOT NULL,title TEXT NOT NULL,status TEXT NOT NULL,assigned_id TEXT,assigned_kind TEXT,current_session_id TEXT,current_bookmark TEXT,blocked_by TEXT NOT NULL DEFAULT '[]',priority INTEGER NOT NULL DEFAULT 0,attempts TEXT NOT NULL DEFAULT '[]',created_at REAL NOT NULL,status_changed_at REAL NOT NULL,org_id TEXT NOT NULL DEFAULT 'personal');
CREATE TABLE IF NOT EXISTS setup(scope TEXT PRIMARY KEY,skills TEXT NOT NULL,mcp TEXT NOT NULL,plugins TEXT NOT NULL,hooks TEXT NOT NULL,declared_at REAL NOT NULL);
CREATE TABLE IF NOT EXISTS registry(kind TEXT NOT NULL,slug TEXT NOT NULL,definition TEXT NOT NULL,registered_at REAL NOT NULL,PRIMARY KEY(kind,slug));
CREATE TABLE IF NOT EXISTS projects(project_id TEXT PRIMARY KEY,name TEXT NOT NULL,vaults TEXT NOT NULL DEFAULT '[]',repos TEXT NOT NULL DEFAULT '[]',boards TEXT NOT NULL DEFAULT '[]',created_at REAL NOT NULL,deleted_at REAL,org_id TEXT NOT NULL DEFAULT 'personal');
CREATE TABLE IF NOT EXISTS repos(slug TEXT PRIMARY KEY,url TEXT NOT NULL,default_branch TEXT NOT NULL,registered_at REAL NOT NULL);
CREATE TABLE IF NOT EXISTS session_repos(session_id TEXT NOT NULL,slug TEXT NOT NULL,attached_at REAL NOT NULL,PRIMARY KEY(session_id,slug));
CREATE TABLE IF NOT EXISTS envoy_watermarks(session_id TEXT PRIMARY KEY,seq INTEGER NOT NULL) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS observed_sessions(hermes_id TEXT PRIMARY KEY) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS observed_messages(hermes_id TEXT NOT NULL,message_id INTEGER NOT NULL,PRIMARY KEY(hermes_id,message_id)) WITHOUT ROWID;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_event() -> Event {
        Event::SessionCreated {
            session_id: "fixture-session".into(),
            hermes_id: "fixture-hermes".into(),
            source: "olympus".into(),
            model: Some("fixture-model".into()),
            title: Some("legacy postcard fixture".into()),
            started_at: 1.0,
            message_count: 0,
            input_tokens: 1,
            output_tokens: 2,
            agent: None,
            node: None,
        }
    }

    #[test]
    fn open_migrates_postcard_payloads_to_json_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("olympus.db");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(SCHEMA).unwrap();
        let event = fixture_event();
        let legacy = postcard::to_allocvec(&event).unwrap();
        let legacy = zstd::stream::encode_all(legacy.as_slice(), 3).unwrap();
        connection
            .execute(
                "INSERT INTO events(event_type,payload,created_at,session_id) VALUES(?1,?2,?3,?4)",
                params![
                    event_type(&event),
                    legacy,
                    event_time(&event),
                    event_session_id(&event)
                ],
            )
            .unwrap();
        let before: i64 = connection
            .query_row("SELECT length(payload) FROM events", [], |row| row.get(0))
            .unwrap();
        drop(connection);

        let log = Log::open(&path).unwrap();
        assert_eq!(log.read_all().unwrap(), vec![(1, event.clone())]);
        drop(log);

        let connection = Connection::open(&path).unwrap();
        let migrated: Vec<u8> = connection
            .query_row("SELECT payload FROM events", [], |row| row.get(0))
            .unwrap();
        let json = zstd::stream::decode_all(migrated.as_slice()).unwrap();
        assert_eq!(serde_json::from_slice::<Event>(&json).unwrap(), event);
        assert_eq!(
            connection
                .query_row(
                    "SELECT value FROM meta WHERE key='event_payload_codec'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "json+zstd-v1"
        );
        let after = migrated.len() as i64;
        drop(connection);

        drop(Log::open(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        let after_second_open: i64 = connection
            .query_row("SELECT length(payload) FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(after, after_second_open);
        eprintln!("fixture event payload bytes: postcard+zstd={before}, json+zstd={after}");
    }

    #[test]
    fn open_migrates_sessions_without_organization_column() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("olympus.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE sessions(
                    session_id TEXT PRIMARY KEY,
                    hermes_id TEXT NOT NULL DEFAULT '',
                    source TEXT NOT NULL DEFAULT '',
                    model TEXT,
                    title TEXT,
                    started_at REAL NOT NULL,
                    message_count INTEGER NOT NULL DEFAULT 0,
                    input_tokens INTEGER NOT NULL DEFAULT 0,
                    output_tokens INTEGER NOT NULL DEFAULT 0,
                    archived INTEGER NOT NULL DEFAULT 0,
                    pinned INTEGER NOT NULL DEFAULT 0,
                    last_activity REAL NOT NULL DEFAULT 0,
                    agent TEXT,
                    node TEXT,
                    parent_session_id TEXT,
                    card_id TEXT,
                    project_id TEXT
                 );
                 INSERT INTO sessions(session_id, started_at) VALUES ('legacy', 1.0);",
            )
            .unwrap();
        drop(connection);

        let log = Log::open(&path).unwrap();
        assert_eq!(
            log.get_session("legacy").unwrap().unwrap().org_id,
            "personal"
        );
    }

    #[test]
    fn append_is_atomic_and_queries_projection() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::open(&dir.path().join("olympus.db")).unwrap();
        log.append(&Event::SessionCreated {
            session_id: "s".into(),
            hermes_id: "h".into(),
            source: "olympus".into(),
            model: None,
            title: Some("test".into()),
            started_at: 1.0,
            message_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            agent: None,
            node: None,
        })
        .unwrap();
        log.append(&Event::SessionOrganizationAssigned {
            session_id: "s".into(),
            organization_id: "org-a".into(),
        })
        .unwrap();
        log.append(&Event::MessageAppended {
            session_id: "s".into(),
            hermes_session_id: "h".into(),
            message_id: 0,
            role: "user".into(),
            content: Some("hello sqlite world".into()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: 2.0,
            token_count: None,
            finish_reason: None,
        })
        .unwrap();
        assert_eq!(log.event_count().unwrap(), 3);
        let session = log.get_session("s").unwrap().unwrap();
        assert_eq!(session.last_activity, 2.0);
        assert_eq!(session.org_id, "org-a");
        assert_eq!(log.recent_messages("s", 50).unwrap().len(), 1);
        assert_eq!(log.search("sqlite", 10).unwrap().len(), 1);
    }
}

// ── migration fixture tests ──────────────────────────────────────────────────
//
// These tests live outside the inner `mod tests` block so they can reference
// the private module-level helpers (event_type, event_session_id, event_time,
// SCHEMA) directly.  The postcard crate is a production dep (required by the
// migration path) and is vendored here as a minimal `encode_legacy` helper
// rather than being called inline — keeping the encode concern in one place.

#[cfg(test)]
fn encode_legacy_for_test(event: &Event) -> Vec<u8> {
    let postcard_bytes = postcard::to_allocvec(event).expect("postcard encode failed");
    zstd::stream::encode_all(postcard_bytes.as_slice(), 3).expect("zstd compress failed")
}

#[cfg(test)]
fn insert_legacy(conn: &Connection, event: &Event) {
    let payload = encode_legacy_for_test(event);
    conn.execute(
        "INSERT INTO events(event_type,payload,created_at,session_id) VALUES(?1,?2,?3,?4)",
        params![
            event_type(event),
            payload,
            event_time(event),
            event_session_id(event),
        ],
    )
    .expect("insert legacy event");
}

#[cfg(test)]
fn representative_fixture_events() -> Vec<Event> {
    vec![
        Event::SessionCreated {
            session_id: "fix-sess".into(),
            hermes_id: "fix-h".into(),
            source: "olympus".into(),
            model: Some("fix-model".into()),
            title: Some("fixture session".into()),
            started_at: 1_700_000_000.0,
            message_count: 0,
            input_tokens: 10,
            output_tokens: 20,
            agent: None,
            node: None,
        },
        Event::MessageAppended {
            session_id: "fix-sess".into(),
            hermes_session_id: "fix-h".into(),
            message_id: 0,
            role: "user".into(),
            content: Some("hello from fixture".into()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: 1_700_000_001.0,
            token_count: Some(4),
            finish_reason: None,
        },
        Event::CardCreated {
            card_id: "fix-card".into(),
            board_id: "fix-board".into(),
            title: "Fixture card".into(),
            created_at: 1_700_000_002.0,
        },
        Event::SetupDeclared {
            scope: "org:fixture".into(),
            skills: vec!["code-review".into()],
            mcp: vec![],
            plugins: vec![],
            hooks: vec![],
            declared_at: 1_700_000_003.0,
        },
        Event::EntryRegistered {
            kind: "mcp".into(),
            slug: "fix-mcp".into(),
            definition: r#"{"command":"fix","args":[]}"#.into(),
            registered_at: 1_700_000_004.0,
        },
        Event::RepoRegistered {
            slug: "fix-repo".into(),
            url: "https://github.com/fix/repo".into(),
            default_branch: "main".into(),
            registered_at: 1_700_000_005.0,
        },
    ]
}

#[test]
fn migration_multi_event_fixture_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("olympus.db");

    let expected_events = representative_fixture_events();
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        for ev in &expected_events {
            insert_legacy(&conn, ev);
        }
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, expected_events.len() as i64);
        let marker: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key='event_payload_codec'",
                [],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        assert!(marker.is_none(), "marker must not be set before migration");
    }

    let log = Log::open(&path).unwrap();

    assert_eq!(log.event_count().unwrap(), expected_events.len());

    let stored: Vec<(u64, Event)> = log.read_all().unwrap();
    assert_eq!(stored.len(), expected_events.len());
    for (i, ((seq, decoded), original)) in stored.iter().zip(expected_events.iter()).enumerate() {
        assert!(*seq > 0, "seq must be positive at index {i}");
        assert_eq!(
            decoded, original,
            "event at index {i} (seq {seq}) changed during migration"
        );
    }

    drop(log);

    let conn = Connection::open(&path).unwrap();
    {
        let mut stmt = conn
            .prepare("SELECT seq, payload FROM events ORDER BY seq")
            .unwrap();
        let rows: Vec<(i64, Vec<u8>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(rows.len(), expected_events.len());
        for (i, (seq, payload)) in rows.iter().enumerate() {
            let json = zstd::stream::decode_all(payload.as_slice())
                .unwrap_or_else(|e| panic!("seq {seq}: zstd decompress failed: {e}"));
            let _: Event = serde_json::from_slice(&json).unwrap_or_else(|e| {
                panic!(
                    "seq {seq} (index {i}): json decode failed: {e}\nraw: {}",
                    String::from_utf8_lossy(&json)
                )
            });
        }
    }

    let marker: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key='event_payload_codec'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(marker, "json+zstd-v1");
}

#[test]
fn migration_is_idempotent_on_second_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("olympus.db");

    let events = representative_fixture_events();
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        for ev in &events {
            insert_legacy(&conn, ev);
        }
    }

    let log = Log::open(&path).unwrap();
    drop(log);

    let payloads_after_first: Vec<Vec<u8>> = {
        let conn = Connection::open(&path).unwrap();
        let mut stmt = conn
            .prepare("SELECT payload FROM events ORDER BY seq")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };

    let log2 = Log::open(&path).unwrap();
    assert_eq!(log2.event_count().unwrap(), events.len());
    drop(log2);

    let payloads_after_second: Vec<Vec<u8>> = {
        let conn = Connection::open(&path).unwrap();
        let mut stmt = conn
            .prepare("SELECT payload FROM events ORDER BY seq")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };

    assert_eq!(
        payloads_after_first, payloads_after_second,
        "payload bytes changed on second open — migration is not idempotent"
    );
}

#[test]
fn fresh_database_migration_is_no_op_and_sets_marker() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("olympus.db");

    let log = Log::open(&path).unwrap();
    assert_eq!(
        log.event_count().unwrap(),
        0,
        "fresh DB must have zero events"
    );
    drop(log);

    let conn = Connection::open(&path).unwrap();
    let marker: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key='event_payload_codec'",
            [],
            |r| r.get(0),
        )
        .expect("meta marker must be present after opening a fresh DB");
    assert_eq!(
        marker, "json+zstd-v1",
        "fresh DB must have json+zstd-v1 marker set"
    );
}

// ── size + throughput measurement fixtures ────────────────────────────────────
//
// These tests produce human-readable output (--nocapture) and the results are
// recorded in docs/codec-size-and-batch-throughput-measurement.md.

/// Measures total payload bytes immediately before and after the one-shot
/// postcard+zstd → json+zstd migration using the representative fixture set.
/// Prints a structured report to stderr and flags if the ratio exceeds 1.5x.
/// Hard-asserts ratio < 3.0 to catch catastrophic regressions.
#[test]
fn codec_size_ratio_fixture() {
    let events = representative_fixture_events();
    let n = events.len();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("olympus.db");

    // Phase 1 — build legacy postcard+zstd database.
    let before_bytes: i64 = {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        for ev in &events {
            insert_legacy(&conn, ev);
        }
        conn.query_row(
            "SELECT COALESCE(SUM(length(payload)), 0) FROM events",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };

    // Phase 2 — open Log to trigger migration, then measure again.
    let log = Log::open(&path).unwrap();
    drop(log);

    let after_bytes: i64 = {
        let conn = Connection::open(&path).unwrap();
        conn.query_row(
            "SELECT COALESCE(SUM(length(payload)), 0) FROM events",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };

    let ratio = after_bytes as f64 / before_bytes.max(1) as f64;
    let flag = if ratio > 1.5 {
        "  *** EXCEEDS 1.5x ***"
    } else {
        ""
    };
    eprintln!(
        "\n=== codec_size_ratio_fixture ===\n\
         events       : {n}\n\
         postcard+zstd: {before_bytes} B  ({per_before} B/event)\n\
         json+zstd    : {after_bytes} B  ({per_after} B/event)\n\
         ratio        : {ratio:.3}{flag}\n",
        per_before = before_bytes / n.max(1) as i64,
        per_after = after_bytes / n.max(1) as i64,
    );

    assert!(
        ratio < 3.0,
        "json+zstd payload exceeds 3x postcard+zstd for fixture events (ratio={ratio:.2})"
    );
}

/// Exercises the existing append_batch path with a 100-event workload.
/// Verifies:
///   - All 100 events land correctly (count + content round-trip).
///   - Returned seqs are strictly consecutive, proving the entire batch was
///     committed in a single SQLite transaction with no interleaving gaps.
///   - The same 100 events appended one-by-one via append() produce an
///     identical result, confirming parity between the two code paths.
///
/// Timing is printed for informational evidence; no wall-clock assertions
/// are made so the test is not brittle under load.
#[test]
fn batch_append_throughput_fixture() {
    use std::time::Instant;

    let n: usize = 100;
    let events: Vec<Event> = (0..n)
        .map(|i| Event::SessionCreated {
            session_id: format!("batch-sess-{i:04}"),
            hermes_id: format!("hermes-{i:04}"),
            source: "cli".into(),
            model: Some("test-model".into()),
            title: Some(format!("Batch session {i}")),
            started_at: 1_700_000_000.0 + i as f64,
            message_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            agent: None,
            node: None,
        })
        .collect();

    // ── sequential baseline ──────────────────────────────────────────────────
    let seq_dir = tempfile::tempdir().unwrap();
    let seq_log = Log::open(&seq_dir.path().join("seq.db")).unwrap();
    let t_seq = Instant::now();
    for ev in &events {
        seq_log.append(ev).unwrap();
    }
    let elapsed_seq = t_seq.elapsed();
    assert_eq!(
        seq_log.event_count().unwrap(),
        n,
        "sequential: wrong event count"
    );
    let seq_stored = seq_log.read_all().unwrap();
    assert_eq!(seq_stored.len(), n);

    // ── batch path ───────────────────────────────────────────────────────────
    let batch_dir = tempfile::tempdir().unwrap();
    let batch_log = Log::open(&batch_dir.path().join("batch.db")).unwrap();
    let t_batch = Instant::now();
    let first_seq = batch_log.append_batch(&events).unwrap();
    let elapsed_batch = t_batch.elapsed();

    // Correctness: all events present.
    assert_eq!(
        batch_log.event_count().unwrap(),
        n,
        "batch: wrong event count"
    );
    assert!(
        first_seq.is_some(),
        "append_batch returned None for non-empty input"
    );

    let batch_stored = batch_log.read_all().unwrap();
    assert_eq!(
        batch_stored.len(),
        n,
        "read_all must return all batched events"
    );
    for (i, ((seq, got), want)) in batch_stored.iter().zip(events.iter()).enumerate() {
        assert!(*seq > 0, "seq must be positive at index {i}");
        assert_eq!(
            got, want,
            "event at index {i} (seq {seq}) changed after batch append"
        );
    }

    // Structural proof of single-transaction batch: seqs must be strictly
    // consecutive with no gaps.  A split across N separate transactions could
    // in theory still be consecutive (no concurrent writer), but a gap would
    // be unambiguous evidence of a broken batch.
    let seqs: Vec<u64> = batch_stored.iter().map(|(s, _)| *s).collect();
    for w in seqs.windows(2) {
        assert_eq!(
            w[1],
            w[0] + 1,
            "seq gap {}->{}: batch commit may not be single-transaction",
            w[0],
            w[1]
        );
    }

    // Content parity between sequential and batch paths.
    let seq_events: Vec<&Event> = seq_stored.iter().map(|(_, e)| e).collect();
    let batch_events: Vec<&Event> = batch_stored.iter().map(|(_, e)| e).collect();
    assert_eq!(
        seq_events, batch_events,
        "sequential and batch paths produced different events"
    );

    let seq_us = elapsed_seq.as_micros();
    let batch_us = elapsed_batch.as_micros();
    let speedup = seq_us as f64 / batch_us.max(1) as f64;
    eprintln!(
        "\n=== batch_append_throughput_fixture ===\n\
         events     : {n}\n\
         sequential : {seq_us} µs  ({seq_per:.1} µs/event)\n\
         batch      : {batch_us} µs  ({batch_per:.1} µs/event)\n\
         speedup    : {speedup:.2}x\n\
         (timing is informational — no wall-clock assertions)\n",
        seq_per = seq_us as f64 / n as f64,
        batch_per = batch_us as f64 / n as f64,
    );
}
