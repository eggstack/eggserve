//! Compatibility re-export (Plan 215: authority moved to `eggserve-server`).
//!
//! [`ServerError`] and [`ShutdownResult`] are owned by
//! [`eggserve_server::errors`]; this module re-exports that implementation
//! during the 0.x line so existing `eggserve_core::server::errors::` paths
//! keep working.

pub use eggserve_server::errors::*;
