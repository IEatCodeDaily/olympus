//! WebSocket delta stream (`/ws`) — the reactive half of the contract.
//!
//! Browser clients connect with their Hall cookie and `?organization=…`; legacy
//! operator clients may temporarily use `?token=…`. On connect the
//! server sends a `hello` frame with the current snapshot, then forwards every
//! [`ServerFrame`] broadcast by the view layer. The envelope mirrors
//! `docs/api-contract.md` §ServerFrame exactly (tagged `kind`, camelCase).
//!
//! S8 — session-scoped subscriptions + typing presence:
//! - Default on connect is **firehose** (all frames) — backward compatible.
//! - `subscribe {sessionIds}` narrows a connection to session-scoped frames
//!   (message deltas, typing, etc.) for only those sessions; session-list-level
//!   frames (session.added/updated/removed, sync.status, …) always flow.
//! - `typing {sessionId}` is ephemeral: broadcast as `user.typing` to
//!   subscribers, **never** event-logged.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use super::AppState;
use crate::server::dto::{MessageDto, SessionDto, ToolCallDto};

/// How long (seconds) a `user.typing` indicator is valid before the client
/// should hide it. The client debounces outbound typing frames at ~3 s; the
/// server TTL is deliberately a bit longer to cover debounce gaps.
const TYPING_TTL_SECS: f64 = 5.0;

