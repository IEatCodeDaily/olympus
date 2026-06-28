//! Olympus control plane — core library.
//!
//! Phase 1: append-only event log over redb (`event`, `log`, `compress`).

pub mod compress;
pub mod event;
pub mod log;
pub mod views;
