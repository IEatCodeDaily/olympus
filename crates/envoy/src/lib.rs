//! Olympus Envoy — the runtime-holder library (ADR 0008).
//!
//! Envoy-side code extracted from the monolith (milestone S2): agent
//! discovery, the ACP bridge + adapters, and the per-session runtime table.
//! In S2 the monolith (`olympus-control-plane`) still links this crate
//! in-process; the standalone `olympus-envoy` binary arrives in S3.

pub mod adapter;
pub mod bridge;
pub mod discovery;
pub mod job_table;
pub mod mock_runtime;
pub mod observer;
pub mod runtime_table;
pub mod spool;
pub mod transport;
