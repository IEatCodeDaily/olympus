//! redb-backed append-only event log — the source of truth.
//!
//! redb tables:
//! - `"events"`: `u64` (monotonic sequence number) → postcard-serialized event.
//! - `"meta"`:   `&str` → bytes (e.g. `next_seq`).
//!
//! `MessageAppended` content/tool_calls/reasoning fields are zstd-compressed
//! before postcard serialization, and transparently decompressed on read.

use std::path::Path;

use anyhow::{Context, Result};
use redb::{Database, ReadableTable, TableDefinition};

use crate::compress;
use crate::event::Event;

/// Events table: monotonic u64 sequence → postcard bytes.
const EVENTS: TableDefinition<u64, &[u8]> = TableDefinition::new("events");

/// Metadata table. `next_seq` value is stored as little-endian u64 bytes.
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");

const NEXT_SEQ_KEY: &str = "next_seq";

/// A stored event with its compression-sensitive large text fields compressed.
///
/// This is the on-disk shape. We convert from `Event` → `StoredEvent` before
/// postcard-serializing, and the reverse on read.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredEvent {
    variant: StoredVariant,
}

#[derive(serde::Serialize, serde::Deserialize)]
enum StoredVariant {
    SessionCreated {
        session_id: String,
        hermes_id: String,
        source: String,
        model: Option<String>,
        title: Option<String>,
        started_at: f64,
        message_count: u64,
        input_tokens: u64,
        output_tokens: u64,
    },
    MessageAppended {
        session_id: String,
        hermes_session_id: String,
        message_id: u64,
        role: String,
        /// zstd-compressed content bytes.
        content: Option<Vec<u8>>,
        tool_name: Option<String>,
        /// zstd-compressed tool_calls bytes.
        tool_calls: Option<Vec<u8>>,
        /// zstd-compressed reasoning bytes.
        reasoning: Option<Vec<u8>>,
        timestamp: f64,
        token_count: Option<u64>,
        finish_reason: Option<String>,
    },
    SessionUpdated {
        session_id: String,
        title: Option<String>,
        model: Option<String>,
        archived: Option<bool>,
        message_count: Option<u64>,
    },
}

/// Convert a logical `Event` into its compressed on-disk shape.
fn to_stored(event: &Event) -> Result<StoredEvent> {
    let variant = match event {
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
        } => StoredVariant::SessionCreated {
            session_id: session_id.clone(),
            hermes_id: hermes_id.clone(),
            source: source.clone(),
            model: model.clone(),
            title: title.clone(),
            started_at: *started_at,
            message_count: *message_count,
            input_tokens: *input_tokens,
            output_tokens: *output_tokens,
        },
        Event::MessageAppended {
            session_id,
            hermes_session_id,
            message_id,
            role,
            content,
            tool_name,
            tool_calls,
            reasoning,
            timestamp,
            token_count,
            finish_reason,
        } => StoredVariant::MessageAppended {
            session_id: session_id.clone(),
            hermes_session_id: hermes_session_id.clone(),
            message_id: *message_id,
            role: role.clone(),
            content: content
                .as_deref()
                .map(|s| compress::compress(s.as_bytes()))
                .transpose()?,
            tool_name: tool_name.clone(),
            tool_calls: tool_calls
                .as_deref()
                .map(|s| compress::compress(s.as_bytes()))
                .transpose()?,
            reasoning: reasoning
                .as_deref()
                .map(|s| compress::compress(s.as_bytes()))
                .transpose()?,
            timestamp: *timestamp,
            token_count: *token_count,
            finish_reason: finish_reason.clone(),
        },
        Event::SessionUpdated {
            session_id,
            title,
            model,
            archived,
            message_count,
        } => StoredVariant::SessionUpdated {
            session_id: session_id.clone(),
            title: title.clone(),
            model: model.clone(),
            archived: *archived,
            message_count: *message_count,
        },
    };
    Ok(StoredEvent { variant })
}

