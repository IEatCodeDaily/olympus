//! Bridge manager — owns agent runtime instances per managed session.
//!
//! Each Olympus-managed session has a [`HermesAgentRuntime`] (or test mock) that
//! drives a real `hermes acp` child process. The [`BridgeManager`] creates and
//! registers runtimes, and provides access for the REST handlers to send prompts
//! and stream events.
//!
//! In production the runtime factory spawns a real `hermes acp`; in tests a
//! mock factory returns a fake runtime so the server can be exercised without
//! the real binary.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::RwLock;

use crate::bridge::AgentRuntime;
use crate::event::Event;
use crate::log::Log;

/// What an agent runtime needs to spawn: which agent (Hermes profile) drives it
/// and on which node. The factory turns this into a concrete runtime.
#[derive(Debug, Clone, Default)]
pub struct RuntimeSpec {
    /// Hermes profile to run as (`None` → the server's default profile).
    pub agent: Option<String>,
    /// Node to run on ("local" for now; multi-node is post-MVP).
    pub node: Option<String>,
    /// The session space — the agent's working directory. `None` falls back to
    /// the server's cwd (legacy behavior); production always sets this to the
    /// per-session space so agents operate in a scoped directory, not the host.
    pub cwd: Option<String>,
    /// MCP servers to inject into the ACP session/new request (resolved from
    /// the registry by the setup adapter). Each value is the harness's native
    /// MCP server JSON. `None`/empty → no MCP servers (legacy behavior).
    pub mcp_servers: Vec<serde_json::Value>,
    /// Extra environment variables for the child process (from the setup
    /// adapter, e.g. HERMES_SKILLS_PATH). Default empty.
    pub env: Vec<(String, String)>,
}

/// A type-erased runtime factory. Production uses HermesAgentRuntime; tests
/// inject a mock. The spec carries the agent/node binding so the factory can
/// route to the right Hermes profile.
pub type RuntimeFactory = Arc<dyn Fn(&RuntimeSpec) -> Arc<dyn AgentRuntime> + Send + Sync>;

/// The result of creating a new managed session: the Olympus session id and the
/// Hermes session id captured from the ACP `session/new` response.
pub struct NewSession {
    pub session_id: String,
    pub hermes_id: String,
    /// Creation timestamp (epoch seconds) — lets the caller build the view row /
    /// DTO without re-reading the whole log.
    pub started_at: f64,
    /// The materialized session-space path, if a spaces root is configured.
    pub space: Option<String>,
}

/// The result of forking an observed session into a managed one.
pub struct ForkedSession {
    pub session_id: String,
    pub hermes_id: String,
}

/// Manages the lifecycle of agent runtimes for managed (olympus-source) sessions.
pub struct BridgeManager {
    /// Event log (for appending SessionCreated / MessageAppended events).
    log: Arc<Log>,
    /// Factory that produces a fresh runtime per session.
    factory: RuntimeFactory,
    /// Active runtimes keyed by Olympus session id.
    runtimes: RwLock<HashMap<String, Arc<dyn AgentRuntime>>>,
    /// Sessions with a turn currently in-flight (prompt sent, awaiting Done).
    /// Authoritative liveness signal for Olympus-managed sessions.
    in_flight: RwLock<HashSet<String>>,
    /// Sessions blocked on a permission decision, keyed by session id → the
    /// pending ACP `session/request_permission` request id to respond to. A
    /// session in this map has liveness "input-required".
    awaiting_input: RwLock<HashMap<String, String>>,
    /// Root directory under which per-session spaces are materialized
    /// (`<spaces_root>/<session_id>/`), i.e. `~/.olympus/<org>/sessions/` per
    /// ADR 0005 §4. `None` disables space creation — used by tests so they never
    /// touch the filesystem. Set in production from the org-scoped workspace root.
    spaces_root: Option<PathBuf>,
}

impl BridgeManager {
    /// Create a bridge manager with the given runtime factory.
    pub fn with_factory(log: Arc<Log>, factory: RuntimeFactory) -> Self {
        Self {
            log,
            factory,
            runtimes: RwLock::new(HashMap::new()),
            in_flight: RwLock::new(HashSet::new()),
            awaiting_input: RwLock::new(HashMap::new()),
            spaces_root: None,
        }
    }

