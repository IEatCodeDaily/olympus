//! Hall↔Envoy wire frames (ADR 0008 §1).
//!
//! JSON-lines frames (one compact JSON object per line), internally tagged on
//! `"kind"` with camelCase field names — wire-compatible in style with the
//! existing node protocol (`node.rs` hello/heartbeat/bye).
//!
//! Frame families:
//! - Hall→Envoy ([`HallFrame`]): `ensure_runtime`, `prompt`, `steer`,
//!   `cancel`, `stop`, `respond_permission`, `drain`, `probe` — each with a
//!   Hall-assigned `reqId`; plus `ack {sessionId, seq}` (spool truncation
//!   watermark) and `resume_from {sessionId, seq}` (replay cursor at
//!   reconnect).
//! - Envoy→Hall ([`EnvoyFrame`]): `hello`, `heartbeat`, `bye`,
//!   `resp {reqId, ok|error}`, `event {sessionId, turnId, seq, payload}`,
//!   `runtimes {…}` (in hello and on change).
//!
//! Unknown fields are tolerated everywhere (no `deny_unknown_fields`); a hello
//! with an unexpected `protocolVersion` still *parses* — rejection is Hall's
//! policy decision, not serde's.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::AgentCommand;
use crate::runtime::RuntimeSpec;
use crate::version::BuildVersion;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    AgentRuntime,
    JobRunner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStream {
    Stdout,
    Stderr,
}

/// A normalized, read-only observation from Hermes `state.db`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ObservedEvent {
    Session {
        #[serde(rename = "hermesId")]
        hermes_id: String,
        source: String,
        model: Option<String>,
        title: Option<String>,
        #[serde(rename = "startedAt")]
        started_at: f64,
        #[serde(rename = "messageCount")]
        message_count: u64,
        #[serde(rename = "inputTokens")]
        input_tokens: u64,
        #[serde(rename = "outputTokens")]
        output_tokens: u64,
        archived: bool,
    },
    Message {
        #[serde(rename = "hermesId")]
        hermes_id: String,
        #[serde(rename = "messageId")]
        message_id: u64,
        role: String,
        content: Option<String>,
        #[serde(rename = "toolName")]
        tool_name: Option<String>,
        #[serde(rename = "toolCalls")]
        tool_calls: Option<String>,
        reasoning: Option<String>,
        timestamp: f64,
        #[serde(rename = "tokenCount")]
        token_count: Option<u64>,
        #[serde(rename = "finishReason")]
        finish_reason: Option<String>,
    },
}

/// One entry in the envoy's runtimes table: which session it holds, its
/// backing Hermes session id, and resume metadata (ADR 0008 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub session_id: String,
    /// The harness-side (Hermes/ACP) session id backing this runtime.
    #[serde(default)]
    pub hermes_id: Option<String>,
    /// Runtime lifecycle state (e.g. "running", "idle", "stopped").
    #[serde(default)]
    pub state: String,
    /// Capability-derived: whether this runtime's harness supports
    /// cross-process resume (`loadSession` + `sessionCapabilities.resume`).
    #[serde(default)]
    pub resumable: bool,
    /// Highest per-session event `seq` this envoy has assigned.
    #[serde(default)]
    pub last_seq: u64,
}

