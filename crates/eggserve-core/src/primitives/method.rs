//! Compatibility re-export (Plan 217: method moved to direct crates).
//!
//! The canonical method is owned by
//! [`eggserve_primitives::method`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::method::`
//! paths keep working. There is one validation authority, not two.

pub use eggserve_primitives::method::*;
