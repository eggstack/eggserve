//! Python native bridge facade (Plan 206 Track B).
//!
//! Module layout: `errors` centralizes exception mapping; `body_bridge` and
//! `response_bridge` each own one channel state machine; `request_bridge`
//! owns `PyRequest`; `tunnel_bridge` owns tunnel duplex; `static_responder`
//! owns caller-owned composition (no routing, no event-loop mixing);
//! `sync_handler` owns the single Python-to-canonical conversion;
//! `runtime` owns `PyServer` lifecycle on the shared native runtime;
//! `lifecycle` documents the state machine; `async_handler` points to the
//! Python-side Plan 204 asyncio bridge (no duplicated Rust conversion).
//! Public PyO3 registration stays small/auditable here.

pub mod async_handler;
pub mod body_bridge;
pub mod errors;
pub mod lifecycle;
pub mod request_bridge;
pub mod response_bridge;
pub mod runtime;
pub mod static_responder;
pub mod sync_handler;
pub mod tunnel_bridge;

pub use body_bridge::{PyBodyChunkIterator, PyRequestBody};
pub use errors::ServerRequestError;
pub use request_bridge::PyRequest;
pub use response_bridge::PyResponse;
pub use runtime::PyServer;
pub use static_responder::{PyStaticPolicyWrapper, PyStaticResponder, ServerBodySource, ServerSecureRoot};
pub use tunnel_bridge::{PyTunnel, PyTunnelCapability, PyTunnelRequest};
