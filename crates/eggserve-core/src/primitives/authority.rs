//! Compatibility re-export (Plan 216: authority moved to direct crates).
//!
//! The canonical authority is owned by
//! [`eggserve_primitives::authority`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::authority::`
//! paths keep working. Tunnel intent carries this shared type, so no
//! authority conversion crosses the direct/compatibility boundary.

pub use eggserve_primitives::authority::*;
