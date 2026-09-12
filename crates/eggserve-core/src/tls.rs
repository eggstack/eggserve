//! EggServe's compatibility facade for the neutral [`eggnet_tls`] crate.
//!
//! Identity parsing, SNI selection, client authentication, trust/CRL bounds,
//! and reload snapshots live in `eggnet-tls`. This module preserves the
//! historical `eggserve_core::tls` import path. HTTP/3-specific QUIC config
//! assembly remains here because it depends on EggServe's `Http3Config`.

pub use eggnet_tls::*;

#[cfg(feature = "http3")]
use std::path::Path;
#[cfg(feature = "http3")]
use std::sync::Arc;

/// Build the EggServe-specific QUIC server configuration from the neutral
/// crate's validated certificate/key loader.
#[cfg(feature = "http3")]
pub(crate) fn load_quic_server_config(
    cert_path: &Path,
    key_path: &Path,
    config: &crate::server::Http3Config,
) -> Result<quinn::ServerConfig, TlsError> {
    let (certs, key) = eggnet_tls::load_identity(cert_path, key_path)?;
    let mut tls = rustls::ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| TlsError::InvalidKey(e.to_string()))?;
    // HTTP/3 has its own ALPN and QUIC must not accept application 0-RTT in
    // this initial implementation because generic services may be replayable.
    tls.alpn_protocols = vec![b"h3".to_vec()];
    tls.max_early_data_size = 0;
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(Arc::new(tls))
        .map_err(|e| TlsError::InvalidKey(format!("invalid QUIC TLS config: {e}")))?;

    let mut transport = quinn::TransportConfig::default();
    transport
        .max_concurrent_bidi_streams(quinn::VarInt::from_u32(config.max_concurrent_bidi_streams))
        .max_concurrent_uni_streams(quinn::VarInt::from_u32(config.max_concurrent_uni_streams))
        .stream_receive_window(
            quinn::VarInt::try_from(config.stream_receive_window)
                .map_err(|_| TlsError::InvalidKey("stream receive window too large".into()))?,
        )
        .receive_window(
            quinn::VarInt::try_from(config.connection_receive_window)
                .map_err(|_| TlsError::InvalidKey("connection receive window too large".into()))?,
        )
        .send_window(config.send_window)
        .max_idle_timeout(Some(
            quinn::IdleTimeout::try_from(config.max_idle_timeout)
                .map_err(|_| TlsError::InvalidKey("idle timeout too large".into()))?,
        ));

    let mut server = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    server
        .transport_config(Arc::new(transport))
        .migration(false)
        .max_incoming(config.max_pending_handshakes);
    Ok(server)
}
