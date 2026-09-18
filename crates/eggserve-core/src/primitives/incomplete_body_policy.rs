//! Compatibility re-export (Plan 217: incomplete-body policy moved to direct crates).
//!
//! The canonical policy is owned by
//! [`eggserve_primitives::incomplete_body_policy`]; this module re-exports
//! that implementation so existing
//! `eggserve_core::primitives::incomplete_body_policy::` paths keep working.

pub use eggserve_primitives::incomplete_body_policy::*;
