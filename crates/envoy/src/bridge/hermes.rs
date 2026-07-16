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
pub use olympus_proto::{AcpFraming, ModelSetStyle};

/// Backward-compatible command-table entry point used by the runtime factory.
pub fn acp_command_for_agent(agent: Option<&str>) -> Vec<String> {
    command_for_agent(agent)
}

/// ACP stdio is newline-delimited JSON for every supported adapter. Keep the
/// selection seam explicit so a future adapter with a different transport does
/// not silently change the protocol client.
pub fn acp_framing_for_agent(agent: Option<&str>) -> AcpFraming {
    match AgentKind::from_agent_str(agent.unwrap_or_default()) {
        AgentKind::Hermes => AcpFraming::NewlineJson,
        AgentKind::ClaudeCode | AgentKind::Codex => AcpFraming::NewlineJson,
    }
}

/// How the harness accepts a mid-session model switch. `hermes acp` implements
/// the Hermes-native `session/set_model`; the Zed Claude Code and Codex adapters
/// only implement the ACP-standard `session/set_config_option { configId:
/// "model" }` and return `-32601 Method not found` for `session/set_model`.
/// Keep this explicit next to framing so the client never assumes one uniform
/// ACP surface across harnesses.
pub fn model_set_style_for_agent(agent: Option<&str>) -> ModelSetStyle {
    match AgentKind::from_agent_str(agent.unwrap_or_default()) {
        AgentKind::Hermes => ModelSetStyle::SetModel,
        AgentKind::ClaudeCode | AgentKind::Codex => ModelSetStyle::ConfigOption,
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
    pub model_set_style: ModelSetStyle,
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
            model_set_style: ModelSetStyle::SetModel,
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
        let client = AcpClient::with_events_and_model_style(
            writer,
            self.config.framing,
            self.config.model_set_style,
            self.event_tx.clone(),
        );
        client.start_reader(reader);
        state.child = Some(child);
        state.client = Some(Arc::clone(&client));
        Ok(client)
    }

    async fn start_once(&self, session_id: Option<&str>) -> Result<()> {
        let client = self.spawn_client().await?;
        client.initialize().await?;
        match session_id {
            Some(session_id) => {
                client
                    .session_resume(session_id, &self.config.cwd, &self.config.mcp_servers)
                    .await
            }
            None => {
                client
                    .session_new(&self.config.cwd, &self.config.mcp_servers)
                    .await
            }
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

    async fn cleanup_runtime(&self) -> Result<()> {
        let (client, child) = {
            let mut state = self.state.lock().await;
            (state.client.take(), state.child.take())
        };
        drop(client);
        if let Some(mut child) = child {
            child.reap().await?;
        }
        Ok(())
    }

    async fn failure_report(&self, error: anyhow::Error) -> String {
        let tail = self.stderr_tail().await;
        let cleanup_error = self.cleanup_runtime().await.err();
        let mut message = format!("{error:#}\n{}", diagnostic_tail(&tail));
        if let Some(cleanup_error) = cleanup_error {
            message.push_str(&format!("\ncleanup failed: {cleanup_error:#}"));
        }
        message
    }

    async fn fail_start(&self, error: anyhow::Error) -> anyhow::Error {
        anyhow::anyhow!(self.failure_report(error).await)
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
        let mut failures = Vec::with_capacity(2);
        for attempt in 1..=2 {
            match tokio::time::timeout(
                Duration::from_secs(self.config.start_timeout_secs),
                self.start_once(session_id),
            )
            .await
            {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(error)) => return Err(self.fail_start(error).await),
                Err(_) => {
                    let error = anyhow::anyhow!(
                        "timed out after {}s waiting for ACP initialize/session response",
                        self.config.start_timeout_secs
                    );
                    failures.push(format!(
                        "attempt {attempt}:\n{}",
                        self.failure_report(error).await
                    ));
                }
            }
        }
        anyhow::bail!(
            "ACP startup timed out after 2 attempts with fresh processes\n{}",
            failures.join("\n")
        )
    }

    async fn fork_session(&self, session_id: &str) -> Result<()> {
        let handshake = async {
            let client = self.spawn_client().await?;
            client.initialize().await?;
            client
                .session_fork(session_id, &self.config.cwd, &self.config.mcp_servers)
                .await
        };
        match tokio::time::timeout(
            Duration::from_secs(self.config.start_timeout_secs),
            handshake,
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(self.fail_start(error).await),
            Err(_) => {
                let error = anyhow::anyhow!(
                    "timed out after {}s waiting for ACP initialize/session/fork response",
                    self.config.start_timeout_secs
                );
                Err(self.fail_start(error).await)
            }
        }
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
        self.cleanup_runtime().await
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
            AcpFraming::NewlineJson
        );
        assert_eq!(
            acp_framing_for_agent(Some("codex")),
            AcpFraming::NewlineJson
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

    #[cfg(unix)]
    #[tokio::test]
    async fn startup_timeout_retries_once_and_reports_both_stderr_tails() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-acp.sh");
        let counter = dir.path().join("attempts");
        std::fs::write(
            &script,
            "#!/usr/bin/env bash\nn=0\n[ ! -f \"$1\" ] || n=$(cat \"$1\")\nn=$((n + 1))\necho $n > \"$1\"\necho attempt-$n >&2\nsleep 300\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = HermesAgentRuntime::new_arc(HermesRuntimeConfig {
            command: vec![
                script.to_string_lossy().into_owned(),
                counter.to_string_lossy().into_owned(),
            ],
            cwd: dir.path().to_string_lossy().into_owned(),
            session_source: None,
            event_buffer: 8,
            start_timeout_secs: 1,
            mcp_servers: Vec::new(),
            env: Vec::new(),
            framing: AcpFraming::NewlineJson,
            ..Default::default()
        });

        let error = runtime.start(None).await.unwrap_err().to_string();

        assert_eq!(std::fs::read_to_string(counter).unwrap().trim(), "2");
        assert!(error.contains("attempt 1"), "{error}");
        assert!(error.contains("attempt-1"), "{error}");
        assert!(error.contains("attempt 2"), "{error}");
        assert!(error.contains("attempt-2"), "{error}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn startup_timeout_reaps_the_adapter_process_tree() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-acp.sh");
        let pidfile = dir.path().join("pids");
        std::fs::write(
            &script,
            "#!/usr/bin/env bash\necho $$ > \"$1\"\nsleep 300 &\necho $! >> \"$1\"\nwait\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = HermesAgentRuntime::new_arc(HermesRuntimeConfig {
            command: vec![
                script.to_string_lossy().into_owned(),
                pidfile.to_string_lossy().into_owned(),
            ],
            cwd: dir.path().to_string_lossy().into_owned(),
            session_source: None,
            event_buffer: 8,
            start_timeout_secs: 1,
            mcp_servers: Vec::new(),
            env: Vec::new(),
            framing: AcpFraming::NewlineJson,
            ..Default::default()
        });

        let outcome = tokio::time::timeout(Duration::from_secs(5), runtime.start(None)).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let pids = std::fs::read_to_string(&pidfile)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.parse::<u32>().ok())
            .collect::<Vec<_>>();
        let alive = pids
            .iter()
            .copied()
            .filter(|pid| std::path::Path::new(&format!("/proc/{pid}")).exists())
            .collect::<Vec<_>>();

        // Always clean up a red-test process before asserting.
        let _ = runtime.stop().await;
        for pid in &pids {
            let _ = std::process::Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }

        assert!(
            outcome.is_ok(),
            "runtime did not enforce its own startup deadline"
        );
        assert!(alive.is_empty(), "startup leaked process ids: {alive:?}");
    }
}
