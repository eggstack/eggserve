//! Compatibility re-export (Plan 217: service contract moved to direct crates).
//!
//! The canonical application service contract is owned by
//! [`eggserve_server::service`]; this module re-exports that implementation
//! so existing `eggserve_core::server::service::` and
//! `eggserve_core::server::Service` paths keep working. There is one service
//! contract, one error taxonomy, and one tunnel-acceptance entry point
//! (`Service::call_with_tunnel`), not two.
//!
//! Ordinary services keep implementing `Service::call`; the default
//! `call_with_tunnel` drops the one-shot capability and runs `call`, so
//! denial stays ordinary HTTP. Tunnel-aware services implement
//! `call_with_tunnel` (or use `service_fn_with_tunnel`) to receive the
//! server-owned `TunnelCapability` alongside the canonical request.
//!
//! The 0.1 Hyper error-response helpers (`ServiceError::to_response*`)
//! move to the connection pipeline (`super::connection::response::
//! service_error_to_response`) so the canonical error type stays
//! transport-neutral. `ServiceError::status_code` (direct) is the
//! transport-neutral status authority.

pub use eggserve_server::service::*;
