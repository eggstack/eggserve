//! Compatibility re-export (Plan 217: interim capability moved to direct crates).
//!
//! The canonical interim vocabulary is owned by
//! [`eggserve_primitives::interim`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::interim::`
//! paths keep working. There is one interim state machine, not two.

pub use eggserve_primitives::interim::*;
