//! SQLite FTS5 search facade (ADR 0009).
//!
//! The old Tantivy writer/index has been removed. `Log::append` updates the
//! `messages_fts` virtual table in the same transaction as the event and message
//! projection, so incremental indexing and explicit rebuilds are no-ops.

use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Result};

use crate::log::Log;
pub use crate::log::SearchHit;

pub struct SearchIndex {
    log: Option<Arc<Log>>,
}

impl SearchIndex {
    /// Compatibility constructor for tests and transitional call sites. Runtime
    /// code should use `from_log` so searches execute against SQLite FTS5.
    pub fn open(_path: &Path) -> Result<Self> {
        Ok(Self { log: None })
    }

    pub fn from_log(log: Arc<Log>) -> Self {
        Self { log: Some(log) }
    }

    pub fn build_from_log(&mut self, _log: &Log) -> Result<()> {
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_message(
        &mut self,
        _session_id: &str,
        _message_id: u64,
        _content: &str,
        _role: &str,
        _tool_name: Option<&str>,
        _timestamp: f64,
    ) -> Result<()> {
        Ok(())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let Some(log) = &self.log else {
            bail!("search index is not attached to the SQLite store");
        };
        log.search(query, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;

    #[test]
    fn fts_search_reads_transactional_message_projection() {
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(Log::open(&dir.path().join("olympus.db")).unwrap());
        log.append(&Event::SessionCreated {
            session_id: "s".into(),
            hermes_id: "h".into(),
            source: "olympus".into(),
            model: None,
            title: None,
            started_at: 1.0,
            message_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            agent: None,
            node: None,
        })
        .unwrap();
        log.append(&Event::MessageAppended {
            session_id: "s".into(),
            hermes_session_id: "h".into(),
            message_id: 1,
            role: "user".into(),
            content: Some("olympus sqlite migration".into()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: 2.0,
            token_count: None,
            finish_reason: None,
        })
        .unwrap();

        let search = SearchIndex::from_log(log);
        let hits = search.search("sqlite", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, "s");
        assert!(hits[0].snippet.contains("<mark>sqlite</mark>"));
    }
}
