//! Connection metadata for a request.
//!
//! [`ConnectionInfo`] carries transport-level metadata about the connection
//! on which a request was received. It is separate from request headers
//! and is not mixed into the header block.

use std::fmt;
use std::net::SocketAddr;

use crate::primitives::authority::Authority;
use crate::primitives::proxy::ProxySourceKind;

/// TLS metadata for a connection.
///
/// Contains verified information about the TLS session, if any. Bounded to
/// avoid exposing implementation-specific internals (no cipher/provider
/// enums, no private-key material).
///
/// Plan 203: `alpn` carries the negotiated application protocol (`h2`,
/// `http/1.1`, `h3`) when known; `client_authenticated` and
/// `peer_certificates_present` report the verified client-certificate state
/// (present implies verified through the configured WebPKI trust policy);
/// `peer_certificate_chain` is the opt-in bounded DER chain (see
/// `RuntimeConfig::tls_expose_peer_chain`; `None` unless explicitly enabled).
/// Caller-asserted metadata via [`crate::server::connection::ConnectionContext`]
/// must be distinguished from EggServe-terminated sessions where provenance
/// matters (see `docs/downstream-app-server.md`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TlsInfo {
    /// The negotiated TLS protocol version (e.g., "TLSv1.3"), if available.
    pub protocol_version: Option<String>,
    /// The Server Name Indication (SNI) value, if available.
    ///
    /// Validated/bounded (max 253 chars, ASCII DNS) before use; never carries
    /// key material.
    pub server_name: Option<String>,
    /// Negotiated ALPN identifier (`h2`, `http/1.1`, `h3`), if known.
    pub alpn: Option<String>,
    /// `true` when a client certificate was presented and verified against
    /// the configured trust policy.
    pub client_authenticated: bool,
    /// `true` when a verified peer certificate chain is present.
    pub peer_certificates_present: bool,
    /// Opt-in bounded DER chain (leaf first). `None` unless
    /// `tls_expose_peer_chain` is enabled. Bounded to at most 8 certificates;
    /// oversized chains are suppressed to `None` rather than truncated.
    pub peer_certificate_chain: Option<Vec<Vec<u8>>>,
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
/// Raw transport peer/local endpoints are always preserved. Trusted
/// proxy-reported values (Plan 202) populate a separate provenance-tagged
/// effective layer and never overwrite the raw endpoints.
///
/// `Forwarded` and `X-Forwarded-*` headers are ordinary untrusted headers by
/// default; they populate trusted fields only when an explicit
/// [`crate::primitives::proxy::TrustedProxyConfig`] trusts the immediate
/// peer. The canonical `Host`/request-target is never silently rewritten;
/// trusted authority populates [`ConnectionInfo::effective_authority`] for
/// the downstream application to use explicitly.
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
/// Connection metadata is never mixed into request headers. Untrusted
/// deployments should keep reading `Forwarded` or `X-Forwarded-*` headers
/// separately under their own trust model; trusted deployments read the
/// effective accessors below.
///
/// # Migration (Plan 163)
///
/// `local_addr` and `remote_addr` were previously mandatory `SocketAddr`
/// fields. They are now `Option<SocketAddr>`: wrap TCP addresses in
/// `Some(..)` and use `None` for non-socket transports. Prefer the
/// constructors below over struct literals.
///
/// # Migration (Plan 202)
///
/// New provenance-tagged effective fields default to `None` (no trusted
/// proxy metadata). Existing constructors preserve that default; struct
/// literals must add `..Default::default()`-style fields or use the
/// constructors. `Clone`/`PartialEq` include the new fields.
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
    /// Proxy-reported source endpoint from a trusted PROXY preamble, if any.
    ///
    /// `None` for `LOCAL`/`UNKNOWN`/`UNSPEC`/UNIX (truthful absence) and
    /// when no preamble was accepted. Never replaces [`Self::remote_addr`].
    pub proxy_source: Option<SocketAddr>,
    /// Proxy-reported destination endpoint from a trusted PROXY preamble.
    pub proxy_destination: Option<SocketAddr>,
    /// Which PROXY preamble version populated the proxy layer, if any.
    pub proxy_provenance: Option<ProxySourceKind>,
    /// Final effective client endpoint (PROXY wins over header-derived).
    ///
    /// `None` means no trusted override; use [`Self::remote_addr`].
    /// Ports are `0` when a header carried an IP without a port.
    pub effective_client: Option<SocketAddr>,
    /// Trusted external scheme override (from header policy only; PROXY
    /// carries no scheme). `None` means use [`Self::scheme`].
    pub effective_scheme: Option<Scheme>,
    /// Trusted external authority override (from header policy only).
    /// `None` means use the canonical request authority. Never rewrites the
    /// request target/Host by itself.
    pub effective_authority: Option<Authority>,
    /// Which header family populated the forwarded layer, if any.
    pub forwarded_provenance: Option<ProxySourceKind>,
}

