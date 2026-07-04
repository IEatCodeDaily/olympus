//! HermesAgentRuntime — [`AgentRuntime`] backed by a real `hermes acp` child process.
//!
//! Spawns `hermes acp` over stdio, performs the ACP handshake (`initialize` →
//! `session/new`), and bridges [`AgentCommand`]s / [`AgentEvent`]s via newline-
//! delimited JSON-RPC (the transport `hermes acp` actually uses — see
//! `docs/reviews/acp-wire-spike.md` §"Verdict").
//!
//! # Wire transport
//!
//! Despite the ACP spec mentioning Content-Length framing, Hermes' ACP adapter
//! uses **newline-delimited JSON-RPC 2.0**: one compact JSON object per line,
//! terminated by `\n`. The spike confirmed this by reading the Python ACP
//! library source (`acp/sender.py:33`):
//! ```text
//! data = (json.dumps(payload, separators=(",", ":")) + "\n").encode("utf-8")
//! ```
//!
//! The [`NlFrame`] codec below implements this transport. The Content-Length
//! [`Frame`](crate::bridge::acp::Frame) codec in `acp.rs` remains available for
//! spec-compliant peers that prefer it.

use std::env;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::stream::Stream;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;
use tracing::debug;

use super::{AgentCommand, AgentEvent, AgentRuntime};
use crate::adapter::AgentKind;
use crate::bridge::acp::{AcpId, AcpMessage, AcpNotification, AcpRequest, AcpResponse, Frame};

const CLAUDE_CODE_ACP_PACKAGE: &str = "@zed-industries/claude-code-acp@0.16.2";
const CODEX_ACP_PACKAGE: &str = "@zed-industries/codex-acp@0.16.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpFraming {
    NewlineJson,
    ContentLength,
}

/// Select the ACP adapter command for a session's agent string.
///
/// Hermes profiles still run through `hermes acp` (with `-p <profile>` when a
/// profile is explicitly selected). Claude Code and Codex run through pinned Zed
/// ACP adapters via `npx -y`, so the control plane does not depend on mutable
/// globally-installed adapter binaries.
pub fn acp_command_for_agent(agent: Option<&str>) -> Vec<String> {
    let agent = agent.unwrap_or_default();
    match AgentKind::from_agent_str(agent) {
        AgentKind::Hermes => {
            if agent.is_empty() {
                vec!["hermes".into(), "acp".into()]
            } else {
                vec![
                    "hermes".into(),
                    "-p".into(),
                    agent.to_string(),
                    "acp".into(),
                ]
            }
        }
        AgentKind::ClaudeCode => vec!["npx".into(), "-y".into(), CLAUDE_CODE_ACP_PACKAGE.into()],
        AgentKind::Codex => vec!["npx".into(), "-y".into(), CODEX_ACP_PACKAGE.into()],
    }
}

pub fn acp_framing_for_agent(_agent: Option<&str>) -> AcpFraming {
    // All current ACP adapters (hermes acp, claude-code-acp, codex-acp) use
    // newline-delimited JSON on the wire, regardless of what the ACP spec
    // says about Content-Length framing. If a future adapter requires CL
    // framing, switch on AgentKind here.
    AcpFraming::NewlineJson
}

// ---------------------------------------------------------------------------
// Newline-delimited JSON-RPC codec (the transport `hermes acp` actually uses)
// ---------------------------------------------------------------------------

/// Newline-delimited JSON-RPC frame codec.
///
/// Wire format: one compact JSON object per line, terminated by `\n`.
pub struct NlFrame;

impl NlFrame {
    /// Encode a JSON value into a newline-terminated byte buffer.
    pub fn encode_value(msg: &Value) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec(msg).context("serialize JSON-RPC")?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Encode a typed [`AcpMessage`] into a newline-terminated byte buffer.
    pub fn encode(msg: &AcpMessage) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec(msg).context("serialize JSON-RPC")?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Decode a single line (without trailing newline) into an [`AcpMessage`].
    ///
    /// Returns `None` for blank / whitespace-only lines (the receiver simply
    /// skips them rather than erroring).
    pub fn decode_line(line: &[u8]) -> Option<AcpMessage> {
        let trimmed = std::str::from_utf8(line).ok()?.trim();
        if trimmed.is_empty() {
            return None;
        }
        let msg: AcpMessage = serde_json::from_str(trimmed).ok()?;
        Some(msg)
    }
}

