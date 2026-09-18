//! Compatibility re-export (Plan 217: lifecycle moved to direct crates).
//!
//! The canonical lifecycle is owned by
//! [`eggserve_primitives::request_lifecycle`]; this module re-exports that
//! implementation so existing
//! `eggserve_core::primitives::request_lifecycle::` paths keep working.
//! There is one lifecycle/cancellation authority, not two. The direct crate
//! uses an executor-neutral notifier so the canonical type stays
//! Hyper/Tokio-free.

pub use eggserve_primitives::request_lifecycle::*;
