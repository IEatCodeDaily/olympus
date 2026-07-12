//! Harness-agnostic ACP client over arbitrary async I/O.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tracing::debug;

use super::acp::{AcpId, AcpMessage, AcpNotification, AcpRequest, AcpResponse, AgentEventAcpExt};
use super::framing::{ContentLength, Framing, NewlineJson};
use super::{AgentCommand, AgentEvent};
use olympus_proto::AcpFraming;

pub type ClientWriter = Pin<Box<dyn AsyncWrite + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    Initialize,
    Handshake,
    Command,
}

struct ClientState {
    pending: HashMap<String, PendingKind>,
    session_id: Option<String>,
    resumable: bool,
}

/// ACP method mapping, request correlation, and event demultiplexing.
pub struct AcpClient {
    writer: Mutex<ClientWriter>,
    framing: AcpFraming,
    next_id: AtomicI64,
    state: Arc<Mutex<ClientState>>,
    events: tokio::sync::broadcast::Sender<AgentEvent>,
}

impl AcpClient {
    pub fn new(writer: ClientWriter, framing: AcpFraming, event_buffer: usize) -> Arc<Self> {
        let (events, _) = tokio::sync::broadcast::channel(event_buffer);
        Self::with_events(writer, framing, events)
    }

    pub fn with_events(
        writer: ClientWriter,
        framing: AcpFraming,
        events: tokio::sync::broadcast::Sender<AgentEvent>,
    ) -> Arc<Self> {
        Arc::new(Self {
            writer: Mutex::new(writer),
            framing,
            next_id: AtomicI64::new(1),
            state: Arc::new(Mutex::new(ClientState {
                pending: HashMap::new(),
                session_id: None,
                resumable: false,
            })),
            events,
        })
    }

    fn codec(framing: AcpFraming) -> Box<dyn Framing> {
        match framing {
            AcpFraming::NewlineJson => Box::new(NewlineJson),
            AcpFraming::ContentLength => Box::new(ContentLength),
        }
    }

    fn alloc_id(&self) -> AcpId {
        AcpId::from(self.next_id.fetch_add(1, Ordering::SeqCst))
    }

