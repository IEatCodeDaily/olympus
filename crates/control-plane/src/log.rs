//! SQLite-backed event log and durable projections (ADR 0009).
//!
//! Every append writes the immutable event and applies its projection in the
//! same WAL transaction. Message history and FTS stay on disk; callers page it
//! on demand instead of retaining decompressed messages in process memory.

use std::collections::HashSet;
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
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)
            .context("initializing Olympus SQLite schema")?;
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

    /// Import the old redb log once, keeping ONLY Olympus-native events — the
    /// imported Hermes state.db mirror is rebuilt from state.db on every boot
    /// (see main.rs), so copying those events here would only be deleted again
    /// by `retain_native` (at the cost of ~1GB of writes and 137K FTS trigger
    /// deletes). Pages in bounded chunks so history is never resident in RAM.
    ///
    /// Runs only when the SQLite events table is empty (one-shot). Events are
    /// scanned in seq order, so a session's `SessionCreated` (which carries the
    /// `source`) is always seen before its messages.
    pub fn migrate_from_redb(&self, path: &Path) -> Result<usize> {
        if !path.exists() || self.event_count()? > 0 {
            return Ok(0);
        }
        let legacy = crate::legacy_log::Log::open(path).context("opening legacy redb log")?;
        const CHUNK: usize = 2_000;
        let mut native_sessions: HashSet<String> = HashSet::new();
        let mut migrated = 0usize;
        let mut next_seq = 0u64;
        loop {
            let page = legacy.read_from(next_seq, CHUNK)?;
            if page.is_empty() {
                break;
            }
            next_seq = page.last().map(|(s, _)| s + 1).unwrap_or(next_seq);
            let mut keep = Vec::new();
            for (_, event) in page {
                if let Event::SessionCreated {
                    session_id, source, ..
                } = &event
                {
                    if source == "olympus" {
                        native_sessions.insert(session_id.clone());
                    }
                }
                if is_native(&event, &native_sessions) {
                    keep.push(event);
                }
            }
            if !keep.is_empty() {
                self.append_batch(&keep)?;
                migrated += keep.len();
            }
        }
        Ok(migrated)
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
             SELECT session_id FROM sessions WHERE source = 'olympus';",
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
    let encoded = postcard::to_allocvec(event).context("encoding event")?;
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
    let event = postcard::from_bytes(&decoded).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            decoded.len(),
            rusqlite::types::Type::Blob,
            Box::new(e),
        )
    })?;
    Ok((seq, event))
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
fn is_native(event: &Event, native: &HashSet<String>) -> bool {
    match event {
        Event::SetupDeclared { .. }
        | Event::EntryRegistered { .. }
        | Event::CardCreated { .. }
        | Event::CardAssigned { .. }
        | Event::CardClaimed { .. }
        | Event::CardBlocked { .. }
        | Event::CardCompleted { .. }
        | Event::CardReassigned { .. }
        | Event::SessionForked { .. }
        | Event::CardSessionLinked { .. }
        | Event::SessionHandover { .. }
        | Event::RepoRegistered { .. }
        | Event::RepoRemoved { .. }
        | Event::SessionRepoAttached { .. }
        | Event::ProjectCreated { .. }
        | Event::ProjectUpdated { .. }
        | Event::ProjectDeleted { .. }
        | Event::SessionProjectAttached { .. }
        | Event::ProjectOrganizationAssigned { .. }
        | Event::CardOrganizationAssigned { .. } => true,
        Event::SessionCreated { source, .. } => source == "olympus",
        Event::SessionUpdated { session_id, .. }
        | Event::MessageAppended { session_id, .. }
        | Event::MessageRemoved { session_id, .. }
        | Event::SessionOrganizationAssigned { session_id, .. } => native.contains(session_id),
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
"#;

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn migrates_legacy_redb_once_and_preserves_event_order() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_path = dir.path().join("eventlog.redb");
        let legacy = crate::legacy_log::Log::open(&legacy_path).unwrap();
        legacy
            .append(&Event::SessionCreated {
                session_id: "legacy".into(),
                hermes_id: "h-legacy".into(),
                source: "olympus".into(),
                model: None,
                title: Some("migrated".into()),
                started_at: 1.0,
                message_count: 0,
                input_tokens: 0,
                output_tokens: 0,
                agent: None,
                node: None,
            })
            .unwrap();
        drop(legacy);

        let log = Log::open(&dir.path().join("olympus.db")).unwrap();
        assert_eq!(log.migrate_from_redb(&legacy_path).unwrap(), 1);
        assert_eq!(log.migrate_from_redb(&legacy_path).unwrap(), 0);
        assert_eq!(
            log.get_session("legacy").unwrap().unwrap().title.as_deref(),
            Some("migrated")
        );
    }
}
