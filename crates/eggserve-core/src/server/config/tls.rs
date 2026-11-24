//! TLS configuration ownership (Plan 206 Track E).
//!
//! TLS fields (`tls_config`, `tls_reload_handle`, `tls_expose_peer_chain`)
//! live on [`super::runtime::RuntimeConfig`] with `tls`-gated types from
//! `crate::tls`. This module is the protocol owner for future TLS-specific
//! defaults/validation; shared constraints stay in `crate::runtime_limits`.
//! No new knobs are added by the split; builder setters stay on
//! [`super::RuntimeConfigBuilder`] in the parent facade. ALPN coherence
//! between the effective TLS configuration and the HTTP/2 capability is
//! enforced by the parent runtime validator (`runtime::RuntimeConfig::validate`).

//! Plan 203 identity/reload semantics are unchanged.
