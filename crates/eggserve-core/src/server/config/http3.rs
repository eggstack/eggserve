//! HTTP/3/QUIC transport configuration facade (Plan 220).
//!
//! Implementation authority lives in `eggserve-h3::Http3Config`; this module
//! preserves the `eggserve_core::server::Http3Config` import path with no
//! second resolver, defaults table, or validator.

#[cfg(feature = "http3")]
pub use eggserve_h3::Http3Config;
