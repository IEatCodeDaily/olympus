//! Phase 6 — tantivy full-text search index.
//!
//! A derived projection of the event log: BM25 keyword search over all message
//! content with highlighted snippets. The index is rebuildable from the log at
//! any time (`build_from_log`); live messages can be added incrementally
//! (`index_message`). See ADR 0002 §10A.3 and the plan Phase 6.

use std::path::Path;

use anyhow::{Context, Result};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{
    Field, IndexRecordOption, Schema, SchemaBuilder, TextFieldIndexing, TextOptions, Value, FAST,
};
use tantivy::snippet::SnippetGenerator;
use tantivy::{doc, Index, IndexReader, IndexWriter, TantivyDocument};

use crate::event::Event;
use crate::log::Log;

/// Schema field names (single source of truth for field→name mapping).
const F_SESSION_ID: &str = "session_id";
const F_MESSAGE_ID: &str = "message_id";
const F_CONTENT: &str = "content";
const F_ROLE: &str = "role";
const F_TOOL_NAME: &str = "tool_name";
const F_TIMESTAMP: &str = "timestamp";

/// The tantivy-backed full-text search index over message content.
///
/// Holds an `IndexWriter` (owned, so writes are straightforward) and a
/// long-lived `IndexReader` for queries.
pub struct SearchIndex {
    index: Index,
    writer: IndexWriter,
    reader: IndexReader,
    // Cached schema fields (resolved once at open).
    session_id: Field,
    message_id: Field,
    content: Field,
    role: Field,
    tool_name: Field,
    timestamp: Field,
}

/// A single search hit — the unit returned by [`SearchIndex::search`].
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub session_id: String,
    pub message_id: u64,
    /// Highlighted snippet showing the match context (may be empty if the
    /// content was too short to snippet).
    pub snippet: String,
    /// BM25 relevance score.
    pub score: f32,
}

impl SearchIndex {
    /// Open (or create) a tantivy index at `dir`.
    ///
    /// The directory is created if it doesn't exist. The schema is fixed
    /// (see [`build_schema`]); re-opening an existing index with the same
    /// schema is safe.
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating search index dir {dir:?}"))?;

        let schema = build_schema();
        let index = Index::open_in_dir(dir).or_else(|_| {
            // Directory exists but has no index yet — create fresh.
            Index::create_in_dir(dir, schema.clone())
                .with_context(|| format!("creating tantivy index in {dir:?}"))
        })?;

        // If the index was just created, its schema already matches. If it
        // was opened from disk, trust the on-disk schema (we own it).
        // Use a SINGLE merge thread: the index is 100s of MB and only grows on
        // sync activity, so default per-core merge threads (4 on this box) were
        // burning 17% CPU at idle doing nothing useful.
        let writer = index
            .writer_with_num_threads(1, 50_000_000) // 1 thread, 50 MB heap
            .context("opening index writer")?;

        let reader = index
            .reader_builder()
            .reload_policy(tantivy::ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .context("opening index reader")?;

        let fields = resolve_fields(&index.schema());

        Ok(SearchIndex {
            index,
            writer,
            reader,
            session_id: fields.session_id,
            message_id: fields.message_id,
            content: fields.content,
            role: fields.role,
            tool_name: fields.tool_name,
            timestamp: fields.timestamp,
        })
    }

    /// Build the index by replaying the entire event log.
    ///
    /// Clears any existing index content first, then indexes every
    /// `MessageAppended` event whose `content` is `Some(_)`. This is the
    /// authoritative rebuild path (ADR §10A: "on schema change, rebuild from
    /// the log").
    pub fn build_from_log(&mut self, log: &Log) -> Result<()> {
        // Clear existing documents.
        self.writer
            .delete_all_documents()
            .context("clearing index for rebuild")?;

        let events = log.read_all().context("reading log for index rebuild")?;
        for (_, event) in &events {
            if let Event::MessageAppended {
                session_id,
                message_id,
                role,
                content: Some(content),
                tool_name,
                timestamp,
                ..
            } = event
            {
                self.add_document(
                    session_id,
                    *message_id,
                    content,
                    role,
                    tool_name.as_deref(),
                    *timestamp,
                )?;
            }
        }

        self.writer.commit().context("committing rebuilt index")?;
        self.reader
            .reload()
            .context("reloading reader after rebuild")?;
        Ok(())
    }

