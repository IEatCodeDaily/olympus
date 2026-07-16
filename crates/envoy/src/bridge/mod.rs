//! Olympus agent bridge — uniform command queue → real agent runtime.
//!
//! The bridge is the seam between Olympus's session model and an external agent
//! runtime (Hermes via ACP). Olympus issues high-level [`AgentCommand`]s and
//! consumes [`AgentEvent`]s; the concrete runtime (e.g. [`HermesAgentRuntime`])
//! maps those onto the real wire protocol.
//!
//! See `docs/plans/2026-06-28-olympus-mvp.md` Task 4.1 and
//! `docs/reviews/acp-wire-spike.md` for the source-verified ACP method table.

pub mod acp;
pub mod child;
pub mod client;
pub mod framing;
pub mod hermes;

use std::pin::Pin;

use futures::stream::Stream;

// The serde data types (AgentCommand, AgentEvent, PermissionOption) moved to
// `olympus-proto` (ADR 0008 — proto is the only crate Hall and Envoy share).
// Re-exported here so existing call sites keep working unchanged.
pub use olympus_proto::{AgentCommand, AgentEvent, PermissionOption};

/// A runtime that drives an external agent (Hermes via ACP).
///
/// Implementations own the child process + stdio pipes and expose a uniform
/// command/event interface. The trait is async; events arrive as a [`Stream`]
/// of [`AgentEvent`].
#[async_trait::async_trait]
pub trait AgentRuntime: Send + Sync {
    /// Start (or resume) a session. Spawns the child process if needed and
    /// performs the ACP handshake + `session/new` / `session/resume`.
    async fn start(&self, session_id: Option<&str>) -> anyhow::Result<()>;
    /// Fork an existing Hermes session into a new runtime-owned session.
    async fn fork_session(&self, session_id: &str) -> anyhow::Result<()>;
    /// Send a command to the active session.
    async fn send(&self, cmd: AgentCommand) -> anyhow::Result<()>;
    /// Respond to a pending `session/request_permission` request by echoing the
    /// JSON-RPC `request_id` with the user's chosen `option_id` (or a
    /// `cancelled` outcome when `option_id` is None). Default impl is a no-op so
    /// runtimes that never gate tool calls (e.g. the mock) need not implement it.
    async fn respond_permission(
        &self,
        _request_id: &str,
        _option_id: Option<&str>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    /// Borrow the stream of agent events.
    fn events(&self) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send>>;
    /// Stop the runtime (close the child).
    async fn stop(&self) -> anyhow::Result<()>;
    /// The Hermes session id captured from the ACP session/new or session/resume
    /// response. Returns None if the runtime hasn't started or captured the id yet.
    async fn hermes_session_id(&self) -> Option<String>;
    /// Whether this runtime's adapter advertised cross-process session resume
    /// in its `initialize` response (`agentCapabilities.loadSession` +
    /// `sessionCapabilities.resume` — ADR 0008 §3, capability-driven, never
    /// harness-name-driven). Default false: fail closed when the capability
    /// was absent or never captured.
    async fn resumable(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::acp::{
        AcpMessage, AcpNotification, AcpRequest, AcpResponse, AgentEventAcpExt,
    };
    use crate::bridge::framing::{ContentLength, Framing};
    use serde_json::json;

    // ---- Test 1: frame encode/decode round-trips a JSON-RPC message ----

    #[test]
    fn frame_encode_decode_round_trips_request() {
        let req = AcpRequest {
            jsonrpc: "2.0".into(),
            id: 42.into(),
            method: "session/prompt".into(),
            params: json!({
                "sessionId": "s-1",
                "prompt": [{"type": "text", "text": "hello"}],
            }),
        };
        let msg = AcpMessage::Request(req);
        let mut frame = ContentLength.encode(&msg).expect("encode");
        assert!(frame.starts_with(b"Content-Length: "));
        let decoded = ContentLength
            .decode(&mut frame)
            .expect("decode")
            .expect("complete frame");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn request_permission_maps_to_awaiting_input() {
        // An incoming session/request_permission request → AwaitingInput with
        // the JSON-RPC id echoed and the options parsed.
        let req = AcpRequest {
            jsonrpc: "2.0".into(),
            id: 5.into(),
            method: "session/request_permission".into(),
            params: json!({
                "sessionId": "s-1",
                "toolCall": { "toolCallId": "call_1", "title": "Write config.json" },
                "options": [
                    { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
                    { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" }
                ]
            }),
        };
        let event = AgentEvent::from_request(&req).expect("maps to event");
        match event {
            AgentEvent::AwaitingInput {
                request_id,
                tool_call,
                options,
            } => {
                assert_eq!(request_id, "5");
                assert_eq!(tool_call, "Write config.json");
                assert_eq!(options.len(), 2);
                assert_eq!(options[0].option_id, "allow-once");
                assert_eq!(options[1].kind, "reject_once");
            }
            other => panic!("expected AwaitingInput, got {other:?}"),
        }
    }

    #[test]
    fn non_permission_request_maps_to_none() {
        let req = AcpRequest {
            jsonrpc: "2.0".into(),
            id: 1.into(),
            method: "fs/read_text_file".into(),
            params: json!({}),
        };
        assert_eq!(AgentEvent::from_request(&req), None);
    }

    #[test]
    fn frame_encode_decode_round_trips_notification() {
        let notif = AcpNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: json!({
                "sessionId": "s-1",
                "update": {
                    "content": {"text": "P", "type": "text"},
                    "sessionUpdate": "agent_message_chunk",
                },
            }),
        };
        let msg = AcpMessage::Notification(notif);
        let mut frame = ContentLength.encode(&msg).expect("encode");
        let decoded = ContentLength
            .decode(&mut frame)
            .expect("decode")
            .expect("complete frame");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn frame_encode_decode_round_trips_response() {
        let resp = AcpResponse {
            jsonrpc: "2.0".into(),
            id: 7.into(),
            result: json!({"stopReason": "end_turn"}),
            error: None,
        };
        let msg = AcpMessage::Response(resp);
        let mut frame = ContentLength.encode(&msg).expect("encode");
        let decoded = ContentLength
            .decode(&mut frame)
            .expect("decode")
            .expect("complete frame");
        assert_eq!(decoded, msg);
    }

    // ---- Test 2: AgentCommand::Steer serializes to session/prompt with "/steer ..." ----

    #[test]
    fn steer_command_serializes_to_session_prompt_with_steer_text() {
        let cmd = AgentCommand::Steer {
            text: "be concise".into(),
        };
        let req = AcpRequest::from_command(&cmd, "sess-1", 1.into()).expect("map steer");
        assert_eq!(req.method, "session/prompt");
        let prompt = req.params["prompt"].as_array().expect("prompt is array");
        let text = prompt[0]["text"].as_str().expect("text");
        assert_eq!(text, "/steer be concise");
    }

    #[test]
    fn prompt_command_serializes_to_plain_session_prompt() {
        let cmd = AgentCommand::Prompt {
            text: "hello world".into(),
            model: None,
        };
        let req = AcpRequest::from_command(&cmd, "sess-1", 1.into()).expect("map prompt");
        assert_eq!(req.method, "session/prompt");
        let text = req.params["prompt"][0]["text"].as_str().unwrap();
        assert_eq!(text, "hello world");
        // model is not part of session/prompt (set_model is a separate call)
        assert!(req.params.get("modelId").is_none());
    }

    #[test]
    fn slash_command_serializes_to_prompt_with_slash() {
        let cmd = AgentCommand::Slash {
            command: "compact".into(),
        };
        let req = AcpRequest::from_command(&cmd, "sess-1", 1.into()).expect("map slash");
        assert_eq!(req.method, "session/prompt");
        let text = req.params["prompt"][0]["text"].as_str().unwrap();
        assert_eq!(text, "/compact");
    }

    #[test]
    fn switch_model_command_serializes_to_session_set_model() {
        let cmd = AgentCommand::SwitchModel {
            model: "zai:glm-4.5".into(),
        };
        let req = AcpRequest::from_command(&cmd, "sess-1", 1.into()).expect("map switch_model");
        assert_eq!(req.method, "session/set_model");
        assert_eq!(req.params["modelId"].as_str().unwrap(), "zai:glm-4.5");
    }

    #[test]
    fn set_model_uses_set_model_for_hermes_style() {
        use olympus_proto::ModelSetStyle;
        let req = AcpRequest::set_model("sess-1", "zai:glm-4.5", ModelSetStyle::SetModel, 7.into());
        assert_eq!(req.method, "session/set_model");
        assert_eq!(req.params["sessionId"].as_str().unwrap(), "sess-1");
        assert_eq!(req.params["modelId"].as_str().unwrap(), "zai:glm-4.5");
        assert!(req.params.get("configId").is_none());
    }

    #[test]
    fn set_model_uses_config_option_for_claude_and_codex_style() {
        use olympus_proto::ModelSetStyle;
        // Claude Code / Codex adapters return -32601 "Method not found" for
        // session/set_model; they only accept session/set_config_option.
        let req = AcpRequest::set_model(
            "sess-1",
            "claude-sonnet-4-5",
            ModelSetStyle::ConfigOption,
            7.into(),
        );
        assert_eq!(req.method, "session/set_config_option");
        assert_eq!(req.params["sessionId"].as_str().unwrap(), "sess-1");
        assert_eq!(req.params["configId"].as_str().unwrap(), "model");
        assert_eq!(req.params["value"].as_str().unwrap(), "claude-sonnet-4-5");
        assert!(req.params.get("modelId").is_none());
    }

    #[test]
    fn model_set_style_is_harness_specific() {
        use crate::bridge::hermes::model_set_style_for_agent;
        use olympus_proto::ModelSetStyle;
        assert_eq!(
            model_set_style_for_agent(Some("default")),
            ModelSetStyle::SetModel
        );
        assert_eq!(
            model_set_style_for_agent(Some("coding-agent")),
            ModelSetStyle::SetModel
        );
        assert_eq!(
            model_set_style_for_agent(Some("claude-code")),
            ModelSetStyle::ConfigOption
        );
        assert_eq!(
            model_set_style_for_agent(Some("codex")),
            ModelSetStyle::ConfigOption
        );
    }

    // ---- Test 3: Cancel emits a JSON-RPC notification (no id), not a request ----

    #[test]
    fn cancel_command_emits_notification_without_id() {
        let cmd = AgentCommand::Cancel;
        let notif = AcpNotification::from_command(&cmd, "sess-1").expect("map cancel");
        assert_eq!(notif.method, "session/cancel");
        // A notification has no `id` field when serialized
        let serialized = serde_json::to_value(&notif).unwrap();
        assert!(serialized.get("id").is_none(), "cancel must not have an id");
        assert_eq!(notif.params["sessionId"].as_str().unwrap(), "sess-1");
    }

    // ---- Test 4: session/update notification → AgentEvent mapping ----

    #[test]
    fn agent_message_chunk_maps_to_text_event() {
        let notif = spike_agent_message_chunk("P");
        let event = AgentEvent::from_notification(&notif).expect("map chunk");
        assert_eq!(event, AgentEvent::Text("P".into()));
    }

    #[test]
    fn agent_message_chunk_concatenates_to_pong() {
        // Simulate the two PONG chunks from the spike
        let chunks = ["P", "ONG"];
        let mut text = String::new();
        for c in chunks {
            let notif = spike_agent_message_chunk(c);
            if let AgentEvent::Text(t) = AgentEvent::from_notification(&notif).unwrap() {
                text.push_str(&t);
            } else {
                panic!("expected Text event");
            }
        }
        assert_eq!(text, "PONG");
    }

    #[test]
    fn tool_call_maps_to_toolcall_event() {
        let notif = AcpNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: json!({
                "sessionId": "s-1",
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "tc-1",
                    "title": "terminal: echo hi",
                    "kind": "execute",
                    "content": [{"type": "content", "content": {"text": "echo hi", "type": "text"}}],
                },
            }),
        };
        let event = AgentEvent::from_notification(&notif).expect("map tool_call");
        match event {
            AgentEvent::ToolCall {
                id,
                name,
                args,
                status,
                result,
            } => {
                assert_eq!(id.as_deref(), Some("tc-1"));
                assert!(name.contains("terminal") || name.contains("echo") || !name.is_empty());
                assert!(!args.is_empty());
                assert_eq!(status.as_deref(), Some("pending"));
                assert!(result.is_none());
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_update_with_status_completed_maps_to_result() {
        let notif = AcpNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: json!({
                "sessionId": "s-1",
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "tc-1",
                    "status": "completed",
                    "content": [{
                        "type": "content",
                        "content": {"text": "exit_code: 0", "type": "text"}
                    }],
                },
            }),
        };
        let event = AgentEvent::from_notification(&notif).expect("map tool_call_update");
        match event {
            AgentEvent::ToolCall { result, .. } => {
                assert!(result.is_some(), "completed tool call should carry result");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn final_response_maps_to_done_event() {
        let resp = AcpResponse {
            jsonrpc: "2.0".into(),
            id: 3.into(),
            result: json!({"stopReason": "end_turn"}),
            error: None,
        };
        let event = AgentEvent::from_response(&resp).expect("map response");
        assert_eq!(
            event,
            AgentEvent::Done {
                finish_reason: Some("end_turn".into()),
            }
        );
    }

    #[test]
    fn cancelled_response_maps_to_done_cancelled() {
        let resp = AcpResponse {
            jsonrpc: "2.0".into(),
            id: 7.into(),
            result: json!({"stopReason": "cancelled"}),
            error: None,
        };
        let event = AgentEvent::from_response(&resp).expect("map response");
        assert_eq!(
            event,
            AgentEvent::Done {
                finish_reason: Some("cancelled".into()),
            }
        );
    }

    // ---- helpers ----

    fn spike_agent_message_chunk(text: &str) -> AcpNotification {
        AcpNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: json!({
                "sessionId": "2651c325-3bea-426a-a94f-89a3987e6398",
                "update": {
                    "content": {"text": text, "type": "text"},
                    "sessionUpdate": "agent_message_chunk",
                },
            }),
        }
    }
}
