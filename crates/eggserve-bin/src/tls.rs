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
    EncryptedPrivateKeyNotSupported,
    InvalidKey(String),
}

impl fmt::Display for TlsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TlsError::CertFileNotFound(path) => {
                write!(f, "certificate file not found: {}", path)
            }
            TlsError::KeyFileNotFound(path) => write!(f, "key file not found: {}", path),
            TlsError::CertReadError(msg) => write!(f, "failed to read certificate: {}", msg),
            TlsError::KeyReadError(msg) => write!(f, "failed to read key: {}", msg),
            TlsError::NoCertificatesFound => {
                write!(f, "no valid certificates found in certificate file")
            }
            TlsError::NoPrivateKeyFound => {
                write!(f, "no valid private key found in key file")
            }
            TlsError::MultiplePrivateKeysFound => {
                write!(f, "multiple private keys found; exactly one is required")
            }
            TlsError::EncryptedPrivateKeyNotSupported => {
                write!(f, "encrypted private keys are not supported")
            }
            TlsError::InvalidKey(msg) => write!(f, "invalid private key: {}", msg),
        }
    }
}

impl std::error::Error for TlsError {}

pub fn load_tls_config(cert_path: &Path, key_path: &Path) -> Result<Arc<ServerConfig>, TlsError> {
    let cert_file = File::open(cert_path)
        .map_err(|_| TlsError::CertFileNotFound(cert_path.display().to_string()))?;
    let mut cert_reader = BufReader::new(cert_file);

    let certs: Vec<_> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError::CertReadError(e.to_string()))?;

    if certs.is_empty() {
        return Err(TlsError::NoCertificatesFound);
    }

    let key_file = File::open(key_path)
        .map_err(|_| TlsError::KeyFileNotFound(key_path.display().to_string()))?;
    let mut key_reader = BufReader::new(key_file);

    let mut private_key = None;
    let mut pkcs_count = 0u32;

    for item in rustls_pemfile::read_all(&mut key_reader) {
        match item {
            Ok(Item::Pkcs1Key(key)) => {
                pkcs_count += 1;
                private_key = Some(rustls::pki_types::PrivateKeyDer::Pkcs1(key));
            }
            Ok(Item::Pkcs8Key(key)) => {
                pkcs_count += 1;
                private_key = Some(rustls::pki_types::PrivateKeyDer::Pkcs8(key));
            }
            Ok(Item::Sec1Key(key)) => {
                pkcs_count += 1;
                private_key = Some(rustls::pki_types::PrivateKeyDer::Sec1(key));
            }
            Ok(_) => {}
            Err(e) => {
                return Err(TlsError::KeyReadError(e.to_string()));
            }
        }
    }

    if pkcs_count > 1 {
        return Err(TlsError::MultiplePrivateKeysFound);
    }

    let key = private_key.ok_or(TlsError::NoPrivateKeyFound)?;

    let cert_chain: Vec<_> = certs
        .into_iter()
        .map(rustls::pki_types::CertificateDer::from)
        .collect();

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .map_err(|e| TlsError::InvalidKey(e.to_string()))?;

    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_file(content: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn missing_cert_file_returns_error() {
        let result = load_tls_config(
            Path::new("/nonexistent/cert.pem"),
            Path::new("/nonexistent/key.pem"),
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            TlsError::CertFileNotFound(_) => {}
            other => panic!("expected CertFileNotFound, got: {:?}", other),
        }
    }

    #[test]
    fn missing_key_file_returns_error() {
        let cert = create_temp_file(b"-----BEGIN CERTIFICATE-----\nMIIBkTCB+wIJALHM5P4G1w4tMA0GCSqGSIb3DQEBCwUAMBExDzANBgNVBAMMBnRl\n-----END CERTIFICATE-----\n");
        let result = load_tls_config(cert.path(), Path::new("/nonexistent/key.pem"));
        assert!(result.is_err());
        match result.unwrap_err() {
            TlsError::KeyFileNotFound(_) => {}
            other => panic!("expected KeyFileNotFound, got: {:?}", other),
        }
    }

    #[test]
    fn empty_cert_file_returns_error() {
        let cert = create_temp_file(b"");
        let key = create_temp_file(b"");
        let result = load_tls_config(cert.path(), key.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            TlsError::NoCertificatesFound => {}
            other => panic!("expected NoCertificatesFound, got: {:?}", other),
        }
    }

    #[test]
    fn empty_key_file_returns_error() {
        let cert_content = b"-----BEGIN CERTIFICATE-----\nMIIBkTCB+wIJALHM5P4G1w4tMA0GCSqGSIb3DQEBCwUAMBExDzANBgNVBAMMBnRl\n-----END CERTIFICATE-----\n";
        let cert = create_temp_file(cert_content);
        let key = create_temp_file(b"");
        let result = load_tls_config(cert.path(), key.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            TlsError::NoPrivateKeyFound => {}
            other => panic!("expected NoPrivateKeyFound, got: {:?}", other),
        }
    }

    #[test]
    fn invalid_key_file_returns_error() {
        let cert_content = b"-----BEGIN CERTIFICATE-----\nMIIBkTCB+wIJALHM5P4G1w4tMA0GCSqGSIb3DQEBCwUAMBExDzANBgNVBAMMBnRl\n-----END CERTIFICATE-----\n";
        let cert = create_temp_file(cert_content);
        let key = create_temp_file(b"not a real key");
        let result = load_tls_config(cert.path(), key.path());
        assert!(result.is_err());
    }
}