/// Hall→Envoy frames. Request frames carry a Hall-assigned `reqId`; the envoy
/// replies with [`EnvoyFrame::Resp`] echoing it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HallFrame {
    /// Ensure a runtime exists for `session_id` (spawn or resume). `resume_id`
    /// is the harness session id to resume; `spec` is the spawn configuration.
    EnsureRuntime {
        #[serde(rename = "reqId")]
        req_id: u64,
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(default)]
        spec: RuntimeSpec,
        /// Harness session id to resume (ADR §3: Hall must verify the
        /// returned id matches — provenance check).
        #[serde(default, rename = "resumeId")]
        resume_id: Option<String>,
    },
    /// Send a prompt to the session's runtime.
    Prompt {
        #[serde(rename = "reqId")]
        req_id: u64,
        #[serde(rename = "sessionId")]
        session_id: String,
        text: String,
        #[serde(default)]
        model: Option<String>,
    },
    /// Inject mid-turn guidance.
    Steer {
        #[serde(rename = "reqId")]
        req_id: u64,
        #[serde(rename = "sessionId")]
        session_id: String,
        text: String,
    },
    /// Cancel the running turn.
    Cancel {
        #[serde(rename = "reqId")]
        req_id: u64,
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    /// Stop the session's runtime (close the child).
    Stop {
        #[serde(rename = "reqId")]
        req_id: u64,
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    /// Answer a pending `session/request_permission`.
    RespondPermission {
        #[serde(rename = "reqId")]
        req_id: u64,
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "requestId")]
        request_id: String,
        /// The chosen permission option id; `None` → cancelled outcome.
        #[serde(default, rename = "optionId")]
        option_id: Option<String>,
    },
    /// Begin draining this envoy: no new sessions; hand over held sessions.
    Drain {
        #[serde(rename = "reqId")]
        req_id: u64,
        /// Target node for handover, if directed.
        #[serde(default, rename = "toNode")]
        to_node: Option<String>,
    },
    /// Health-gate probe: envoy replies with agent discovery (ADR §5).
    Probe {
        #[serde(rename = "reqId")]
        req_id: u64,
    },
    DispatchJob {
        #[serde(rename = "reqId")]
        req_id: u64,
        #[serde(rename = "jobId")]
        job_id: String,
        argv: Vec<String>,
        #[serde(default, rename = "envAllowlist")]
        env_allowlist: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(rename = "timeoutSecs")]
        timeout_secs: u64,
        #[serde(rename = "maxOutputBytes")]
        max_output_bytes: u64,
    },
    CancelJob {
        #[serde(rename = "reqId")]
        req_id: u64,
        #[serde(rename = "jobId")]
        job_id: String,
    },
    /// Spool truncation watermark: Hall has durably applied events for
    /// `session_id` up to and including `seq`.
    Ack {
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
    },
    /// Replay cursor at reconnect: envoy replays spooled events with
    /// `seq > seq`, then streams live.
    ResumeFrom {
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
    },
}

/// Envoy→Hall frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnvoyFrame {
    /// Registration handshake. `protocol_version` is the frame-schema compat
    /// gate (Hall rejects unknown values — fail closed, but the frame always
    /// *parses*); `version` is the build identity drain decisions key on.
    Hello {
        #[serde(rename = "nodeId")]
        node_id: String,
        hostname: String,
        #[serde(rename = "slotsTotal")]
        slots_total: u32,
        #[serde(rename = "protocolVersion")]
        protocol_version: u32,
        version: BuildVersion,
        /// Agents discovered on this envoy's host (harness-native JSON;
        /// proto stays decoupled from Hall's `AgentInfo` DTO).
        #[serde(default)]
        agents: Option<Value>,
        /// The envoy's runtimes table (which sessions it holds + lastSeq),
        /// used by Hall to relearn locations and drive `resume_from`.
        #[serde(default)]
        runtimes: Vec<RuntimeStatus>,
        #[serde(default)]
        roles: Vec<NodeRole>,
    },
    /// Liveness beat.
    Heartbeat {
        #[serde(rename = "nodeId")]
        node_id: String,
        #[serde(default, rename = "slotsUsed")]
        slots_used: u32,
    },
    /// Graceful disconnect.
    Bye {
        #[serde(rename = "nodeId")]
        node_id: String,
    },
    /// Reply to a Hall request frame: ok, or an error message. `result`
    /// carries request-specific payload (e.g. probe → discovery report).
    Resp {
        #[serde(rename = "reqId")]
        req_id: u64,
        ok: bool,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        result: Option<Value>,
    },
    /// A session event. `seq` is a per-session monotonic counter assigned by
    /// the envoy — the ordering/idempotency key for exactly-once replay.
    Event {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "turnId")]
        turn_id: String,
        seq: u64,
        payload: crate::agent::AgentEvent,
    },
    /// A host observation, sequenced and spooled exactly like runtime events.
    Observed {
        #[serde(rename = "sessionId")]
        session_id: String,
        seq: u64,
        payload: ObservedEvent,
    },
    /// Runtimes-table update (sent in hello and on change).
    Runtimes { runtimes: Vec<RuntimeStatus> },
    JobOutput {
        #[serde(rename = "jobId")]
        job_id: String,
        seq: u64,
        stream: JobStream,
        data: String,
    },
    JobResult {
        #[serde(rename = "jobId")]
        job_id: String,
        seq: u64,
        #[serde(rename = "exitCode")]
        exit_code: Option<i32>,
        truncated: bool,
        #[serde(rename = "timedOut")]
        timed_out: bool,
        cancelled: bool,
    },
}

