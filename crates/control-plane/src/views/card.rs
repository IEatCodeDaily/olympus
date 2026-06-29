//! Card projection — an in-memory materialized view of cards on the board.
//!
//! A deterministic projection of the event log (ADR 0002 §2.4, §6). On restart
//! it is rebuilt by [`super::ViewManager::replay`]; live events are applied via
//! [`CardView::apply`]. The log remains the sole source of truth.
//!
//! A card's status is derived from the event stream:
//! - `CardCreated` → status "todo"
//! - `CardAssigned` → status "assigned" (an agent has been given it)
//! - `CardClaimed` → status "claimed" (the agent began work)
//! - `CardBlocked` → status "blocked"
//! - `CardCompleted` → status "done"
//! - `CardReassigned` → status "assigned" (back to the new agent's queue)

use std::collections::HashMap;

use crate::event::Event;

/// A single attempt (session) on a card. ADR §6.2: reassignment forwards the
/// prior session as a "previous attempt" block to the new session.
#[derive(Debug, Clone)]
pub struct CardAttempt {
    pub session_id: String,
    pub assigned_id: String,
    pub bookmark: String,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    pub outcome: String,
}

/// A row in the card projection.
#[derive(Debug, Clone)]
pub struct CardRow {
    pub card_id: String,
    pub board_id: String,
    pub title: String,
    pub status: String,
    pub assigned_id: Option<String>,
    pub assigned_kind: Option<String>,
    pub current_session_id: Option<String>,
    pub current_bookmark: Option<String>,
    pub blocked_by: Vec<String>,
    pub priority: i64,
    pub attempts: Vec<CardAttempt>,
    pub created_at: f64,
    pub status_changed_at: f64,
}

/// Filters applied to [`CardView::list`].
#[derive(Debug, Clone, Default)]
pub struct CardFilters {
    pub board_id: Option<String>,
    pub status: Option<String>,
}

/// In-memory projection of cards from the event log (ADR §2.4, §6).
pub struct CardView {
    cards: HashMap<String, CardRow>,
}

impl CardView {
    pub fn new() -> Self {
        Self {
            cards: HashMap::new(),
        }
    }

    /// Apply an event, mutating the projection. Card events mutate card state;
    /// all other events are ignored.
    pub fn apply(&mut self, event: &Event) {
        match event {
            Event::CardCreated {
                card_id,
                board_id,
                title,
                created_at,
            } => {
                self.cards.insert(
                    card_id.clone(),
                    CardRow {
                        card_id: card_id.clone(),
                        board_id: board_id.clone(),
                        title: title.clone(),
                        status: "todo".into(),
                        assigned_id: None,
                        assigned_kind: None,
                        current_session_id: None,
                        current_bookmark: None,
                        blocked_by: Vec::new(),
                        priority: 0,
                        attempts: Vec::new(),
                        created_at: *created_at,
                        status_changed_at: *created_at,
                    },
                );
            }
            Event::CardAssigned {
                card_id,
                assigned_id,
                assigned_kind,
                session_id,
                attempt_bookmark,
                assigned_at,
            } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.assigned_id = Some(assigned_id.clone());
                    card.assigned_kind = Some(assigned_kind.clone());
                    card.current_session_id = Some(session_id.clone());
                    card.current_bookmark = Some(attempt_bookmark.clone());
                    card.status = "assigned".into();
                    card.status_changed_at = *assigned_at;
                    card.attempts.push(CardAttempt {
                        session_id: session_id.clone(),
                        assigned_id: assigned_id.clone(),
                        bookmark: attempt_bookmark.clone(),
                        started_at: *assigned_at,
                        ended_at: None,
                        outcome: "running".into(),
                    });
                }
            }
            Event::CardClaimed {
                card_id,
                claimed_at,
            } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.status = "claimed".into();
                    card.status_changed_at = *claimed_at;
                }
            }
            Event::CardBlocked {
                card_id,
                blocked_by,
                blocked_at,
            } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.blocked_by = blocked_by.clone();
                    card.status = "blocked".into();
                    card.status_changed_at = *blocked_at;
                }
            }
            Event::CardCompleted {
                card_id,
                completed_at,
            } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.status = "done".into();
                    card.status_changed_at = *completed_at;
                    // End the current attempt.
                    if let Some(attempt) = card.attempts.last_mut() {
                        attempt.ended_at = Some(*completed_at);
                        attempt.outcome = "done".into();
                    }
                }
            }
            Event::CardReassigned {
                card_id,
                assigned_id,
                assigned_kind,
                session_id,
                attempt_bookmark,
                previous_session_id,
                reassigned_at,
            } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    // Close out the previous attempt.
                    for attempt in card.attempts.iter_mut() {
                        if attempt.session_id == *previous_session_id && attempt.ended_at.is_none()
                        {
                            attempt.ended_at = Some(*reassigned_at);
                            attempt.outcome = "reassigned".into();
                        }
                    }
                    card.assigned_id = Some(assigned_id.clone());
                    card.assigned_kind = Some(assigned_kind.clone());
                    card.current_session_id = Some(session_id.clone());
                    card.current_bookmark = Some(attempt_bookmark.clone());
                    card.status = "assigned".into();
                    card.status_changed_at = *reassigned_at;
                    card.attempts.push(CardAttempt {
                        session_id: session_id.clone(),
                        assigned_id: assigned_id.clone(),
                        bookmark: attempt_bookmark.clone(),
                        started_at: *reassigned_at,
                        ended_at: None,
                        outcome: "running".into(),
                    });
                }
            }
            _ => {}
        }
    }

    /// Return rows matching `filters`, ordered by `created_at` descending
    /// (most recent first), with card_id as a stable tiebreaker.
    pub fn list(&self, filters: &CardFilters) -> Vec<&CardRow> {
        let mut rows: Vec<&CardRow> = self
            .cards
            .values()
            .filter(|row| match (&filters.board_id, &filters.status) {
                (Some(b), _) if row.board_id != *b => false,
                (_, Some(s)) if row.status != *s => false,
                _ => true,
            })
            .collect();
        rows.sort_by(|a, b| {
            b.created_at
                .partial_cmp(&a.created_at)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.card_id.cmp(&b.card_id))
        });
        rows
    }

    /// Look up a single card by id.
    pub fn get(&self, card_id: &str) -> Option<&CardRow> {
        self.cards.get(card_id)
    }
}