/// Monotonic counter for anonymous display names (`anon-<N>`).
static ANON_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_anon_id() -> u64 {
    ANON_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Resolve the display name from the optional `?name=` query value. An empty,
/// whitespace-only, or over-long name falls back to `anon-<N>`.
fn resolve_name(raw: Option<String>) -> String {
    match raw {
        Some(n) => {
            let trimmed = n.trim();
            if trimmed.is_empty() || trimmed.len() > 64 {
                format!("anon-{}", next_anon_id())
            } else {
                trimmed.to_string()
            }
        }
        None => format!("anon-{}", next_anon_id()),
    }
}

/// Server→client frames (api-contract.md §ServerFrame). Internally tagged on
/// `kind`, camelCase fields.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ServerFrame {
    #[serde(rename = "hello")]
    Hello { snapshot: Snapshot },
    #[serde(rename = "session.added")]
    SessionAdded { session: SessionDto },
    #[serde(rename = "session.updated", rename_all = "camelCase")]
    SessionUpdated {
        session_id: String,
        changes: serde_json::Value,
    },
    #[serde(rename = "session.removed", rename_all = "camelCase")]
    SessionRemoved { session_id: String },
    #[serde(rename = "message.appended", rename_all = "camelCase")]
    MessageAppended {
        session_id: String,
        message: MessageDto,
    },
    /// A chunk of assistant text streamed from the agent (token-level delta).
    #[serde(rename = "message.delta", rename_all = "camelCase")]
    MessageDelta {
        session_id: String,
        message_id: u64,
        text_delta: String,
    },
    /// A tool call the agent is invoking, streamed live (not at turn end).
    /// Lets the UI interleave tool cards between text chunks as they happen.
    #[serde(rename = "message.toolCall", rename_all = "camelCase")]
    MessageToolCall {
        session_id: String,
        message_id: u64,
        tool_call: ToolCallDto,
    },
    /// A reasoning/CoT chunk streamed from the agent (token-level delta).
    #[serde(rename = "message.reasoning", rename_all = "camelCase")]
    MessageReasoning {
        session_id: String,
        message_id: u64,
        text_delta: String,
    },
    /// The agent's turn has finished (stopReason from ACP).
    #[serde(rename = "message.done", rename_all = "camelCase")]
    MessageDone {
        session_id: String,
        message_id: u64,
        finish_reason: Option<String>,
    },
    #[serde(rename = "sync.status")]
    SyncStatus { connected: bool },
    #[serde(rename = "cards.changed")]
    CardsChanged,
    /// The agent is blocked awaiting a permission decision for a gated tool
    /// call (ACP `session/request_permission`). The UI shows the options and
    /// POSTs the choice to `/api/sessions/:id/permission`.
    #[serde(rename = "permission.required", rename_all = "camelCase")]
    PermissionRequired {
        session_id: String,
        tool_call: String,
        options: serde_json::Value,
    },
    /// A structured lifecycle/progress event for a session (shown in the Logs
    /// panel). `level` is "info" | "warn" | "error"; `source` is the subsystem
    /// that emitted it (e.g. "bridge", "repo", "vault", "jj"); `message` is
    /// human-readable.
    #[serde(rename = "session.log", rename_all = "camelCase")]
    SessionLog {
        session_id: String,
        level: String,
        source: String,
        message: String,
        timestamp: f64,
    },
    /// Ephemeral typing indicator (S8). Broadcast to a session's subscribers
    /// when a connected client sends `typing {sessionId}`. Never event-logged;
    /// the client hides it once `expiresAt` passes.
    #[serde(rename = "user.typing", rename_all = "camelCase")]
    UserTyping {
        session_id: String,
        who: String,
        expires_at: f64,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Snapshot {
    pub sessions: u64,
    pub messages: u64,
}

/// Client→server frames (S8). Tagged on `kind`, camelCase fields.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ClientFrame {
    /// Add `sessionIds` to this connection's subscription set. The first
    /// `subscribe` transitions the connection from firehose to filtered mode.
    #[serde(rename_all = "camelCase")]
    Subscribe {
        #[serde(default)]
        session_ids: Vec<String>,
    },
    /// Remove `sessionIds` from the subscription set. With no `sessionIds`
    /// (or when the set becomes empty) the connection reverts to firehose.
    #[serde(rename_all = "camelCase")]
    Unsubscribe {
        #[serde(default)]
        session_ids: Vec<String>,
    },
    /// Typing presence — broadcast as `user.typing` to this session's
    /// subscribers. Ephemeral: never persisted to the event log.
    #[serde(rename_all = "camelCase")]
    Typing { session_id: String },
}

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    token: Option<String>,
    organization: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

/// WS upgrade handler. The Origin check already ran in the auth middleware;
/// here we validate the `?token=` query param (browsers can't set the
/// Authorization header on a WS upgrade). The optional `?name=` is the
/// display name for typing attribution (falls back to `anon-<N>`).
pub async fn ws_handler(
    State(state): State<AppState>,
    Query(q): Query<WsQuery>,
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let authorization = match authorize_ws(&state, &q, &headers) {
        Ok(authorization) => authorization,
        Err((status, message)) => return (status, message).into_response(),
    };
    let who = resolve_name(q.name);
    ws.on_upgrade(move |socket| handle_socket(socket, state, who, authorization))
}

#[derive(Debug, PartialEq, Eq)]
struct WsAuthorization {
    organization_id: Option<String>,
    user_id: Option<String>,
    session_token: Option<String>,
}

fn authorize_ws(
    state: &AppState,
    q: &WsQuery,
    headers: &axum::http::HeaderMap,
) -> Result<WsAuthorization, (StatusCode, &'static str)> {
    let legacy_ok = q
        .token
        .as_deref()
        .map(|t| t == state.token.as_str())
        .unwrap_or(false);
    let session_token = super::identity::session_token(headers);
    let cookie_principal = session_token
        .as_deref()
        .and_then(|token| {
            state
                .auth_store
                .resolve_session(token, super::identity::unix_timestamp())
                .ok()
        })
        .flatten();
    let cookie_organization = match (cookie_principal.as_ref(), q.organization.as_ref()) {
        (Some(principal), Some(organization_id)) => match state
            .auth_store
            .user_has_organization(&principal.user_id, organization_id)
        {
            Ok(true) => Some(organization_id.clone()),
            _ => return Err((StatusCode::FORBIDDEN, "organization access denied")),
        },
        (Some(_), None) => return Err((StatusCode::BAD_REQUEST, "organization required")),
        (None, _) => None,
    };
    if !legacy_ok && cookie_principal.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized"));
    }
    Ok(WsAuthorization {
        organization_id: cookie_organization,
        user_id: cookie_principal.map(|principal| principal.user_id),
        session_token,
    })
}

