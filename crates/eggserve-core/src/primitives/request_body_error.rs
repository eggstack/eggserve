//! Compatibility re-export (Plan 217: body errors moved to direct crates).
//!
//! The canonical error taxonomy is owned by
//! [`eggserve_primitives::request_body_error`]; this module re-exports that
//! implementation so existing
//! `eggserve_core::primitives::request_body_error::` paths keep working.
//! There is one body-error authority, not two.

pub use eggserve_primitives::request_body_error::*;
