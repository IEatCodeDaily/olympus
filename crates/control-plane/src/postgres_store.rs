use std::sync::Mutex;

use anyhow::{Context, Result};
use pgvector::Vector;
use postgres::{Client, NoTls, Transaction};

use crate::event::Event;
use crate::log::SearchHit;

pub(crate) struct PostgresStore {
    client: Mutex<Client>,
}

impl PostgresStore {
    pub(crate) fn open(dsn: &str) -> Result<Self> {
        let mut client = Client::connect(dsn, NoTls).context("connecting to PostgreSQL")?;
        client
            .batch_execute(MIGRATIONS)
            .context("migrating PostgreSQL Hall storage")?;
        Ok(Self {
            client: Mutex::new(client),
        })
    }

    pub(crate) fn append(&self, event: &Event) -> Result<u64> {
        let mut client = self.client.lock().expect("PostgreSQL mutex poisoned");
        let mut tx = client.transaction()?;
        let seq = append_in_tx(&mut tx, event)?;
        tx.commit()?;
        Ok(seq)
    }

    pub(crate) fn append_batch(&self, events: &[Event]) -> Result<Option<u64>> {
        if events.is_empty() {
            return Ok(None);
        }
        let mut client = self.client.lock().expect("PostgreSQL mutex poisoned");
        let mut tx = client.transaction()?;
        let mut first = None;
        for event in events {
            first.get_or_insert(append_in_tx(&mut tx, event)?);
        }
        tx.commit()?;
        Ok(first)
    }

    pub(crate) fn read_from(&self, seq: u64, limit: usize) -> Result<Vec<(u64, Event)>> {
        let mut client = self.client.lock().expect("PostgreSQL mutex poisoned");
        client
            .query(
                "SELECT seq,payload FROM hall_events WHERE seq >= $1 ORDER BY seq LIMIT $2",
                &[&(seq as i64), &(limit.min(i64::MAX as usize) as i64)],
            )?
            .into_iter()
            .map(|row| {
                let payload: Vec<u8> = row.get(1);
                let json = zstd::stream::decode_all(payload.as_slice())?;
                Ok((row.get::<_, i64>(0) as u64, serde_json::from_slice(&json)?))
            })
            .collect()
    }

    pub(crate) fn event_count(&self) -> Result<usize> {
        let mut client = self.client.lock().expect("PostgreSQL mutex poisoned");
        Ok(client
            .query_one("SELECT COUNT(*) FROM hall_events", &[])?
            .get::<_, i64>(0) as usize)
    }

