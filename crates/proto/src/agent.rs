//! Agent command/event types — the uniform vocabulary between Olympus's
//! session model and an external agent runtime (Hermes via ACP).
//!
//! Moved from `olympus-control-plane`'s `bridge/mod.rs` (ADR 0008: proto is
//! the only shared crate). The `AgentRuntime` trait and the ACP mapping logic
//! stay envoy-side; these are pure serde data types.

use serde::{Deserialize, Serialize};

/// A high-level command Olympus issues to the agent runtime.
///
/// Each variant maps onto a real ACP method (source-verified in the spike):
/// - [`AgentCommand::Prompt`]  → `session/prompt` (text)
/// - [`AgentCommand::Steer`]   → `session/prompt` with text `"/steer <text>"`
///   (NOT an ACP method — slash is intercepted inside prompt handling)
/// - [`AgentCommand::Slash`]   → `session/prompt` with text `"/<command>"`
/// - [`AgentCommand::Cancel`]  → `session/cancel` (a *notification*, no id)
/// - [`AgentCommand::SwitchModel`] → `session/set_model`
/// - [`AgentCommand::Stop`]    → close the ACP child process
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentCommand {
    /// Send a prompt to the active session. `model` is optional; if set, the
    /// runtime switches model before prompting.
    Prompt {
        text: String,
        #[serde(default)]
        model: Option<String>,
    },
    /// Inject mid-turn guidance. Best-effort and turn-scoped: only lands while
    /// a turn is actively running.
    Steer { text: String },
    /// Invoke a Hermes slash command as prompt text (e.g. "compact", "reset").
    Slash { command: String },
    /// Cancel the running turn.
    Cancel,
    /// Switch the active session's model.
    SwitchModel { model: String },
    /// Stop the runtime (close the child process).
    Stop,
}

/// A permission option the agent offers for a gated tool call (ACP
/// `session/request_permission`). Mirrors the ACP `PermissionOption` shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionOption {
    /// Unique id echoed back in the response outcome.
    pub option_id: String,
    /// Human-readable label ("Allow once", "Reject", …).
    pub name: String,
    /// Hint: allow_once | allow_always | reject_once | reject_always.
    pub kind: String,
}

/// A streaming event emitted by the agent runtime, derived from
/// `session/update` notifications and final prompt responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentEvent {
    /// A chunk of assistant text (from `agent_message_chunk`).
    Text(String),
    /// A tool call / tool result (from `tool_call` / `tool_call_update`).
    ToolCall {
        /// ACP `toolCallId` — the stable key for matching updates to calls.
        #[serde(default)]
        id: Option<String>,
        name: String,
        args: String,
        /// ACP lifecycle status: "pending" | "in_progress" | "completed" | "failed".
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        result: Option<String>,
    },
    /// A reasoning chunk (from `agent_thought_chunk`).
    Reasoning(String),
    /// The agent is blocked awaiting a permission decision for a gated tool call
    /// (ACP `session/request_permission`). The turn is paused until the client
    /// responds with a chosen `option_id`. `request_id` is the JSON-serialized
    /// JSON-RPC id to echo in the response.
    AwaitingInput {
        request_id: String,
        tool_call: String,
        #[serde(default)]
        options: Vec<PermissionOption>,
    },
    /// The turn finished. `finish_reason` mirrors ACP `stopReason`
    /// (e.g. "end_turn", "cancelled").
    Done {
        #[serde(default)]
        finish_reason: Option<String>,
    },
    /// An error from the runtime.
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(v: &T) {
        let json = serde_json::to_string(v).expect("serialize");
        let back: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&back, v, "round-trip mismatch for {json}");
    }

    #[test]
    fn agent_command_round_trips_every_variant() {
        for cmd in [
            AgentCommand::Prompt {
                text: "hi".into(),
                model: Some("m".into()),
            },
            AgentCommand::Steer { text: "s".into() },
            AgentCommand::Slash {
                command: "compact".into(),
            },
            AgentCommand::Cancel,
            AgentCommand::SwitchModel { model: "m2".into() },
            AgentCommand::Stop,
        ] {
            round_trip(&cmd);
        }
    }

    #[test]
    fn agent_event_round_trips_every_variant() {
        for ev in [
            AgentEvent::Text("t".into()),
            AgentEvent::ToolCall {
                id: Some("tc-1".into()),
                name: "terminal".into(),
                args: "{}".into(),
                status: Some("completed".into()),
                result: Some("ok".into()),
            },
            AgentEvent::Reasoning("r".into()),
            AgentEvent::AwaitingInput {
                request_id: "5".into(),
                tool_call: "Write config".into(),
                options: vec![PermissionOption {
                    option_id: "allow-once".into(),
                    name: "Allow once".into(),
                    kind: "allow_once".into(),
                }],
            },
            AgentEvent::Done {
                finish_reason: Some("end_turn".into()),
            },
            AgentEvent::Error("boom".into()),
        ] {
            round_trip(&ev);
        }
    }
}