impl ConnectionInfo {
    /// Create metadata from explicit parts.
    ///
    /// TCP/TLS callers must pass `Some` for both addresses. Non-socket
    /// callers must pass `None` for both; half-present endpoints are
    /// collapsed to `None` by [`ConnectionInfo::socket_endpoints`] and
    /// reported as absent by [`ConnectionInfo::has_socket_endpoints`].
    ///
    /// Trusted proxy layers default to absent (`None` provenance).
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
            proxy_source: None,
            proxy_destination: None,
            proxy_provenance: None,
            effective_client: None,
            effective_scheme: None,
            effective_authority: None,
            forwarded_provenance: None,
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
            proxy_source: None,
            proxy_destination: None,
            proxy_provenance: None,
            effective_client: None,
            effective_scheme: None,
            effective_authority: None,
            forwarded_provenance: None,
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
            proxy_source: None,
            proxy_destination: None,
            proxy_provenance: None,
            effective_client: None,
            effective_scheme: None,
            effective_authority: None,
            forwarded_provenance: None,
        }
    }

    /// Attach a trusted PROXY preamble result (runtime only).
    ///
    /// `source`/`destination` may both be `None` for `LOCAL`/`UNKNOWN`/
    /// `UNSPEC`/UNIX (truthful absence); provenance is still recorded so
    /// observability can distinguish "preamble accepted, no identity" from
    /// "no preamble". Also populates [`Self::effective_client`] when the
    /// preamble carries a source and no effective client is set yet (PROXY
    /// wins over later header-derived values).
    pub fn with_proxy_endpoints(
        mut self,
        source: Option<SocketAddr>,
        destination: Option<SocketAddr>,
        kind: ProxySourceKind,
    ) -> Self {
        self.proxy_source = source;
        self.proxy_destination = destination;
        self.proxy_provenance = Some(kind);
        if self.effective_client.is_none() {
            self.effective_client = source;
        }
        self
    }

    /// Attach trusted header-derived effective metadata (runtime only).
    ///
    /// PROXY-derived [`Self::effective_client`] wins when already present;
    /// scheme/authority always come from the header layer. Provenance is
    /// recorded separately from [`Self::proxy_provenance`].
    pub fn with_forwarded_effective(
        mut self,
        effective: &crate::primitives::proxy::ForwardedEffective,
    ) -> Self {
        if self.effective_client.is_none() {
            self.effective_client = effective.client;
        }
        if self.effective_scheme.is_none() {
            self.effective_scheme = effective.scheme;
        }
        if self.effective_authority.is_none() {
            self.effective_authority = effective.authority.clone();
        }
        self.forwarded_provenance = Some(effective.provenance);
        self
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

    /// Final effective client endpoint: trusted override if present, else raw peer.
    pub fn effective_client_addr(&self) -> Option<SocketAddr> {
        self.effective_client.or(self.remote_addr)
    }

    /// Final effective scheme: trusted override if present, else direct scheme.
    pub fn effective_scheme_value(&self) -> Scheme {
        self.effective_scheme.unwrap_or(self.scheme)
    }

    /// Trusted external authority override, if accepted.
    pub fn effective_authority_value(&self) -> Option<&Authority> {
        self.effective_authority.as_ref()
    }

    /// Returns `true` when any trusted proxy layer was accepted.
    pub fn has_trusted_proxy_metadata(&self) -> bool {
        self.proxy_provenance.is_some() || self.forwarded_provenance.is_some()
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

impl TlsInfo {
    /// Maximum SNI length tracked for observability (DNS limit).
    pub const MAX_SERVER_NAME_LEN: usize = 253;
    /// Maximum peer certificates exposed via `peer_certificate_chain`.
    pub const MAX_PEER_CERTIFICATES: usize = 8;
    /// Maximum bytes per peer certificate exposed via `peer_certificate_chain`.
    pub const MAX_PEER_CERT_BYTES: usize = 64 * 1024;

    /// Sanitized SNI for observability: bounded ASCII, no key material.
    pub fn sanitized_server_name(&self) -> Option<String> {
        self.server_name.as_ref().map(|n| {
            let filtered: String = n
                .chars()
                .filter(|c| (0x20..=0x7E).contains(&(*c as u32)))
                .collect();
            if filtered.len() > Self::MAX_SERVER_NAME_LEN {
                filtered[..Self::MAX_SERVER_NAME_LEN].to_owned()
            } else {
                filtered
            }
        })
    }

    /// Whether this session carries a verified client identity.
    pub fn is_client_authenticated(&self) -> bool {
        self.client_authenticated
    }
}

impl fmt::Display for TlsInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TLS")?;
        if let Some(ref v) = self.protocol_version {
            write!(f, " {v}")?;
        }
        if let Some(ref n) = self.sanitized_server_name() {
            write!(f, " SNI={n}")?;
        }
        if let Some(ref a) = self.alpn {
            write!(f, " ALPN={a}")?;
        }
        if self.client_authenticated {
            write!(f, " client-auth")?;
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
            ..Default::default()
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
            ..Default::default()
        };
        assert_eq!(format!("{info}"), "TLS");
    }

    #[test]
    fn tls_info_plan203_metadata() {
        let info = TlsInfo {
            protocol_version: Some("TLSv1.3".to_string()),
            server_name: Some("example.com".to_string()),
            alpn: Some("h2".to_string()),
            client_authenticated: true,
            peer_certificates_present: true,
            peer_certificate_chain: None,
        };
        let display = format!("{info}");
        assert!(display.contains("h2"));
        assert!(display.contains("client-auth"));
        assert!(info.is_client_authenticated());
        assert_eq!(info.sanitized_server_name().as_deref(), Some("example.com"));
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
                ..Default::default()
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