    pub(crate) fn accept_envoy_seq(&self, session_id: &str, seq: u64) -> Result<bool> {
        let mut client = self.client.lock().expect("PostgreSQL mutex poisoned");
        let mut tx = client.transaction()?;
        let current = tx
            .query_opt(
                "SELECT seq FROM hall_envoy_watermarks WHERE session_id=$1 FOR UPDATE",
                &[&session_id],
            )?
            .map(|row| row.get::<_, i64>(0) as u64);
        if current.is_some_and(|watermark| seq <= watermark) {
            return Ok(false);
        }
        let expected = current.map_or(0, |watermark| watermark + 1);
        anyhow::ensure!(
            seq == expected,
            "envoy event sequence gap for {session_id}: expected {expected}, got {seq}"
        );
        tx.execute(
            "INSERT INTO hall_envoy_watermarks(session_id,seq) VALUES($1,$2)
             ON CONFLICT(session_id) DO UPDATE SET seq=excluded.seq",
            &[&session_id, &(seq as i64)],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub(crate) fn envoy_watermark(&self, session_id: &str) -> Result<Option<u64>> {
        let mut client = self.client.lock().expect("PostgreSQL mutex poisoned");
        Ok(client
            .query_opt(
                "SELECT seq FROM hall_envoy_watermarks WHERE session_id=$1",
                &[&session_id],
            )?
            .map(|row| row.get::<_, i64>(0) as u64))
    }

    pub(crate) fn accept_observed(
        &self,
        transport_session_id: &str,
        seq: u64,
        hermes_id: &str,
        message_id: Option<u64>,
        event: &Event,
    ) -> Result<bool> {
        let mut client = self.client.lock().expect("PostgreSQL mutex poisoned");
        let mut tx = client.transaction()?;
        let current = tx
            .query_opt(
                "SELECT seq FROM hall_envoy_watermarks WHERE session_id=$1 FOR UPDATE",
                &[&transport_session_id],
            )?
            .map(|row| row.get::<_, i64>(0) as u64);
        if current.is_some_and(|watermark| seq <= watermark) {
            return Ok(false);
        }
        let expected = current.map_or(0, |watermark| watermark + 1);
        anyhow::ensure!(
            seq == expected,
            "envoy observation sequence gap for {transport_session_id}: expected {expected}, got {seq}"
        );
        tx.execute(
            "INSERT INTO hall_observed_sessions(hermes_id) VALUES($1) ON CONFLICT DO NOTHING",
            &[&hermes_id],
        )?;
        let is_new = if let Some(message_id) = message_id {
            tx.execute(
                "INSERT INTO hall_observed_messages(hermes_id,message_id) VALUES($1,$2) ON CONFLICT DO NOTHING",
                &[&hermes_id, &(message_id as i64)],
            )? == 1
        } else {
            true
        };
        if is_new {
            append_in_tx(&mut tx, event)?;
        }
        tx.execute(
            "INSERT INTO hall_envoy_watermarks(session_id,seq) VALUES($1,$2)
             ON CONFLICT(session_id) DO UPDATE SET seq=excluded.seq",
            &[&transport_session_id, &(seq as i64)],
        )?;
        tx.commit()?;
        Ok(is_new)
    }

    pub(crate) fn retain_native(&self) -> Result<()> {
        let mut client = self.client.lock().expect("PostgreSQL mutex poisoned");
        let mut tx = client.transaction()?;
        tx.batch_execute(
            "DELETE FROM hall_events WHERE session_id IS NOT NULL AND session_id NOT IN
             (SELECT session_id FROM hall_sessions WHERE source='olympus' UNION SELECT hermes_id FROM hall_observed_sessions);
             DELETE FROM hall_messages WHERE session_id NOT IN
             (SELECT session_id FROM hall_sessions WHERE source='olympus' UNION SELECT hermes_id FROM hall_observed_sessions);
             DELETE FROM hall_sessions WHERE source != 'olympus';",
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let mut client = self.client.lock().expect("PostgreSQL mutex poisoned");
        Ok(client
            .query(
                "SELECT m.session_id,m.message_id,
                        ts_headline('english',COALESCE(m.content,''),websearch_to_tsquery('english',$1),'StartSel=<mark>,StopSel=</mark>'),
                        ts_rank_cd(m.search_vector,websearch_to_tsquery('english',$1)),m.created_at,COALESCE(s.source,'')
                 FROM hall_messages m LEFT JOIN hall_sessions s USING(session_id)
                 WHERE m.search_vector @@ websearch_to_tsquery('english',$1)
                 ORDER BY 4 DESC LIMIT $2",
                &[&query, &(limit.min(i64::MAX as usize) as i64)],
            )?
            .into_iter()
            .map(|row| SearchHit {
                session_id: row.get(0),
                message_id: row.get::<_, i64>(1) as u64,
                snippet: row.get(2),
                score: row.get(3),
                timestamp: row.get(4),
                source: row.get(5),
            })
            .collect())
    }

    pub(crate) fn set_embedding(
        &self,
        session_id: &str,
        message_id: u64,
        model: &str,
        embedding: &[f32],
    ) -> Result<()> {
        anyhow::ensure!(!embedding.is_empty(), "embedding must not be empty");
        let vector = Vector::from(embedding.to_vec());
        self.client
            .lock()
            .expect("PostgreSQL mutex poisoned")
            .execute(
                "INSERT INTO hall_message_embeddings(session_id,message_id,model,embedding)
             VALUES($1,$2,$3,$4)
             ON CONFLICT(session_id,message_id,model) DO UPDATE SET embedding=excluded.embedding",
                &[&session_id, &(message_id as i64), &model, &vector],
            )?;
        Ok(())
    }

    pub(crate) fn semantic_search(
        &self,
        model: &str,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        anyhow::ensure!(!embedding.is_empty(), "embedding must not be empty");
        let vector = Vector::from(embedding.to_vec());
        let mut client = self.client.lock().expect("PostgreSQL mutex poisoned");
        Ok(client
            .query(
                "SELECT m.session_id,m.message_id,COALESCE(m.content,''),
                    (1-(e.embedding <=> $2))::real,m.created_at,COALESCE(s.source,'')
             FROM hall_message_embeddings e JOIN hall_messages m USING(session_id,message_id)
             LEFT JOIN hall_sessions s USING(session_id) WHERE e.model=$1
             ORDER BY e.embedding <=> $2 LIMIT $3",
                &[&model, &vector, &(limit.min(i64::MAX as usize) as i64)],
            )?
            .into_iter()
            .map(|row| SearchHit {
                session_id: row.get(0),
                message_id: row.get::<_, i64>(1) as u64,
                snippet: row.get(2),
                score: row.get(3),
                timestamp: row.get(4),
                source: row.get(5),
            })
            .collect())
    }
}

fn append_in_tx(tx: &mut Transaction<'_>, event: &Event) -> Result<u64> {
    let json = serde_json::to_vec(event)?;
    let payload = zstd::stream::encode_all(json.as_slice(), 3)?;
    let row = tx.query_one(
        "INSERT INTO hall_events(event_type,payload,created_at,session_id) VALUES($1,$2,$3,$4) RETURNING seq",
        &[&event_type(event), &payload, &event_time(event), &event_session_id(event)],
    )?;
    apply_projection(tx, event)?;
    Ok(row.get::<_, i64>(0) as u64)
}

fn apply_projection(tx: &mut Transaction<'_>, event: &Event) -> Result<()> {
    match event {
        Event::SessionCreated {
            session_id,
            hermes_id,
            source,
            model,
            title,
            started_at,
            ..
        } => {
            tx.execute("INSERT INTO hall_sessions(session_id,hermes_id,source,model,title,started_at,last_activity) VALUES($1,$2,$3,$4,$5,$6,$6) ON CONFLICT(session_id) DO UPDATE SET hermes_id=excluded.hermes_id,source=excluded.source,model=excluded.model,title=excluded.title,started_at=excluded.started_at",
                &[session_id,hermes_id,source,model,title,started_at])?;
        }
        Event::SessionUpdated {
            session_id,
            title,
            model,
            ..
        } => {
            tx.execute("UPDATE hall_sessions SET title=COALESCE($2,title),model=COALESCE($3,model) WHERE session_id=$1", &[session_id,title,model])?;
        }
        Event::MessageAppended {
            session_id,
            message_id,
            role,
            content,
            tool_name,
            timestamp,
            ..
        } => {
            tx.execute("INSERT INTO hall_messages(session_id,message_id,role,content,tool_name,created_at) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(session_id,message_id) DO UPDATE SET role=excluded.role,content=excluded.content,tool_name=excluded.tool_name,created_at=excluded.created_at",
                &[session_id,&(*message_id as i64),role,content,tool_name,timestamp])?;
            tx.execute("UPDATE hall_sessions SET last_activity=GREATEST(last_activity,$2) WHERE session_id=$1", &[session_id,timestamp])?;
        }
        Event::MessageRemoved {
            session_id,
            message_id,
            ..
        } => {
            tx.execute(
                "DELETE FROM hall_messages WHERE session_id=$1 AND message_id=$2",
                &[session_id, &(*message_id as i64)],
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn event_type(event: &Event) -> &'static str {
    match event {
        Event::SessionCreated { .. } => "session.created",
        Event::SessionUpdated { .. } => "session.updated",
        Event::MessageAppended { .. } => "message.appended",
        Event::MessageRemoved { .. } => "message.removed",
        _ => "event",
    }
}

fn event_time(event: &Event) -> f64 {
    match event {
        Event::SessionCreated { started_at, .. } => *started_at,
        Event::MessageAppended { timestamp, .. } => *timestamp,
        _ => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64()),
    }
}

fn event_session_id(event: &Event) -> Option<&str> {
    match event {
        Event::SessionCreated { session_id, .. }
        | Event::SessionUpdated { session_id, .. }
        | Event::MessageAppended { session_id, .. }
        | Event::MessageRemoved { session_id, .. } => Some(session_id),
        _ => None,
    }
}

const MIGRATIONS: &str = r#"
CREATE EXTENSION IF NOT EXISTS vector;
CREATE TABLE IF NOT EXISTS hall_schema_migrations(version integer PRIMARY KEY, applied_at timestamptz NOT NULL DEFAULT now());
INSERT INTO hall_schema_migrations(version) VALUES(1) ON CONFLICT DO NOTHING;
CREATE TABLE IF NOT EXISTS hall_events(seq bigserial PRIMARY KEY,event_type text NOT NULL,payload bytea NOT NULL,created_at double precision NOT NULL,session_id text);
CREATE INDEX IF NOT EXISTS hall_events_session_idx ON hall_events(session_id);
CREATE TABLE IF NOT EXISTS hall_sessions(session_id text PRIMARY KEY,hermes_id text NOT NULL DEFAULT '',source text NOT NULL DEFAULT '',model text,title text,started_at double precision NOT NULL,last_activity double precision NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS hall_messages(session_id text NOT NULL,message_id bigint NOT NULL,role text NOT NULL,content text,tool_name text,created_at double precision NOT NULL,search_vector tsvector GENERATED ALWAYS AS (to_tsvector('english',coalesce(content,''))) STORED,PRIMARY KEY(session_id,message_id));
CREATE INDEX IF NOT EXISTS hall_messages_fts_idx ON hall_messages USING gin(search_vector);
CREATE TABLE IF NOT EXISTS hall_message_embeddings(session_id text NOT NULL,message_id bigint NOT NULL,model text NOT NULL,embedding vector NOT NULL,PRIMARY KEY(session_id,message_id,model),FOREIGN KEY(session_id,message_id) REFERENCES hall_messages(session_id,message_id) ON DELETE CASCADE);
CREATE TABLE IF NOT EXISTS hall_observed_sessions(hermes_id text PRIMARY KEY);
CREATE TABLE IF NOT EXISTS hall_observed_messages(hermes_id text NOT NULL,message_id bigint NOT NULL,PRIMARY KEY(hermes_id,message_id));
CREATE TABLE IF NOT EXISTS hall_envoy_watermarks(session_id text PRIMARY KEY,seq bigint NOT NULL);
"#;
