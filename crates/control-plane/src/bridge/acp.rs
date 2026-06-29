//! ACP wire client — JSON-RPC 2.0 framing, message types, and
//! [`AgentCommand`]/[`AgentEvent`] mapping.
//!
//! # Transport
//!
//! The ACP specification uses Content-Length framing (LSP-style):
//! `Content-Length: <n>\r\n\r\n<json-body>`. The spike note
//! (`docs/reviews/acp-wire-spike.md` §"Verdict" line 14) observed the Python
//! reference client using newline-delimited JSON, but that was the client's own
//! readline loop, not a transport requirement. Hermes' ACP adapter follows the
//! spec's Content-Length framing. This implementation uses Content-Length
//! framing as mandated by the task spec and ACP proper.
//!
//! # Message model
//!
//! JSON-RPC 2.0 has three message shapes:
//! - **Request**: has `id`, expects a response with the same `id`.
//! - **Response**: has `id`, carries `result` (and optional `error`).
//! - **Notification**: no `id`, no response expected.
//!
//! See `docs/reviews/acp-wire-spike.md` for captured frames and
//! `docs/plans/2026-06-28-olympus-mvp.md` Task 4.1 for the method table.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{AgentCommand, AgentEvent};

// ---------------------------------------------------------------------------
// JSON-RPC id type
// ---------------------------------------------------------------------------

/// A JSON-RPC id. The spec allows string, number, or null; ACP uses integers.
/// We keep it as a loose [`Value`] so we never mangle the peer's id on
/// round-trip, but the constructor is integer-typed for Olympus's own requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AcpId(pub Value);

impl From<i64> for AcpId {
    fn from(n: i64) -> Self {
        Self(Value::from(n))
    }
}

// ---------------------------------------------------------------------------
// Message envelopes
// ---------------------------------------------------------------------------

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpRequest {
    pub jsonrpc: String,
    pub id: AcpId,
    pub method: String,
    pub params: Value,
}

/// A JSON-RPC 2.0 notification (request without an `id`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpResponse {
    pub jsonrpc: String,
    pub id: AcpId,
    #[serde(default)]
    pub result: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

/// Any JSON-RPC message (request, response, or notification).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AcpMessage {
    Request(AcpRequest),
    Response(AcpResponse),
    Notification(AcpNotification),
}

// ---------------------------------------------------------------------------
// Command → request/notification mapping
// ---------------------------------------------------------------------------

impl AcpRequest {
    /// Map an [`AgentCommand`] to an ACP **request** (has an `id`).
    ///
    /// Used for: Prompt, Steer, Slash, SwitchModel.
    /// Not used for: Cancel (a notification), Stop (closes the child).
    pub fn from_command(cmd: &AgentCommand, session_id: &str, id: AcpId) -> anyhow::Result<Self> {
        let (method, params) = match cmd {
            AgentCommand::Prompt { text, model: _ } => {
                // model switching is a separate session/set_model call; the
                // Prompt command carries model as a hint the runtime honours
                // before issuing session/prompt. It is NOT a session/prompt
                // param.
                (
                    "session/prompt",
                    json!({
                        "sessionId": session_id,
                        "prompt": [{"type": "text", "text": text}],
                    }),
                )
            }
            AgentCommand::Steer { text } => (
                "session/prompt",
                json!({
                    "sessionId": session_id,
                    "prompt": [{"type": "text", "text": format!("/steer {text}")}],
                }),
            ),
            AgentCommand::Slash { command } => (
                "session/prompt",
                json!({
                    "sessionId": session_id,
                    "prompt": [{"type": "text", "text": format!("/{command}")}],
                }),
            ),
            AgentCommand::SwitchModel { model } => (
                "session/set_model",
                json!({
                    "sessionId": session_id,
                    "modelId": model,
                }),
            ),
            AgentCommand::Cancel | AgentCommand::Stop => {
                anyhow::bail!(
                    "command {:?} is not a request — use AcpNotification::from_command",
                    cmd
                );
            }
        };
        Ok(Self {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params,
        })
    }
}