impl Default for CardView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card_created(id: &str, board: &str, title: &str, ts: f64) -> Event {
        Event::CardCreated {
            card_id: id.into(),
            board_id: board.into(),
            title: title.into(),
            created_at: ts,
        }
    }

    fn card_assigned(card_id: &str, agent: &str, sess: &str, bm: &str, ts: f64) -> Event {
        Event::CardAssigned {
            card_id: card_id.into(),
            assigned_id: agent.into(),
            assigned_kind: "agent".into(),
            session_id: sess.into(),
            attempt_bookmark: bm.into(),
            assigned_at: ts,
        }
    }

    // ---- REQUIRED BEHAVIOR 1: CardCreated creates a "todo" card ----

    #[test]
    fn card_created_adds_todo_row() {
        let mut v = CardView::new();
        v.apply(&card_created("c1", "b1", "Do stuff", 100.0));

        let row = v.get("c1").expect("c1 must exist");
        assert_eq!(row.card_id, "c1");
        assert_eq!(row.board_id, "b1");
        assert_eq!(row.title, "Do stuff");
        assert_eq!(row.status, "todo");
        assert!(row.assigned_id.is_none());
        assert!(row.current_session_id.is_none());
        assert_eq!(row.created_at, 100.0);
        assert!(row.attempts.is_empty());
    }

    #[test]
    fn get_unknown_card_returns_none() {
        let v = CardView::new();
        assert!(v.get("nope").is_none());
    }

    // ---- REQUIRED BEHAVIOR 2: CardAssigned transitions to "assigned" + records attempt ----

    #[test]
    fn card_assigned_transitions_to_assigned_and_records_attempt() {
        let mut v = CardView::new();
        v.apply(&card_created("c1", "b1", "T", 100.0));
        v.apply(&card_assigned("c1", "zephyr", "sess-1", "attempt-1", 101.0));

        let row = v.get("c1").unwrap();
        assert_eq!(row.status, "assigned");
        assert_eq!(row.assigned_id.as_deref(), Some("zephyr"));
        assert_eq!(row.assigned_kind.as_deref(), Some("agent"));
        assert_eq!(row.current_session_id.as_deref(), Some("sess-1"));
        assert_eq!(row.current_bookmark.as_deref(), Some("attempt-1"));
        assert_eq!(row.attempts.len(), 1);
        assert_eq!(row.attempts[0].session_id, "sess-1");
        assert_eq!(row.attempts[0].assigned_id, "zephyr");
        assert_eq!(row.attempts[0].outcome, "running");
    }

    #[test]
    fn card_assigned_for_unknown_card_is_noop() {
        let mut v = CardView::new();
        v.apply(&card_assigned("ghost", "z", "s", "bm", 1.0));
        assert!(v.get("ghost").is_none());
    }

    // ---- REQUIRED BEHAVIOR 3: CardClaimed transitions to "claimed" ----

    #[test]
    fn card_claimed_transitions_to_claimed() {
        let mut v = CardView::new();
        v.apply(&card_created("c1", "b1", "T", 100.0));
        v.apply(&card_assigned("c1", "z", "s1", "bm1", 101.0));
        v.apply(&Event::CardClaimed {
            card_id: "c1".into(),
            claimed_at: 102.0,
        });

        let row = v.get("c1").unwrap();
        assert_eq!(row.status, "claimed");
        assert_eq!(row.status_changed_at, 102.0);
    }

    // ---- REQUIRED BEHAVIOR 4: CardBlocked transitions to "blocked" with deps ----

    #[test]
    fn card_blocked_sets_blocked_by() {
        let mut v = CardView::new();
        v.apply(&card_created("c1", "b1", "T", 100.0));
        v.apply(&Event::CardBlocked {
            card_id: "c1".into(),
            blocked_by: vec!["c0".into(), "c2".into()],
            blocked_at: 103.0,
        });

        let row = v.get("c1").unwrap();
        assert_eq!(row.status, "blocked");
        assert_eq!(row.blocked_by, vec!["c0", "c2"]);
    }

    // ---- REQUIRED BEHAVIOR 5: CardCompleted transitions to "done" + closes attempt ----

    #[test]
    fn card_completed_transitions_to_done_and_closes_attempt() {
        let mut v = CardView::new();
        v.apply(&card_created("c1", "b1", "T", 100.0));
        v.apply(&card_assigned("c1", "z", "s1", "bm1", 101.0));
        v.apply(&Event::CardClaimed {
            card_id: "c1".into(),
            claimed_at: 102.0,
        });
        v.apply(&Event::CardCompleted {
            card_id: "c1".into(),
            completed_at: 105.0,
        });

        let row = v.get("c1").unwrap();
        assert_eq!(row.status, "done");
        assert_eq!(row.attempts.len(), 1);
        assert_eq!(row.attempts[0].ended_at, Some(105.0));
        assert_eq!(row.attempts[0].outcome, "done");
    }

    // ---- REQUIRED BEHAVIOR 6: CardReassigned closes old attempt + opens new ----

    #[test]
    fn card_reassigned_closes_prior_and_opens_new_attempt() {
        let mut v = CardView::new();
        v.apply(&card_created("c1", "b1", "T", 100.0));
        v.apply(&card_assigned("c1", "zephyr", "sess-1", "bm-1", 101.0));
        v.apply(&Event::CardReassigned {
            card_id: "c1".into(),
            assigned_id: "talos".into(),
            assigned_kind: "agent".into(),
            session_id: "sess-2".into(),
            attempt_bookmark: "bm-2".into(),
            previous_session_id: "sess-1".into(),
            reassigned_at: 110.0,
        });

        let row = v.get("c1").unwrap();
        assert_eq!(row.status, "assigned");
        assert_eq!(row.assigned_id.as_deref(), Some("talos"));
        assert_eq!(row.current_session_id.as_deref(), Some("sess-2"));
        assert_eq!(row.current_bookmark.as_deref(), Some("bm-2"));
        assert_eq!(row.attempts.len(), 2);

        // Prior attempt closed
        assert_eq!(row.attempts[0].session_id, "sess-1");
        assert_eq!(row.attempts[0].ended_at, Some(110.0));
        assert_eq!(row.attempts[0].outcome, "reassigned");

        // New attempt running
        assert_eq!(row.attempts[1].session_id, "sess-2");
        assert_eq!(row.attempts[1].assigned_id, "talos");
        assert!(row.attempts[1].ended_at.is_none());
        assert_eq!(row.attempts[1].outcome, "running");
    }

    // ---- REQUIRED BEHAVIOR 7: list filters by board and status ----

    #[test]
    fn list_filters_by_board() {
        let mut v = CardView::new();
        v.apply(&card_created("c1", "b1", "A", 1.0));
        v.apply(&card_created("c2", "b2", "B", 2.0));
        v.apply(&card_created("c3", "b1", "C", 3.0));

        let b1 = v.list(&CardFilters {
            board_id: Some("b1".into()),
            status: None,
        });
        assert_eq!(b1.len(), 2);
        let ids: Vec<&str> = b1.iter().map(|r| r.card_id.as_str()).collect();
        assert!(ids.contains(&"c1"));
        assert!(ids.contains(&"c3"));
    }

    #[test]
    fn list_filters_by_status() {
        let mut v = CardView::new();
        v.apply(&card_created("c1", "b1", "A", 1.0));
        v.apply(&card_created("c2", "b1", "B", 2.0));
        v.apply(&card_assigned("c2", "z", "s1", "bm1", 3.0));

        let todo = v.list(&CardFilters {
            board_id: Some("b1".into()),
            status: Some("todo".into()),
        });
        assert_eq!(todo.len(), 1);
        assert_eq!(todo[0].card_id, "c1");

        let assigned = v.list(&CardFilters {
            board_id: Some("b1".into()),
            status: Some("assigned".into()),
        });
        assert_eq!(assigned.len(), 1);
        assert_eq!(assigned[0].card_id, "c2");
    }

    #[test]
    fn list_orders_by_created_at_desc() {
        let mut v = CardView::new();
        v.apply(&card_created("old", "b1", "A", 1.0));
        v.apply(&card_created("newest", "b1", "B", 3.0));
        v.apply(&card_created("mid", "b1", "C", 2.0));

        let rows = v.list(&CardFilters::default());
        let ids: Vec<&str> = rows.iter().map(|r| r.card_id.as_str()).collect();
        assert_eq!(ids, vec!["newest", "mid", "old"]);
    }

    #[test]
    fn list_default_returns_all() {
        let mut v = CardView::new();
        v.apply(&card_created("c1", "b1", "A", 1.0));
        v.apply(&card_created("c2", "b2", "B", 2.0));
        assert_eq!(v.list(&CardFilters::default()).len(), 2);
    }
}
