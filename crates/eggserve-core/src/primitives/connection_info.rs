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

/// Paired socket endpoints for a TCP/TLS connection.
///
/// Both addresses are real transport identities observed at accept time.
/// Non-socket transports (for example a caller-owned byte stream) expose
/// no endpoints at all rather than fabricated IP/port values. Downstream
/// code that needs peer identity for such transports must retain it
/// outside EggServe and associate it with its own service wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SocketEndpoints {
    /// The local socket address the connection was accepted on.
    pub local: SocketAddr,
    /// The remote socket address of the peer.
    pub remote: SocketAddr,
}

/// Immutable connection metadata for an HTTP request.
///
/// Values come from the actual transport. `Forwarded` and
/// `X-Forwarded-*` headers are ordinary untrusted headers and are not
/// part of this type.
///
/// # Socket endpoints
///
/// Real TCP/TLS connections expose actual socket endpoints (`Some`).
/// Caller-owned non-socket transports expose `None` for both addresses;
/// EggServe never fabricates an IP/port. Use
/// [`ConnectionInfo::with_socket_addrs`] for TCP/TLS and
/// [`ConnectionInfo::without_socket_addrs`] for opaque streams.
/// [`ConnectionInfo::socket_endpoints`] returns the paired view when
/// both addresses are present.
///
/// # Separation from headers
///
/// Connection metadata is never mixed into request headers. Callers who
/// need proxy-trusted values should read `Forwarded` or
/// `X-Forwarded-*` headers separately and validate them according to
/// their trust model.
///
/// # Migration (Plan 163)
///
/// `local_addr` and `remote_addr` were previously mandatory `SocketAddr`
/// fields. They are now `Option<SocketAddr>`: wrap TCP addresses in
/// `Some(..)` and use `None` for non-socket transports. Prefer the
/// constructors below over struct literals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionInfo {
    /// The local socket address, when the transport has one.
    pub local_addr: Option<SocketAddr>,
    /// The remote socket address, when the transport has one.
    pub remote_addr: Option<SocketAddr>,
    /// The request URI scheme (e.g., `http` or `https`).
    pub scheme: Scheme,
    /// TLS session metadata, if EggServe performed or knows the TLS session.
    ///
    /// Opaque encrypted transports (for example an anonymity-network
    /// stream carrying HTTP) leave this as `None` unless the caller
    /// explicitly terminates HTTPS on the stream.
    pub tls: Option<TlsInfo>,
}

impl ConnectionInfo {
    /// Create metadata from explicit parts.
    ///
    /// TCP/TLS callers must pass `Some` for both addresses. Non-socket
    /// callers must pass `None` for both; half-present endpoints are
    /// collapsed to `None` by [`ConnectionInfo::socket_endpoints`] and
    /// reported as absent by [`ConnectionInfo::has_socket_endpoints`].
    pub fn new(
        local_addr: Option<SocketAddr>,
        remote_addr: Option<SocketAddr>,
        scheme: Scheme,
        tls: Option<TlsInfo>,
    ) -> Self {
        Self {
            local_addr,
            remote_addr,
            scheme,
            tls,
        }
    }

    /// Metadata for a real TCP/TLS connection with observed endpoints.
    pub fn with_socket_addrs(
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
        scheme: Scheme,
        tls: Option<TlsInfo>,
    ) -> Self {
        Self {
            local_addr: Some(local_addr),
            remote_addr: Some(remote_addr),
            scheme,
            tls,
        }
    }

    /// Metadata for a caller-owned non-socket stream.
    ///
    /// No socket endpoints are recorded. The caller supplies the
    /// trustworthy semantic `scheme`; an anonymity-network transport is
    /// `Scheme::Http` unless HTTPS was explicitly terminated on it.
    pub fn without_socket_addrs(scheme: Scheme, tls: Option<TlsInfo>) -> Self {
        Self {
            local_addr: None,
            remote_addr: None,
            scheme,
            tls,
        }
    }

    /// Paired socket endpoints when both addresses are present.
    pub fn socket_endpoints(&self) -> Option<SocketEndpoints> {
        match (self.local_addr, self.remote_addr) {
            (Some(local), Some(remote)) => Some(SocketEndpoints { local, remote }),
            _ => None,
        }
    }

    /// Returns `true` when both socket endpoints are present.
    pub fn has_socket_endpoints(&self) -> bool {
        self.local_addr.is_some() && self.remote_addr.is_some()
    }
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
        let a = ConnectionInfo::with_socket_addrs(
            "127.0.0.1:8000".parse().unwrap(),
            "127.0.0.1:12345".parse().unwrap(),
            Scheme::Http,
            None,
        );
        let b = ConnectionInfo::with_socket_addrs(
            "127.0.0.1:8000".parse().unwrap(),
            "127.0.0.1:12345".parse().unwrap(),
            Scheme::Http,
            None,
        );
        assert_eq!(a, b);
    }

    #[test]
    fn connection_info_with_tls() {
        let info = ConnectionInfo::with_socket_addrs(
            "0.0.0.0:443".parse().unwrap(),
            "10.0.0.1:54321".parse().unwrap(),
            Scheme::Https,
            Some(TlsInfo {
                protocol_version: Some("TLSv1.3".to_string()),
                server_name: Some("example.com".to_string()),
            }),
        );
        assert_eq!(info.scheme, Scheme::Https);
        assert!(info.tls.is_some());
    }

    #[test]
    fn non_socket_connection_has_no_endpoints() {
        let info = ConnectionInfo::without_socket_addrs(Scheme::Http, None);
        assert_eq!(info.local_addr, None);
        assert_eq!(info.remote_addr, None);
        assert!(!info.has_socket_endpoints());
        assert!(info.socket_endpoints().is_none());
    }

    #[test]
    fn socket_connection_exposes_paired_endpoints() {
        let local: SocketAddr = "127.0.0.1:8000".parse().unwrap();
        let remote: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let info = ConnectionInfo::with_socket_addrs(local, remote, Scheme::Http, None);
        assert!(info.has_socket_endpoints());
        let endpoints = info.socket_endpoints().unwrap();
        assert_eq!(endpoints.local, local);
        assert_eq!(endpoints.remote, remote);
    }
}