impl HallFrame {
    /// The Hall-assigned request id, for frames that expect a `resp`.
    /// `ack`/`resume_from` are fire-and-forget and return `None`.
    pub fn req_id(&self) -> Option<u64> {
        match self {
            HallFrame::EnsureRuntime { req_id, .. }
            | HallFrame::Prompt { req_id, .. }
            | HallFrame::Steer { req_id, .. }
            | HallFrame::Cancel { req_id, .. }
            | HallFrame::Stop { req_id, .. }
            | HallFrame::RespondPermission { req_id, .. }
            | HallFrame::Drain { req_id, .. }
            | HallFrame::Probe { req_id }
            | HallFrame::DispatchJob { req_id, .. }
            | HallFrame::CancelJob { req_id, .. } => Some(*req_id),
            HallFrame::Ack { .. } | HallFrame::ResumeFrom { .. } => None,
        }
    }
}

impl From<&HallFrame> for Option<AgentCommand> {
    /// Map a session frame onto the runtime-level [`AgentCommand`] it drives,
    /// where a direct mapping exists.
    fn from(frame: &HallFrame) -> Self {
        match frame {
            HallFrame::Prompt { text, model, .. } => Some(AgentCommand::Prompt {
                text: text.clone(),
                model: model.clone(),
            }),
            HallFrame::Steer { text, .. } => Some(AgentCommand::Steer { text: text.clone() }),
            HallFrame::Cancel { .. } => Some(AgentCommand::Cancel),
            HallFrame::Stop { .. } => Some(AgentCommand::Stop),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentEvent;
    use crate::version::PROTOCOL_VERSION;
    use serde_json::json;

    fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(v: &T) {
        let json = serde_json::to_string(v).expect("serialize");
        let back: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&back, v, "round-trip mismatch for {json}");
    }

    fn sample_runtime_status() -> RuntimeStatus {
        RuntimeStatus {
            session_id: "s-1".into(),
            hermes_id: Some("h-1".into()),
            state: "running".into(),
            resumable: true,
            last_seq: 42,
        }
    }

    #[test]
    fn hall_frame_round_trips_every_variant() {
        let frames = [
            HallFrame::EnsureRuntime {
                req_id: 1,
                session_id: "s-1".into(),
                spec: RuntimeSpec {
                    agent: Some("default".into()),
                    ..Default::default()
                },
                resume_id: Some("h-1".into()),
            },
            HallFrame::Prompt {
                req_id: 2,
                session_id: "s-1".into(),
                text: "hi".into(),
                model: Some("m".into()),
            },
            HallFrame::Steer {
                req_id: 3,
                session_id: "s-1".into(),
                text: "focus".into(),
            },
            HallFrame::Cancel {
                req_id: 4,
                session_id: "s-1".into(),
            },
            HallFrame::Stop {
                req_id: 5,
                session_id: "s-1".into(),
            },
            HallFrame::RespondPermission {
                req_id: 6,
                session_id: "s-1".into(),
                request_id: "9".into(),
                option_id: Some("allow-once".into()),
            },
            HallFrame::Drain {
                req_id: 7,
                to_node: Some("envoy-2".into()),
            },
            HallFrame::Probe { req_id: 8 },
            HallFrame::Ack {
                session_id: "s-1".into(),
                seq: 10,
            },
            HallFrame::ResumeFrom {
                session_id: "s-1".into(),
                seq: 7,
            },
        ];
        for f in &frames {
            round_trip(f);
        }
    }

    #[test]
    fn envoy_frame_round_trips_every_variant() {
        let frames = [
            EnvoyFrame::Hello {
                node_id: "envoy-1".into(),
                hostname: "talos".into(),
                slots_total: 4,
                protocol_version: PROTOCOL_VERSION,
                version: BuildVersion::for_binary("0.1.0"),
                agents: Some(json!([{"id": "default", "kind": "hermes"}])),
                runtimes: vec![sample_runtime_status()],
                roles: vec![NodeRole::AgentRuntime, NodeRole::JobRunner],
            },
            EnvoyFrame::Heartbeat {
                node_id: "envoy-1".into(),
                slots_used: 2,
            },
            EnvoyFrame::Bye {
                node_id: "envoy-1".into(),
            },
            EnvoyFrame::Resp {
                req_id: 1,
                ok: true,
                error: None,
                result: Some(json!({"agents": []})),
            },
            EnvoyFrame::Resp {
                req_id: 2,
                ok: false,
                error: Some("spawn failed".into()),
                result: None,
            },
            EnvoyFrame::Event {
                session_id: "s-1".into(),
                turn_id: "t-1".into(),
                seq: 11,
                payload: AgentEvent::Text("chunk".into()),
            },
            EnvoyFrame::Observed {
                session_id: "observed:s-1".into(),
                seq: 12,
                payload: ObservedEvent::Message {
                    hermes_id: "s-1".into(),
                    message_id: 3,
                    role: "user".into(),
                    content: Some("hello".into()),
                    tool_name: None,
                    tool_calls: None,
                    reasoning: None,
                    timestamp: 1.0,
                    token_count: None,
                    finish_reason: None,
                },
            },
            EnvoyFrame::Runtimes {
                runtimes: vec![sample_runtime_status()],
            },
        ];
        for f in &frames {
            round_trip(f);
        }
    }

    #[test]
    fn frames_are_kind_tagged_camel_case() {
        let f = HallFrame::EnsureRuntime {
            req_id: 1,
            session_id: "s-1".into(),
            spec: RuntimeSpec::default(),
            resume_id: None,
        };
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["kind"], "ensure_runtime");
        assert_eq!(v["reqId"], 1);
        assert_eq!(v["sessionId"], "s-1");

        let e = EnvoyFrame::Event {
            session_id: "s-1".into(),
            turn_id: "t-1".into(),
            seq: 3,
            payload: AgentEvent::Text("x".into()),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "event");
        assert_eq!(v["turnId"], "t-1");
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let f: HallFrame = serde_json::from_str(
            r#"{"kind":"prompt","reqId":1,"sessionId":"s-1","text":"hi","futureField":{"x":1}}"#,
        )
        .expect("unknown field must not break deserialization");
        assert!(matches!(f, HallFrame::Prompt { req_id: 1, .. }));

        let e: EnvoyFrame = serde_json::from_str(
            r#"{"kind":"heartbeat","nodeId":"envoy-1","slotsUsed":1,"newThing":true}"#,
        )
        .expect("unknown field must not break deserialization");
        assert!(matches!(e, EnvoyFrame::Heartbeat { .. }));
    }

