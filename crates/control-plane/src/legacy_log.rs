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
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        node: Option<String>,
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
    MessageRemoved {
        session_id: String,
        hermes_session_id: String,
        message_id: u64,
    },
    SessionUpdated {
        session_id: String,
        title: Option<String>,
        model: Option<String>,
        archived: Option<bool>,
        message_count: Option<u64>,
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        node: Option<String>,
        #[serde(default)]
        hermes_id: Option<String>,
    },
    CardCreated {
        card_id: String,
        board_id: String,
        title: String,
        created_at: f64,
    },
    CardAssigned {
        card_id: String,
        assigned_id: String,
        assigned_kind: String,
        session_id: String,
        attempt_bookmark: String,
        assigned_at: f64,
    },
    CardClaimed {
        card_id: String,
        claimed_at: f64,
    },
    CardBlocked {
        card_id: String,
        blocked_by: Vec<String>,
        blocked_at: f64,
    },
    CardCompleted {
        card_id: String,
        completed_at: f64,
    },
    CardReassigned {
        card_id: String,
        assigned_id: String,
        assigned_kind: String,
        session_id: String,
        attempt_bookmark: String,
        previous_session_id: String,
        reassigned_at: f64,
    },
    SetupDeclared {
        scope: String,
        skills: Vec<String>,
        mcp: Vec<String>,
        plugins: Vec<String>,
        hooks: Vec<String>,
        declared_at: f64,
    },
    EntryRegistered {
        kind: String,
        slug: String,
        definition: String,
        registered_at: f64,
    },
    SessionForked {
        parent_session_id: String,
        child_session_id: String,
        fork_type: String,
        fork_point: Option<u64>,
        forked_at: f64,
    },
    CardSessionLinked {
        card_id: String,
        session_id: String,
        linked_at: f64,
    },
    SessionHandover {
        source_session_id: String,
        target_session_id: String,
        from_agent_kind: String,
        to_agent_kind: String,
        translated_message_count: u64,
        handed_over_at: f64,
    },
    /// V2 of SessionUpdated adding `pinned`. Appended at the END of the enum:
    /// postcard is positional, so legacy `SessionUpdated` records keep their
    /// variant index and still decode. New writes use this variant.
    SessionUpdatedV2 {
        session_id: String,
        title: Option<String>,
        model: Option<String>,
        archived: Option<bool>,
        message_count: Option<u64>,
        agent: Option<String>,
        node: Option<String>,
        hermes_id: Option<String>,
        pinned: Option<bool>,
    },
    // ---- Repo management (appended AFTER SessionUpdatedV2) ----
    RepoRegistered {
        slug: String,
        url: String,
        default_branch: String,
        registered_at: f64,
    },
    RepoRemoved {
        slug: String,
        removed_at: f64,
    },
    SessionRepoAttached {
        session_id: String,
        slug: String,
        attached_at: f64,
    },
    // ---- Project events (context container) — MUST remain at the END ----
    ProjectCreated {
        project_id: String,
        name: String,
        created_at: f64,
    },
    ProjectUpdated {
        project_id: String,
        name: Option<String>,
        vaults: Option<Vec<String>>,
        repos: Option<Vec<String>>,
        boards: Option<Vec<String>>,
    },
    ProjectDeleted {
        project_id: String,
        deleted_at: f64,
    },
    SessionProjectAttached {
        session_id: String,
        project_id: String,
        attached_at: f64,
    },
    SessionOrganizationAssigned {
        session_id: String,
        organization_id: String,
    },
    ProjectOrganizationAssigned {
        project_id: String,
        organization_id: String,
    },
    CardOrganizationAssigned {
        card_id: String,
        organization_id: String,
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
            agent,
            node,
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
            agent: agent.clone(),
            node: node.clone(),
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
        Event::MessageRemoved {
            session_id,
            hermes_session_id,
            message_id,
        } => StoredVariant::MessageRemoved {
            session_id: session_id.clone(),
            hermes_session_id: hermes_session_id.clone(),
            message_id: *message_id,
        },
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
        } => StoredVariant::SessionUpdatedV2 {
            session_id: session_id.clone(),
            title: title.clone(),
            model: model.clone(),
            archived: *archived,
            message_count: *message_count,
            agent: agent.clone(),
            node: node.clone(),
            hermes_id: hermes_id.clone(),
            pinned: *pinned,
        },
        Event::CardCreated {
            card_id,
            board_id,
            title,
            created_at,
        } => StoredVariant::CardCreated {
            card_id: card_id.clone(),
            board_id: board_id.clone(),
            title: title.clone(),
            created_at: *created_at,
        },
        Event::CardAssigned {
            card_id,
            assigned_id,
            assigned_kind,
            session_id,
            attempt_bookmark,
            assigned_at,
        } => StoredVariant::CardAssigned {
            card_id: card_id.clone(),
            assigned_id: assigned_id.clone(),
            assigned_kind: assigned_kind.clone(),
            session_id: session_id.clone(),
            attempt_bookmark: attempt_bookmark.clone(),
            assigned_at: *assigned_at,
        },
        Event::CardClaimed {
            card_id,
            claimed_at,
        } => StoredVariant::CardClaimed {
            card_id: card_id.clone(),
            claimed_at: *claimed_at,
        },
        Event::CardBlocked {
            card_id,
            blocked_by,
            blocked_at,
        } => StoredVariant::CardBlocked {
            card_id: card_id.clone(),
            blocked_by: blocked_by.clone(),
            blocked_at: *blocked_at,
        },
        Event::CardCompleted {
            card_id,
            completed_at,
        } => StoredVariant::CardCompleted {
            card_id: card_id.clone(),
            completed_at: *completed_at,
        },
        Event::CardReassigned {
            card_id,
            assigned_id,
            assigned_kind,
            session_id,
            attempt_bookmark,
            previous_session_id,
            reassigned_at,
        } => StoredVariant::CardReassigned {
            card_id: card_id.clone(),
            assigned_id: assigned_id.clone(),
            assigned_kind: assigned_kind.clone(),
            session_id: session_id.clone(),
            attempt_bookmark: attempt_bookmark.clone(),
            previous_session_id: previous_session_id.clone(),
            reassigned_at: *reassigned_at,
        },
        Event::SetupDeclared {
            scope,
            skills,
            mcp,
            plugins,
            hooks,
            declared_at,
        } => StoredVariant::SetupDeclared {
            scope: scope.clone(),
            skills: skills.clone(),
            mcp: mcp.clone(),
            plugins: plugins.clone(),
            hooks: hooks.clone(),
            declared_at: *declared_at,
        },
        Event::EntryRegistered {
            kind,
            slug,
            definition,
            registered_at,
        } => StoredVariant::EntryRegistered {
            kind: kind.clone(),
            slug: slug.clone(),
            definition: definition.clone(),
            registered_at: *registered_at,
        },
        Event::SessionForked {
            parent_session_id,
            child_session_id,
            fork_type,
            fork_point,
            forked_at,
        } => StoredVariant::SessionForked {
            parent_session_id: parent_session_id.clone(),
            child_session_id: child_session_id.clone(),
            fork_type: fork_type.clone(),
            fork_point: *fork_point,
            forked_at: *forked_at,
        },
        Event::CardSessionLinked {
            card_id,
            session_id,
            linked_at,
        } => StoredVariant::CardSessionLinked {
            card_id: card_id.clone(),
            session_id: session_id.clone(),
            linked_at: *linked_at,
        },
        Event::SessionHandover {
            source_session_id,
            target_session_id,
            from_agent_kind,
            to_agent_kind,
            translated_message_count,
            handed_over_at,
        } => StoredVariant::SessionHandover {
            source_session_id: source_session_id.clone(),
            target_session_id: target_session_id.clone(),
            from_agent_kind: from_agent_kind.clone(),
            to_agent_kind: to_agent_kind.clone(),
            translated_message_count: *translated_message_count,
            handed_over_at: *handed_over_at,
        },
        Event::RepoRegistered {
            slug,
            url,
            default_branch,
            registered_at,
        } => StoredVariant::RepoRegistered {
            slug: slug.clone(),
            url: url.clone(),
            default_branch: default_branch.clone(),
            registered_at: *registered_at,
        },
        Event::RepoRemoved { slug, removed_at } => StoredVariant::RepoRemoved {
            slug: slug.clone(),
            removed_at: *removed_at,
        },
        Event::SessionRepoAttached {
            session_id,
            slug,
            attached_at,
        } => StoredVariant::SessionRepoAttached {
            session_id: session_id.clone(),
            slug: slug.clone(),
            attached_at: *attached_at,
        },
        Event::ProjectCreated {
            project_id,
            name,
            created_at,
        } => StoredVariant::ProjectCreated {
            project_id: project_id.clone(),
            name: name.clone(),
            created_at: *created_at,
        },
        Event::ProjectUpdated {
            project_id,
            name,
            vaults,
            repos,
            boards,
        } => StoredVariant::ProjectUpdated {
            project_id: project_id.clone(),
            name: name.clone(),
            vaults: vaults.clone(),
            repos: repos.clone(),
            boards: boards.clone(),
        },
        Event::ProjectDeleted {
            project_id,
            deleted_at,
        } => StoredVariant::ProjectDeleted {
            project_id: project_id.clone(),
            deleted_at: *deleted_at,
        },
        Event::SessionProjectAttached {
            session_id,
            project_id,
            attached_at,
        } => StoredVariant::SessionProjectAttached {
            session_id: session_id.clone(),
            project_id: project_id.clone(),
            attached_at: *attached_at,
        },
        Event::SessionOrganizationAssigned {
            session_id,
            organization_id,
        } => StoredVariant::SessionOrganizationAssigned {
            session_id: session_id.clone(),
            organization_id: organization_id.clone(),
        },
        Event::ProjectOrganizationAssigned {
            project_id,
            organization_id,
        } => StoredVariant::ProjectOrganizationAssigned {
            project_id: project_id.clone(),
            organization_id: organization_id.clone(),
        },
        Event::CardOrganizationAssigned {
            card_id,
            organization_id,
        } => StoredVariant::CardOrganizationAssigned {
            card_id: card_id.clone(),
            organization_id: organization_id.clone(),
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
            agent,
            node,
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
            agent,
            node,
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
        StoredVariant::MessageRemoved {
            session_id,
            hermes_session_id,
            message_id,
        } => Event::MessageRemoved {
            session_id,
            hermes_session_id,
            message_id,
        },
        StoredVariant::SessionUpdated {
            session_id,
            title,
            model,
            archived,
            message_count,
            agent,
            node,
            hermes_id,
        } => Event::SessionUpdated {
            session_id,
            title,
            model,
            archived,
            message_count,
            agent,
            node,
            hermes_id,
            pinned: None,
        },
        StoredVariant::SessionUpdatedV2 {
            session_id,
            title,
            model,
            archived,
            message_count,
            agent,
            node,
            hermes_id,
            pinned,
        } => Event::SessionUpdated {
            session_id,
            title,
            model,
            archived,
            message_count,
            agent,
            node,
            hermes_id,
            pinned,
        },
        StoredVariant::CardCreated {
            card_id,
            board_id,
            title,
            created_at,
        } => Event::CardCreated {
            card_id,
            board_id,
            title,
            created_at,
        },
        StoredVariant::CardAssigned {
            card_id,
            assigned_id,
            assigned_kind,
            session_id,
            attempt_bookmark,
            assigned_at,
        } => Event::CardAssigned {
            card_id,
            assigned_id,
            assigned_kind,
            session_id,
            attempt_bookmark,
            assigned_at,
        },
        StoredVariant::CardClaimed {
            card_id,
            claimed_at,
        } => Event::CardClaimed {
            card_id,
            claimed_at,
        },
        StoredVariant::CardBlocked {
            card_id,
            blocked_by,
            blocked_at,
        } => Event::CardBlocked {
            card_id,
            blocked_by,
            blocked_at,
        },
        StoredVariant::CardCompleted {
            card_id,
            completed_at,
        } => Event::CardCompleted {
            card_id,
            completed_at,
        },
        StoredVariant::CardReassigned {
            card_id,
            assigned_id,
            assigned_kind,
            session_id,
            attempt_bookmark,
            previous_session_id,
            reassigned_at,
        } => Event::CardReassigned {
            card_id,
            assigned_id,
            assigned_kind,
            session_id,
            attempt_bookmark,
            previous_session_id,
            reassigned_at,
        },
        StoredVariant::SetupDeclared {
            scope,
            skills,
            mcp,
            plugins,
            hooks,
            declared_at,
        } => Event::SetupDeclared {
            scope,
            skills,
            mcp,
            plugins,
            hooks,
            declared_at,
        },
        StoredVariant::EntryRegistered {
            kind,
            slug,
            definition,
            registered_at,
        } => Event::EntryRegistered {
            kind,
            slug,
            definition,
            registered_at,
        },
        StoredVariant::SessionForked {
            parent_session_id,
            child_session_id,
            fork_type,
            fork_point,
            forked_at,
        } => Event::SessionForked {
            parent_session_id,
            child_session_id,
            fork_type,
            fork_point,
            forked_at,
        },
        StoredVariant::CardSessionLinked {
            card_id,
            session_id,
            linked_at,
        } => Event::CardSessionLinked {
            card_id,
            session_id,
            linked_at,
        },
        StoredVariant::SessionHandover {
            source_session_id,
            target_session_id,
            from_agent_kind,
            to_agent_kind,
            translated_message_count,
            handed_over_at,
        } => Event::SessionHandover {
            source_session_id,
            target_session_id,
            from_agent_kind,
            to_agent_kind,
            translated_message_count,
            handed_over_at,
        },
        StoredVariant::RepoRegistered {
            slug,
            url,
            default_branch,
            registered_at,
        } => Event::RepoRegistered {
            slug,
            url,
            default_branch,
            registered_at,
        },
        StoredVariant::RepoRemoved { slug, removed_at } => Event::RepoRemoved { slug, removed_at },
        StoredVariant::SessionRepoAttached {
            session_id,
            slug,
            attached_at,
        } => Event::SessionRepoAttached {
            session_id,
            slug,
            attached_at,
        },
        StoredVariant::ProjectCreated {
            project_id,
            name,
            created_at,
        } => Event::ProjectCreated {
            project_id,
            name,
            created_at,
        },
        StoredVariant::ProjectUpdated {
            project_id,
            name,
            vaults,
            repos,
            boards,
        } => Event::ProjectUpdated {
            project_id,
            name,
            vaults,
            repos,
            boards,
        },
        StoredVariant::ProjectDeleted {
            project_id,
            deleted_at,
        } => Event::ProjectDeleted {
            project_id,
            deleted_at,
        },
        StoredVariant::SessionProjectAttached {
            session_id,
            project_id,
            attached_at,
        } => Event::SessionProjectAttached {
            session_id,
            project_id,
            attached_at,
        },
        StoredVariant::SessionOrganizationAssigned {
            session_id,
            organization_id,
        } => Event::SessionOrganizationAssigned {
            session_id,
            organization_id,
        },
        StoredVariant::ProjectOrganizationAssigned {
            project_id,
            organization_id,
        } => Event::ProjectOrganizationAssigned {
            project_id,
            organization_id,
        },
        StoredVariant::CardOrganizationAssigned {
            card_id,
            organization_id,
        } => Event::CardOrganizationAssigned {
            card_id,
            organization_id,
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

    /// Rewrite the log keeping only Olympus-NATIVE events, dropping everything
    /// derived from a `state.db` import. Called on boot before re-importing
    /// `state.db`, so Olympus-native durable records (setup declarations, cards,
    /// olympus-source sessions + their messages) survive a restart while the
    /// state.db mirror is rebuilt fresh (keeping import idempotent).
    ///
    /// Native = SetupDeclared, all Card* events, and SessionCreated with
    /// `source == "olympus"` plus the messages/updates belonging to those
    /// olympus sessions. Everything else (imported sessions/messages) is dropped.
    pub fn retain_native(&self) -> Result<()> {
        // Collect the survivors first (read txn), then rewrite (write txn).
        let all = self.read_all()?;
        // Which session ids are olympus-native? (their SessionCreated said so)
        let mut native_sessions = std::collections::HashSet::new();
        for (_seq, ev) in &all {
            if let Event::SessionCreated {
                session_id, source, ..
            } = ev
            {
                if source == "olympus" {
                    native_sessions.insert(session_id.clone());
                }
            }
        }
        let is_native = |ev: &Event| -> bool {
            match ev {
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
                | Event::SessionOrganizationAssigned { session_id, .. } => {
                    native_sessions.contains(session_id)
                }
            }
        };
        let survivors: Vec<Event> = all
            .into_iter()
            .filter(|(_s, ev)| is_native(ev))
            .map(|(_s, ev)| ev)
            .collect();

        // Rewrite: clear the events table + reset the seq, then re-append.
        let txn = self.db.begin_write().context("begin write for retain")?;
        {
            let mut events = txn.open_table(EVENTS)?;
            // redb has no truncate; delete every key.
            let keys: Vec<u64> = events
                .iter()?
                .filter_map(|it| it.ok().map(|(k, _v)| k.value()))
                .collect();
            for k in keys {
                events.remove(k)?;
            }
            let mut meta = txn.open_table(META)?;
            write_next_seq(&mut meta, 0)?;
        }
        txn.commit()?;
        // Re-append survivors in order (contiguous seqs from 0).
        self.append_batch(&survivors)?;
        Ok(())
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

    #[test]
    fn retain_native_keeps_setup_and_olympus_sessions_drops_imported() {
        let (_f, log) = fresh_log();
        // Olympus-native: a setup declaration + an olympus session + its message.
        log.append(&Event::SetupDeclared {
            scope: "org:acme".into(),
            skills: vec!["code-review".into()],
            mcp: vec![],
            plugins: vec![],
            hooks: vec![],
            declared_at: 1.0,
        })
        .unwrap();
        log.append(&Event::SessionCreated {
            session_id: "oly-1".into(),
            hermes_id: "h".into(),
            source: "olympus".into(),
            model: None,
            title: None,
            started_at: 2.0,
            message_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            agent: Some("coding-agent".into()),
            node: None,
        })
        .unwrap();
        log.append(&sample_message("oly-1", 0, "hi")).unwrap();
        // Imported (state.db): a cli session + its message.
        log.append(&sample_session_created("cli-1")).unwrap(); // source=cli
        log.append(&sample_message("cli-1", 0, "imported")).unwrap();

        log.retain_native().unwrap();

        let kept: Vec<Event> = log
            .read_all()
            .unwrap()
            .into_iter()
            .map(|(_s, e)| e)
            .collect();
        // Setup + olympus session + its message survive; cli session + msg dropped.
        assert!(kept
            .iter()
            .any(|e| matches!(e, Event::SetupDeclared { scope, .. } if scope == "org:acme")));
        assert!(kept.iter().any(
            |e| matches!(e, Event::SessionCreated { session_id, .. } if session_id == "oly-1")
        ));
        assert!(kept.iter().any(
            |e| matches!(e, Event::MessageAppended { session_id, .. } if session_id == "oly-1")
        ));
        assert!(!kept.iter().any(
            |e| matches!(e, Event::SessionCreated { session_id, .. } if session_id == "cli-1")
        ));
        assert!(!kept.iter().any(
            |e| matches!(e, Event::MessageAppended { session_id, .. } if session_id == "cli-1")
        ));
        // Seqs are contiguous from 0 after the rewrite.
        let seqs: Vec<u64> = log
            .read_all()
            .unwrap()
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        assert_eq!(seqs, vec![0, 1, 2]);
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
            agent: None,
            node: None,
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
