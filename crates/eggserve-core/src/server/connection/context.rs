//! Public connection facade: transport context, shutdown token, outcome.
//!
//! Owns the caller-visible connection vocabulary shared by TCP/TLS `Server`
//! and caller-owned transports. No Hyper types, no driver state, no service
//! dispatch. `ConnectionShutdown` semantics (level-triggered, idempotent,
//! pre-signal safe) are immutable Plan 178 input.

use std::sync::Arc;

use crate::primitives::connection_info::{Scheme, SocketEndpoints, TlsInfo};

/// Trustworthy per-connection transport description supplied by the caller.
///
/// For real TCP/TLS connections the runtime builds this from observed
/// socket addresses and the completed handshake. For caller-owned streams
/// (for example an anonymity-network byte stream) the caller asserts the
/// semantic `scheme` explicitly: such a transport is `Scheme::Http` unless
/// HTTPS was explicitly terminated on it. `tls` is present only when
/// EggServe performed or otherwise knows the TLS session; opaque encrypted
/// transports leave it as `None`.
///
/// No I2P `Destination`, tunnel IDs, router identities, or LeaseSet types
/// enter EggServe. If downstream code needs peer identity it retains that
/// identity outside EggServe and associates it with its own service
/// wrapper/session state. Forwarded/`X-Forwarded-*` values remain ordinary
/// untrusted HTTP headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionContext {
    /// Local socket address when the transport has one.
    pub local_addr: Option<std::net::SocketAddr>,
    /// Remote socket address when the transport has one.
    pub remote_addr: Option<std::net::SocketAddr>,
    /// HTTP vs HTTPS semantic scheme.
    pub scheme: Scheme,
    /// TLS session metadata when EggServe knows the session.
    pub tls: Option<TlsInfo>,
}

