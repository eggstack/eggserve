//! Canonical, transport-independent EggServe application values.
//!
//! This crate is the single home of the mature request, response, header,
//! lifecycle, proxy, and policy contracts. It deliberately has no transport,
//! async-runtime, TLS, QUIC, or platform dependencies. Hyper conversion and
//! filesystem resolution live in the server and static layers respectively.

pub mod primitives;

pub use primitives::*;
