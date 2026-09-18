//! Compatibility re-export (Plan 217: body source moved to direct crates).
//!
//! The canonical body source is owned by
//! [`eggserve_primitives::body`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::body::`
//! paths keep working. There is one body-source authority, not two.

pub use eggserve_primitives::body::*;
