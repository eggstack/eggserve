//! Compatibility re-exports for the server-owned HTTP interop adapter.

//! This module retains the historical `eggserve_core::primitives::interop`
//! path. Enable core's `http-interop` feature to expose the server authority.

pub use eggserve_server::interop::*;