    pub fn start_reader<R>(&self, mut reader: R)
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        let codec = Self::codec(self.framing);
        let state = Arc::clone(&self.state);
        let events = self.events.clone();
        tokio::spawn(async move {
            let mut buffer = Vec::new();
            let mut chunk = [0_u8; 4096];
            loop {
                match reader.read(&mut chunk).await {
                    Ok(0) => break,
                    Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                    Err(error) => {
                        debug!(target: "olympus.bridge.client", %error, "ACP read failed");
                        break;
                    }
                }
                loop {
                    match codec.decode(&mut buffer) {
                        Ok(Some(message)) => handle_message(message, &state, &events).await,
                        Ok(None) => break,
                        Err(error) => {
                            debug!(target: "olympus.bridge.client", %error, "ACP frame decode failed");
                            return;
                        }
                    }
                }
            }
        });
    }

    async fn write(&self, message: &AcpMessage) -> Result<()> {
        let bytes = Self::codec(self.framing).encode(message)?;
        let mut writer = self.writer.lock().await;
        writer.write_all(&bytes).await?;
        writer.flush().await?;
        Ok(())
    }

    async fn request(&self, request: AcpRequest, kind: PendingKind) -> Result<()> {
        self.state
            .lock()
            .await
            .pending
            .insert(id_key(&request.id), kind);
        if let Err(error) = self.write(&AcpMessage::Request(request.clone())).await {
            self.state.lock().await.pending.remove(&id_key(&request.id));
            return Err(error);
        }
        Ok(())
    }

    pub async fn initialize(&self) -> Result<()> {
        self.request(
            build_initialize_request(self.alloc_id()),
            PendingKind::Initialize,
        )
        .await
    }

    pub async fn session_new(&self, cwd: &str, mcp_servers: &[Value]) -> Result<()> {
        self.request(
            build_session_new_request(cwd, mcp_servers, self.alloc_id()),
            PendingKind::Handshake,
        )
        .await
    }

    pub async fn session_resume(&self, session_id: &str, cwd: &str) -> Result<()> {
        // ACP resume responses are not required to echo the already-known id.
        // Seed it before sending, but still wait for request correlation to
        // lift the replay gate before the runtime reports ready.
        self.state.lock().await.session_id = Some(session_id.to_string());
        self.request(
            build_session_resume_request(session_id, cwd, self.alloc_id()),
            PendingKind::Handshake,
        )
        .await
    }

    pub async fn session_fork(&self, session_id: &str, cwd: &str) -> Result<()> {
        self.request(
            build_session_fork_request(session_id, cwd, self.alloc_id()),
            PendingKind::Handshake,
        )
        .await
    }

    pub async fn send_command(&self, command: &AgentCommand) -> Result<()> {
        let session_id = self
            .session_id()
            .await
            .context("no active session — was start() called?")?;
        match command {
            AgentCommand::Cancel => {
                let notification = AcpNotification::from_command(command, &session_id)?;
                self.write(&AcpMessage::Notification(notification)).await
            }
            AgentCommand::Prompt {
                model: Some(model), ..
            } => {
                let switch = AgentCommand::SwitchModel {
                    model: model.clone(),
                };
                let request = AcpRequest::from_command(&switch, &session_id, self.alloc_id())?;
                self.request(request, PendingKind::Command).await?;
                let request = AcpRequest::from_command(command, &session_id, self.alloc_id())?;
                self.request(request, PendingKind::Command).await
            }
            AgentCommand::Stop => anyhow::bail!("stop is a child lifecycle command"),
            _ => {
                let request = AcpRequest::from_command(command, &session_id, self.alloc_id())?;
                self.request(request, PendingKind::Command).await
            }
        }
    }

    pub async fn respond_permission(
        &self,
        request_id: &str,
        option_id: Option<&str>,
    ) -> Result<()> {
        let id = serde_json::from_str(request_id)
            .with_context(|| format!("parsing permission request id {request_id:?}"))?;
        let outcome = match option_id {
            Some(option_id) => json!({ "outcome": "selected", "optionId": option_id }),
            None => json!({ "outcome": "cancelled" }),
        };
        self.write(&AcpMessage::Response(AcpResponse {
            jsonrpc: "2.0".into(),
            id: AcpId(id),
            result: json!({ "outcome": outcome }),
            error: None,
        }))
        .await
    }

    pub async fn session_id(&self) -> Option<String> {
        self.state.lock().await.session_id.clone()
    }

    pub async fn resumable(&self) -> bool {
        self.state.lock().await.resumable
    }

    pub async fn handshake_pending(&self) -> bool {
        self.state
            .lock()
            .await
            .pending
            .values()
            .any(|kind| *kind == PendingKind::Handshake)
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AgentEvent> {
        self.events.subscribe()
    }
}

fn id_key(id: &AcpId) -> String {
    serde_json::to_string(&id.0).unwrap_or_default()
}

async fn handle_message(
    message: AcpMessage,
    state: &Arc<Mutex<ClientState>>,
    events: &tokio::sync::broadcast::Sender<AgentEvent>,
) {
    let mut state = state.lock().await;
    if let AcpMessage::Response(response) = &message {
        let pending = state.pending.remove(&id_key(&response.id));
        if pending == Some(PendingKind::Initialize) {
            state.resumable = parse_resumable_capability(&response.result);
        }
        if let Some(session_id) = response.result.get("sessionId").and_then(Value::as_str) {
            state.session_id = Some(session_id.to_string());
        }
    }
    let replaying = state
        .pending
        .values()
        .any(|kind| *kind == PendingKind::Handshake);
    drop(state);

    // Resuming adapters can replay the entire persisted transcript before the
    // correlated handshake response. It is already in Olympus's event log.
    if replaying && matches!(message, AcpMessage::Notification(_)) {
        return;
    }
    let event = match &message {
        AcpMessage::Notification(notification) => AgentEvent::from_notification(notification),
        AcpMessage::Response(response) => AgentEvent::from_response(response),
        AcpMessage::Request(request) => AgentEvent::from_request(request),
    };
    if let Some(event) = event {
        let _ = events.send(event);
    }
}