/// Convert a compressed on-disk shape back into the logical `Event`.
fn from_stored(stored: StoredEvent) -> Result<Event> {
    Ok(match stored.variant {
        StoredVariant::SessionCreated {
            session_id,
            hermes_id,
            source,
            model,
            title,
            started_at,
            message_count,
            input_tokens,
            output_tokens,
        } => Event::SessionCreated {
            session_id,
            hermes_id,
            source,
            model,
            title,
            started_at,
            message_count,
            input_tokens,
            output_tokens,
        },
        StoredVariant::MessageAppended {
            session_id,
            hermes_session_id,
            message_id,
            role,
            content,
            tool_name,
            tool_calls,
            reasoning,
            timestamp,
            token_count,
            finish_reason,
        } => Event::MessageAppended {
            session_id,
            hermes_session_id,
            message_id,
            role,
            content: content
                .as_deref()
                .map(compress::decompress)
                .transpose()?
                .map(String::from_utf8)
                .transpose()
                .context("decompressed content was not valid UTF-8")?,
            tool_name,
            tool_calls: tool_calls
                .as_deref()
                .map(compress::decompress)
                .transpose()?
                .map(String::from_utf8)
                .transpose()
                .context("decompressed tool_calls was not valid UTF-8")?,
            reasoning: reasoning
                .as_deref()
                .map(compress::decompress)
                .transpose()?
                .map(String::from_utf8)
                .transpose()
                .context("decompressed reasoning was not valid UTF-8")?,
            timestamp,
            token_count,
            finish_reason,
        },
        StoredVariant::SessionUpdated {
            session_id,
            title,
            model,
            archived,
            message_count,
        } => Event::SessionUpdated {
            session_id,
            title,
            model,
            archived,
            message_count,
        },
    })
}

/// The append-only event log backed by a redb database file.
pub struct Log {
    db: Database,
}

impl Log {
    /// Open (or create) a log at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let db = Database::create(path).context("opening redb log")?;
        // Ensure tables exist.
        let txn = db.begin_write().context("begin write for table init")?;
        {
            let _ = txn.open_table(EVENTS).context("open events table")?;
            let _ = txn.open_table(META).context("open meta table")?;
        }
        txn.commit()?;
        Ok(Self { db })
    }

    /// Append an event, returning the assigned monotonic sequence number.
    pub fn append(&self, event: &Event) -> Result<u64> {
        let stored = to_stored(event)?;
        let bytes = postcard::to_allocvec(&stored).context("postcard-encoding event")?;
        let txn = self.db.begin_write().context("begin write for append")?;
        let seq = {
            let mut meta = txn.open_table(META)?;
            let next = read_next_seq(&meta)?;
            let mut events = txn.open_table(EVENTS)?;
            events.insert(next, bytes.as_slice())?;
            write_next_seq(&mut meta, next + 1)?;
            next
        };
        txn.commit()?;
        Ok(seq)
    }

    /// Append many events in a SINGLE write transaction, returning the sequence
    /// number assigned to the first event (subsequent events are contiguous).
    ///
    /// `append()` commits (and fsyncs) once per event, which is far too slow for
    /// bulk import (one transaction per message → ~100k fsyncs). This batches an
    /// arbitrary number of events into one transaction so a full state.db import
    /// is a handful of commits instead of one-per-row. Returns `None` if `events`
    /// is empty.
    pub fn append_batch(&self, events: &[Event]) -> Result<Option<u64>> {
        if events.is_empty() {
            return Ok(None);
        }
        let txn = self
            .db
            .begin_write()
            .context("begin write for append_batch")?;
        let first = {
            let mut meta = txn.open_table(META)?;
            let mut next = read_next_seq(&meta)?;
            let first = next;
            let mut table = txn.open_table(EVENTS)?;
            for event in events {
                let stored = to_stored(event)?;
                let bytes = postcard::to_allocvec(&stored).context("postcard-encoding event")?;
                table.insert(next, bytes.as_slice())?;
                next += 1;
            }
            write_next_seq(&mut meta, next)?;
            first
        };
        txn.commit()?;
        Ok(Some(first))
    }

    /// Read up to `limit` events starting at sequence `seq` (inclusive).
    pub fn read_from(&self, seq: u64, limit: usize) -> Result<Vec<(u64, Event)>> {
        let txn = self.db.begin_read().context("begin read for read_from")?;
        let table = txn.open_table(EVENTS).context("open events for read")?;
        let mut out = Vec::new();
        for item in table.range(seq..)? {
            if out.len() >= limit {
                break;
            }
            let (k, v) = item?;
            let s = k.value();
            let bytes = v.value();
            let stored: StoredEvent = postcard::from_bytes(bytes)?;
            let event = from_stored(stored)?;
            out.push((s, event));
        }
        Ok(out)
    }

    /// Read all events in sequence order (for replay).
    pub fn read_all(&self) -> Result<Vec<(u64, Event)>> {
        let txn = self.db.begin_read().context("begin read for read_all")?;
        let table = txn.open_table(EVENTS).context("open events for read_all")?;
        let mut out = Vec::new();
        for item in table.iter()? {
            let (k, v) = item?;
            let bytes = v.value();
            let stored: StoredEvent = postcard::from_bytes(bytes)?;
            let event = from_stored(stored)?;
            out.push((k.value(), event));
        }
        Ok(out)
    }
}

