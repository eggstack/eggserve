//! Compatibility re-export (Plan 217: connection info moved to direct crates).
//!
//! The canonical connection metadata is owned by
//! [`eggserve_primitives::connection_info`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::connection_info::`
//! paths keep working. Transport metadata is never fabricated; there is one
//! truthful authority, not two.

pub use eggserve_primitives::connection_info::*;