    /// Set the root directory for per-session spaces (builder style). When set,
    /// `create_draft` eagerly materializes `<spaces_root>/<session_id>/` and the
    /// agent runs with that as its cwd.
    pub fn with_spaces_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.spaces_root = Some(root.into());
        self
    }

    /// The on-disk path of a session's space, if a spaces root is configured.
    pub fn space_path(&self, session_id: &str) -> Option<PathBuf> {
        self.spaces_root.as_ref().map(|r| r.join(session_id))
    }

    /// Materialize a session's space directory (idempotent). Returns the path,
    /// or `None` if no spaces root is configured (tests). A bare space is just
    /// an empty dir; it becomes a jj worktree only when a repo is attached.
    pub fn ensure_space(&self, session_id: &str) -> Result<Option<PathBuf>> {
        let Some(path) = self.space_path(session_id) else {
            return Ok(None);
        };
        std::fs::create_dir_all(&path)
            .with_context(|| format!("creating session space {}", path.display()))?;
        Ok(Some(path))
    }

    /// Remove a session's space directory (best-effort GC on archive/delete).
    pub fn remove_space(&self, session_id: &str) {
        if let Some(path) = self.space_path(session_id) {
            if path.exists() {
                if let Err(e) = std::fs::remove_dir_all(&path) {
                    tracing::warn!(error = %e, path = %path.display(), "failed to remove session space");
                }
            }
        }
    }

    /// Mint a fresh durable Olympus session id: `<utc-compact>-<hash>`, e.g.
    /// `20260630T154812Z-a1b2c3d4`. Stable from birth (no draft→real rename) —
    /// only the space and agent session are lazy. Node is NOT baked into the id
    /// (per ADR 0005 §6): the node is inferred from the chosen agent and stored
    /// as a separate field, so the id stays portable across nodes.
    fn new_session_id(&self) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // Compact UTC stamp without pulling chrono: derive YYYYMMDDThhmmssZ.
        let stamp = compact_utc_stamp(now);
        let hash = &uuid::Uuid::new_v4().simple().to_string()[..8];
        format!("{stamp}-{hash}")
    }

    /// Mark a session as having a turn in-flight (called when a prompt is sent).
    pub async fn mark_in_flight(&self, session_id: &str) {
        self.in_flight.write().await.insert(session_id.to_string());
    }

    /// Clear the in-flight flag (called when the turn's Done/Error arrives).
    pub async fn clear_in_flight(&self, session_id: &str) {
        self.in_flight.write().await.remove(session_id);
    }

    /// Snapshot of all session ids with a turn currently in-flight.
    pub async fn in_flight_set(&self) -> HashSet<String> {
        self.in_flight.read().await.clone()
    }

    /// Mark a session as blocked awaiting a permission decision, storing the
    /// ACP request id to respond to later.
    pub async fn mark_awaiting_input(&self, session_id: &str, request_id: &str) {
        self.awaiting_input
            .write()
            .await
            .insert(session_id.to_string(), request_id.to_string());
    }

    /// Clear the awaiting-input flag (permission answered or turn ended).
    pub async fn clear_awaiting_input(&self, session_id: &str) {
        self.awaiting_input.write().await.remove(session_id);
    }

    /// Snapshot of session ids currently blocked awaiting a permission decision.
    pub async fn awaiting_input_set(&self) -> HashSet<String> {
        self.awaiting_input.read().await.keys().cloned().collect()
    }

    /// Respond to a session's pending permission request with the chosen option
    /// (or `None` to cancel). Looks up the stored request id, forwards it to the
    /// runtime, and clears the awaiting flag. Errors if no pending request.
    pub async fn respond_permission(
        &self,
        session_id: &str,
        option_id: Option<&str>,
    ) -> Result<()> {
        let request_id = self
            .awaiting_input
            .read()
            .await
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no pending permission request for session"))?;
        let runtime = self
            .runtimes
            .read()
            .await
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no runtime for session"))?;
        runtime.respond_permission(&request_id, option_id).await?;
        self.clear_awaiting_input(session_id).await;
        Ok(())
    }

    /// Create a new managed session **optimistically** — no agent runtime is
    /// spawned. This returns instantly: it allocates an Olympus session id,
    /// appends a `SessionCreated` event (source=olympus, managed) with the
    /// chosen agent/node, and returns. The expensive ACP handshake is deferred
    /// to the first [`Self::ensure_runtime`] call (i.e. the first send).
    ///
    /// `hermes_id` is empty until the runtime actually starts and captures it;
    /// it is backfilled via a `SessionUpdated{hermes_id}` event on first send.
    pub fn create_draft(&self, spec: &RuntimeSpec) -> Result<NewSession> {
        let now = chrono_epoch_pub();
        // A durable id, stable from birth: `<utc>-<node>-<hash>`. There is no
        // draft→real rename — only the space and agent session are lazy.
        let session_id = self.new_session_id();

        // Eagerly materialize the session space (the agent's working directory).
        // A bare space is just an empty dir; cheap, and it means an agent is
        // never spawned without a scoped cwd. GC'd on archive/delete.
        let space = self.ensure_space(&session_id)?;

        let event = Event::SessionCreated {
            session_id: session_id.clone(),
            hermes_id: String::new(),
            source: "olympus".into(),
            model: None,
            title: None,
            started_at: now,
            message_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            agent: spec.agent.clone(),
            node: spec.node.clone(),
        };
        self.log
            .append(&event)
            .context("appending SessionCreated (draft)")?;

        Ok(NewSession {
            session_id,
            hermes_id: String::new(),
            started_at: now,
            space: space.map(|p| p.to_string_lossy().into_owned()),
        })
    }

    /// Ensure a runtime exists for a managed session, spawning it lazily on the
    /// first send. Returns the runtime plus the (possibly newly captured)
    /// Hermes session id.
    ///
    /// - If a runtime is already registered, returns it (no spawn).
    /// - Otherwise spawns one via the factory and performs the ACP handshake.
    ///   When `resume_hermes_id` is `Some` and non-empty, it resumes that Hermes
    ///   session (survives server restarts); otherwise it creates a fresh one.
    ///
    /// On a fresh start the captured Hermes id is returned in `NewSession` so the
    /// caller can backfill it onto the session row.
    pub async fn ensure_runtime(
        &self,
        session_id: &str,
        spec: &RuntimeSpec,
        resume_hermes_id: Option<&str>,
    ) -> Result<(Arc<dyn AgentRuntime>, String)> {
        if let Some(rt) = self.runtimes.read().await.get(session_id).cloned() {
            let hid = rt.hermes_session_id().await.unwrap_or_default();
            return Ok((rt, hid));
        }

        let runtime = (self.factory)(spec);
        let resume = resume_hermes_id.filter(|s| !s.is_empty());
        runtime
            .start(resume)
            .await
            .context("starting agent runtime (lazy)")?;
        let hermes_id = runtime
            .hermes_session_id()
            .await
            .unwrap_or_else(|| format!("sess-{}", chrono_millis()));

        self.runtimes
            .write()
            .await
            .insert(session_id.to_string(), runtime.clone());

        Ok((runtime, hermes_id))
    }

    /// Fork an existing Hermes session into a new managed Olympus session,
    /// append the SessionCreated event, and register the runtime for prompts.
    pub async fn fork_session(
        &self,
        source_hermes_id: &str,
        model: Option<String>,
        title: Option<String>,
        message_count: u64,
    ) -> Result<ForkedSession> {
        let runtime = (self.factory)(&RuntimeSpec::default());
        runtime
            .fork_session(source_hermes_id)
            .await
            .context("forking agent runtime session")?;

        let hermes_id = runtime
            .hermes_session_id()
            .await
            .unwrap_or_else(|| format!("fork-{}", chrono_millis()));
        let session_id = format!("oly-{}", &hermes_id[..hermes_id.len().min(8)]);

        // A fork is a real managed session — give it its own space too.
        let _ = self.ensure_space(&session_id);

        let now = chrono_epoch_pub();
        self.log.append(&Event::SessionCreated {
            session_id: session_id.clone(),
            hermes_id: hermes_id.clone(),
            source: "olympus".into(),
            model,
            title,
            started_at: now,
            message_count,
            input_tokens: 0,
            output_tokens: 0,
            agent: None,
            node: None,
        })?;

        self.runtimes
            .write()
            .await
            .insert(session_id.clone(), runtime);

        Ok(ForkedSession {
            session_id,
            hermes_id,
        })
    }

    /// Send a prompt to a managed session and return the runtime so the caller
    /// can drain its event stream. Returns None if the session is not managed
    /// (or unknown).
    pub async fn get_runtime(&self, session_id: &str) -> Option<Arc<dyn AgentRuntime>> {
        self.runtimes.read().await.get(session_id).cloned()
    }

    /// Append a SessionUpdated event that backfills the real Hermes id captured
    /// when a lazily-spawned runtime started.
    pub fn backfill_hermes_id(&self, session_id: &str, hermes_id: &str) -> Result<()> {
        self.log.append(&Event::SessionUpdated {
            session_id: session_id.to_string(),
            title: None,
            model: None,
            archived: None,
            message_count: None,
            agent: None,
            node: None,
            hermes_id: Some(hermes_id.to_string()),
            pinned: None,
        })?;
        Ok(())
    }

    /// Append a SessionUpdated event that sets the session title. Used to derive
    /// a human title from the first user message when the session has none
    /// (API/UI-created sessions start title-less and otherwise show "Untitled").
    /// Returns the event so the caller can apply it to the views + broadcast it.
    pub fn set_title(&self, session_id: &str, title: &str) -> Result<Event> {
        let event = Event::SessionUpdated {
            session_id: session_id.to_string(),
            title: Some(title.to_string()),
            model: None,
            archived: None,
            message_count: None,
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        };
        self.log.append(&event)?;
        Ok(event)
    }

    /// Append a user message event to the log for a session, returning the event
    /// so the caller can also apply it to the in-memory views (the log is the
    /// source of truth, but the views serve reads and must stay current).
    pub fn append_user_message(
        &self,
        session_id: &str,
        hermes_id: &str,
        message_id: u64,
        text: &str,
    ) -> Result<Event> {
        let now = chrono_epoch_pub();
        let event = Event::MessageAppended {
            session_id: session_id.to_string(),
            hermes_session_id: hermes_id.to_string(),
            message_id,
            role: "user".into(),
            content: Some(text.to_string()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: now,
            token_count: None,
            finish_reason: None,
        };
        self.log.append(&event)?;
        Ok(event)
    }

    /// Append the final assistant message to the log, returning the event so the
    /// caller can apply it to the views.
    pub fn append_assistant_message(
        &self,
        session_id: &str,
        hermes_id: &str,
        message_id: u64,
        text: &str,
        tool_calls: &Option<String>,
        finish_reason: Option<&str>,
    ) -> Result<Event> {
        let now = chrono_epoch_pub();
        let event = Event::MessageAppended {
            session_id: session_id.to_string(),
            hermes_session_id: hermes_id.to_string(),
            message_id,
            role: "assistant".into(),
            content: Some(text.to_string()),
            tool_name: None,
            tool_calls: tool_calls.clone(),
            reasoning: None,
            timestamp: now,
            token_count: None,
            finish_reason: finish_reason.map(|s| s.to_string()),
        };
        self.log.append(&event)?;
        Ok(event)
    }

    /// Append a system message to the log for synthetic control-plane notices
    /// such as agent runtime errors, returning the event so callers can also
    /// apply it to the in-memory views.
    pub fn append_system_message(
        &self,
        session_id: &str,
        hermes_id: &str,
        message_id: u64,
        text: &str,
        finish_reason: Option<&str>,
    ) -> Result<Event> {
        let now = chrono_epoch_pub();
        let event = Event::MessageAppended {
            session_id: session_id.to_string(),
            hermes_session_id: hermes_id.to_string(),
            message_id,
            role: "system".into(),
            content: Some(text.to_string()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: now,
            token_count: None,
            finish_reason: finish_reason.map(|s| s.to_string()),
        };
        self.log.append(&event)?;
        Ok(event)
    }
}

/// Current epoch seconds as f64.
pub fn chrono_epoch_pub() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Current epoch milliseconds as u128 (for unique id generation).
fn chrono_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Format epoch seconds as a compact UTC stamp `YYYYMMDDThhmmssZ` (no chrono).
/// Uses the civil-from-days algorithm (Howard Hinnant) for the date part.
fn compact_utc_stamp(epoch_secs: u64) -> String {
    let days = (epoch_secs / 86_400) as i64;
    let secs_of_day = epoch_secs % 86_400;
    let (hh, mm, ss) = (
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    );
    // days since 1970-01-01 → civil date (Hinnant's algorithm).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}{m:02}{d:02}T{hh:02}{mm:02}{ss:02}Z")
}

#[cfg(test)]
mod space_tests {
    use super::*;

    fn test_log() -> (tempfile::NamedTempFile, Arc<Log>) {
        let f = tempfile::NamedTempFile::new().unwrap();
        let log = Arc::new(Log::open(f.path()).unwrap());
        (f, log)
    }

    #[test]
    fn compact_utc_stamp_matches_known_epoch() {
        // 2021-01-01T00:00:00Z = 1609459200
        assert_eq!(compact_utc_stamp(1_609_459_200), "20210101T000000Z");
        // 1970-01-01T00:00:00Z = 0
        assert_eq!(compact_utc_stamp(0), "19700101T000000Z");
        // 2026-06-30T15:48:12Z = 1782834492
        assert_eq!(compact_utc_stamp(1_782_834_492), "20260630T154812Z");
    }

    #[test]
    fn new_session_id_has_expected_shape() {
        let (_f, log) = test_log();
        let mgr = BridgeManager::with_factory(log, crate::server::test_support::mock_factory());
        let id = mgr.new_session_id();
        // <utc-stamp>-<hash8> — node is NOT in the id (ADR 0005 §6).
        assert!(id.starts_with("20") || id.starts_with("19"), "id = {id}");
        let hash = id.rsplit('-').next().unwrap();
        assert_eq!(hash.len(), 8, "hash part should be 8 chars: {id}");
        // Exactly two segments: the stamp and the hash (no node segment).
        assert_eq!(
            id.matches('-').count(),
            1,
            "id should be <stamp>-<hash>: {id}"
        );
    }

    #[test]
    fn ensure_and_remove_space_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("olympus-space-test-{}", chrono_millis()));
        let (_f, log) = test_log();
        let mgr = BridgeManager::with_factory(log, crate::server::test_support::mock_factory())
            .with_spaces_root(&tmp);
        let path = mgr.ensure_space("sess-1").unwrap().unwrap();
        assert!(path.exists() && path.is_dir());
        assert!(path.ends_with("sess-1"));
        mgr.remove_space("sess-1");
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn no_spaces_root_means_no_space() {
        let (_f, log) = test_log();
        let mgr = BridgeManager::with_factory(log, crate::server::test_support::mock_factory());
        assert!(mgr.space_path("sess-1").is_none());
        assert!(mgr.ensure_space("sess-1").unwrap().is_none());
    }

    #[test]
    fn create_draft_with_spaces_root_materializes_and_returns_space() {
        let tmp = std::env::temp_dir().join(format!("olympus-draft-test-{}", chrono_millis()));
        let (_f, log) = test_log();
        let mgr = BridgeManager::with_factory(log, crate::server::test_support::mock_factory())
            .with_spaces_root(&tmp);
        let ns = mgr.create_draft(&RuntimeSpec::default()).unwrap();
        // id is <stamp>-<hash> (no node segment, ADR 0005 §6).
        assert_eq!(ns.session_id.matches('-').count(), 1);
        let space = ns.space.expect("space path should be set");
        assert!(std::path::Path::new(&space).is_dir());
        assert!(space.ends_with(&ns.session_id));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
