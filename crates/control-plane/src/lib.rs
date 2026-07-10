//! Olympus control plane — core library.
//!
//! Phase 1: append-only event log over redb (`event`, `log`, `compress`).
//! Phase 2: in-memory views (`views`).
//! Phase 6: tantivy full-text search (`search`).

pub mod auth;
pub mod compress;
pub mod edit_model;
pub mod event;
pub mod import;
pub mod irc;
pub mod legacy_log;
pub mod log;
pub mod node;
pub mod projects;
pub mod proxy;
pub mod repos;
pub mod search;
pub mod server;
pub mod state_db_reader;
pub mod sync;
pub mod vault;
pub mod views;

// The envoy-side modules (ACP bridge + setup adapters) moved to
// `olympus-envoy` (ADR 0008 milestone S2). Re-exported here so existing
// `crate::bridge::…` / `crate::adapter::…` call sites keep working unchanged
// while the monolith still links the envoy lib in-process.
pub use olympus_envoy::adapter;
pub use olympus_envoy::bridge;