impl AcpNotification {
    /// Map an [`AgentCommand`] to an ACP **notification** (no `id`).
    ///
    /// Currently only [`AgentCommand::Cancel`] maps to a notification
    /// (`session/cancel`).
    pub fn from_command(cmd: &AgentCommand, session_id: &str) -> anyhow::Result<Self> {
        match cmd {
            AgentCommand::Cancel => Ok(Self {
                jsonrpc: "2.0".into(),
                method: "session/cancel".into(),
                params: json!({ "sessionId": session_id }),
            }),
            other => anyhow::bail!(
                "command {:?} is not a notification — use AcpRequest::from_command",
                other
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// session/update notification → AgentEvent mapping
// ---------------------------------------------------------------------------

impl AgentEvent {
    /// Map a `session/update` notification into an [`AgentEvent`].
    ///
    /// Recognised `sessionUpdate` kinds (from the spike):
    /// - `agent_message_chunk` → [`AgentEvent::Text`]
    /// - `agent_thought_chunk` → [`AgentEvent::Reasoning`]
    /// - `tool_call`            → [`AgentEvent::ToolCall`] (result None)
    /// - `tool_call_update`     → [`AgentEvent::ToolCall`] (result Some if completed)
    /// - `user_message_chunk`   → ignored (Olympus echoes its own prompts)
    /// - `available_commands_update` → ignored
    ///
    /// Returns `None` for notifications Olympus does not surface (the caller
    /// drops them silently).
    pub fn from_notification(notif: &AcpNotification) -> Option<Self> {
        if notif.method != "session/update" {
            return None;
        }
        let update = notif.params.get("update")?;
        let kind = update.get("sessionUpdate")?.as_str()?;
        match kind {
            "agent_message_chunk" => {
                let text = update
                    .get("content")
                    .and_then(|c| c.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                Some(AgentEvent::Text(text.into()))
            }
            "agent_thought_chunk" => {
                let text = update
                    .get("content")
                    .and_then(|c| c.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                Some(AgentEvent::Reasoning(text.into()))
            }
            "tool_call" => {
                let title = update
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("tool");
                let args = update
                    .get("content")
                    .map(|c| serde_json::to_string(c).unwrap_or_default())
                    .unwrap_or_default();
                Some(AgentEvent::ToolCall {
                    name: title.into(),
                    args,
                    result: None,
                })
            }
            "tool_call_update" => {
                let title = update
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("tool");
                let content_str = update
                    .get("content")
                    .map(|c| serde_json::to_string(c).unwrap_or_default())
                    .unwrap_or_default();
                let status = update.get("status").and_then(|s| s.as_str());
                // Only attach a result when the tool call has completed.
                let result = if status == Some("completed") {
                    Some(content_str)
                } else {
                    None
                };
                Some(AgentEvent::ToolCall {
                    name: title.into(),
                    args: String::new(),
                    result,
                })
            }
            // user_message_chunk, available_commands_update, etc. are not surfaced.
            _ => None,
        }
    }

    /// Map a final prompt response (the reply to a `session/prompt` request)
    /// into a [`AgentEvent::Done`], carrying `stopReason` as `finish_reason`.
    pub fn from_response(resp: &AcpResponse) -> Option<Self> {
        if resp.error.is_some() {
            return Some(AgentEvent::Error(
                resp.error
                    .as_ref()
                    .map(|e| serde_json::to_string(e).unwrap_or_else(|_| "error".into()))
                    .unwrap_or_else(|| "error".into()),
            ));
        }
        let stop_reason = resp
            .result
            .get("stopReason")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        Some(AgentEvent::Done {
            finish_reason: stop_reason,
        })
    }
}

// ---------------------------------------------------------------------------
// Content-Length framing
// ---------------------------------------------------------------------------

/// LSP/ACP-style Content-Length frame codec.
///
/// Wire format: `Content-Length: <n>\r\n\r\n<json-body-of-n-bytes>`
pub struct Frame;

impl Frame {
    /// Encode a message into a Content-Length-framed byte buffer.
    pub fn encode(msg: &AcpMessage) -> anyhow::Result<Vec<u8>> {
        let body = serde_json::to_vec(msg)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut out = Vec::with_capacity(header.len() + body.len());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(&body);
        Ok(out)
    }

    /// Decode a JSON body (the bytes after the `Content-Length` header) into a
    /// message.
    pub fn decode(body: &[u8]) -> anyhow::Result<AcpMessage> {
        let msg: AcpMessage = serde_json::from_slice(body)?;
        Ok(msg)
    }
}
