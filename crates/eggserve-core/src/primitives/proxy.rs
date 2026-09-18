//! Compatibility re-export (Plan 217: proxy metadata moved to direct crates).
//!
//! The canonical proxy vocabulary is owned by
//! [`eggserve_primitives::proxy`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::proxy::`
//! paths keep working. There is one PROXY/forwarded authority, not two.

pub use eggserve_primitives::proxy::*;
