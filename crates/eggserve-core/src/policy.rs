//! Compatibility re-export (Plan 215: authority moved to `eggserve-primitives`).
//!
//! Security policy types are owned by
//! [`eggserve_primitives::policy`]; this module re-exports that
//! implementation so existing `eggserve_core::policy::` paths keep working.
//! The types are semantically identical (safe defaults deny all optional
//! behaviors); unification removes the nominal duplicate.

pub use eggserve_primitives::policy::*;
