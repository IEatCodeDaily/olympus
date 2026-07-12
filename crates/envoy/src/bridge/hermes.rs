//! `AgentRuntime` composition for ACP harness children.

use std::env;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::stream::Stream;
use serde_json::Value;
use tokio::sync::Mutex;

use super::child::{command_for_agent, ChildHandle, SpawnSpec};
use super::client::AcpClient;
use super::{AgentCommand, AgentEvent, AgentRuntime};
use crate::adapter::AgentKind;

pub use super::client::{
    build_initialize_request, build_session_fork_request, build_session_new_request,
    build_session_resume_request, parse_resumable_capability,
};
pub use olympus_proto::AcpFraming;

/// Backward-compatible command-table entry point used by the runtime factory.
pub fn acp_command_for_agent(agent: Option<&str>) -> Vec<String> {
    command_for_agent(agent)
}

/// Hermes uses its newline protocol; spec-compliant Claude/Codex peers use ACP
/// Content-Length framing. Selection remains at composition time, not in the
/// protocol client.
pub fn acp_framing_for_agent(agent: Option<&str>) -> AcpFraming {
    match AgentKind::from_agent_str(agent.unwrap_or_default()) {
        AgentKind::Hermes => AcpFraming::NewlineJson,
        AgentKind::ClaudeCode | AgentKind::Codex => AcpFraming::ContentLength,
    }
}

/// Configuration for [`HermesAgentRuntime`]. Its public shape is preserved for
/// the existing factory seam.
#[derive(Clone)]
pub struct HermesRuntimeConfig {
    pub command: Vec<String>,
    pub cwd: String,
    pub session_source: Option<String>,
    pub event_buffer: usize,
    pub start_timeout_secs: u64,
    pub mcp_servers: Vec<Value>,
    pub env: Vec<(String, String)>,
    pub framing: AcpFraming,
}

impl Default for HermesRuntimeConfig {
    fn default() -> Self {
        Self {
            command: vec!["hermes".into(), "acp".into()],
            cwd: env::current_dir()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".into()),
            session_source: Some("olympus".into()),
            event_buffer: 256,
            start_timeout_secs: 30,
            mcp_servers: Vec::new(),
            env: Vec::new(),
            framing: AcpFraming::NewlineJson,
        }
    }
}

struct RuntimeState {
    child: Option<ChildHandle>,
    client: Option<Arc<AcpClient>>,
}

pub struct HermesAgentRuntime {
    config: HermesRuntimeConfig,
    state: Mutex<RuntimeState>,
    event_tx: tokio::sync::broadcast::Sender<AgentEvent>,
}

impl HermesAgentRuntime {
    pub fn new_arc(config: HermesRuntimeConfig) -> Arc<Self> {
        // A stable broadcast sender is owned by the runtime so every events()
        // call subscribes independently, including across child composition.
        let (event_tx, _) = tokio::sync::broadcast::channel(config.event_buffer);
        Arc::new(Self {
            config,
            state: Mutex::new(RuntimeState {
                child: None,
                client: None,
            }),
            event_tx,
        })
    }

    async fn spawn_client(&self) -> Result<Arc<AcpClient>> {
        let mut state = self.state.lock().await;
        if state.child.is_some() {
            anyhow::bail!("runtime already started");
        }
        if self.config.command.is_empty() {
            anyhow::bail!("ACP child command is empty");
        }
        let mut env = self.config.env.clone();
        if let Some(source) = &self.config.session_source {
            env.push(("HERMES_ACP_SESSION_SOURCE".into(), source.clone()));
        }
        let spec = SpawnSpec {
            command: self.config.command.clone(),
            cwd: self.config.cwd.clone(),
            env,
        };
        let mut child = ChildHandle::spawn(&spec)?;
        let reader = child.take_reader()?;
        let writer = child.take_writer()?;
        let client = AcpClient::with_events(writer, self.config.framing, self.event_tx.clone());
        client.start_reader(reader);
        state.child = Some(child);
        state.client = Some(Arc::clone(&client));
        Ok(client)
    }

