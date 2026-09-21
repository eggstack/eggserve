//! Compatibility facade for the direct connection vocabulary.
//!
//! The context, shutdown token, and outcome are transport-neutral and are
//! owned by `eggserve-server`; H2/TLS/proxy composition consumes the same
//! types without maintaining a second vocabulary.

pub use eggserve_server::connection::{ConnectionContext, ConnectionOutcome, ConnectionShutdown};
