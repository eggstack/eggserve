//! Compatibility re-export (Plan 217: request envelope moved to direct crates).
//!
//! The canonical envelope is owned by
//! [`eggserve_primitives::request`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::request::`
//! paths keep working. There is one request envelope, not two.
//!
//! Validated tunnel *intent* rides the context as cloneable routing metadata
//! (`RequestContext::tunnel_request`); one-shot tunnel *acceptance* stays
//! server-owned and reaches services via
//! `eggserve-server::Service::call_with_tunnel`, never as a top-level field.

pub use eggserve_primitives::request::*;