    /// Index a single message incrementally (live path).
    ///
    /// Messages with `None` content are silently skipped (nothing to index).
    pub fn index_message(
        &mut self,
        session_id: &str,
        message_id: u64,
        content: &str,
        role: &str,
        tool_name: Option<&str>,
        timestamp: f64,
    ) -> Result<()> {
        self.add_document(session_id, message_id, content, role, tool_name, timestamp)?;
        self.writer
            .commit()
            .context("committing incremental index")?;
        self.reader
            .reload()
            .context("reloading reader after incremental add")?;
        Ok(())
    }

    /// Search the `content` field for `query` (BM25), returning at most `limit`
    /// hits sorted by relevance, each with a highlighted snippet.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let searcher = self.reader.searcher();

        let query_parser = QueryParser::for_index(&self.index, vec![self.content]);

        let parsed = query_parser
            .parse_query(query)
            .with_context(|| format!("parsing search query {query:?}"))?;

        let top_docs = searcher
            .search(&parsed, &TopDocs::with_limit(limit))
            .context("executing search query")?;

        let snippet_gen = SnippetGenerator::create(&searcher, &parsed, self.content)?;

        let mut hits = Vec::with_capacity(top_docs.len());
        for (score, doc_addr) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_addr)?;
            let session_id = doc
                .get_first(self.session_id)
                .and_then(|v| v.as_str())
                .map(String::from)
                .context("search hit missing session_id")?;
            let message_id = doc
                .get_first(self.message_id)
                .and_then(|v| v.as_u64())
                .context("search hit missing message_id")?;

            let content_text = doc
                .get_first(self.content)
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let snippet = snippet_gen.snippet(content_text).to_html();
            hits.push(SearchHit {
                session_id,
                message_id,
                snippet,
                score,
            });
        }
        Ok(hits)
    }

    // ---- internal ----

    /// Low-level: add one document to the writer (no commit).
    fn add_document(
        &mut self,
        session_id: &str,
        message_id: u64,
        content: &str,
        role: &str,
        tool_name: Option<&str>,
        timestamp: f64,
    ) -> Result<()> {
        // tantivy 0.22 requires explicit Value trait import for the macro.
        let tool_val: &str = tool_name.unwrap_or("");
        self.writer.add_document(doc!(
            self.session_id => session_id,
            self.message_id => message_id,
            self.content => content,
            self.role => role,
            self.tool_name => tool_val,
            self.timestamp => timestamp,
        ))?;
        Ok(())
    }
}

/// Cached field handles resolved from a schema.
struct Fields {
    session_id: Field,
    message_id: Field,
    content: Field,
    role: Field,
    tool_name: Field,
    timestamp: Field,
}

/// Build the fixed tantivy schema.
///
/// - `session_id`, `message_id`: stored (returned in hits).
/// - `content`, `role`, `tool_name`: text-indexed for BM25 search.
/// - `timestamp`: fast field (for future sorting/filtering).
fn build_schema() -> Schema {
    let mut builder = SchemaBuilder::new();
    builder.add_text_field(F_SESSION_ID, TextOptions::default().set_stored());
    builder.add_u64_field(
        F_MESSAGE_ID,
        tantivy::schema::INDEXED | tantivy::schema::STORED,
    );

    let text_indexing = TextFieldIndexing::default()
        .set_tokenizer("default")
        .set_index_option(IndexRecordOption::WithFreqsAndPositions);
    let text_opts = TextOptions::default()
        .set_indexing_options(text_indexing)
        .set_stored();

    builder.add_text_field(F_CONTENT, text_opts.clone());
    builder.add_text_field(F_ROLE, text_opts.clone());
    builder.add_text_field(F_TOOL_NAME, text_opts);

    builder.add_f64_field(F_TIMESTAMP, FAST);
    builder.build()
}

