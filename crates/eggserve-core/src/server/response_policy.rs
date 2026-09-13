//! Compatibility re-export (Plan 215: authority moved to `eggserve-server`).
//!
//! [`ResponsePolicy`], [`DatePolicy`], and the denylist validators are owned
//! by [`eggserve_server::response_policy`]; this module re-exports that
//! implementation during the 0.x line so existing
//! `eggserve_core::server::response_policy::` paths keep working.

pub use eggserve_server::response_policy::*;