fn read_next_seq(meta: &redb::Table<&str, &[u8]>) -> Result<u64> {
    Ok(match meta.get(NEXT_SEQ_KEY)? {
        Some(v) => {
            let b = v.value();
            let mut arr = [0u8; 8];
            arr.copy_from_slice(b);
            u64::from_le_bytes(arr)
        }
        None => 0,
    })
}

fn write_next_seq(meta: &mut redb::Table<&str, &[u8]>, val: u64) -> Result<()> {
    meta.insert(NEXT_SEQ_KEY, val.to_le_bytes().as_slice())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;

    fn fresh_log() -> (tempfile::NamedTempFile, Log) {
        let f = tempfile::NamedTempFile::new().unwrap();
        let log = Log::open(f.path()).unwrap();
        (f, log)
    }

    fn sample_session_created(id: &str) -> Event {
        Event::SessionCreated {
            session_id: id.into(),
            hermes_id: format!("hermes-{id}"),
            source: "cli".into(),
            model: Some("glm-5.2".into()),
            title: Some("test session".into()),
            started_at: 1_700_000_000.0,
            message_count: 0,
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    fn sample_message(session_id: &str, msg_id: u64, content: &str) -> Event {
        Event::MessageAppended {
            session_id: session_id.into(),
            hermes_session_id: format!("hermes-{session_id}"),
            message_id: msg_id,
            role: "user".into(),
            content: Some(content.into()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: 1_700_000_000.0 + msg_id as f64,
            token_count: None,
            finish_reason: None,
        }
    }

    #[test]
    fn append_then_read_all_returns_event_with_right_seq() {
        let (_f, log) = fresh_log();
        let seq = log.append(&sample_session_created("sess-1")).unwrap();
        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 1);
        let (got_seq, got_event) = &events[0];
        assert_eq!(*got_seq, seq, "returned seq must match append result");
        assert_eq!(got_event, &sample_session_created("sess-1"));
    }

    #[test]
    fn seq_is_monotonic_across_appends() {
        let (_f, log) = fresh_log();
        let s0 = log.append(&sample_session_created("a")).unwrap();
        let s1 = log.append(&sample_session_created("b")).unwrap();
        let s2 = log.append(&sample_session_created("c")).unwrap();
        assert_eq!((s0, s1, s2), (0, 1, 2));
        let events = log.read_all().unwrap();
        let seqs: Vec<u64> = events.iter().map(|(s, _)| *s).collect();
        assert_eq!(seqs, vec![0, 1, 2]);
    }

    #[test]
    fn append_batch_assigns_contiguous_seqs_and_persists_all() {
        let (_f, log) = fresh_log();
        let batch = vec![
            sample_session_created("a"),
            sample_message("a", 0, "hi"),
            sample_message("a", 1, "there"),
        ];
        let first = log.append_batch(&batch).unwrap();
        assert_eq!(first, Some(0), "first seq of the batch");

        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 3);
        let seqs: Vec<u64> = events.iter().map(|(s, _)| *s).collect();
        assert_eq!(seqs, vec![0, 1, 2], "batch seqs are contiguous");
        assert_eq!(&events[2].1, &sample_message("a", 1, "there"));
    }

    #[test]
    fn append_batch_continues_seq_after_prior_appends() {
        let (_f, log) = fresh_log();
        log.append(&sample_session_created("a")).unwrap(); // seq 0
        let first = log
            .append_batch(&[sample_message("a", 0, "x"), sample_message("a", 1, "y")])
            .unwrap();
        assert_eq!(first, Some(1), "batch continues from prior seq");
        let seqs: Vec<u64> = log.read_all().unwrap().iter().map(|(s, _)| *s).collect();
        assert_eq!(seqs, vec![0, 1, 2]);
    }

    #[test]
    fn append_batch_empty_is_noop() {
        let (_f, log) = fresh_log();
        assert_eq!(log.append_batch(&[]).unwrap(), None);
        assert_eq!(log.read_all().unwrap().len(), 0);
    }

    #[test]
    fn read_from_paginates() {
        let (_f, log) = fresh_log();
        for i in 0..5 {
            log.append(&sample_session_created(&format!("s{i}")))
                .unwrap();
        }
        // Page 1: seq 0..2 (2 items)
        let p1 = log.read_from(0, 2).unwrap();
        assert_eq!(p1.len(), 2);
        assert_eq!(p1[0].0, 0);
        assert_eq!(p1[1].0, 1);
        // Page 2: seq 2..4 (2 items)
        let p2 = log.read_from(2, 2).unwrap();
        assert_eq!(p2.len(), 2);
        assert_eq!(p2[0].0, 2);
        assert_eq!(p2[1].0, 3);
        // Page 3: seq 4.. (1 item)
        let p3 = log.read_from(4, 2).unwrap();
        assert_eq!(p3.len(), 1);
        assert_eq!(p3[0].0, 4);
        // Empty page past end
        let p4 = log.read_from(5, 2).unwrap();
        assert!(p4.is_empty());
    }

    #[test]
    fn reopening_log_persists_events() {
        let (f, log) = fresh_log();
        log.append(&sample_session_created("sess-1")).unwrap();
        log.append(&sample_session_created("sess-2")).unwrap();
        drop(log);

        let reopened = Log::open(f.path()).unwrap();
        let events = reopened.read_all().unwrap();
        assert_eq!(events.len(), 2, "events must survive reopen");
        assert_eq!(events[0].1, sample_session_created("sess-1"));
        assert_eq!(events[1].1, sample_session_created("sess-2"));
        // next_seq preserved: appending continues from 2
        let s = reopened.append(&sample_session_created("sess-3")).unwrap();
        assert_eq!(s, 2);
    }

    #[test]
    fn compressed_message_roundtrips_through_log() {
        let (_f, log) = fresh_log();
        let long_content = "Hello ".repeat(1000);
        log.append(&sample_message("sess-1", 0, &long_content))
            .unwrap();
        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 1);
        match &events[0].1 {
            Event::MessageAppended { content, .. } => {
                assert_eq!(content.as_deref(), Some(long_content.as_str()));
            }
            other => panic!("expected MessageAppended, got {other:?}"),
        }
    }

    #[test]
    fn message_with_none_content_roundtrips() {
        let (_f, log) = fresh_log();
        let e = Event::MessageAppended {
            session_id: "sess-1".into(),
            hermes_session_id: "h-1".into(),
            message_id: 1,
            role: "system".into(),
            content: None,
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: 1.0,
            token_count: None,
            finish_reason: None,
        };
        log.append(&e).unwrap();
        let back = log.read_all().unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].1, e);
    }

    #[test]
    fn empty_log_reads_empty() {
        let (_f, log) = fresh_log();
        assert!(log.read_all().unwrap().is_empty());
        assert!(log.read_from(0, 10).unwrap().is_empty());
    }
}
