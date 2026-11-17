//! Transport-kind vocabulary for forwarded-metadata trust (Plan 321).
//!
//! `trust_unix` means "the runtime owns a local Unix-domain listener and the
//! immediate peer is local". It must never mean "no socket address was
//! recorded": caller-owned byte streams also lack a socket peer, but the
//! runtime does not control who is on the other end.
//!
//! The kind is set by the code that owns the transport (the accept loop or the
//! documented caller-owned entry point), never derived from request data.

use std::net::SocketAddr;

use eggserve_primitives::proxy::TrustedProxyConfig;

/// Which transport accepted a connection, as known by the accepting owner.
///
/// Internal plumbing for the forwarded-header trust decision (Plan 321), not a
/// stable application API. Embedders do not select this value: the runtime
/// sets it at the accept boundary, and the public caller-owned drivers always
/// report [`PeerTransport::Opaque`].
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerTransport {
    /// A runtime-owned TCP, TLS, or QUIC listener with an observed peer.
    Socket,
    /// A runtime-owned Unix-domain listener.
    Unix,
    /// A caller-owned byte stream with no runtime-known peer identity.
    Opaque,
}

impl PeerTransport {
    /// The single forwarded-metadata trust mapping.
    ///
    /// Both the direct and compatibility pipelines call this method; neither
    /// re-derives trust from `remote_addr` absence. The direction is closed:
    /// `Opaque` and a missing socket peer are never trusted by `trust_unix`.
    #[doc(hidden)]
    pub fn trusts_forwarded(
        self,
        remote_addr: Option<SocketAddr>,
        config: &TrustedProxyConfig,
    ) -> bool {
        match self {
            Self::Socket => remote_addr.is_some_and(|peer| config.is_trusted_peer(&peer)),
            Self::Unix => config.trust_unix,
            Self::Opaque => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggserve_primitives::proxy::IpPrefix;

    fn config(trust_unix: bool, trusted_peer: Option<&str>) -> TrustedProxyConfig {
        let mut config = TrustedProxyConfig {
            trust_unix,
            ..TrustedProxyConfig::default()
        };
        if let Some(peer) = trusted_peer {
            config.peers.push(IpPrefix::parse(peer).unwrap());
        }
        config
    }

    #[test]
    fn socket_peer_uses_explicit_peer_policy() {
        let trusted = config(false, Some("127.0.0.1"));
        let peer = Some("127.0.0.1:5000".parse().unwrap());
        assert!(PeerTransport::Socket.trusts_forwarded(peer, &trusted));

        let untrusted = config(false, None);
        assert!(!PeerTransport::Socket.trusts_forwarded(peer, &untrusted));

        // A socket transport without an observed peer fails closed even when
        // trust_unix is set: the address check is the only socket authority.
        let peerless = config(true, None);
        assert!(!PeerTransport::Socket.trusts_forwarded(None, &peerless));
    }

    #[test]
    fn unix_transport_uses_trust_unix_only() {
        let trusted = config(true, None);
        assert!(PeerTransport::Unix.trusts_forwarded(None, &trusted));

        let untrusted = config(false, None);
        assert!(!PeerTransport::Unix.trusts_forwarded(None, &untrusted));
    }

    #[test]
    fn opaque_transport_is_never_trusted() {
        let trusted = config(true, Some("127.0.0.1"));
        assert!(!PeerTransport::Opaque.trusts_forwarded(None, &trusted));
        assert!(!PeerTransport::Opaque
            .trusts_forwarded(Some("127.0.0.1:5000".parse().unwrap()), &trusted));
    }
}
