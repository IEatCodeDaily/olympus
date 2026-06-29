//! WebSocket delta stream (`/ws`) — the reactive half of the contract.
//!
//! Clients connect to `ws://127.0.0.1:8787/ws?token=…`. On connect the server
//! sends a `hello` frame with the current snapshot, then forwards every
//! [`ServerFrame`] broadcast by the view layer. The envelope mirrors
//! `docs/api-contract.md` §ServerFrame exactly (tagged `kind`, camelCase).

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
use crate::server::dto::{MessageDto, SessionDto};

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
    #[serde(rename = "sync.status")]
    SyncStatus { connected: bool },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Snapshot {
    pub sessions: u64,
    pub messages: u64,
}

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    token: Option<String>,
}

/// WS upgrade handler. The Origin check already ran in the auth middleware;
/// here we validate the `?token=` query param (browsers can't set the
/// Authorization header on a WS upgrade).
pub async fn ws_handler(
    State(state): State<AppState>,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let ok = q
        .token
        .as_deref()
        .map(|t| t == state.token.as_str())
        .unwrap_or(false);
    if !ok {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    // Greet with the current snapshot.
    let hello = ServerFrame::Hello {
        snapshot: Snapshot {
            sessions: state.snapshot_sessions,
            messages: state.snapshot_messages,
        },
    };
    if send_frame(&mut socket, &hello).await.is_err() {
        return;
    }

    let mut rx = state.deltas.subscribe();
    loop {
        tokio::select! {
            // Forward broadcast deltas to the client.
            delta = rx.recv() => {
                match delta {
                    Ok(frame) => {
                        if send_frame(&mut socket, &frame).await.is_err() {
                            break;
                        }
                    }
                    // Lagged: drop and continue (client will reconcile via REST).
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            // Drain client messages; we only care about close/ping liveness.
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
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
}
