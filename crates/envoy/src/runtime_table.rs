//! Per-session runtime table — the envoy-side half of the old monolith
//! `BridgeManager` (ADR 0008 milestone S2).
//!
//! Owns the `session id → runtime` map and the factory that spawns runtimes.
//! Session bookkeeping (draft creation, spaces, event-log appends, hermes-id
//! backfill) stays hall-side; the hall calls into this table through the
//! factory seam.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::RwLock;

use crate::bridge::{AgentCommand, AgentRuntime};
use olympus_proto::RuntimeSpec;

/// A type-erased runtime factory. Production uses HermesAgentRuntime; tests
/// inject a mock. The spec carries the agent/node binding so the factory can
/// route to the right Hermes profile.
pub type RuntimeFactory = Arc<dyn Fn(&RuntimeSpec) -> Arc<dyn AgentRuntime> + Send + Sync>;

/// One registered runtime plus the capability flags captured from its
/// adapter's `initialize` response (ADR 0008 §3).
pub struct RuntimeEntry {
    pub runtime: Arc<dyn AgentRuntime>,
    /// Whether the adapter advertised cross-process session resume
    /// (`agentCapabilities.loadSession` + `sessionCapabilities.resume`).
    /// Fail closed: false when the capability was absent or never captured.
    pub resumable: bool,
    /// Last time this runtime received a command (prompt, steer, etc.).
    /// Used by `reap_idle()` to terminate sessions that have been idle
    /// longer than the configured threshold. Updated on every `ensure_runtime`
    /// and `send` call.
    pub last_activity: std::time::Instant,
}

/// The result of forking a source agent session into a fresh runtime: the
/// started runtime and the agent session id it captured.
pub struct ForkedRuntime {
    pub runtime: Arc<dyn AgentRuntime>,
    pub hermes_id: String,
}

/// Active agent runtimes keyed by Olympus session id, plus the factory that
/// spawns them (ensure/send/stop per-session mechanics).
pub struct RuntimeTable {
    /// Factory that produces a fresh runtime per session.
    factory: RuntimeFactory,
    /// Active runtimes keyed by Olympus session id.
    runtimes: RwLock<HashMap<String, RuntimeEntry>>,
}

impl RuntimeTable {
    /// Create a runtime table with the given runtime factory.
    pub fn with_factory(factory: RuntimeFactory) -> Self {
        Self {
            factory,
            runtimes: RwLock::new(HashMap::new()),
        }
    }

    /// Ensure a runtime exists for a managed session, spawning it lazily on
    /// the first send. Returns the runtime plus the (possibly newly captured)
    /// Hermes session id.
    ///
    /// - If a runtime is already registered, returns it (no spawn).
    /// - Otherwise spawns one via the factory and performs the ACP handshake.
    ///   When `resume_hermes_id` is `Some` and non-empty, it resumes that
    ///   Hermes session (survives server restarts); otherwise it creates a
    ///   fresh one.
    pub async fn ensure_runtime(
        &self,
        session_id: &str,
        spec: &RuntimeSpec,
        resume_hermes_id: Option<&str>,
    ) -> Result<(Arc<dyn AgentRuntime>, String)> {
        if let Some(entry) = self.runtimes.read().await.get(session_id) {
            let rt = entry.runtime.clone();
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

        // Capability flags come from the adapter's initialize response,
        // captured during start(). Fail closed (false) when absent.
        let resumable = runtime.resumable().await;
        self.runtimes.write().await.insert(
            session_id.to_string(),
            RuntimeEntry {
                runtime: runtime.clone(),
                resumable,
                last_activity: std::time::Instant::now(),
            },
        );

        Ok((runtime, hermes_id))
    }

    /// Fork a source agent session into a fresh runtime (not yet registered —
    /// the caller assigns the Olympus session id and calls [`Self::register`]).
    pub async fn fork_runtime(&self, source_hermes_id: &str) -> Result<ForkedRuntime> {
        let runtime = (self.factory)(&RuntimeSpec::default());
        runtime
            .fork_session(source_hermes_id)
            .await
            .context("forking agent runtime session")?;

        let hermes_id = runtime
            .hermes_session_id()
            .await
            .unwrap_or_else(|| format!("fork-{}", chrono_millis()));
        Ok(ForkedRuntime { runtime, hermes_id })
    }

    /// Register an already-started runtime under an Olympus session id,
    /// capturing its capability flags.
    pub async fn register(&self, session_id: &str, runtime: Arc<dyn AgentRuntime>) {
        let resumable = runtime.resumable().await;
        self.runtimes
            .write()
            .await
            .insert(session_id.to_string(), RuntimeEntry { runtime, resumable, last_activity: std::time::Instant::now() });
    }

    /// The runtime registered for a session, if any.
    pub async fn get(&self, session_id: &str) -> Option<Arc<dyn AgentRuntime>> {
        self.runtimes
            .read()
            .await
            .get(session_id)
            .map(|e| e.runtime.clone())
    }

    /// The `resumable` capability flag for a session's runtime, if registered.
    pub async fn resumable(&self, session_id: &str) -> Option<bool> {
        self.runtimes
            .read()
            .await
            .get(session_id)
            .map(|e| e.resumable)
    }

    /// Send a command to a session's runtime. Errors if no runtime is
    /// registered for the session.
    pub async fn send(&self, session_id: &str, cmd: AgentCommand) -> Result<()> {
        let runtime = self
            .get(session_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("no runtime for session"))?;
        runtime.send(cmd).await?;
        // Touch last_activity so the reaper doesn't kill an active session.
        if let Some(entry) = self.runtimes.write().await.get_mut(session_id) {
            entry.last_activity = std::time::Instant::now();
        }
        Ok(())
    }

    /// Stop and deregister a session's runtime (no-op if none registered).
    pub async fn stop(&self, session_id: &str) -> Result<()> {
        let entry = self.runtimes.write().await.remove(session_id);
        if let Some(entry) = entry {
            entry.runtime.stop().await?;
        }
        Ok(())
    }

    /// Reap runtimes that have been idle longer than `threshold`.
    ///
    /// Called by a background task in `main.rs` on a fixed interval. Stops the
    /// child process and removes the entry from the table. The session can be
    /// resumed later via `ensure_runtime` with a `resume_hermes_id` — the
    /// conversation history persists on disk in the agent's session store.
    ///
    /// Returns the number of sessions reaped (for logging).
    pub async fn reap_idle(&self, threshold: std::time::Duration) -> usize {
        let now = std::time::Instant::now();
        let mut to_reap = Vec::new();

        {
            let runtimes = self.runtimes.read().await;
            for (sid, entry) in runtimes.iter() {
                if now.duration_since(entry.last_activity) > threshold {
                    to_reap.push(sid.clone());
                }
            }
        }

        let count = to_reap.len();
        for sid in &to_reap {
            if let Some(entry) = self.runtimes.write().await.remove(sid) {
                tracing::info!(
                    session_id = %sid,
                    idle_secs = now.duration_since(entry.last_activity).as_secs(),
                    "reaping idle runtime"
                );
                let _ = entry.runtime.stop().await;
            }
        }
        count
    }
}

/// Current epoch milliseconds as u128 (for unique id generation).
fn chrono_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
