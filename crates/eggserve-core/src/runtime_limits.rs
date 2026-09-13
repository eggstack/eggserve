//! Compatibility re-export (Plan 215: authority moved to `eggserve-server`).
//!
//! Shared runtime defaults, bounds, [`SharedRuntimeValues`], and
//! [`Violation`] are owned by [`eggserve_server::runtime_limits`]; this
//! module re-exports that implementation during the 0.x line. Adapters from
//! compatibility config types live next to those types (`From` impls in
//! [`crate::limits`] and [`crate::server::config`]) so no constant or
//! constraint is duplicated.

pub use eggserve_server::runtime_limits::*;
