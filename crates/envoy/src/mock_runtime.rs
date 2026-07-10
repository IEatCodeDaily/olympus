//! A lightweight mock [`AgentRuntime`] for the envoy binary's `--mock` mode.
//!
//! Production uses [`HermesAgentRuntime`] (spawns a real `hermes acp` child); for
//! CI integration tests and manual smoke-checks the `--mock` flag swaps in this
//! echo runtime so the UDS session-RPC round-trip can be proven without a real
//! agent child. It echoes the prompt text back as `AgentEvent::Text` then emits
//! `AgentEvent::Done`, matching the shape a real turn takes.

use std::pin::Pin;
use std::sync::Arc;

use futures::stream::Stream;
use tokio::sync::Mutex;

use crate::bridge::{AgentCommand, AgentEvent, AgentRuntime};

/// A mock runtime that captures a synthetic session id and echoes prompts.
pub struct MockAgentRuntime {
    session_id: Mutex<Option<String>>,
    events_tx: tokio::sync::broadcast::Sender<AgentEvent>,
}

impl MockAgentRuntime {
    /// Create a fresh mock runtime (Arc-wrapped, the factory shape).
    pub fn new_arc() -> Arc<Self> {
        let (tx, _rx) = tokio::sync::broadcast::channel(256);
        Arc::new(Self {
            session_id: Mutex::new(None),
            events_tx: tx,
        })
    }
}

#[async_trait::async_trait]
impl AgentRuntime for MockAgentRuntime {
    async fn start(&self, _session_id: Option<&str>) -> anyhow::Result<()> {
        let id = format!("mock-{}", short_ts());
        *self.session_id.lock().await = Some(id);
        Ok(())
    }

    async fn fork_session(&self, _session_id: &str) -> anyhow::Result<()> {
        let id = format!("mock-fork-{}", short_ts());
        *self.session_id.lock().await = Some(id);
        Ok(())
    }

    async fn send(&self, cmd: AgentCommand) -> anyhow::Result<()> {
        if let AgentCommand::Prompt { text, .. } = &cmd {
            let text = text.clone();
            let tx = self.events_tx.clone();
            tokio::spawn(async move {
                tokio::task::yield_now().await;
                let reply = if text.to_lowercase().contains("pong") {
                    "PONG".to_string()
                } else {
                    format!("echo: {text}")
                };
                let _ = tx.send(AgentEvent::Text(reply));
                let _ = tx.send(AgentEvent::Done {
                    finish_reason: Some("end_turn".into()),
                });
            });
        }
        Ok(())
    }

    fn events(&self) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send>> {
        use tokio_stream::StreamExt as _;
        Box::pin(
            tokio_stream::wrappers::BroadcastStream::new(self.events_tx.subscribe())
                .filter_map(|res| res.ok()),
        )
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn hermes_session_id(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }
}

fn short_ts() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{nanos:08x}")
}
