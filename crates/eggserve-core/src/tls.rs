//! Shared rustls server configuration loading.
//!
//! This is deliberately a small path-based loader used by the CLI and the
//! Python compatibility façade.  EggServe does not accept Python SSL
//! contexts, expose key material, or provide certificate management.

use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use rustls::ServerConfig;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

#[derive(Debug)]
pub enum TlsError {
    CertFileNotFound(String),
    KeyFileNotFound(String),
    CertReadError(String),
    KeyReadError(String),
    NoCertificatesFound,
    NoPrivateKeyFound,
    MultiplePrivateKeysFound,
    InvalidKey(String),
}

impl fmt::Display for TlsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CertFileNotFound(path) => write!(f, "certificate file not found: {path}"),
            Self::KeyFileNotFound(path) => write!(f, "key file not found: {path}"),
            Self::CertReadError(msg) => write!(f, "failed to read certificate: {msg}"),
            Self::KeyReadError(msg) => write!(f, "failed to read key: {msg}"),
            Self::NoCertificatesFound => {
                write!(f, "no valid certificates found in certificate file")
            }
            Self::NoPrivateKeyFound => write!(f, "no valid private key found in key file"),
            Self::MultiplePrivateKeysFound => {
                write!(f, "multiple private keys found; exactly one is required")
            }
            Self::InvalidKey(msg) => write!(f, "invalid private key: {msg}"),
        }
    }
}

impl std::error::Error for TlsError {}

/// Load a rustls server configuration from PEM certificate and key files.
pub fn load_tls_config(cert_path: &Path, key_path: &Path) -> Result<Arc<ServerConfig>, TlsError> {
    load_tls_config_with_http2(cert_path, key_path, cfg!(feature = "http2"))
}

/// Load a rustls server configuration and explicitly choose the advertised
/// application protocols.
///
/// Rust CLI callers built with `http2` normally use the default loader, which
/// advertises `h2` before `http/1.1`. Compatibility frontends can pass
/// `false` to keep their HTTP/1.1-only contract even when linked with a core
/// build that has H2 enabled.
pub fn load_tls_config_with_http2(
    cert_path: &Path,
    key_path: &Path,
    http2: bool,
) -> Result<Arc<ServerConfig>, TlsError> {
    let (certs, key) = load_identity(cert_path, key_path)?;
    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| TlsError::InvalidKey(e.to_string()))?;
    config.alpn_protocols = advertised_alpn(http2);
    Ok(Arc::new(config))
}

fn load_identity(
    cert_path: &Path,
    key_path: &Path,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), TlsError> {
    let cert_file = File::open(cert_path)
        .map_err(|_| TlsError::CertFileNotFound(cert_path.display().to_string()))?;
    let cert_reader = BufReader::new(cert_file);
    let certs = CertificateDer::pem_reader_iter(cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError::CertReadError(e.to_string()))?;
    if certs.is_empty() {
        return Err(TlsError::NoCertificatesFound);
    }

    let key_file = File::open(key_path)
        .map_err(|_| TlsError::KeyFileNotFound(key_path.display().to_string()))?;
    let key_reader = BufReader::new(key_file);
    let mut private_key = None;
    let mut key_count = 0;
    for item in PrivateKeyDer::pem_reader_iter(key_reader) {
        let key = item.map_err(|e| TlsError::KeyReadError(e.to_string()))?;
        key_count += 1;
        private_key = Some(key);
    }
    if key_count > 1 {
        return Err(TlsError::MultiplePrivateKeysFound);
    }
    let key = private_key.ok_or(TlsError::NoPrivateKeyFound)?;
    Ok((certs, key))
}

/// Build a QUIC-specific server configuration from the validated PEM
/// identity. This is crate-private so Quinn and private key material never
/// become part of EggServe's public API.
#[cfg(feature = "http3")]
pub(crate) fn load_quic_server_config(
    cert_path: &Path,
    key_path: &Path,
    config: &crate::server::Http3Config,
) -> Result<quinn::ServerConfig, TlsError> {
    let (certs, key) = load_identity(cert_path, key_path)?;
    let mut tls = ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
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

fn advertised_alpn(http2: bool) -> Vec<Vec<u8>> {
    if http2 {
        vec![b"h2".to_vec(), b"http/1.1".to_vec()]
    } else {
        vec![b"http/1.1".to_vec()]
    }
}

#[cfg(test)]
mod tests {
    use super::advertised_alpn;

    #[test]
    fn h2_alpn_is_preferred_only_when_enabled() {
        assert_eq!(
            advertised_alpn(true),
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        );
        assert_eq!(advertised_alpn(false), vec![b"http/1.1".to_vec()]);
    }
}
