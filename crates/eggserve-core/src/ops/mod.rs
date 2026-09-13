//! Compatibility re-export (Plan 215: authority moved to `eggserve-server`).
//!
//! The per-runtime observability vocabulary, context, and process-global
//! compatibility path are owned by [`eggserve_server::ops`]; this module
//! re-exports that implementation during the 0.x line so existing
//! `eggserve_core::ops::` paths keep working.

pub use eggserve_server::ops::*;
