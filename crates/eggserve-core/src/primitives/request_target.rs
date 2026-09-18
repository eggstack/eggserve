//! Compatibility re-export (Plan 217: request target moved to direct crates).
//!
//! The canonical target classifier is owned by
//! [`eggserve_primitives::request_target`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::request_target::`
//! paths keep working. There is one target-form authority, not two.

pub use eggserve_primitives::request_target::*;
