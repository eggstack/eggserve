//! Connection metadata for a request.
//!
//! [`ConnectionInfo`] carries transport-level metadata about the connection
//! on which a request was received. It is separate from request headers
//! and is not mixed into the header block.

use std::fmt;
use std::net::SocketAddr;

/// TLS metadata for a connection.
///
/// Contains information about the TLS session, if any. Bounded to
/// avoid exposing implementation-specific internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsInfo {
    /// The negotiated TLS protocol version (e.g., "TLSv1.3"), if available.
    pub protocol_version: Option<String>,
    /// The Server Name Indication (SNI) value, if available.
    pub server_name: Option<String>,
}

/// Immutable connection metadata for an HTTP request.
///
/// Values come from the actual transport. `Forwarded` and
/// `X-Forwarded-*` headers are ordinary untrusted headers and are not
/// part of this type.
///
/// # Separation from headers
///
/// Connection metadata is never mixed into request headers. Callers who
/// need proxy-trusted values should read `Forwarded` or
/// `X-Forwarded-*` headers separately and validate them according to
/// their trust model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionInfo {
    /// The local socket address the connection was accepted on.
    pub local_addr: SocketAddr,
    /// The remote socket address of the peer.
    pub remote_addr: SocketAddr,
    /// The request URI scheme (e.g., `http` or `https`).
    pub scheme: Scheme,
    /// TLS session metadata, if the connection is TLS-secured.
    pub tls: Option<TlsInfo>,
}

/// The request URI scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scheme {
    /// Plain HTTP.
    Http,
    /// HTTPS (HTTP over TLS).
    Https,
}

impl Scheme {
    /// Returns the scheme as a string slice.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }
}

impl fmt::Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for TlsInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TLS")?;
        if let Some(ref v) = self.protocol_version {
            write!(f, " {v}")?;
        }
        if let Some(ref n) = self.server_name {
            write!(f, " SNI={n}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_as_str() {
        assert_eq!(Scheme::Http.as_str(), "http");
        assert_eq!(Scheme::Https.as_str(), "https");
    }

    #[test]
    fn scheme_display() {
        assert_eq!(format!("{}", Scheme::Http), "http");
        assert_eq!(format!("{}", Scheme::Https), "https");
    }

    #[test]
    fn tls_info_display() {
        let info = TlsInfo {
            protocol_version: Some("TLSv1.3".to_string()),
            server_name: Some("example.com".to_string()),
        };
        let display = format!("{info}");
        assert!(display.contains("TLSv1.3"));
        assert!(display.contains("example.com"));
    }

    #[test]
    fn tls_info_minimal() {
        let info = TlsInfo {
            protocol_version: None,
            server_name: None,
        };
        assert_eq!(format!("{info}"), "TLS");
    }

    #[test]
    fn connection_info_equality() {
        let a = ConnectionInfo {
            local_addr: "127.0.0.1:8000".parse().unwrap(),
            remote_addr: "127.0.0.1:12345".parse().unwrap(),
            scheme: Scheme::Http,
            tls: None,
        };
        let b = ConnectionInfo {
            local_addr: "127.0.0.1:8000".parse().unwrap(),
            remote_addr: "127.0.0.1:12345".parse().unwrap(),
            scheme: Scheme::Http,
            tls: None,
        };
        assert_eq!(a, b);
    }

    #[test]
    fn connection_info_with_tls() {
        let info = ConnectionInfo {
            local_addr: "0.0.0.0:443".parse().unwrap(),
            remote_addr: "10.0.0.1:54321".parse().unwrap(),
            scheme: Scheme::Https,
            tls: Some(TlsInfo {
                protocol_version: Some("TLSv1.3".to_string()),
                server_name: Some("example.com".to_string()),
            }),
        };
        assert_eq!(info.scheme, Scheme::Https);
        assert!(info.tls.is_some());
    }
}
