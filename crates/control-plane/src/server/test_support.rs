//! Test-only helpers: a mock [`AgentRuntime`] that simulates an agent without
//! spawning `hermes acp`.

use std::pin::Pin;
use std::sync::Arc;

use futures::stream::Stream;
use tokio::sync::Mutex;

use crate::bridge::{AgentCommand, AgentEvent, AgentRuntime};

/// A mock runtime that captures a fixed Hermes session id and can emit scripted
/// events when prompted. Uses a broadcast channel (matching the real runtime) so
/// multiple turns each get their own event stream — critical for testing the
/// multi-turn "second turn drops reply" bug.
pub struct MockAgentRuntime {
    session_id: Mutex<Option<String>>,
    events_tx: tokio::sync::broadcast::Sender<AgentEvent>,
}

impl MockAgentRuntime {
    fn new() -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(256);
        Self {
            session_id: Mutex::new(None),
            events_tx: tx,
        }
    }
}

#[async_trait::async_trait]
impl AgentRuntime for MockAgentRuntime {
    async fn start(&self, _session_id: Option<&str>) -> anyhow::Result<()> {
        let id = format!("mock-{}", uuid_short());
        *self.session_id.lock().await = Some(id);
        Ok(())
    }

    async fn fork_session(&self, _session_id: &str) -> anyhow::Result<()> {
        let id = format!("mock-fork-{}", uuid_short());
        *self.session_id.lock().await = Some(id);
        Ok(())
    }

    async fn send(&self, cmd: AgentCommand) -> anyhow::Result<()> {
        if let AgentCommand::Prompt { text, .. } = &cmd {
            // Simulate the agent echoing the prompt text back as a response.
            let response = if text.to_lowercase().contains("pong") {
                "PONG".to_string()
            } else {
                format!("echo: {text}")
            };
            // broadcast::send is sync; Err (no subscribers) is fine.
            let _ = self.events_tx.send(AgentEvent::Text(response));
            let _ = self.events_tx.send(AgentEvent::Done {
                finish_reason: Some("end_turn".into()),
            });
        }
        Ok(())
    }

    fn events(&self) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send>> {
        // Each call subscribes and gets its own fresh receiver (broadcast), so
        // multiple turns each see their events.
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

fn uuid_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{nanos:08x}")
}

/// A factory function (for `BridgeManager::with_factory`) that produces mock
/// runtimes.
pub fn mock_factory() -> super::bridge_mgr::RuntimeFactory {
    Arc::new(|_spec: &super::bridge_mgr::RuntimeSpec| {
        Arc::new(MockAgentRuntime::new()) as Arc<dyn AgentRuntime>
    })
}