/// Operator authorization for the dedicated terminal WebSocket (ADR 0021).
/// Accepts the installation token (legacy operator) or a valid Hall session
/// cookie. Returns true if the caller is an authenticated operator. (A finer
/// operator-only RBAC is future hardening per the terminal review; today any
/// authenticated Hall principal is an operator.)
pub(crate) fn authorize_operator(
    state: &AppState,
    query_token: Option<&str>,
    headers: &axum::http::HeaderMap,
) -> bool {
    let legacy_ok = state.allow_installation_token
        && query_token
            .map(|t| t == state.token.as_str())
            .unwrap_or(false);
    if legacy_ok {
        return true;
    }
    super::identity::session_token(headers)
        .as_deref()
        .and_then(|token| {
            state
                .auth_store
                .resolve_session(token, super::identity::unix_timestamp())
                .ok()
                .flatten()
        })
        .is_some()
}

fn websocket_authorization_is_current(state: &AppState, authorization: &WsAuthorization) -> bool {
    match (
        authorization.session_token.as_deref(),
        authorization.user_id.as_deref(),
        authorization.organization_id.as_deref(),
    ) {
        (Some(token), Some(user_id), Some(organization_id)) => {
            let principal = state
                .auth_store
                .resolve_session(token, super::identity::unix_timestamp())
                .ok()
                .flatten();
            principal.is_some_and(|principal| principal.user_id == user_id)
                && state
                    .auth_store
                    .user_has_organization(user_id, organization_id)
                    .unwrap_or(false)
        }
        (None, None, None) => true,
        _ => false,
    }
}

/// Decide whether a frame should be delivered to a connection given its
/// subscription state.
///
/// `None` = firehose (default): deliver everything.
/// `Some(set)` = filtered: session-list-level frames always flow; session-
/// scoped frames (deltas, typing, logs, …) only for sessions in the set.
fn should_deliver(frame: &ServerFrame, subscriptions: &Option<HashSet<String>>) -> bool {
    match subscriptions {
        None => true,
        Some(set) => match frame {
            // Session-list-level — always delivered.
            ServerFrame::Hello { .. }
            | ServerFrame::SessionAdded { .. }
            | ServerFrame::SessionUpdated { .. }
            | ServerFrame::SessionRemoved { .. }
            | ServerFrame::SyncStatus { .. }
            | ServerFrame::CardsChanged => true,
            // Session-scoped — deliver only if subscribed.
            ServerFrame::MessageAppended { session_id, .. }
            | ServerFrame::MessageDelta { session_id, .. }
            | ServerFrame::MessageToolCall { session_id, .. }
            | ServerFrame::MessageReasoning { session_id, .. }
            | ServerFrame::MessageDone { session_id, .. }
            | ServerFrame::SessionLog { session_id, .. }
            | ServerFrame::PermissionRequired { session_id, .. }
            | ServerFrame::UserTyping { session_id, .. } => set.contains(session_id.as_str()),
        },
    }
}

