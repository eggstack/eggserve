//! Compatibility re-export (Plan 217: request context moved to direct crates).
//!
//! The canonical context is owned by
//! [`eggserve_primitives::request_context`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::request_context::`
//! paths keep working. There is one context authority, not two.
//!
//! Validated tunnel *intent* rides here as cloneable routing metadata
//! (`tunnel_request`); one-shot tunnel *acceptance* stays server-owned
//! (`eggserve_server::tunnel::TunnelCapability`) and reaches services via
//! `Service::call_with_tunnel`, never through this transport-neutral struct.
//!
//! The 0.1 one-shot slot (`with_tunnel`/`take_tunnel`/`tunnel_shared`/
//! `tunnel_sidecar`) moves to the server transport boundary. Compatibility
//! services must migrate to `call_with_tunnel` (or
//! `service_fn_with_tunnel`); `take_tunnel` is removed in the 0.2 line
//! (see `docs/migration-guide.md`).

pub use eggserve_primitives::request_context::*;
