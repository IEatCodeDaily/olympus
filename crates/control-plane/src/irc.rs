//! In-process IRC bus for inter-agent messaging (ADR 0006 §2, footgun 2).
//!
//! Stolen wholesale from omp's IRC semantics: peers register, DM each other,
//! have inboxes, can list peers. This is the in-process (single-node) impl;
//! cross-node over iroh is a later step (footgun 2 resolution).
//!
//! The bus is ephemeral (in-memory) — IRC messages are real-time, not durable.
//! The event log is NOT involved; the bus is a runtime concern. If a node
//! restarts, in-flight IRC messages are lost (by design — this matches omp).
//!
//! API surface:
//! - register(session_id) → peer joins the bus with an inbox
//! - send(from, to, message) → DM another peer
//! - list() → enumerate active peers
//! - inbox(session_id) → drain pending messages
//! - unregister(session_id) → peer leaves (inbox dropped)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// An IRC message (a DM from one peer to another).
#[derive(Debug, Clone)]
pub struct IrcMessage {
    pub from: String,
    pub to: String,
    pub content: String,
    pub timestamp: f64,
}

/// A registered peer on the bus. Each peer has an inbox channel.
struct Peer {
    /// Inbox sender — the bus pushes messages here; the peer drains via the
    /// receiver (held by the peer's runtime, or polled via the API).
    inbox_tx: mpsc::UnboundedSender<IrcMessage>,
}

/// The in-process IRC bus. Thread-safe via RwLock.
#[derive(Clone)]
pub struct IrcBus {
    peers: Arc<RwLock<HashMap<String, Peer>>>,
}

impl IrcBus {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new peer. Returns an inbox receiver the caller drains.
    /// If the peer is already registered, returns the existing inbox
    /// (idempotent — re-registering is a no-op).
    pub async fn register(&self, session_id: &str) -> mpsc::UnboundedReceiver<IrcMessage> {
        let (tx, rx) = mpsc::unbounded_channel();

        let mut peers = self.peers.write().await;
        // If already registered, return a fresh receiver (the old one may be
        // dropped). Replace the sender to reset the inbox.
        peers.insert(session_id.to_string(), Peer { inbox_tx: tx });
        rx
    }

    /// Send a DM from one peer to another. Returns Err if the sender is not
    /// registered or the recipient doesn't exist.
    pub async fn send(&self, from: &str, to: &str, content: &str) -> Result<(), IrcError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        let msg = IrcMessage {
            from: from.to_string(),
            to: to.to_string(),
            content: content.to_string(),
            timestamp: now,
        };

        let peers = self.peers.read().await;
        if !peers.contains_key(from) {
            return Err(IrcError::SenderNotRegistered(from.to_string()));
        }
        let Some(peer) = peers.get(to) else {
            return Err(IrcError::RecipientNotFound(to.to_string()));
        };
        peer.inbox_tx
            .send(msg)
            .map_err(|_| IrcError::InboxClosed(to.to_string()))
    }

    /// List all registered peer session_ids.
    pub async fn list_peers(&self) -> Vec<String> {
        let peers = self.peers.read().await;
        peers.keys().cloned().collect()
    }

    /// Check if a peer is registered.
    pub async fn is_registered(&self, session_id: &str) -> bool {
        self.peers.read().await.contains_key(session_id)
    }

    /// Unregister a peer (they leave the bus). Their inbox is dropped.
    pub async fn unregister(&self, session_id: &str) {
        self.peers.write().await.remove(session_id);
    }

    /// Drain (poll) a peer's inbox. Returns messages in FIFO order.
    /// The peer must have registered first (via register()).
    /// This is a non-blocking drain — returns empty vec if no messages.
    ///
    /// Note: this requires the caller to hold the receiver returned by
    /// register(). For API-level polling without holding the receiver,
    /// use the IrcBusHandle abstraction (below).
    pub async fn peer_count(&self) -> usize {
        self.peers.read().await.len()
    }
}