async fn handle_socket(
    mut socket: WebSocket,
    state: AppState,
    who: String,
    authorization: WsAuthorization,
) {
    let organization_id = authorization.organization_id.clone();
    // Greet with a snapshot scoped to the authenticated organization. Legacy
    // operator connections retain the installation-wide aggregate.
    let snapshot = if let Some(organization_id) = organization_id.as_deref() {
        let views = state.views.read().await;
        let sessions: Vec<_> = views
            .sessions
            .list(&crate::views::Filters::default())
            .into_iter()
            .filter(|session| session.org_id == organization_id)
            .collect();
        Snapshot {
            sessions: sessions.len() as u64,
            messages: sessions
                .iter()
                .map(|session| views.messages.count(&session.session_id))
                .sum(),
        }
    } else {
        Snapshot {
            sessions: state.snapshot_sessions,
            messages: state.snapshot_messages,
        }
    };
    let hello = ServerFrame::Hello { snapshot };
    if send_frame(&mut socket, &hello).await.is_err() {
        return;
    }

    let mut rx = state.deltas.subscribe();
    // None = firehose (default); Some(set) = subscription-filtered.
    let mut subscriptions: Option<HashSet<String>> = None;
    let mut membership_check = tokio::time::interval(std::time::Duration::from_secs(30));

    loop {
        tokio::select! {
            _ = membership_check.tick(), if authorization.user_id.is_some() => {
                if !websocket_authorization_is_current(&state, &authorization) {
                    break;
                }
            }
            // Forward broadcast deltas to the client (filtered by subscription).
            delta = rx.recv() => {
                match delta {
                    Ok(frame) => {
                        if should_deliver(&frame, &subscriptions)
                            && frame_belongs_to_organization(
                                &frame,
                                organization_id.as_deref(),
                                &state,
                            )
                            .await
                            && send_frame(&mut socket, &frame).await.is_err()
                        {
                            break;
                        }
                    }
                    // Lagged: drop and continue (client will reconcile via REST).
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            // Drain client messages: handle subscribe/unsubscribe/typing,
            // ignore everything else (ping/pong liveness is automatic).
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        handle_client_frame(
                            text.as_str(),
                            &who,
                            organization_id.as_deref(),
                            &state,
                            &mut subscriptions,
                        ).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
}

async fn frame_belongs_to_organization(
    frame: &ServerFrame,
    organization_id: Option<&str>,
    state: &AppState,
) -> bool {
    let Some(organization_id) = organization_id else {
        return true;
    };
    match frame {
        ServerFrame::SessionAdded { session } => session.org_id == organization_id,
        ServerFrame::SessionUpdated { session_id, .. }
        | ServerFrame::MessageAppended { session_id, .. }
        | ServerFrame::MessageDelta { session_id, .. }
        | ServerFrame::MessageToolCall { session_id, .. }
        | ServerFrame::MessageReasoning { session_id, .. }
        | ServerFrame::MessageDone { session_id, .. }
        | ServerFrame::SessionLog { session_id, .. }
        | ServerFrame::PermissionRequired { session_id, .. }
        | ServerFrame::UserTyping { session_id, .. } => state
            .views
            .read()
            .await
            .sessions
            .get(session_id)
            .is_some_and(|session| session.org_id == organization_id),
        // Removal currently carries no ownership metadata and occurs after the
        // row is gone. Deny it rather than risk a cross-organization leak.
        ServerFrame::SessionRemoved { .. } | ServerFrame::CardsChanged => false,
        ServerFrame::Hello { .. } | ServerFrame::SyncStatus { .. } => true,
    }
}

/// Parse and apply a single inbound client frame.
async fn handle_client_frame(
    raw: &str,
    who: &str,
    organization_id: Option<&str>,
    state: &AppState,
    subscriptions: &mut Option<HashSet<String>>,
) {
    let Ok(frame) = serde_json::from_str::<ClientFrame>(raw) else {
        return; // ignore malformed / unknown frames
    };
    match frame {
        ClientFrame::Subscribe { session_ids } => {
            let set = subscriptions.get_or_insert_with(HashSet::new);
            for id in session_ids {
                set.insert(id);
            }
        }
        ClientFrame::Unsubscribe { session_ids } => {
            if session_ids.is_empty() {
                // No ids = revert to firehose.
                *subscriptions = None;
            } else if let Some(set) = subscriptions {
                for id in session_ids {
                    set.remove(&id);
                }
                if set.is_empty() {
                    *subscriptions = None;
                }
            }
        }
        ClientFrame::Typing { session_id } => {
            if let Some(organization_id) = organization_id {
                let allowed = state
                    .views
                    .read()
                    .await
                    .sessions
                    .get(&session_id)
                    .is_some_and(|session| session.org_id == organization_id);
                if !allowed {
                    return;
                }
            }
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            let frame = ServerFrame::UserTyping {
                session_id,
                who: who.to_string(),
                expires_at: now + TYPING_TTL_SECS,
            };
            // Broadcast via the shared delta channel. Each connection's
            // `should_deliver` filters it to only this session's subscribers.
            // Never written to the event log — typing is ephemeral by design.
            let _ = state.deltas.send(frame);
        }
    }
}

async fn send_frame(socket: &mut WebSocket, frame: &ServerFrame) -> Result<(), ()> {
    let json = serde_json::to_string(frame).map_err(|_| ())?;
    socket
        .send(Message::Text(json.into()))
        .await
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_frame_serializes_to_contract_shape() {
        let f = ServerFrame::Hello {
            snapshot: Snapshot {
                sessions: 5,
                messages: 42,
            },
        };
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["kind"], "hello");
        assert_eq!(v["snapshot"]["sessions"], 5);
        assert_eq!(v["snapshot"]["messages"], 42);
    }

    #[test]
    fn session_updated_uses_camelcase_kind_and_fields() {
        let f = ServerFrame::SessionUpdated {
            session_id: "s1".into(),
            changes: serde_json::json!({ "title": "renamed" }),
        };
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["kind"], "session.updated");
        assert_eq!(v["sessionId"], "s1");
        assert_eq!(v["changes"]["title"], "renamed");
    }

    #[test]
    fn message_appended_frame_shape() {
        let f = ServerFrame::MessageAppended {
            session_id: "s1".into(),
            message: MessageDto {
                message_id: 7,
                session_id: "s1".into(),
                role: "assistant".into(),
                content: Some("hi".into()),
                tool_name: None,
                tool_calls: None,
                reasoning: None,
                timestamp: 1.0,
                token_count: None,
                finish_reason: None,
            },
        };
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["kind"], "message.appended");
        assert_eq!(v["sessionId"], "s1");
        assert_eq!(v["message"]["messageId"], 7);
    }

    #[test]
    fn message_delta_frame_shape() {
        let f = ServerFrame::MessageDelta {
            session_id: "s1".into(),
            message_id: 5,
            text_delta: "PON".into(),
        };
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["kind"], "message.delta");
        assert_eq!(v["sessionId"], "s1");
        assert_eq!(v["messageId"], 5);
        assert_eq!(v["textDelta"], "PON");
    }

    #[test]
    fn message_done_frame_shape() {
        let f = ServerFrame::MessageDone {
            session_id: "s1".into(),
            message_id: 5,
            finish_reason: Some("end_turn".into()),
        };
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["kind"], "message.done");
        assert_eq!(v["sessionId"], "s1");
        assert_eq!(v["messageId"], 5);
        assert_eq!(v["finishReason"], "end_turn");
    }

    // ── S8: user.typing serialization ──────────────────────────────────

    #[test]
    fn user_typing_frame_shape() {
        let f = ServerFrame::UserTyping {
            session_id: "s1".into(),
            who: "alice".into(),
            expires_at: 1234.5,
        };
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["kind"], "user.typing");
        assert_eq!(v["sessionId"], "s1");
        assert_eq!(v["who"], "alice");
        assert_eq!(v["expiresAt"], 1234.5);
    }

    // ── S8: ClientFrame deserialization ────────────────────────────────

    #[test]
    fn client_frame_subscribe_parses() {
        let json = r#"{"kind":"subscribe","sessionIds":["s1","s2"]}"#;
        let f: ClientFrame = serde_json::from_str(json).unwrap();
        assert_eq!(
            f,
            ClientFrame::Subscribe {
                session_ids: vec!["s1".into(), "s2".into()]
            }
        );
    }

    #[test]
    fn client_frame_unsubscribe_empty_reverts_to_firehose() {
        let json = r#"{"kind":"unsubscribe"}"#;
        let f: ClientFrame = serde_json::from_str(json).unwrap();
        assert_eq!(
            f,
            ClientFrame::Unsubscribe {
                session_ids: vec![]
            }
        );
    }

    #[test]
    fn client_frame_typing_parses() {
        let json = r#"{"kind":"typing","sessionId":"s3"}"#;
        let f: ClientFrame = serde_json::from_str(json).unwrap();
        assert_eq!(
            f,
            ClientFrame::Typing {
                session_id: "s3".into()
            }
        );
    }

    #[test]
    fn client_frame_unknown_kind_ignored() {
        let json = r#"{"kind":"bogus"}"#;
        assert!(serde_json::from_str::<ClientFrame>(json).is_err());
    }

    // ── S8: subscription filtering logic ───────────────────────────────

    #[test]
    fn firehose_delivers_everything() {
        let subs: Option<HashSet<String>> = None;
        assert!(should_deliver(
            &ServerFrame::MessageDelta {
                session_id: "s1".into(),
                message_id: 1,
                text_delta: "x".into(),
            },
            &subs,
        ));
        assert!(should_deliver(
            &ServerFrame::UserTyping {
                session_id: "s1".into(),
                who: "a".into(),
                expires_at: 0.0,
            },
            &subs,
        ));
    }

    #[test]
    fn session_list_frames_always_delivered_when_filtered() {
        let subs = Some(HashSet::from(["s1".to_string()]));
        // These have no sessionId — always delivered.
        assert!(should_deliver(
            &ServerFrame::SessionAdded {
                session: SessionDto {
                    id: "s2".into(),
                    hermes_id: "h".into(),
                    org_id: "o".into(),
                    owner_id: "u".into(),
                    context_id: None,
                    source: "cli".into(),
                    model: None,
                    title: None,
                    started_at: 0.0,
                    last_activity: 0.0,
                    message_count: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    archived: false,
                    pinned: false,
                    forked_from: None,
                    fork_point: None,
                    fork_type: None,
                    managed: false,
                    agent: None,
                    node: None,
                    liveness: "idle".into(),
                    parent_session_id: None,
                    card_id: None,
                    capabilities: None,
                },
            },
            &subs,
        ));
        assert!(should_deliver(&ServerFrame::CardsChanged, &subs));
        assert!(should_deliver(
            &ServerFrame::SyncStatus { connected: true },
            &subs,
        ));
    }

    #[test]
    fn two_clients_subscription_filtering() {
        // Simulate the plan's scenario: two clients, one subscribed to s1,
        // one in firehose mode. A delta for s1 goes to both; a delta for s2
        // goes to only the firehose client.
        let firehose: Option<HashSet<String>> = None;
        let subscribed = Some(HashSet::from(["s1".to_string()]));

        let s1_delta = ServerFrame::MessageDelta {
            session_id: "s1".into(),
            message_id: 1,
            text_delta: "x".into(),
        };
        let s2_delta = ServerFrame::MessageDelta {
            session_id: "s2".into(),
            message_id: 2,
            text_delta: "y".into(),
        };
        let s1_typing = ServerFrame::UserTyping {
            session_id: "s1".into(),
            who: "a".into(),
            expires_at: 0.0,
        };
        let s2_typing = ServerFrame::UserTyping {
            session_id: "s2".into(),
            who: "b".into(),
            expires_at: 0.0,
        };

        // Firehose client gets everything.
        assert!(should_deliver(&s1_delta, &firehose));
        assert!(should_deliver(&s2_delta, &firehose));
        assert!(should_deliver(&s1_typing, &firehose));
        assert!(should_deliver(&s2_typing, &firehose));

        // Subscribed client gets only s1-scoped frames.
        assert!(should_deliver(&s1_delta, &subscribed));
        assert!(!should_deliver(&s2_delta, &subscribed));
        assert!(should_deliver(&s1_typing, &subscribed));
        assert!(!should_deliver(&s2_typing, &subscribed));
    }

    #[tokio::test]
    async fn unsubscribe_all_reverts_to_firehose() {
        let mut subs: Option<HashSet<String>> =
            Some(HashSet::from(["s1".to_string(), "s2".to_string()]));
        let state = make_test_state();
        // Unsubscribe specific session.
        handle_client_frame(
            r#"{"kind":"unsubscribe","sessionIds":["s1"]}"#,
            "x",
            None,
            &state,
            &mut subs,
        )
        .await;
        let set = subs.as_ref().unwrap();
        assert!(set.contains("s2") && !set.contains("s1"));

        // Unsubscribe the last session → reverts to firehose (None).
        handle_client_frame(
            r#"{"kind":"unsubscribe","sessionIds":["s2"]}"#,
            "x",
            None,
            &state,
            &mut subs,
        )
        .await;
        assert!(subs.is_none());

        // Bare unsubscribe also reverts to firehose.
        subs = Some(HashSet::from(["s3".to_string()]));
        handle_client_frame(r#"{"kind":"unsubscribe"}"#, "x", None, &state, &mut subs).await;
        assert!(subs.is_none());
    }

    #[tokio::test]
    async fn typing_broadcast_produces_frame_with_ttl() {
        let mut subs: Option<HashSet<String>> = None;
        let state = make_test_state();

        // Subscribe BEFORE the broadcast so the receiver catches the frame.
        let mut rx = state.deltas.subscribe();
        let before = now_secs();
        handle_client_frame(
            r#"{"kind":"typing","sessionId":"s9"}"#,
            "alice",
            None,
            &state,
            &mut subs,
        )
        .await;

        // The frame was broadcast on the delta channel — receive it and
        // verify the shape + that expiresAt is ~5s in the future.
        let frame = rx.try_recv().expect("typing frame was not broadcast");
        match frame {
            ServerFrame::UserTyping {
                session_id,
                who,
                expires_at,
            } => {
                assert_eq!(session_id, "s9");
                assert_eq!(who, "alice");
                assert!(
                    expires_at >= before + TYPING_TTL_SECS - 0.5,
                    "expiresAt should be ~{TYPING_TTL_SECS}s in the future, got {expires_at}"
                );
                assert!(expires_at <= now_secs() + TYPING_TTL_SECS + 0.5);
            }
            other => panic!("expected UserTyping, got {other:?}"),
        }

        // Crucially: typing must NOT mutate the subscription set.
        assert!(subs.is_none());
    }

    #[tokio::test]
    async fn organization_filter_blocks_other_sessions_and_global_cards() {
        let state = make_test_state();
        add_session(&state, "s-a", "org-a").await;
        add_session(&state, "s-b", "org-b").await;
        let own = ServerFrame::MessageDelta {
            session_id: "s-a".into(),
            message_id: 1,
            text_delta: "own".into(),
        };
        let other = ServerFrame::MessageDelta {
            session_id: "s-b".into(),
            message_id: 2,
            text_delta: "other".into(),
        };

        assert!(frame_belongs_to_organization(&own, Some("org-a"), &state).await);
        assert!(!frame_belongs_to_organization(&other, Some("org-a"), &state).await);
        assert!(
            !frame_belongs_to_organization(&ServerFrame::CardsChanged, Some("org-a"), &state,)
                .await
        );
        assert!(frame_belongs_to_organization(&other, None, &state).await);
    }

    #[tokio::test]
    async fn organization_client_cannot_spoof_typing_for_another_session() {
        let state = make_test_state();
        add_session(&state, "s-b", "org-b").await;
        let mut rx = state.deltas.subscribe();
        let mut subscriptions = None;

        handle_client_frame(
            r#"{"kind":"typing","sessionId":"s-b"}"#,
            "alice",
            Some("org-a"),
            &state,
            &mut subscriptions,
        )
        .await;

        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn websocket_authorization_requires_cookie_membership_scope() {
        let state = make_test_state();
        state
            .auth_store
            .bootstrap_admin("admin", "password-123", "default", "Default")
            .unwrap();
        let principal = state
            .auth_store
            .authenticate("admin", "password-123")
            .unwrap()
            .unwrap();
        let organization = state
            .auth_store
            .organizations_for_user(&principal.user_id)
            .unwrap()
            .remove(0);
        let session = state
            .auth_store
            .create_session(
                &principal.user_id,
                super::super::identity::unix_timestamp(),
                60,
            )
            .unwrap();
        let headers = axum::http::HeaderMap::from_iter([(
            axum::http::header::COOKIE,
            format!("olympus_session={}", session.token)
                .parse()
                .unwrap(),
        )]);

        let missing_scope = WsQuery {
            token: None,
            organization: None,
            name: None,
        };
        assert_eq!(
            authorize_ws(&state, &missing_scope, &headers)
                .unwrap_err()
                .0,
            StatusCode::BAD_REQUEST
        );
        let member_scope = WsQuery {
            token: None,
            organization: Some(organization.id),
            name: None,
        };
        let authorized = authorize_ws(&state, &member_scope, &headers).unwrap();
        assert_eq!(authorized.organization_id, member_scope.organization);
        assert_eq!(
            authorized.user_id.as_deref(),
            Some(principal.user_id.as_str())
        );
        let nonmember_scope = WsQuery {
            token: None,
            organization: Some("another-org".into()),
            name: None,
        };
        assert_eq!(
            authorize_ws(&state, &nonmember_scope, &headers)
                .unwrap_err()
                .0,
            StatusCode::FORBIDDEN
        );
        assert!(websocket_authorization_is_current(&state, &authorized));
        state.auth_store.revoke_session(&session.token).unwrap();
        assert!(!websocket_authorization_is_current(&state, &authorized));
        let legacy = WsQuery {
            token: Some("t".into()),
            organization: None,
            name: None,
        };
        assert_eq!(
            authorize_ws(&state, &legacy, &axum::http::HeaderMap::new()).unwrap(),
            WsAuthorization {
                organization_id: None,
                user_id: None,
                session_token: None,
            }
        );
    }

    // ── helpers ─────────────────────────────────────────────────────────

    fn now_secs() -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
    }

    async fn add_session(state: &AppState, session_id: &str, organization_id: &str) {
        let mut views = state.views.write().await;
        views.apply(&crate::event::Event::SessionCreated {
            session_id: session_id.into(),
            hermes_id: format!("hermes-{session_id}"),
            source: "cli".into(),
            model: None,
            title: None,
            started_at: 1.0,
            message_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            agent: None,
            node: None,
        });
        views.apply(&crate::event::Event::SessionOrganizationAssigned {
            session_id: session_id.into(),
            organization_id: organization_id.into(),
        });
    }

    /// A minimal AppState whose only useful field is `deltas` (a broadcast
    /// channel) — enough to exercise the typing-broadcast path.
    fn make_test_state() -> AppState {
        use crate::server::{ImportState, IMPORT_DONE};
        use crate::{irc::IrcBus, node::NodeRegistry, proxy::ProxyTable};
        use std::sync::{atomic::AtomicBool, Arc};
        use tokio::sync::RwLock;

        let dir = tempfile::tempdir().unwrap();
        let log = std::sync::Arc::new(crate::log::Log::open(&dir.path().join("l.redb")).unwrap());

        AppState {
            views: Arc::new(RwLock::new(crate::views::ViewManager::new())),
            search: Arc::new(RwLock::new(
                crate::search::SearchIndex::open(&dir.path().join("idx")).unwrap(),
            )),
            token: Arc::new("t".into()),
            capability_signer: Arc::new(crate::server::capability::CapabilitySigner::for_tests()),
            auth_store: Arc::new(crate::auth_store::AuthStore::open_in_memory().unwrap()),
            allow_installation_token: true,
            session_cookie_secure: true,
            import_state: ImportState(Arc::new(std::sync::atomic::AtomicU8::new(IMPORT_DONE))),
            hermes_profile: Arc::new("p".into()),
            deltas: tokio::sync::broadcast::channel(64).0,
            snapshot_sessions: 0,
            snapshot_messages: 0,
            log: log.clone(),
            bridge: Arc::new(crate::server::bridge_mgr::BridgeManager::with_factory(
                log,
                crate::server::test_support::mock_factory(),
            )),
            sync_connected: Arc::new(AtomicBool::new(true)),
            irc: IrcBus::new(),
            nodes: NodeRegistry::new(),
            envoy_conns: crate::server::envoy_conn::EnvoyConnections::new(),
            hall_pty: crate::server::terminal_ws::HallTerminals::new(),
            hall_iroh_id: None,
            proxy: ProxyTable::new(),
            edge: crate::edge::EdgeManager::new(crate::edge::MemoryDriver::available()),
            vaults: Arc::new(crate::vault::VaultStore::with_jj_mode(
                dir.path().join("v"),
                crate::vault::JjMode::Disabled,
            )),
            state_db: None,
            projects: Arc::new(crate::projects::ProjectStore::new(dir.path().join("p"))),
            repos: Arc::new(crate::repos::RepoStore::new(dir.path().join("r"), "r")),
            enroll: crate::enroll::EnrollStore::new(),
            home: Arc::new(dir.path().to_path_buf()),
        }
    }
}
