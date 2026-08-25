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
use rustls_pemfile::Item;

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
    let cert_file = File::open(cert_path)
        .map_err(|_| TlsError::CertFileNotFound(cert_path.display().to_string()))?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError::CertReadError(e.to_string()))?;
    if certs.is_empty() {
        return Err(TlsError::NoCertificatesFound);
    }

    let key_file = File::open(key_path)
        .map_err(|_| TlsError::KeyFileNotFound(key_path.display().to_string()))?;
    let mut key_reader = BufReader::new(key_file);
    let mut private_key = None;
    let mut key_count = 0;
    for item in rustls_pemfile::read_all(&mut key_reader) {
        match item {
            Ok(Item::Pkcs1Key(key)) => {
                key_count += 1;
                private_key = Some(rustls::pki_types::PrivateKeyDer::Pkcs1(key));
            }
            Ok(Item::Pkcs8Key(key)) => {
                key_count += 1;
                private_key = Some(rustls::pki_types::PrivateKeyDer::Pkcs8(key));
            }
            Ok(Item::Sec1Key(key)) => {
                key_count += 1;
                private_key = Some(rustls::pki_types::PrivateKeyDer::Sec1(key));
            }
            Ok(_) => {}
            Err(e) => return Err(TlsError::KeyReadError(e.to_string())),
        }
    }
    if key_count > 1 {
        return Err(TlsError::MultiplePrivateKeysFound);
    }
    let key = private_key.ok_or(TlsError::NoPrivateKeyFound)?;
    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| TlsError::InvalidKey(e.to_string()))?;
    // EggServe serves HTTP/1.1 only; advertise that over ALPN so the
    // documented "HTTP/1.1 ALPN only" contract is enforced natively instead
    // of being left to client defaults.
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}
