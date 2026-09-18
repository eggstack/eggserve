//! Experimental HTTP/3 and QUIC transport adapter for EggServe (Plan 220).
//!
//! This package owns the coordinated Quinn/H3/H3-Quinn version set **and**
//! the actual H3/QUIC transport adapter: endpoint lifecycle, request
//! conversion, response streaming/trailers, Extended CONNECT tunnel bridging,
//! shutdown/drain, close classification, and H3-only transport configuration.
//!
//! Canonical service semantics stay in `eggserve-primitives` /
//! `eggserve-server`. This crate depends downward on those layers
//! (`primitives <- server <- h3`); they never depend upward. The
//! compatibility core consumes this adapter behind its optional `http3`
//! feature, so default builds contain no H3/QUIC dependencies.
//!
//! The adapter entry points are expressed in canonical types (`Service`,
//! server `RuntimeConfig`, `OpsContext`, semaphores, `ShutdownResult`)
//! plus the H3-owned [`Http3Config`]. Quinn/H3 transport types stay
//! crate-internal or doc-hidden and are not stable EggServe application APIs.
//! H3 remains experimental with Plan 192-195 blockers intact.

pub mod adapter;
pub mod config;
pub mod endpoint;
pub mod quic;
pub mod request;
pub mod response;
pub mod tunnel;

pub use adapter::{accept_loop, apply_alt_svc};
pub use config::Http3Config;
#[doc(hidden)]
pub use quic::{
    endpoint_from_socket, load_quic_server_config, server_endpoint, validate_same_port_udp,
};

#[doc(hidden)]
pub use h3;
#[doc(hidden)]
pub use h3_quinn;
#[doc(hidden)]
pub use quinn;

/// Versions of the coordinated H3/QUIC compatibility set used by this crate.
pub const H3_VERSION: &str = "0.0.8";
pub const H3_QUINN_VERSION: &str = "0.0.10";
pub const QUINN_VERSION: &str = "0.11.11";

/// Returns the dependency set used for qualification and diagnostics.
pub const fn dependency_versions() -> (&'static str, &'static str, &'static str) {
    (H3_VERSION, H3_QUINN_VERSION, QUINN_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinated_dependency_versions_are_explicit() {
        assert_eq!(dependency_versions(), ("0.0.8", "0.0.10", "0.11.11"));
    }

    #[test]
    fn h3_config_authority_lives_here() {
        let cfg = Http3Config::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_concurrent_bidi_streams, 100);
        assert!(cfg.validate().is_ok());
    }
}