// ---------------------------------------------------------------------------
// Handshake request builders
// ---------------------------------------------------------------------------

/// Build the ACP `initialize` request.
pub fn build_initialize_request(id: AcpId) -> AcpRequest {
    AcpRequest {
        jsonrpc: "2.0".into(),
        id,
        method: "initialize".into(),
        params: json!({
            "protocolVersion": 1,
            "clientCapabilities": {
                "fs": {
                    "readTextFile": true,
                    "writeTextFile": true,
                }
            },
            "clientInfo": {
                "name": "olympus-control-plane",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }),
    }
}

/// Build the ACP `session/new` request. If `mcp_servers` is non-empty, they're
/// passed as the `mcpServers` param (session-scoped MCP activation per
/// ADR 0006 §9.3). Otherwise `mcpServers: []` (legacy behavior).
pub fn build_session_new_request(cwd: &str, mcp_servers: &[Value], id: AcpId) -> AcpRequest {
    AcpRequest {
        jsonrpc: "2.0".into(),
        id,
        method: "session/new".into(),
        params: json!({
            "cwd": cwd,
            "mcpServers": mcp_servers,
        }),
    }
}

/// Build the ACP `session/resume` request.
pub fn build_session_resume_request(session_id: &str, cwd: &str, id: AcpId) -> AcpRequest {
    AcpRequest {
        jsonrpc: "2.0".into(),
        id,
        method: "session/resume".into(),
        params: json!({
            "sessionId": session_id,
            "cwd": cwd,
            "mcpServers": [],
        }),
    }
}

/// Build the ACP `session/fork` request.
pub fn build_session_fork_request(session_id: &str, cwd: &str, id: AcpId) -> AcpRequest {
    AcpRequest {
        jsonrpc: "2.0".into(),
        id,
        method: "session/fork".into(),
        params: json!({
            "sessionId": session_id,
            "cwd": cwd,
            "mcpServers": [],
        }),
    }
}

// ---------------------------------------------------------------------------
// Message → AgentEvent mapping
// ---------------------------------------------------------------------------

/// Map any [`AcpMessage`] (as read from the wire) into an optional [`AgentEvent`].
///
/// - Notifications are mapped via [`AgentEvent::from_notification`].
/// - Responses are mapped via [`AgentEvent::from_response`].
/// - Requests (never sent by the agent) produce `None`.
pub fn map_message_to_event(msg: &AcpMessage) -> Option<AgentEvent> {
    match msg {
        AcpMessage::Notification(notif) => AgentEvent::from_notification(notif),
        AcpMessage::Response(resp) => AgentEvent::from_response(resp),
        AcpMessage::Request(req) => AgentEvent::from_request(req),
    }
}

async fn handle_incoming_message(
    msg: &AcpMessage,
    tx: &tokio::sync::broadcast::Sender<AgentEvent>,
    session_id_shared: &Arc<Mutex<Option<String>>>,
) {
    if let AcpMessage::Response(resp) = msg {
        if let Some(sid) = resp.result.get("sessionId").and_then(|v| v.as_str()) {
            *session_id_shared.lock().await = Some(sid.to_string());
            debug!(target: "olympus.bridge.hermes", session_id = %sid, "captured session id");
        }
    }
    if let Some(event) = map_message_to_event(msg) {
        let _ = tx.send(event);
    }
}

async fn read_content_length_message(
    reader: &mut BufReader<ChildStdout>,
) -> Result<Option<AcpMessage>> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }
    }

    let Some(len) = content_length else {
        return Ok(None);
    };
    let mut body = vec![0; len];
    reader.read_exact(&mut body).await?;
    Ok(Some(Frame::decode(&body)?))
}

// ---------------------------------------------------------------------------
// HermesAgentRuntime
// ---------------------------------------------------------------------------

/// Internal state held behind a lock so `&self` trait methods work.
struct RuntimeState {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    session_id: Option<String>,
}

impl RuntimeState {
    fn new() -> Self {
        Self {
            child: None,
            stdin: None,
            session_id: None,
        }
    }
}