impl Default for IrcBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors from the IRC bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrcError {
    SenderNotRegistered(String),
    RecipientNotFound(String),
    InboxClosed(String),
}

impl std::fmt::Display for IrcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SenderNotRegistered(id) => write!(f, "sender {id} is not registered on the bus"),
            Self::RecipientNotFound(id) => write!(f, "recipient {id} is not registered on the bus"),
            Self::InboxClosed(id) => write!(f, "inbox for {id} is closed"),
        }
    }
}

impl std::error::Error for IrcError {}

/// A handle that holds both the bus and the inbox receiver, for API-level
/// polling. Created when a peer registers via the API endpoint.
pub struct PeerHandle {
    pub session_id: String,
    pub bus: IrcBus,
    inbox: Arc<RwLock<mpsc::UnboundedReceiver<IrcMessage>>>,
}

impl PeerHandle {
    /// Drain all pending messages from the inbox (non-blocking).
    pub async fn drain(&self) -> Vec<IrcMessage> {
        let mut inbox = self.inbox.write().await;
        let mut msgs = Vec::new();
        while let Ok(msg) = inbox.try_recv() {
            msgs.push(msg);
        }
        msgs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_list() {
        let bus = IrcBus::new();
        let _rx = bus.register("sess-1").await;
        let _rx2 = bus.register("sess-2").await;
        let peers = bus.list_peers().await;
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&"sess-1".to_string()));
        assert!(peers.contains(&"sess-2".to_string()));
    }

    #[tokio::test]
    async fn send_dm_delivers_to_inbox() {
        let bus = IrcBus::new();
        let mut rx = bus.register("sess-1").await;
        let _rx2 = bus.register("sess-2").await;

        bus.send("sess-2", "sess-1", "hello").await.unwrap();

        let msg = rx.recv().await.expect("should receive message");
        assert_eq!(msg.from, "sess-2");
        assert_eq!(msg.to, "sess-1");
        assert_eq!(msg.content, "hello");
    }

    #[tokio::test]
    async fn send_to_unregistered_recipient_fails() {
        let bus = IrcBus::new();
        let _rx = bus.register("sess-1").await;

        let err = bus.send("sess-1", "ghost", "hi").await.unwrap_err();
        assert_eq!(err, IrcError::RecipientNotFound("ghost".into()));
    }

    #[tokio::test]
    async fn send_from_unregistered_sender_fails() {
        let bus = IrcBus::new();
        let _rx = bus.register("sess-1").await;

        let err = bus.send("ghost", "sess-1", "hi").await.unwrap_err();
        assert_eq!(err, IrcError::SenderNotRegistered("ghost".into()));
    }

    #[tokio::test]
    async fn unregister_removes_peer() {
        let bus = IrcBus::new();
        let _rx = bus.register("sess-1").await;
        assert_eq!(bus.peer_count().await, 1);

        bus.unregister("sess-1").await;
        assert_eq!(bus.peer_count().await, 0);
    }

    #[tokio::test]
    async fn re_register_replaces_inbox() {
        let bus = IrcBus::new();
        let mut old_rx = bus.register("sess-1").await;
        // Re-register replaces the sender; old receiver gets no more messages.
        let mut new_rx = bus.register("sess-1").await;

        bus.send("sess-1", "sess-1", "self-msg").await.unwrap();
        // Old receiver should NOT get it (sender was replaced).
        assert!(old_rx.try_recv().is_err());
        // New receiver gets it.
        let msg = new_rx
            .recv()
            .await
            .expect("new receiver should get message");
        assert_eq!(msg.content, "self-msg");
    }

    #[tokio::test]
    async fn multiple_messages_queue_in_order() {
        let bus = IrcBus::new();
        let mut rx = bus.register("receiver").await;
        let _ = bus.register("sender").await;

        for i in 0..5 {
            bus.send("sender", "receiver", &format!("msg-{i}"))
                .await
                .unwrap();
        }

        for i in 0..5 {
            let msg = rx.recv().await.expect("should receive");
            assert_eq!(msg.content, format!("msg-{i}"));
        }
    }
}
