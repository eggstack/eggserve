//! Compatibility re-export (Plan 217: body policy moved to direct crates).
//!
//! The canonical policy is owned by
//! [`eggserve_primitives::request_body_policy`]; this module re-exports that
//! implementation so existing
//! `eggserve_core::primitives::request_body_policy::` paths keep working.
//! There is one policy authority, not two.

pub use eggserve_primitives::request_body_policy::*;
