//! TLS configuration ownership (Plan 206 Track E).
//!
//! TLS fields (`tls_config`, `tls_reload_handle`, `tls_expose_peer_chain`)
//! live on [`super::runtime::RuntimeConfig`] with `tls`-gated types from
//! `crate::tls`. This module is the protocol owner for future TLS-specific
//! defaults/validation; shared constraints stay in `crate::runtime_limits`.
//! No new knobs are added by the split; builder setters stay on
//! [`super::RuntimeConfigBuilder`] in the parent facade.

//! Plan 203 identity/reload semantics are unchanged.