impl ConnectionContext {
    /// Create a context from explicit parts.
    pub fn new(
        local_addr: Option<std::net::SocketAddr>,
        remote_addr: Option<std::net::SocketAddr>,
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

    /// Context for a real TCP connection with observed endpoints.
    pub fn for_tcp(
        local_addr: std::net::SocketAddr,
        remote_addr: std::net::SocketAddr,
        tls: Option<TlsInfo>,
    ) -> Self {
        let scheme = if tls.is_some() {
            Scheme::Https
        } else {
            Scheme::Http
        };
        Self {
            local_addr: Some(local_addr),
            remote_addr: Some(remote_addr),
            scheme,
            tls,
        }
    }

    /// Context for a real QUIC connection with observed UDP endpoints.
    ///
    /// QUIC uses UDP for its packet transport, but HTTP/3 still has HTTPS
    /// origin semantics and a completed TLS 1.3 session. Keeping this
    /// constructor separate from [`Self::for_tcp`] prevents transport
    /// metadata from being mislabelled at the canonical request boundary.
    pub fn for_quic(
        local_addr: std::net::SocketAddr,
        remote_addr: std::net::SocketAddr,
        tls: TlsInfo,
    ) -> Self {
        Self {
            local_addr: Some(local_addr),
            remote_addr: Some(remote_addr),
            scheme: Scheme::Https,
            tls: Some(tls),
        }
    }

    /// Context for a caller-owned non-socket byte stream.
    ///
    /// No socket endpoints are recorded and no addresses are fabricated.
    pub fn for_non_socket(scheme: Scheme, tls: Option<TlsInfo>) -> Self {
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

    /// Convert into the per-request [`crate::primitives::connection_info::ConnectionInfo`].
    pub fn connection_info(&self) -> crate::primitives::connection_info::ConnectionInfo {
        crate::primitives::connection_info::ConnectionInfo {
            local_addr: self.local_addr,
            remote_addr: self.remote_addr,
            scheme: self.scheme,
            tls: self.tls.clone(),
        }
    }
}

/// Per-connection graceful-shutdown token for caller-owned streams.
///
/// The caller retains ownership and calls [`ConnectionShutdown::shutdown`]
/// to request graceful connection shutdown independently of the TCP
/// `ServerHandle`. Dropping the token without shutdown is equivalent to
/// never requesting shutdown; in-flight work still observes hard timeouts,
/// protocol errors, and task cancellation via drop semantics. Permits and
/// producer tasks are released on driver exit regardless of outcome.
///
/// Shutdown is level-triggered: once [`ConnectionShutdown::shutdown`] has
/// been called, every current and future [`ConnectionShutdown::cancelled`]
/// waiter completes without polling. Signaling before the connection driver
/// registers its waiter is still observed promptly.
#[derive(Debug, Clone, Default)]
pub struct ConnectionShutdown {
    inner: Arc<ConnectionShutdownInner>,
}

#[derive(Debug, Default)]
struct ConnectionShutdownInner {
    notify: tokio::sync::Notify,
    flag: std::sync::atomic::AtomicBool,
}

impl ConnectionShutdown {
    /// Create a new un-signalled shutdown token.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ConnectionShutdownInner {
                notify: tokio::sync::Notify::new(),
                flag: std::sync::atomic::AtomicBool::new(false),
            }),
        }
    }

    /// Request graceful connection shutdown.
    ///
    /// Idempotent: repeated calls have no additional effect.
    pub fn shutdown(&self) {
        self.inner
            .flag
            .store(true, std::sync::atomic::Ordering::Release);
        self.inner.notify.notify_waiters();
    }

    /// Returns `true` once shutdown has been requested.
    pub fn is_shutdown(&self) -> bool {
        self.inner.flag.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Wait until shutdown is requested.
    ///
    /// Level-triggered: returns immediately if shutdown was already
    /// signaled, regardless of whether signaling happened before this
    /// waiter registered. Uses check/register/recheck so no interleaving
    /// can leave a waiter pending after the flag is true. The loop only
    /// defends against spurious wakeups and never busy-spins.
    pub async fn cancelled(&self) {
        loop {
            if self.is_shutdown() {
                return;
            }
            let notified = self.inner.notify.notified();
            tokio::pin!(notified);
            // Register the waiter before re-checking so a shutdown that
            // races between the first check and registration is still
            // observed.
            notified.as_mut().enable();
            if self.is_shutdown() {
                return;
            }
            notified.await;
            if self.is_shutdown() {
                return;
            }
        }
    }
}

/// Outcome of driving one HTTP/1 connection to completion.
///
/// Returned by [`serve_http1_connection`] for internal observability.
/// Every exit releases all permits and producer tasks; no outcome leaks
/// admission state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionOutcome {
    /// Clean EOF / keep-alive close with no error.
    Normal,
    /// Protocol or client error (malformed request, framing rejection,
    /// connection error, client disconnect).
    ClientError,
    /// Header-read timeout fired.
    HeaderTimeout,
    /// Keep-alive idle timeout fired: no in-flight request and no
    /// outstanding response body for the configured interval.
    IdleTimeout,
    /// Response write no-progress timeout fired: a response body was
    /// outstanding but no forward socket progress was made in time.
    WriteTimeout,
    /// Total connection lifetime expired.
    TotalTimeout,
    /// Graceful shutdown was requested (caller token or server signal).
    Shutdown,
    /// Unexpected internal failure.
    Internal,
}

impl ConnectionOutcome {
    /// Returns `true` for a clean close with no error or timeout.
    ///
    /// Keep-alive idle expiry counts as clean: the connection completed
    /// every response and turned over routinely. Write-stall expiry does
    /// not: a response was abandoned mid-transmission.
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::Normal | Self::Shutdown | Self::IdleTimeout)
    }
}

