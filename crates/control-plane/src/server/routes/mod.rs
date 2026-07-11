//! Per-resource axum route modules split out of `server::mod` (ARCH-B).
//!
//! Each resource module exposes `pub fn router() -> Router<AppState>`; the
//! state-generic routers are merged in `build_router`, which applies the auth
//! `route_layer` and the single terminal `.with_state(state)` (the existing
//! axum 0.8 state pattern).

pub(crate) mod agents;
pub(crate) mod cards;
pub(crate) mod enroll;
pub(crate) mod events;
pub(crate) mod irc;
pub(crate) mod nodes;
pub(crate) mod organizations;
pub(crate) mod projects;
pub(crate) mod registry;
pub(crate) mod repos;
pub(crate) mod search;
pub(crate) mod sessions;
pub(crate) mod setup;
pub(crate) mod support;
pub(crate) mod vaults;
