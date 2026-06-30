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

use std::collections::HashMap;
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
}

impl BridgeManager {
    /// Create a bridge manager with the given runtime factory.
    pub fn with_factory(log: Arc<Log>, factory: RuntimeFactory) -> Self {
        Self {
            log,
            factory,
            runtimes: RwLock::new(HashMap::new()),
        }
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
        let now = chrono_epoch();
        // A client-stable id derived from creation time; the real Hermes id is
        // backfilled lazily so the UI never blocks on the handshake.
        let session_id = format!("oly-draft-{}", chrono_millis());

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

        let now = chrono_epoch();
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
        })?;
        Ok(())
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
        let now = chrono_epoch();
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
        finish_reason: Option<&str>,
    ) -> Result<Event> {
        let now = chrono_epoch();
        let event = Event::MessageAppended {
            session_id: session_id.to_string(),
            hermes_session_id: hermes_id.to_string(),
            message_id,
            role: "assistant".into(),
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
fn chrono_epoch() -> f64 {
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
