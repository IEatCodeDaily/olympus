//! Test-only helpers: a mock [`AgentRuntime`] that simulates an agent without
//! spawning `hermes acp`.

use std::pin::Pin;
use std::sync::Arc;

use futures::stream::Stream;
use tokio::sync::Mutex;

use crate::bridge::{AgentCommand, AgentEvent, AgentRuntime};

/// A mock runtime that captures a fixed Hermes session id and can emit scripted
/// events when prompted.
pub struct MockAgentRuntime {
    session_id: Mutex<Option<String>>,
    events_tx: tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    events_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<AgentEvent>>>,
}

impl MockAgentRuntime {
    fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            session_id: Mutex::new(None),
            events_tx: tx,
            events_rx: Mutex::new(Some(rx)),
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

    async fn send(&self, cmd: AgentCommand) -> anyhow::Result<()> {
        if let AgentCommand::Prompt { text, .. } = &cmd {
            // Simulate the agent echoing the prompt text back as a response.
            let response = if text.to_lowercase().contains("pong") {
                "PONG".to_string()
            } else {
                format!("echo: {text}")
            };
            let _ = self.events_tx.send(AgentEvent::Text(response));
            let _ = self.events_tx.send(AgentEvent::Done {
                finish_reason: Some("end_turn".into()),
            });
        }
        Ok(())
    }

    fn events(&self) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send>> {
        let rx_opt = self
            .events_rx
            .try_lock()
            .ok()
            .and_then(|mut guard| guard.take());
        match rx_opt {
            Some(rx) => Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx)),
            None => Box::pin(futures::stream::empty()),
        }
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
    Arc::new(|| Arc::new(MockAgentRuntime::new()) as Arc<dyn AgentRuntime>)
}
