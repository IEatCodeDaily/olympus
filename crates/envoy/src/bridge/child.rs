//! Child-process lifecycle seam for ACP harnesses.
//!
//! This is intentionally the only Envoy bridge module that imports
//! `tokio::process`; protocol and framing tests use in-memory I/O instead.

use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::process::Child;
use tokio::sync::Mutex;

use crate::adapter::AgentKind;

pub const STDERR_BUF_CAP: usize = 8 * 1024;
const CODEX_ACP_PACKAGE: &str = "@zed-industries/codex-acp@0.16.0";

pub type ChildReader = Pin<Box<dyn AsyncRead + Send>>;
pub type ChildWriter = Pin<Box<dyn AsyncWrite + Send>>;

/// Pure spawn description, independently testable without starting a process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnSpec {
    pub command: Vec<String>,
    pub cwd: String,
    pub env: Vec<(String, String)>,
}

impl SpawnSpec {
    pub fn for_agent(agent: Option<&str>, cwd: String) -> Self {
        Self {
            command: command_for_agent(agent),
            cwd,
            env: Vec::new(),
        }
    }
}

/// Invocation table for the supported harness adapters.
pub fn command_for_agent(agent: Option<&str>) -> Vec<String> {
    let agent = agent.unwrap_or_default();
    match AgentKind::from_agent_str(agent) {
        AgentKind::Hermes if agent.is_empty() || agent == "default" => {
            vec!["hermes".into(), "acp".into()]
        }
        AgentKind::Hermes => vec![
            "hermes".into(),
            "-p".into(),
            agent.to_string(),
            "acp".into(),
        ],
        AgentKind::ClaudeCode => vec![claude_acp_binary()],
        AgentKind::Codex => vec!["bunx".into(), CODEX_ACP_PACKAGE.into()],
    }
}

fn claude_acp_binary() -> String {
    if let Ok(path) = std::env::var("OLYMPUS_CLAUDE_ACP_BIN") {
        return path;
    }
    let base = std::env::var("OLYMPUS_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME").map(|home| std::path::PathBuf::from(home).join(".olympus"))
        })
        .unwrap_or_else(|_| std::path::PathBuf::from(".olympus"));
    base.join("adapters")
        .join("claude-agent-acp")
        .join("node_modules")
        .join(".bin")
        .join("claude-agent-acp")
        .to_string_lossy()
        .into_owned()
}

/// Owned child plus its stdio endpoints and bounded diagnostic tail.
pub struct ChildHandle {
    child: Child,
    reader: Option<ChildReader>,
    writer: Option<ChildWriter>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_task: Option<tokio::task::JoinHandle<()>>,
}

impl ChildHandle {
    pub fn spawn(spec: &SpawnSpec) -> Result<Self> {
        let program = spec.command.first().context("ACP child command is empty")?;
        let mut command = tokio::process::Command::new(program);
        command
            .args(&spec.command[1..])
            .current_dir(&spec.cwd)
            .envs(spec.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .spawn()
            .with_context(|| format!("spawning {:?}", spec.command))?;
        let writer = child
            .stdin
            .take()
            .context("child stdin pipe was not captured")?;
        let reader = child
            .stdout
            .take()
            .context("child stdout pipe was not captured")?;
        let stderr_pipe = child
            .stderr
            .take()
            .context("child stderr pipe was not captured")?;
        let stderr = Arc::new(Mutex::new(Vec::with_capacity(STDERR_BUF_CAP)));
        let stderr_task = capture_stderr(stderr_pipe, Arc::clone(&stderr));
        Ok(Self {
            child,
            reader: Some(Box::pin(reader)),
            writer: Some(Box::pin(writer)),
            stderr,
            stderr_task: Some(stderr_task),
        })
    }

    pub fn take_reader(&mut self) -> Result<ChildReader> {
        self.reader.take().context("child stdout already taken")
    }

    pub fn take_writer(&mut self) -> Result<ChildWriter> {
        self.writer.take().context("child stdin already taken")
    }

    /// Poll process health without blocking.
    pub fn early_exit(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(format!("child exited: {status}")),
            Ok(None) => None,
            Err(error) => Some(format!("child poll failed: {error}")),
        }
    }

    pub fn stderr_buffer(&self) -> Arc<Mutex<Vec<u8>>> {
        Arc::clone(&self.stderr)
    }

    /// Close stdio, terminate, and reap the process.
    pub async fn reap(&mut self) -> Result<()> {
        self.writer.take();
        if self.child.try_wait()?.is_none()
            && tokio::time::timeout(std::time::Duration::from_millis(500), self.child.wait())
                .await
                .is_err()
        {
            signal_process_group(&self.child, libc::SIGTERM);
            if tokio::time::timeout(std::time::Duration::from_secs(1), self.child.wait())
                .await
                .is_err()
            {
                signal_process_group(&self.child, libc::SIGKILL);
                let _ = self.child.wait().await;
            }
        }
        if let Some(task) = self.stderr_task.take() {
            task.abort();
            let _ = task.await;
        }
        Ok(())
    }
}

fn capture_stderr(
    mut stderr: tokio::process::ChildStderr,
    buffer: Arc<Mutex<Vec<u8>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut chunk = [0_u8; 512];
        loop {
            let read = match stderr.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            let mut guard = buffer.lock().await;
            push_bounded(&mut guard, &chunk[..read]);
        }
    })
}

#[cfg(unix)]
fn signal_process_group(child: &Child, signal: libc::c_int) {
    let Some(pid) = child.id() else {
        return;
    };
    // SAFETY: negative pid addresses the process group created specifically
    // for this adapter. ESRCH is benign because the process already exited.
    unsafe {
        libc::kill(-(pid as libc::pid_t), signal);
    }
}

#[cfg(not(unix))]
fn signal_process_group(_child: &Child, _signal: libc::c_int) {}

fn push_bounded(buffer: &mut Vec<u8>, bytes: &[u8]) {
    if bytes.len() >= STDERR_BUF_CAP {
        buffer.clear();
        buffer.extend_from_slice(&bytes[bytes.len() - STDERR_BUF_CAP..]);
        return;
    }
    let overflow = buffer
        .len()
        .saturating_add(bytes.len())
        .saturating_sub(STDERR_BUF_CAP);
    if overflow > 0 {
        buffer.drain(..overflow.min(buffer.len()));
    }
    buffer.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_install_default_is_not_treated_as_profile() {
        assert_eq!(command_for_agent(None), vec!["hermes", "acp"]);
        assert_eq!(command_for_agent(Some("default")), vec!["hermes", "acp"]);
        assert_eq!(
            command_for_agent(Some("gpt55")),
            vec!["hermes", "-p", "gpt55", "acp"]
        );
    }

    #[test]
    fn invocation_table_is_pinned_without_running_harnesses() {
        let claude = command_for_agent(Some("claude-code"));
        assert_eq!(claude.len(), 1);
        assert!(
            claude[0].ends_with("/adapters/claude-agent-acp/node_modules/.bin/claude-agent-acp")
        );
        assert_eq!(
            command_for_agent(Some("codex")),
            vec!["bunx", "@zed-industries/codex-acp@0.16.0"]
        );
    }

    #[test]
    fn stderr_tail_is_bounded_and_keeps_newest_bytes() {
        let mut buffer = vec![b'a'; STDERR_BUF_CAP - 2];
        push_bounded(&mut buffer, b"WXYZ");
        assert_eq!(buffer.len(), STDERR_BUF_CAP);
        assert!(buffer.ends_with(b"WXYZ"));
    }
}