    async fn wait_for_handshake(&self, label: &str, client: &AcpClient) -> Result<()> {
        let deadline =
            std::time::Instant::now() + Duration::from_secs(self.config.start_timeout_secs);
        loop {
            if !client.handshake_pending().await && client.session_id().await.is_some() {
                return Ok(());
            }
            let early_exit = {
                let mut state = self.state.lock().await;
                state.child.as_mut().and_then(ChildHandle::early_exit)
            };
            if let Some(exit) = early_exit {
                let tail = self.stderr_tail().await;
                anyhow::bail!(
                    "ACP {label} handshake failed — {exit}\n{}",
                    diagnostic_tail(&tail)
                );
            }
            if std::time::Instant::now() >= deadline {
                let tail = self.stderr_tail().await;
                anyhow::bail!(
                    "timed out after {}s waiting for ACP {label} response\n{}",
                    self.config.start_timeout_secs,
                    diagnostic_tail(&tail)
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn stderr_tail(&self) -> String {
        let buffer = self
            .state
            .lock()
            .await
            .child
            .as_ref()
            .map(ChildHandle::stderr_buffer);
        let Some(buffer) = buffer else {
            return String::new();
        };
        let guard = buffer.lock().await;
        String::from_utf8_lossy(guard.as_slice()).trim().to_string()
    }

    async fn client(&self) -> Result<Arc<AcpClient>> {
        self.state
            .lock()
            .await
            .client
            .clone()
            .context("runtime not started")
    }
}

fn diagnostic_tail(tail: &str) -> String {
    if tail.is_empty() {
        "(no stderr captured)".into()
    } else {
        format!("stderr:\n{tail}")
    }
}

#[async_trait::async_trait]
impl AgentRuntime for HermesAgentRuntime {
    async fn start(&self, session_id: Option<&str>) -> Result<()> {
        let client = self.spawn_client().await?;
        client.initialize().await?;
        match session_id {
            Some(session_id) => client.session_resume(session_id, &self.config.cwd).await?,
            None => {
                client
                    .session_new(&self.config.cwd, &self.config.mcp_servers)
                    .await?
            }
        }
        self.wait_for_handshake("session/new|resume", &client).await
    }

    async fn fork_session(&self, session_id: &str) -> Result<()> {
        let client = self.spawn_client().await?;
        client.initialize().await?;
        client.session_fork(session_id, &self.config.cwd).await?;
        self.wait_for_handshake("session/fork", &client).await
    }

    async fn send(&self, command: AgentCommand) -> Result<()> {
        if command == AgentCommand::Stop {
            return self.stop().await;
        }
        self.client().await?.send_command(&command).await
    }

    async fn respond_permission(&self, request_id: &str, option_id: Option<&str>) -> Result<()> {
        self.client()
            .await?
            .respond_permission(request_id, option_id)
            .await
    }

    fn events(&self) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send>> {
        use tokio_stream::StreamExt as _;
        Box::pin(
            tokio_stream::wrappers::BroadcastStream::new(self.event_tx.subscribe())
                .filter_map(|result| result.ok()),
        )
    }

    async fn stop(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        state.client.take();
        if let Some(mut child) = state.child.take() {
            child.reap().await?;
        }
        Ok(())
    }

    async fn hermes_session_id(&self) -> Option<String> {
        match self.state.lock().await.client.clone() {
            Some(client) => client.session_id().await,
            None => None,
        }
    }

    async fn resumable(&self) -> bool {
        match self.state.lock().await.client.clone() {
            Some(client) => client.resumable().await,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn command_and_framing_tables_preserve_harness_contracts() {
        assert_eq!(
            acp_command_for_agent(Some("default")),
            vec!["hermes", "acp"]
        );
        assert_eq!(
            acp_framing_for_agent(Some("gpt55")),
            AcpFraming::NewlineJson
        );
        assert_eq!(
            acp_framing_for_agent(Some("claude-code")),
            AcpFraming::ContentLength
        );
        assert_eq!(
            acp_framing_for_agent(Some("codex")),
            AcpFraming::ContentLength
        );
    }

    #[test]
    fn method_builders_keep_existing_wire_shapes() {
        let initialize = build_initialize_request(1.into());
        assert_eq!(initialize.method, "initialize");
        let session = build_session_new_request("/tmp", &[json!({"name":"mcp"})], 2.into());
        assert_eq!(session.method, "session/new");
        assert_eq!(session.params["mcpServers"][0]["name"], "mcp");
    }

    #[test]
    fn diagnostic_tail_surfaces_child_failure_context() {
        assert_eq!(diagnostic_tail(""), "(no stderr captured)");
        assert_eq!(diagnostic_tail("missing dep"), "stderr:\nmissing dep");
    }
}