impl std::fmt::Display for ConnectionOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::ClientError => write!(f, "client-error"),
            Self::HeaderTimeout => write!(f, "header-timeout"),
            Self::IdleTimeout => write!(f, "idle-timeout"),
            Self::WriteTimeout => write!(f, "write-timeout"),
            Self::TotalTimeout => write!(f, "total-timeout"),
            Self::Shutdown => write!(f, "shutdown"),
            Self::Internal => write!(f, "internal"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::connection_info::Scheme;

    #[tokio::test]
    async fn shutdown_before_wait_completes_immediately() {
        let token = ConnectionShutdown::new();
        token.shutdown();
        assert!(token.is_shutdown());
        tokio::time::timeout(std::time::Duration::from_secs(1), token.cancelled())
            .await
            .expect("pre-signaled shutdown must be observed without polling");
    }

    #[tokio::test]
    async fn waiter_registered_before_shutdown_wakes() {
        let token = ConnectionShutdown::new();
        let waiter = {
            let token = token.clone();
            tokio::spawn(async move {
                tokio::time::timeout(std::time::Duration::from_secs(1), token.cancelled())
                    .await
                    .expect("waiter must wake on shutdown");
            })
        };
        // Give the waiter a chance to register before signaling.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        token.shutdown();
        waiter.await.unwrap();
        assert!(token.is_shutdown());
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let token = ConnectionShutdown::new();
        token.shutdown();
        token.shutdown();
        token.shutdown();
        assert!(token.is_shutdown());
        tokio::time::timeout(std::time::Duration::from_secs(1), token.cancelled())
            .await
            .expect("idempotent shutdown must still be observed");
        // Cloned tokens share the same persistent state.
        let cloned = token.clone();
        assert!(cloned.is_shutdown());
        tokio::time::timeout(std::time::Duration::from_secs(1), cloned.cancelled())
            .await
            .expect("cloned token must observe shutdown");
    }

    #[tokio::test]
    async fn shutdown_registration_race_never_loses_signal() {
        // Controlled race: signal shutdown concurrently with waiter
        // registration across many iterations. No interleaving may leave a
        // waiter pending after the flag is true. Bounded deadline only guards
        // against deadlock; success proves level-triggered semantics.
        for _ in 0..100 {
            let token = ConnectionShutdown::new();
            let waiter_token = token.clone();
            let waiter = tokio::spawn(async move {
                tokio::time::timeout(std::time::Duration::from_secs(1), waiter_token.cancelled())
                    .await
                    .expect("waiter must not miss concurrent shutdown");
            });
            token.shutdown();
            waiter.await.unwrap();
            assert!(token.is_shutdown());
        }
    }

    #[test]
    fn tcp_context_has_endpoints_anon_does_not() {
        let tcp = ConnectionContext::for_tcp(
            "127.0.0.1:8000".parse().unwrap(),
            "127.0.0.1:12345".parse().unwrap(),
            None,
        );
        assert!(tcp.has_socket_endpoints());
        assert!(tcp.socket_endpoints().is_some());
        let anon = ConnectionContext::for_non_socket(Scheme::Http, None);
        assert!(!anon.has_socket_endpoints());
        assert!(anon.socket_endpoints().is_none());
    }

    #[test]
    fn quic_context_preserves_udp_endpoints_and_https_semantics() {
        let context = ConnectionContext::for_quic(
            "127.0.0.1:443".parse().unwrap(),
            "127.0.0.1:54321".parse().unwrap(),
            TlsInfo {
                protocol_version: Some("TLSv1.3".into()),
                server_name: Some("example.test".into()),
            },
        );
        assert_eq!(context.scheme, Scheme::Https);
        assert_eq!(context.local_addr.unwrap().port(), 443);
        assert_eq!(context.remote_addr.unwrap().port(), 54321);
        assert_eq!(
            context.tls.unwrap().protocol_version.as_deref(),
            Some("TLSv1.3")
        );
    }

    #[test]
    fn outcome_cleanliness() {
        assert!(ConnectionOutcome::Normal.is_clean());
        assert!(ConnectionOutcome::Shutdown.is_clean());
        assert!(ConnectionOutcome::IdleTimeout.is_clean());
        assert!(!ConnectionOutcome::ClientError.is_clean());
        assert!(!ConnectionOutcome::WriteTimeout.is_clean());
        assert_eq!(
            format!("{}", ConnectionOutcome::TotalTimeout),
            "total-timeout"
        );
    }
}
