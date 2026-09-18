//! EggServe's compatibility facade for the neutral [`eggnet_tls`] crate.
//!
//! Identity parsing, SNI selection, client authentication, trust/CRL bounds,
//! and reload snapshots live in `eggnet-tls`. This module preserves the
//! historical `eggserve_core::tls` import path. HTTP/3-specific QUIC config
//! assembly lives in `eggserve-h3` (Plan 220); this facade delegates there.

pub use eggnet_tls::*;

#[cfg(feature = "http3")]
use std::path::Path;

#[cfg(feature = "http3")]
use eggserve_h3::quinn;

/// Build the EggServe-specific QUIC server configuration.
///
/// Delegates to the H3-owned assembly (`eggserve-h3::load_quic_server_config`);
/// no second QUIC/TLS implementation lives here.
#[cfg(feature = "http3")]
pub(crate) fn load_quic_server_config(
    cert_path: &Path,
    key_path: &Path,
    config: &crate::server::Http3Config,
) -> Result<quinn::ServerConfig, TlsError> {
    eggserve_h3::load_quic_server_config(cert_path, key_path, config)
}
