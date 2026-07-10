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

/// ACP → [`AgentEvent`] mapping, as an extension trait because `AgentEvent`
/// now lives in `olympus-proto` (ADR 0008) and inherent impls on foreign types
/// are not allowed. Import this trait to call `AgentEvent::from_notification`
/// etc. exactly as before.
pub trait AgentEventAcpExt: Sized {
    /// Map a `session/update` notification into an [`AgentEvent`].
    fn from_notification(notif: &AcpNotification) -> Option<Self>;
    /// Map an incoming agent→client **request** into an [`AgentEvent`].
    fn from_request(req: &AcpRequest) -> Option<Self>;
    /// Map a final prompt response into an [`AgentEvent`].
    fn from_response(resp: &AcpResponse) -> Option<Self>;
}

impl AgentEventAcpExt for AgentEvent {
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
    fn from_notification(notif: &AcpNotification) -> Option<Self> {
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
                let id = update
                    .get("toolCallId")
                    .and_then(|i| i.as_str())
                    .map(String::from);
                let args = update
                    .get("content")
                    .map(|c| serde_json::to_string(c).unwrap_or_default())
                    .unwrap_or_default();
                // Initial status: ACP sends "pending" (queued / awaiting
                // permission) or "in_progress" (already running).
                let status = update
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("pending")
                    .to_string();
                Some(AgentEvent::ToolCall {
                    id,
                    name: title.into(),
                    args,
                    status: Some(status),
                    result: None,
                })
            }
            "tool_call_update" => {
                let title = update.get("title").and_then(|t| t.as_str()).unwrap_or("");
                let id = update
                    .get("toolCallId")
                    .and_then(|i| i.as_str())
                    .map(String::from);
                let content_str = update
                    .get("content")
                    .map(|c| serde_json::to_string(c).unwrap_or_default())
                    .unwrap_or_default();
                let status = update
                    .get("status")
                    .and_then(|s| s.as_str())
                    .map(String::from);
                // Attach the result on terminal states (completed OR failed) —
                // a failed tool's error output matters as much as a success.
                let result = if matches!(status.as_deref(), Some("completed") | Some("failed")) {
                    Some(content_str)
                } else {
                    None
                };
                Some(AgentEvent::ToolCall {
                    id,
                    name: title.into(),
                    args: String::new(),
                    status,
                    result,
                })
            }
            // user_message_chunk, available_commands_update, etc. are not surfaced.
            _ => None,
        }
    }

    /// Map an incoming agent→client **request** into an [`AgentEvent`].
    ///
    /// The only request Olympus surfaces is `session/request_permission` (the
    /// agent blocks waiting for a permission decision on a gated tool call).
    /// Other requests (fs/*, terminal/*) are handled elsewhere or ignored.
    fn from_request(req: &AcpRequest) -> Option<Self> {
        if req.method != "session/request_permission" {
            return None;
        }
        // Echo the exact JSON-RPC id back in the response, so serialize it.
        let request_id = serde_json::to_string(&req.id).ok()?;
        let tool_call = req
            .params
            .get("toolCall")
            .and_then(|tc| tc.get("title").or_else(|| tc.get("toolCallId")))
            .and_then(|v| v.as_str())
            .unwrap_or("tool call")
            .to_string();
        let options = req
            .params
            .get("options")
            .and_then(|o| o.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|o| {
                        Some(super::PermissionOption {
                            option_id: o.get("optionId")?.as_str()?.to_string(),
                            name: o
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("")
                                .to_string(),
                            kind: o
                                .get("kind")
                                .and_then(|k| k.as_str())
                                .unwrap_or("")
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(AgentEvent::AwaitingInput {
            request_id,
            tool_call,
            options,
        })
    }

    /// Map a final prompt response (the reply to a `session/prompt` request)
    /// into a [`AgentEvent::Done`], carrying `stopReason` as `finish_reason`.
    fn from_response(resp: &AcpResponse) -> Option<Self> {
        if resp.error.is_some() {
            return Some(AgentEvent::Error(
                resp.error
                    .as_ref()
                    .map(|e| serde_json::to_string(e).unwrap_or_else(|_| "error".into()))
                    .unwrap_or_else(|| "error".into()),
            ));
        }
        // Only a `session/prompt` response carries a `stopReason` and marks the
        // turn complete. Other responses (initialize, session/new, session/resume,
        // session/set_model) are handshake/control replies and must NOT be mapped
        // to a `Done` event — doing so would prematurely end a turn that hasn't
        // even started.
        let stop_reason = resp.result.get("stopReason").and_then(|s| s.as_str())?;
        Some(AgentEvent::Done {
            finish_reason: Some(stop_reason.to_string()),
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
