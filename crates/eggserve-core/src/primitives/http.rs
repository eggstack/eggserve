//! Compatibility re-export (Plan 217: request validation moved to direct crates).
//!
//! The canonical validation vocabulary is owned by
//! [`eggserve_primitives::http`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::http::`
//! paths keep working. There is one validation authority, not two.

pub use eggserve_primitives::http::*;