pub fn build_initialize_request(id: AcpId) -> AcpRequest {
    AcpRequest {
        jsonrpc: "2.0".into(),
        id,
        method: "initialize".into(),
        params: json!({
            "protocolVersion": 1,
            "clientCapabilities": { "fs": { "readTextFile": true, "writeTextFile": true } },
            "clientInfo": { "name": "olympus-control-plane", "version": env!("CARGO_PKG_VERSION") }
        }),
    }
}

pub fn build_session_new_request(cwd: &str, mcp_servers: &[Value], id: AcpId) -> AcpRequest {
    AcpRequest {
        jsonrpc: "2.0".into(),
        id,
        method: "session/new".into(),
        params: json!({ "cwd": cwd, "mcpServers": mcp_servers }),
    }
}

pub fn build_session_resume_request(session_id: &str, cwd: &str, id: AcpId) -> AcpRequest {
    session_request("session/resume", session_id, cwd, id)
}

pub fn build_session_fork_request(session_id: &str, cwd: &str, id: AcpId) -> AcpRequest {
    session_request("session/fork", session_id, cwd, id)
}

fn session_request(method: &str, session_id: &str, cwd: &str, id: AcpId) -> AcpRequest {
    AcpRequest {
        jsonrpc: "2.0".into(),
        id,
        method: method.into(),
        params: json!({ "sessionId": session_id, "cwd": cwd, "mcpServers": [] }),
    }
}

pub fn parse_resumable_capability(result: &Value) -> bool {
    let Some(capabilities) = result.get("agentCapabilities") else {
        return false;
    };
    capabilities
        .get("loadSession")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && capabilities
            .get("sessionCapabilities")
            .and_then(|value| value.get("resume"))
            .is_some_and(|value| !value.is_null())
}

#[cfg(test)]
mod tests {
    use tokio::io::{duplex, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    use super::*;
    use crate::bridge::framing::Framing;

    #[tokio::test]
    async fn duplex_pipe_correlates_handshake_and_demuxes_events() {
        let (client_io, peer_io) = duplex(4096);
        let (reader, writer) = tokio::io::split(client_io);
        let client = AcpClient::new(Box::pin(writer), AcpFraming::NewlineJson, 8);
        client.start_reader(reader);
        client.initialize().await.unwrap();
        client.session_new("/tmp/work", &[]).await.unwrap();

        let mut peer_io = BufReader::new(peer_io);
        let mut initialize_line = String::new();
        let mut session_line = String::new();
        peer_io.read_line(&mut initialize_line).await.unwrap();
        peer_io.read_line(&mut session_line).await.unwrap();
        assert!(initialize_line.contains("\"method\":\"initialize\""));
        assert!(session_line.contains("\"method\":\"session/new\""));

        let init = AcpMessage::Response(AcpResponse {
            jsonrpc: "2.0".into(),
            id: 1.into(),
            result: json!({"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"resume":{}}}}),
            error: None,
        });
        let session = AcpMessage::Response(AcpResponse {
            jsonrpc: "2.0".into(),
            id: 2.into(),
            result: json!({"sessionId":"s-1"}),
            error: None,
        });
        peer_io
            .get_mut()
            .write_all(&NewlineJson.encode(&init).unwrap())
            .await
            .unwrap();
        peer_io
            .get_mut()
            .write_all(&NewlineJson.encode(&session).unwrap())
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while client.session_id().await.is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(client.session_id().await.as_deref(), Some("s-1"));
        assert!(client.resumable().await);
        assert!(!client.handshake_pending().await);
    }

    #[tokio::test]
    async fn replay_notifications_are_dropped_until_correlated_handshake_response() {
        let (client_io, mut peer_io) = duplex(4096);
        let (reader, writer) = tokio::io::split(client_io);
        let client = AcpClient::new(Box::pin(writer), AcpFraming::NewlineJson, 8);
        let mut events = client.subscribe();
        client.start_reader(reader);
        client.session_resume("s-1", "/tmp").await.unwrap();
        let mut discard = vec![0_u8; 1024];
        let _ = peer_io.read(&mut discard).await.unwrap();

        let replay = AcpMessage::Notification(AcpNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: json!({"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"OLD"}}}),
        });
        peer_io
            .write_all(&NewlineJson.encode(&replay).unwrap())
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), events.recv())
                .await
                .is_err()
        );
    }
}
