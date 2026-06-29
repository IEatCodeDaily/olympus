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

/// A type-erased runtime factory. Production uses HermesAgentRuntime; tests
/// inject a mock.
pub type RuntimeFactory = Arc<dyn Fn() -> Arc<dyn AgentRuntime> + Send + Sync>;

/// The result of creating a new managed session: the Olympus session id and the
/// Hermes session id captured from the ACP `session/new` response.
pub struct NewSession {
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

    /// Create a new managed session: spawn a runtime, start it (ACP handshake +
    /// session/new), append a SessionCreated event to the log, and register the
    /// runtime for later prompt calls.
    ///
    /// Returns the session ids so the caller can build the DTO.
    pub async fn create_session(&self) -> Result<NewSession> {
        let runtime = (self.factory)();
        runtime
            .start(None)
            .await
            .context("starting agent runtime")?;

        // The runtime has captured the Hermes session id from session/new.
        // We need to retrieve it. HermesAgentRuntime stores it internally;
        // for the trait we add a method to expose it.
        let hermes_id = runtime
            .hermes_session_id()
            .await
            .unwrap_or_else(|| format!("sess-{}", chrono_millis()));

        let session_id = format!("oly-{}", &hermes_id[..hermes_id.len().min(8)]);

        // Append SessionCreated to the durable log.
        let now = chrono_epoch();
        let event = Event::SessionCreated {
            session_id: session_id.clone(),
            hermes_id: hermes_id.clone(),
            source: "olympus".into(),
            model: None,
            title: None,
            started_at: now,
            message_count: 0,
            input_tokens: 0,
            output_tokens: 0,
        };
        self.log
            .append(&event)
            .context("appending SessionCreated")?;

        // Register the runtime.
        self.runtimes
            .write()
            .await
            .insert(session_id.clone(), runtime);

        Ok(NewSession {
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

    /// Append a user message event to the log for a session.
    pub fn append_user_message(&self, session_id: &str, hermes_id: &str, text: &str) -> Result<()> {
        let now = chrono_epoch();
        self.log.append(&Event::MessageAppended {
            session_id: session_id.to_string(),
            hermes_session_id: hermes_id.to_string(),
            message_id: now as u64,
            role: "user".into(),
            content: Some(text.to_string()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: now,
            token_count: None,
            finish_reason: None,
        })?;
        Ok(())
    }

    /// Append the final assistant message to the log.
    pub fn append_assistant_message(
        &self,
        session_id: &str,
        hermes_id: &str,
        message_id: u64,
        text: &str,
        finish_reason: Option<&str>,
    ) -> Result<()> {
        let now = chrono_epoch();
        self.log.append(&Event::MessageAppended {
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
        })?;
        Ok(())
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
