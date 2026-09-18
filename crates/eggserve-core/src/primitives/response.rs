//! Compatibility re-export (Plan 217: static response planning moved to direct crates).
//!
//! The canonical planning values are owned by
//! [`eggserve_primitives::response`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::response::`
//! paths keep working. There is one planning authority, not two.

pub use eggserve_primitives::response::*;
