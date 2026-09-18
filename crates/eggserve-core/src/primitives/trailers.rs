//! Compatibility re-export (Plan 217: trailers moved to direct crates).
//!
//! The canonical trailer vocabulary is owned by
//! [`eggserve_primitives::trailers`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::trailers::`
//! paths keep working. There is one trailer validator, not two.

pub use eggserve_primitives::trailers::*;