    #[test]
    fn hello_with_wrong_protocol_version_still_parses() {
        // Rejection of incompatible protocol versions is Hall's policy job at
        // registration time — serde must still parse the frame so Hall can see
        // the version and reject it explicitly (fail closed, but informed).
        let json = format!(
            r#"{{"kind":"hello","nodeId":"envoy-9","hostname":"h","slotsTotal":4,
                "protocolVersion":{},"version":{{"semver":"9.9.9"}}}}"#,
            PROTOCOL_VERSION + 40
        );
        let f: EnvoyFrame = serde_json::from_str(&json).expect("wrong version must still parse");
        match f {
            EnvoyFrame::Hello {
                protocol_version,
                version,
                runtimes,
                ..
            } => {
                assert_eq!(protocol_version, PROTOCOL_VERSION + 40);
                assert_eq!(version.git_hash, "unknown"); // #[serde(default)]
                assert!(runtimes.is_empty()); // #[serde(default)]
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn req_id_accessor_covers_request_frames_only() {
        assert_eq!(HallFrame::Probe { req_id: 5 }.req_id(), Some(5));
        assert_eq!(
            HallFrame::Ack {
                session_id: "s".into(),
                seq: 1
            }
            .req_id(),
            None
        );
    }

    #[test]
    fn prompt_frame_maps_to_agent_command() {
        let f = HallFrame::Prompt {
            req_id: 1,
            session_id: "s-1".into(),
            text: "hi".into(),
            model: None,
        };
        let cmd: Option<AgentCommand> = (&f).into();
        assert_eq!(
            cmd,
            Some(AgentCommand::Prompt {
                text: "hi".into(),
                model: None
            })
        );
    }
}
