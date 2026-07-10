//! Error types for HTTP client operations.

use std::fmt;

/// Errors from HTTP client operations.
#[derive(Debug)]
#[allow(dead_code)]
pub enum ClientError {
    /// The URL is malformed or could not be parsed.
    InvalidUrl(String),
    /// The URL scheme is not supported (only http and https are allowed).
    UnsupportedScheme(String),
    /// The URL is missing a host component.
    MissingHost,
    /// A header name or value contains invalid characters.
    InvalidHeader(String),
    /// The request body exceeds the configured maximum size.
    BodyTooLarge { limit: u64, actual: u64 },
    /// The connection or request timed out.
    Timeout(String),
    /// DNS resolution failed.
    DnsError(String),
    /// TCP connection failed.
    ConnectError(String),
    /// TLS handshake or verification failed.
    TlsError(String),
    /// An HTTP protocol error occurred.
    ProtocolError(String),
    /// The response body exceeded the configured maximum size.
    ResponseBodyTooLarge { limit: u64 },
    /// An I/O error occurred during the request or response.
    Io(std::io::Error),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(msg) => write!(f, "invalid URL: {msg}"),
            Self::UnsupportedScheme(s) => write!(f, "unsupported scheme: {s}"),
            Self::MissingHost => write!(f, "URL missing host component"),
            Self::InvalidHeader(msg) => write!(f, "invalid header: {msg}"),
            Self::BodyTooLarge { limit, actual } => {
                write!(f, "request body too large: {actual} bytes (limit: {limit})")
            }
            Self::Timeout(msg) => write!(f, "timeout: {msg}"),
            Self::DnsError(msg) => write!(f, "DNS error: {msg}"),
            Self::ConnectError(msg) => write!(f, "connect error: {msg}"),
            Self::TlsError(msg) => write!(f, "TLS error: {msg}"),
            Self::ProtocolError(msg) => write!(f, "protocol error: {msg}"),
            Self::ResponseBodyTooLarge { limit } => {
                write!(f, "response body exceeded limit: {limit} bytes")
            }
            Self::Io(err) => write!(f, "I/O error: {err}"),
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ClientError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}
