//! Plan 221: neutral TLS loading is owned by `eggnet-tls`.
//!
//! The compatibility `eggserve_core::tls` facade re-exports this authority
//! plus H3-only QUIC assembly; the binary names the neutral substrate
//! directly. H3 QUIC assembly stays in `eggserve-h3` via the compatibility
//! server orchestration until Plan 225.
pub use eggnet_tls::*;