/// Configuration for [`HermesAgentRuntime`].
#[derive(Clone)]
pub struct HermesRuntimeConfig {
    /// Command to invoke `hermes acp` (default: `["hermes", "acp"]`).
    pub command: Vec<String>,
    /// Working directory for the child + ACP session cwd.
    pub cwd: String,
    /// If set, overrides `HERMES_ACP_SESSION_SOURCE` on the child.
    pub session_source: Option<String>,
    /// Channel capacity for the event stream (default 256).
    pub event_buffer: usize,
    /// How long `start()` waits for the `session/new` response (the ACP adapter
    /// can take several seconds to boot). Default 30s.
    pub start_timeout_secs: u64,
    /// MCP servers to pass in the ACP `session/new` request (from the setup
    /// adapter). Default empty (no session-scoped MCP).
    pub mcp_servers: Vec<Value>,
    /// Extra environment variables for the child process (from the setup
    /// adapter, e.g. HERMES_SKILLS_PATH). Default empty.
    pub env: Vec<(String, String)>,
    /// ACP frame encoding used by the child process.
    pub framing: AcpFraming,
}

impl Default for HermesRuntimeConfig {
    fn default() -> Self {
        Self {
            command: vec!["hermes".into(), "acp".into()],
            cwd: env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
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

/// An [`AgentRuntime`] backed by a real `hermes acp` child process.
///
/// Spawns the agent over stdio, performs the ACP handshake, and bridges
/// [`AgentCommand`]s to JSON-RPC requests/notifications. Events from the agent
/// arrive as a [`Stream`] of [`AgentEvent`].
pub struct HermesAgentRuntime {
    config: HermesRuntimeConfig,
    state: Mutex<RuntimeState>,
    next_id: AtomicI64,
    event_tx: tokio::sync::broadcast::Sender<AgentEvent>,
    /// Active ACP session id, shared with the stdout reader task so it can
    /// capture the id from the `session/new` response while `send()` reads it.
    session_id_shared: Arc<Mutex<Option<String>>>,
}

impl HermesAgentRuntime {
    /// Create a new runtime (Arc-wrapped, the common case).
    pub fn new_arc(config: HermesRuntimeConfig) -> Arc<Self> {
        // broadcast (not mpsc): each `events()` call subscribes and gets its
        // own fresh receiver, so multiple turns on the same runtime each see
        // the turn's events. The old mpsc take-once receiver was consumed by
        // the first turn's drain loop, leaving subsequent turns with an empty
        // stream and silently dropping their assistant reply.
        let (tx, _rx) = tokio::sync::broadcast::channel(config.event_buffer);
        Arc::new(Self {
            config,
            state: Mutex::new(RuntimeState::new()),
            next_id: AtomicI64::new(1),
            event_tx: tx,
            session_id_shared: Arc::new(Mutex::new(None)),
        })
    }

    fn alloc_id(&self) -> AcpId {
        AcpId::from(self.next_id.fetch_add(1, Ordering::SeqCst))
    }

    /// Write a raw JSON-RPC message to the child's stdin.
    async fn write_message(&self, msg: &AcpMessage) -> Result<()> {
        let bytes = match self.config.framing {
            AcpFraming::NewlineJson => NlFrame::encode(msg)?,
            AcpFraming::ContentLength => Frame::encode(msg)?,
        };
        let mut state = self.state.lock().await;
        let stdin = state
            .stdin
            .as_mut()
            .context("child stdin not open — was start() called?")?;
        stdin.write_all(&bytes).await?;
        stdin.flush().await?;
        Ok(())
    }

    /// Spawn the stdout reader task that parses JSON-RPC frames, captures the
    /// session id from `session/new`/`session/resume` responses, and pushes
    /// [`AgentEvent`]s into the channel. Holds only the pieces it needs (tx +
    /// the shared session-id cell), so it does not pin the whole runtime alive.
    fn spawn_reader(
        stdout: ChildStdout,
        tx: tokio::sync::broadcast::Sender<AgentEvent>,
        session_id_shared: Arc<Mutex<Option<String>>>,
        framing: AcpFraming,
    ) {
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            match framing {
                AcpFraming::NewlineJson => {
                    let mut lines = reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        debug!(target: "olympus.bridge.hermes", line = %line, "ACP recv");
                        let Some(msg) = NlFrame::decode_line(line.as_bytes()) else {
                            continue;
                        };
                        handle_incoming_message(&msg, &tx, &session_id_shared).await;
                    }
                }
                AcpFraming::ContentLength => {
                    let mut reader = reader;
                    loop {
                        match read_content_length_message(&mut reader).await {
                            Ok(Some(msg)) => {
                                handle_incoming_message(&msg, &tx, &session_id_shared).await
                            }
                            Ok(None) => break,
                            Err(err) => {
                                debug!(target: "olympus.bridge.hermes", error = %err, "ACP content-length read failed");
                                break;
                            }
                        }
                    }
                }
            }
            debug!(target: "olympus.bridge.hermes", "ACP stdout reader closed");
        });
    }

