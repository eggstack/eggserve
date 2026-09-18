//! Compatibility re-export (Plans 199, 216, 217).
//!
//! Plan 199 semantics, Plan 216 ownership, Plan 217 identity: transport-neutral
//! intent vocabulary lives in `eggserve-primitives`, transport execution
//! (including the one-shot `TunnelCapability`) lives in `eggserve-server`.
//! This module re-exports both authorities so existing
//! `eggserve_core::primitives::tunnel::` paths keep working. There is one
//! intent validator, one acceptance state machine, one handshake builder,
//! and one bridge (`run_tunnel`) — no second implementation here.
//!
//! - Intent/validation: [`TunnelKind`], [`ProtocolName`], [`TunnelRequest`],
//!   [`TunnelError`], bounds, `classify_h1_upgrade`,
//!   `classify_extended_protocol`, `validate_handshake_headers` (all owned
//!   by `eggserve-primitives`).
//! - Execution: [`TunnelCapability`], [`TunnelIo`], [`TunnelShared`],
//!   [`TunnelAcceptance`] (all owned by `eggserve-server`).
//!
//! Services receive the capability via `Service::call_with_tunnel`
//! (or `service_fn_with_tunnel`); dropping/ignoring denies with ordinary
//! HTTP. The 0.1 `RequestContext::take_tunnel` slot is removed in the 0.2
//! line (see `docs/migration-guide.md`); intent remains inspectable via
//! `RequestContext::tunnel_request`.
//!
//! H3 stream bridging stays in `server::http3` under the Plan 213 boundary;
//! it stages through the same capability and sidecar, then bridges its own
//! streams.

pub use eggserve_primitives::tunnel::{
    classify_extended_protocol, classify_h1_upgrade, validate_handshake_headers, ProtocolName,
    TunnelError, TunnelKind, TunnelRequest, MAX_TUNNEL_HEADER_BYTES, MAX_TUNNEL_HEADER_COUNT,
    MAX_TUNNEL_PROTOCOL_BYTES, TUNNEL_IO_BUFFER_BYTES,
};
pub use eggserve_server::tunnel::{TunnelAcceptance, TunnelCapability, TunnelIo, TunnelShared};