/// Resolve field handles from a schema by name.
fn resolve_fields(schema: &Schema) -> Fields {
    Fields {
        session_id: schema.get_field(F_SESSION_ID).expect("session_id field"),
        message_id: schema.get_field(F_MESSAGE_ID).expect("message_id field"),
        content: schema.get_field(F_CONTENT).expect("content field"),
        role: schema.get_field(F_ROLE).expect("role field"),
        tool_name: schema.get_field(F_TOOL_NAME).expect("tool_name field"),
        timestamp: schema.get_field(F_TIMESTAMP).expect("timestamp field"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::event::Event;
    use crate::log::Log;

    use super::SearchIndex;

    // ---- helpers ----

    fn fresh_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("create temp dir for tantivy index")
    }

    /// Build a MessageAppended event with sensible defaults.
    fn msg(session_id: &str, message_id: u64, content: &str, role: &str) -> Event {
        Event::MessageAppended {
            session_id: session_id.to_string(),
            hermes_session_id: format!("hermes-{session_id}"),
            message_id,
            role: role.to_string(),
            content: Some(content.to_string()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: 1_700_000_000.0 + message_id as f64,
            token_count: None,
            finish_reason: None,
        }
    }

    /// Seed a log with 2 sessions (3 messages in sess-1, 2 in sess-2). Every
    /// message contains the word "olympus" so a broad query can match all 5.
    fn seed_two_session_log() -> (tempfile::NamedTempFile, Log) {
        let f = tempfile::NamedTempFile::new().unwrap();
        let log = Log::open(f.path()).unwrap();
        log.append(&msg(
            "sess-1",
            1,
            "the quick brown fox jumps olympus",
            "user",
        ))
        .unwrap();
        log.append(&msg(
            "sess-1",
            2,
            "this is a test of the search index olympus",
            "assistant",
        ))
        .unwrap();
        log.append(&msg(
            "sess-1",
            3,
            "lorem ipsum dolor sit amet olympus",
            "user",
        ))
        .unwrap();
        log.append(&msg(
            "sess-2",
            1,
            "a second test session about esp32 firmware olympus",
            "user",
        ))
        .unwrap();
        log.append(&msg(
            "sess-2",
            2,
            "totally unrelated content here olympus",
            "assistant",
        ))
        .unwrap();
        (f, log)
    }

    // ---- tests ----

    #[test]
    fn build_from_log_indexes_all_messages() {
        let (_log_file, log) = seed_two_session_log();
        let dir = fresh_dir();
        let mut index = SearchIndex::open(dir.path()).unwrap();
        index.build_from_log(&log).unwrap();

        // Broad query should find all 5 indexed messages (all contain "olympus").
        let hits = index.search("olympus", 20).unwrap();
        assert_eq!(
            hits.len(),
            5,
            "all 5 messages should be indexed and match a broad query"
        );
    }

    #[test]
    fn search_returns_correct_hits_for_term() {
        let (_log_file, log) = seed_two_session_log();
        let dir = fresh_dir();
        let mut index = SearchIndex::open(dir.path()).unwrap();
        index.build_from_log(&log).unwrap();

        let hits = index.search("test", 20).unwrap();
        // "test" appears in sess-1 msg 2 and sess-2 msg 1.
        assert_eq!(hits.len(), 2, "'test' should match exactly 2 messages");

        // Collect (session_id, message_id) pairs.
        let mut found: HashMap<String, Vec<u64>> = HashMap::new();
        for hit in &hits {
            found
                .entry(hit.session_id.clone())
                .or_default()
                .push(hit.message_id);
        }

        // sess-1 should contain message_id 2.
        let sess1 = found.get("sess-1").expect("sess-1 must have a hit");
        assert!(
            sess1.contains(&2),
            "sess-1 message 2 should match 'test', got {sess1:?}"
        );
        // sess-2 should contain message_id 1.
        let sess2 = found.get("sess-2").expect("sess-2 must have a hit");
        assert!(
            sess2.contains(&1),
            "sess-2 message 1 should match 'test', got {sess2:?}"
        );
    }

    #[test]
    fn search_returns_empty_for_unknown_term() {
        let (_log_file, log) = seed_two_session_log();
        let dir = fresh_dir();
        let mut index = SearchIndex::open(dir.path()).unwrap();
        index.build_from_log(&log).unwrap();

        let hits = index.search("zzzznotaword", 20).unwrap();
        assert!(
            hits.is_empty(),
            "unknown term should return zero hits, got {hits:?}"
        );
    }

    #[test]
    fn search_results_include_snippet_and_score() {
        let (_log_file, log) = seed_two_session_log();
        let dir = fresh_dir();
        let mut index = SearchIndex::open(dir.path()).unwrap();
        index.build_from_log(&log).unwrap();

        let hits = index.search("esp32", 10).unwrap();
        assert_eq!(hits.len(), 1, "esp32 should match 1 message");
        let hit = &hits[0];
        assert_eq!(hit.session_id, "sess-2");
        assert_eq!(hit.message_id, 1);
        assert!(!hit.snippet.is_empty(), "snippet must not be empty");
        assert!(
            hit.score > 0.0,
            "BM25 score must be positive, got {}",
            hit.score
        );
    }

    #[test]
    fn reindex_from_log_produces_same_results() {
        let (_log_file, log) = seed_two_session_log();

        // Build index #1.
        let dir1 = fresh_dir();
        let mut idx1 = SearchIndex::open(dir1.path()).unwrap();
        idx1.build_from_log(&log).unwrap();
        let hits1 = idx1.search("test", 20).unwrap();

        // Build a fresh index #2 from the same log — results must be identical.
        let dir2 = fresh_dir();
        let mut idx2 = SearchIndex::open(dir2.path()).unwrap();
        idx2.build_from_log(&log).unwrap();
        let hits2 = idx2.search("test", 20).unwrap();

        assert_eq!(hits1.len(), hits2.len(), "hit count must match on reindex");

        // Same (session_id, message_id) pairs, in the same rank order.
        let pairs1: Vec<(String, u64)> = hits1
            .iter()
            .map(|h| (h.session_id.clone(), h.message_id))
            .collect();
        let pairs2: Vec<(String, u64)> = hits2
            .iter()
            .map(|h| (h.session_id.clone(), h.message_id))
            .collect();
        assert_eq!(
            pairs1, pairs2,
            "reindex must produce identical hit ordering"
        );
    }

    #[test]
    fn incremental_index_message_is_searchable() {
        let dir = fresh_dir();
        let mut index = SearchIndex::open(dir.path()).unwrap();

        // Index a single message live (no log involved).
        index
            .index_message(
                "sess-live",
                42,
                "a live message about rust async",
                "user",
                None,
                1_700_000_042.0,
            )
            .unwrap();

        let hits = index.search("rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, "sess-live");
        assert_eq!(hits[0].message_id, 42);
    }

    #[test]
    fn search_respects_limit() {
        let (_log_file, log) = seed_two_session_log();
        let dir = fresh_dir();
        let mut index = SearchIndex::open(dir.path()).unwrap();
        index.build_from_log(&log).unwrap();

        // Broad query, limit 2 — should return at most 2 hits even though all 5 match.
        let hits = index.search("olympus", 2).unwrap();
        assert!(
            hits.len() <= 2,
            "limit must cap results, got {}",
            hits.len()
        );
    }

    #[test]
    fn message_with_none_content_is_skipped() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let log = Log::open(f.path()).unwrap();
        // A message with no content.
        log.append(&Event::MessageAppended {
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
        })
        .unwrap();
        // Plus one real message.
        log.append(&msg("sess-1", 2, "real content here", "user"))
            .unwrap();

        let dir = fresh_dir();
        let mut index = SearchIndex::open(dir.path()).unwrap();
        index.build_from_log(&log).unwrap();

        let hits = index.search("content", 10).unwrap();
        // Only the real message should be searchable (None content is skipped).
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message_id, 2);
    }
}
