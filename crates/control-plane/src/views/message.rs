//! Per-session message cache — a bounded sliding-window projection (ADR §11).
//!
//! Holds the most recent `WINDOW_SIZE` messages per session in memory for fast
//! `recent()` reads; the **count** of messages per session is tracked
//! independently of the window and reflects the total derived from the event
//! stream (so an evicted message still counts). Full history lives in the log
//! and is paged on demand — the view never holds all history resident.
//!
//! On restart it is rebuilt by [`super::ViewManager::replay`].

use std::collections::{HashMap, VecDeque};

use crate::event::Event;

/// Default number of messages retained per session in the hot window.
const DEFAULT_WINDOW_SIZE: usize = 50;

/// A row in the per-session message cache.
#[derive(Debug, Clone)]
pub struct MessageRow {
    pub message_id: u64,
    pub role: String,
    /// Decompressed message content (`None` for tool/system messages without text).
    pub content: Option<String>,
    pub tool_name: Option<String>,
    pub timestamp: f64,
    pub token_count: Option<u64>,
    /// OpenAI-style tool calls JSON (on assistant messages), e.g.
    /// `[{"id":...,"function":{"name":..,"arguments":..}}]`. Raw string here;
    /// parsed to JSON at the DTO boundary.
    pub tool_calls: Option<String>,
    /// Assistant reasoning text (where models expose it).
    pub reasoning: Option<String>,
}

/// Per-session message cache (bounded sliding window, ADR §11).
///
/// `hot` holds the recent window; `counts` holds the durable total per session.
/// A session that never had a message has no entry in either map.
pub struct MessageView {
    /// session_id → most-recent messages (newest at the back).
    hot: HashMap<String, VecDeque<MessageRow>>,
    /// session_id → total message count (independent of window eviction).
    counts: HashMap<String, u64>,
    window_size: usize,
}

impl MessageView {
    /// Construct an empty view with the default window size (50).
    pub fn new() -> Self {
        Self::with_window_size(DEFAULT_WINDOW_SIZE)
    }

    /// Construct with a custom window size (primarily for tests).
    pub fn with_window_size(window_size: usize) -> Self {
        Self {
            hot: HashMap::new(),
            counts: HashMap::new(),
            window_size: window_size.max(1),
        }
    }

    /// Apply an event.
    ///
    /// - `MessageAppended` increments the session's count and pushes the row
    ///   onto the window, evicting the oldest when the window exceeds
    ///   `window_size`. `session_id` is taken from the event.
    /// - `SessionCreated` / `SessionUpdated` are ignored (the session view
    ///   owns session metadata).
    pub fn apply(&mut self, event: &Event) {
        match event {
            Event::MessageAppended {
                session_id,
                message_id,
                role,
                content,
                tool_name,
                tool_calls,
                reasoning,
                timestamp,
                token_count,
                ..
            } => {
                // Count every message, independent of window eviction.
                *self.counts.entry(session_id.clone()).or_insert(0) += 1;

                let window = self.hot.entry(session_id.clone()).or_default();
                window.push_back(MessageRow {
                    message_id: *message_id,
                    role: role.clone(),
                    content: content.clone(),
                    tool_name: tool_name.clone(),
                    timestamp: *timestamp,
                    token_count: *token_count,
                    tool_calls: tool_calls.clone(),
                    reasoning: reasoning.clone(),
                });
                // Evict oldest beyond the window.
                while window.len() > self.window_size {
                    window.pop_front();
                }
            }
            Event::MessageRemoved {
                session_id,
                message_id,
                ..
            } => {
                if let Some(window) = self.hot.get_mut(session_id) {
                    if let Some(pos) = window.iter().position(|row| row.message_id == *message_id) {
                        window.remove(pos);
                        if let Some(count) = self.counts.get_mut(session_id) {
                            *count = count.saturating_sub(1);
                            if *count == 0 {
                                self.counts.remove(session_id);
                            }
                        }
                        if window.is_empty() {
                            self.hot.remove(session_id);
                        }
                    }
                }
            }
            // Session + card events don't affect the message cache.
            _ => {}
        }
    }

    /// Return up to `limit` most-recent messages for `session_id`, newest LAST
    /// (arrival / message_id order). If `limit` exceeds the window size, the
    /// whole window is returned (older messages are not resident — fetch them
    /// from the log). Returns an empty vec for an unknown session.
    pub fn recent(&self, session_id: &str, limit: usize) -> Vec<&MessageRow> {
        match self.hot.get(session_id) {
            Some(window) => {
                let take = limit.min(window.len());
                // The N most recent: skip the oldest (len - take), take `take`.
                let start = window.len().saturating_sub(take);
                window.iter().skip(start).take(take).collect()
            }
            None => Vec::new(),
        }
    }

    /// Total number of messages appended to `session_id` (durable, independent
    /// of window eviction). Zero for an unknown session.
    pub fn count(&self, session_id: &str) -> u64 {
        self.counts.get(session_id).copied().unwrap_or(0)
    }

    /// Size of the hot window kept in memory.
    pub fn window_size(&self) -> usize {
        self.window_size
    }
}

impl Default for MessageView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(session_id: &str, id: u64, ts: f64) -> Event {
        Event::MessageAppended {
            session_id: session_id.into(),
            hermes_session_id: format!("h-{session_id}"),
            message_id: id,
            role: "user".into(),
            content: Some(format!("m-{id}")),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: ts,
            token_count: None,
            finish_reason: None,
        }
    }

    #[test]
    fn custom_window_size_evicts_correctly() {
        let mut v = MessageView::with_window_size(3);
        for i in 0..5u64 {
            v.apply(&msg("s", i, i as f64));
        }
        assert_eq!(v.count("s"), 5);
        let recent = v.recent("s", 100);
        assert_eq!(recent.len(), 3, "window holds only 3");
        // newest three: ids 2,3,4
        let ids: Vec<u64> = recent.iter().map(|m| m.message_id).collect();
        assert_eq!(ids, vec![2, 3, 4]);
    }

    #[test]
    fn message_removed_updates_window_and_count() {
        let mut v = MessageView::new();
        v.apply(&msg("s", 1, 1.0));
        v.apply(&msg("s", 2, 2.0));

        v.apply(&Event::MessageRemoved {
            session_id: "s".into(),
            hermes_session_id: "h-s".into(),
            message_id: 1,
        });

        assert_eq!(v.count("s"), 1);
        let ids: Vec<u64> = v.recent("s", 10).iter().map(|m| m.message_id).collect();
        assert_eq!(ids, vec![2]);
    }
}
