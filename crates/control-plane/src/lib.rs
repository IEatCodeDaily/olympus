//! Olympus control plane — core library.
//!
//! Phase 1: append-only event log over redb (`event`, `log`, `compress`).
//! Phase 2: in-memory views (`views`).
//! Phase 6: tantivy full-text search (`search`).

pub mod adapter;
pub mod auth;
pub mod bridge;
pub mod compress;
pub mod event;
pub mod import;
pub mod log;
pub mod search;
pub mod server;
pub mod sync;
pub mod views;
