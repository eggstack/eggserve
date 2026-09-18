//! QUIC TLS and endpoint assembly (Plan 220 H3 authority).
//!
//! Owns EggServe-specific QUIC server configuration from the neutral
//! `eggnet-tls` identity loader plus H3 endpoint construction. QUIC uses
//! TLS 1.3 with `h3` ALPN, no application 0-RTT, bounded transport windows,
//! no migration, and bounded pending handshakes. Reusable PEM parsing stays
//! in `eggnet-tls`; transport policy stays here.

use std::path::Path;
use std::sync::Arc;

use crate::config::Http3Config;
use crate::{h3_quinn, quinn};

/// Build the EggServe-specific QUIC server configuration.
///
/// Uses `eggnet-tls::load_identity` for validated certificate/key loading,
/// then applies H3-owned transport policy (TLS 1.3 only, `h3` ALPN,
/// zero 0-RTT, explicit windows, no migration).
pub fn load_quic_server_config(
    cert_path: &Path,
    key_path: &Path,
    config: &Http3Config,
) -> Result<quinn::ServerConfig, eggnet_tls::TlsError> {
    use eggnet_tls::TlsError;

    let (certs, key) = eggnet_tls::load_identity(cert_path, key_path)?;
    let mut tls = rustls::ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| TlsError::InvalidKey(e.to_string()))?;
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

/// Build an H3/QUIC endpoint bound to `bind_addr`.
///
/// Wraps `h3_quinn::Endpoint::server` without exposing Quinn types in the
/// compatibility facade beyond this H3-owned helper.
#[doc(hidden)]
pub fn server_endpoint(
    quic_config: quinn::ServerConfig,
    bind_addr: std::net::SocketAddr,
) -> Result<h3_quinn::Endpoint, std::io::Error> {
    h3_quinn::Endpoint::server(quic_config, bind_addr).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            format!("H3 UDP bind failed: {e}"),
        )
    })
}

/// Build an H3/QUIC endpoint from a caller-owned bound UDP socket.
///
/// Wraps the socket in Quinn (`TokioRuntime`) without a duplicate bind.
/// Same-port TCP+UDP validation stays with the caller (compatibility
/// `Server`); this helper only wraps.
#[doc(hidden)]
pub fn endpoint_from_socket(
    quic_config: quinn::ServerConfig,
    socket: std::net::UdpSocket,
) -> Result<h3_quinn::Endpoint, std::io::Error> {
    socket.set_nonblocking(true)?;
    quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(quic_config),
        socket,
        Arc::new(quinn::TokioRuntime),
    )
    .map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            format!("H3 UDP wrap failed: {e}"),
        )
    })
}

/// Validate same-port TCP+UDP for prebound sockets (Plan 201 Track E).
///
/// Returns `Ok(())` when the UDP socket's port matches the TCP bind port.
pub fn validate_same_port_udp(
    tcp_bind: std::net::SocketAddr,
    udp_socket: &std::net::UdpSocket,
) -> Result<(), String> {
    let udp_addr = udp_socket
        .local_addr()
        .map_err(|e| format!("prebound H3 UDP local_addr failed: {e}"))?;
    if udp_addr.port() != tcp_bind.port() {
        return Err(format!(
            "prebound H3 UDP port {} does not match TCP port {}; same-port TCP+UDP required",
            udp_addr.port(),
            tcp_bind.port()
        ));
    }
    Ok(())
}