    /// Get the active session ID, or error if not started.
    /// Prefers the id captured by the reader from the `session/new` response;
    /// falls back to an explicitly-provided resume id stored in state.
    async fn session_id_or_default(&self) -> Result<String> {
        if let Some(sid) = self.session_id_shared.lock().await.clone() {
            return Ok(sid);
        }
        let state = self.state.lock().await;
        Ok(state.session_id.clone().unwrap_or_default())
    }
}

#[async_trait::async_trait]
impl AgentRuntime for HermesAgentRuntime {
    async fn start(&self, session_id: Option<&str>) -> Result<()> {
        let mut state = self.state.lock().await;

        if state.child.is_some() {
            anyhow::bail!("runtime already started");
        }

        // Build the child command with env
        let mut cmd = tokio::process::Command::new(&self.config.command[0]);
        cmd.args(&self.config.command[1..]);
        cmd.current_dir(&self.config.cwd);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit()); // logging goes to our stderr
        if let Some(source) = &self.config.session_source {
            cmd.env("HERMES_ACP_SESSION_SOURCE", source);
        }
        // Apply env vars from the setup adapter (e.g. HERMES_SKILLS_PATH).
        for (k, v) in &self.config.env {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawning {:?}", self.config.command))?;
        let stdin = child
            .stdin
            .take()
            .context("child stdin pipe was not captured")?;
        let stdout = child
            .stdout
            .take()
            .context("child stdout pipe was not captured")?;

        // Store session_id for resume
        if let Some(sid) = session_id {
            state.session_id = Some(sid.to_string());
            *self.session_id_shared.lock().await = Some(sid.to_string());
        }
        state.child = Some(child);
        state.stdin = Some(stdin);
        drop(state); // release before we call write_message

        // Spawn the stdout reader so streamed session/update events flow into
        // events() and the session id from session/new is captured.
        Self::spawn_reader(
            stdout,
            self.event_tx.clone(),
            Arc::clone(&self.session_id_shared),
            self.config.framing,
        );

        // --- ACP handshake: initialize ---
        let init_req = build_initialize_request(self.alloc_id());
        debug!(target: "olympus.bridge.hermes", method = %init_req.method, "ACP send");
        self.write_message(&AcpMessage::Request(init_req)).await?;

        // --- session/new or session/resume ---
        let req = if let Some(sid) = session_id {
            build_session_resume_request(sid, &self.config.cwd, self.alloc_id())
        } else {
            build_session_new_request(&self.config.cwd, &self.config.mcp_servers, self.alloc_id())
        };
        debug!(target: "olympus.bridge.hermes", method = %req.method, "ACP send");
        self.write_message(&AcpMessage::Request(req)).await?;

        // Wait for the reader to capture the session id from the session/new
        // (or session/resume) response before returning, so a caller can
        // immediately send() without racing the handshake. The ACP adapter can
        // take a few seconds to boot (loads .env, MCP servers) before it replies.
        let deadline =
            std::time::Instant::now() + Duration::from_secs(self.config.start_timeout_secs);
        loop {
            if self.session_id_shared.lock().await.is_some() {
                break;
            }
            if std::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "timed out after {}s waiting for ACP session/new response",
                    self.config.start_timeout_secs
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Ok(())
    }

    async fn fork_session(&self, session_id: &str) -> Result<()> {
        let mut state = self.state.lock().await;

        if state.child.is_some() {
            anyhow::bail!("runtime already started");
        }

        let mut cmd = tokio::process::Command::new(&self.config.command[0]);
        cmd.args(&self.config.command[1..]);
        cmd.current_dir(&self.config.cwd);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit());
        if let Some(source) = &self.config.session_source {
            cmd.env("HERMES_ACP_SESSION_SOURCE", source);
        }
        for (k, v) in &self.config.env {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawning {:?}", self.config.command))?;
        let stdin = child
            .stdin
            .take()
            .context("child stdin pipe was not captured")?;
        let stdout = child
            .stdout
            .take()
            .context("child stdout pipe was not captured")?;

        *self.session_id_shared.lock().await = None;
        state.child = Some(child);
        state.stdin = Some(stdin);
        drop(state);

        Self::spawn_reader(
            stdout,
            self.event_tx.clone(),
            Arc::clone(&self.session_id_shared),
            self.config.framing,
        );

        let init_req = build_initialize_request(self.alloc_id());
        debug!(target: "olympus.bridge.hermes", method = %init_req.method, "ACP send");
        self.write_message(&AcpMessage::Request(init_req)).await?;

        let fork_req = build_session_fork_request(session_id, &self.config.cwd, self.alloc_id());
        debug!(target: "olympus.bridge.hermes", method = %fork_req.method, "ACP send");
        self.write_message(&AcpMessage::Request(fork_req)).await?;

        let deadline =
            std::time::Instant::now() + Duration::from_secs(self.config.start_timeout_secs);
        loop {
            if let Some(sid) = self.session_id_shared.lock().await.clone() {
                self.state.lock().await.session_id = Some(sid);
                break;
            }
            if std::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "timed out after {}s waiting for ACP session/fork response",
                    self.config.start_timeout_secs
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Ok(())
    }

    async fn send(&self, cmd: AgentCommand) -> Result<()> {
        match &cmd {
            AgentCommand::Stop => {
                self.stop().await?;
            }
            AgentCommand::Cancel => {
                let session_id = self.session_id_or_default().await?;
                if session_id.is_empty() {
                    anyhow::bail!("no active session — was start() called?");
                }
                let notif = AcpNotification::from_command(&cmd, &session_id)?;
                let msg = AcpMessage::Notification(notif);
                self.write_message(&msg).await?;
            }
            AgentCommand::Prompt {
                model: Some(model), ..
            } => {
                let session_id = self.session_id_or_default().await?;
                if session_id.is_empty() {
                    anyhow::bail!("no active session — was start() called?");
                }
                let switch = AgentCommand::SwitchModel {
                    model: model.clone(),
                };
                let switch_req = AcpRequest::from_command(&switch, &session_id, self.alloc_id())?;
                self.write_message(&AcpMessage::Request(switch_req)).await?;
                let req = AcpRequest::from_command(&cmd, &session_id, self.alloc_id())?;
                self.write_message(&AcpMessage::Request(req)).await?;
            }
            _ => {
                let session_id = self.session_id_or_default().await?;
                if session_id.is_empty() {
                    anyhow::bail!("no active session — was start() called?");
                }
                let req = AcpRequest::from_command(&cmd, &session_id, self.alloc_id())?;
                self.write_message(&AcpMessage::Request(req)).await?;
            }
        }
        Ok(())
    }

    async fn respond_permission(
        &self,
        request_id: &str,
        option_id: Option<&str>,
    ) -> Result<()> {
        // The agent's request_id was captured as serialized JSON; parse it back
        // so we echo the exact id shape (integer or string) the agent used.
        let id_value: Value = serde_json::from_str(request_id)
            .with_context(|| format!("parsing permission request id {request_id:?}"))?;
        // ACP RequestPermissionOutcome: "selected" with optionId, or "cancelled".
        let outcome = match option_id {
            Some(opt) => json!({ "outcome": "selected", "optionId": opt }),
            None => json!({ "outcome": "cancelled" }),
        };
        let resp = AcpResponse {
            jsonrpc: "2.0".into(),
            id: AcpId(id_value),
            result: json!({ "outcome": outcome }),
            error: None,
        };
        self.write_message(&AcpMessage::Response(resp)).await?;
        Ok(())
    }

    fn events(&self) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send>> {
        // Each call subscribes and gets its OWN fresh receiver, so every turn's
        // drain loop sees that turn's events. (broadcast, not the old take-once
        // mpsc receiver that was consumed by the first turn.) BroadcastStream
        // yields Result<Item, Lagged> — we drop lag errors, keeping only Ok.
        use tokio_stream::StreamExt as _;
        Box::pin(
            tokio_stream::wrappers::BroadcastStream::new(self.event_tx.subscribe())
                .filter_map(|res| res.ok()),
        )
    }

    async fn stop(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        if let Some(mut stdin) = state.stdin.take() {
            let _ = stdin.shutdown().await;
        }
        if let Some(mut child) = state.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        state.session_id = None;
        Ok(())
    }

    async fn hermes_session_id(&self) -> Option<String> {
        self.session_id_shared.lock().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::acp::{AcpMessage, AcpResponse};
    use serde_json::json;

    // ---- Newline-framed codec ----

    #[test]
    fn newline_encode_produces_json_plus_newline() {
        let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
        let bytes = NlFrame::encode_value(&msg).expect("encode");
        assert!(bytes.ends_with(b"\n"));
        // body before \n must be valid JSON
        let body = std::str::from_utf8(&bytes[..bytes.len() - 1]).expect("utf8");
        let parsed: serde_json::Value = serde_json::from_str(body).expect("parse");
        assert_eq!(parsed["method"], "initialize");
    }

    #[test]
    fn newline_decode_parses_a_single_json_line() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#;
        let msg = NlFrame::decode_line(line.as_bytes()).expect("decode");
        assert!(matches!(msg, AcpMessage::Notification(_)));
    }

    #[test]
    fn newline_decode_rejects_blank_line() {
        let msg = NlFrame::decode_line(b"   \n");
        assert!(msg.is_none(), "blank line should produce no message");
    }

    // ---- Handshake request builders ----

    #[test]
    fn initialize_request_has_correct_shape() {
        let req = build_initialize_request(AcpId::from(1));
        assert_eq!(req.method, "initialize");
        assert_eq!(req.params["protocolVersion"], 1);
        assert_eq!(
            req.params["clientCapabilities"]["fs"]["readTextFile"],
            serde_json::Value::Bool(true)
        );
        assert!(req.params["clientInfo"]["name"].is_string());
    }

    #[test]
    fn session_new_request_includes_cwd() {
        let req = build_session_new_request("/tmp/work", &[], AcpId::from(2));
        assert_eq!(req.method, "session/new");
        assert_eq!(req.params["cwd"], "/tmp/work");
    }

    // ---- Event mapping from raw JSON-RPC messages ----

    #[test]
    fn agent_message_chunk_line_maps_to_text_event() {
        let notif = spike_chunk_notif("P");
        let msg = AcpMessage::Notification(notif);
        let event = map_message_to_event(&msg);
        assert_eq!(event, Some(AgentEvent::Text("P".into())));
    }

    #[test]
    fn final_response_line_maps_to_done_event() {
        let resp = AcpResponse {
            jsonrpc: "2.0".into(),
            id: 3.into(),
            result: json!({"stopReason": "end_turn"}),
            error: None,
        };
        let msg = AcpMessage::Response(resp);
        let event = map_message_to_event(&msg);
        assert_eq!(
            event,
            Some(AgentEvent::Done {
                finish_reason: Some("end_turn".into())
            })
        );
    }

    #[test]
    fn request_message_produces_no_event() {
        let req = crate::bridge::acp::AcpRequest {
            jsonrpc: "2.0".into(),
            id: 99.into(),
            method: "session/prompt".into(),
            params: json!({}),
        };
        let msg = AcpMessage::Request(req);
        let event = map_message_to_event(&msg);
        assert!(event.is_none());
    }

    #[test]
    fn acp_command_for_agent_keeps_hermes_default_and_profiles() {
        assert_eq!(acp_command_for_agent(None), vec!["hermes", "acp"]);
        assert_eq!(
            acp_command_for_agent(Some("gpt55")),
            vec!["hermes", "-p", "gpt55", "acp"]
        );
    }

    #[test]
    fn acp_command_for_agent_selects_pinned_cli_adapters() {
        assert_eq!(
            acp_command_for_agent(Some("claude-code")),
            vec!["npx", "-y", "@zed-industries/claude-code-acp@0.16.2"]
        );
        assert_eq!(
            acp_command_for_agent(Some("codex")),
            vec!["npx", "-y", "@zed-industries/codex-acp@0.16.0"]
        );
    }

    // ---- helpers ----

    fn spike_chunk_notif(text: &str) -> crate::bridge::acp::AcpNotification {
        crate::bridge::acp::AcpNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: json!({
                "sessionId": "s-1",
                "update": {
                    "content": {"text": text, "type": "text"},
                    "sessionUpdate": "agent_message_chunk",
                },
            }),
        }
    }
}
