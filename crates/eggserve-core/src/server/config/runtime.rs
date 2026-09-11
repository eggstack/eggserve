//! Shared runtime configuration authority (Plan 206 Track E).
//!
//! Owns [`RuntimeConfig`] (single validation authority delegating shared
//! checks to `crate::runtime_limits` plus protocol-owned validators) and
//! the `from_shared_runtime` projection. Protocol modules own only their
//! fields/defaults/validation; no new knobs are added by the split.

use std::net::SocketAddr;
use std::time::Duration;

#[cfg(feature = "tls")]
use std::sync::Arc;

use super::http1::Http1Config;
#[cfg(feature = "http2")]
use super::http2::Http2Config;
#[cfg(feature = "http3")]
use super::http3::Http3Config;
use super::RuntimeConfigBuilder;

/// Transport-level runtime configuration.
///
/// All fields have safe defaults that match or strengthen the CLI defaults.
/// Configuration validation occurs at construction time via the builder.
///
/// # Examples
///
/// ```no_run
/// use eggserve_core::server::RuntimeConfig;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
///
/// let config = RuntimeConfig::builder()
///     .bind("127.0.0.1:8000".parse().unwrap())
///     .max_connections(128)
///     .build()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use]
pub struct RuntimeConfig {
    /// Address to bind the listener to.
    pub bind: SocketAddr,
    /// Maximum concurrent connections. Default: 64.
    pub max_connections: usize,
    /// Maximum concurrent file-stream responses. Default: 32.
    pub max_file_streams: usize,
    /// File streaming read chunk size. Default: 8 KiB.
    pub stream_chunk_size: usize,
    /// Timeout for reading request headers. Default: 10s.
    pub header_read_timeout: Duration,
    /// Timeout for a TLS handshake. Default: 10s.
    pub tls_handshake_timeout: Duration,
    /// Timeout wrapping the entire Hyper connection future. Default: 60s.
    ///
    /// This is a maximum connection lifetime: the budget is shared across
    /// all requests on a keep-alive connection, not reset per request. A
    /// connection idle for most of the budget has only the remainder left
    /// for its next request/response cycle.
    pub connection_total_timeout: Duration,
    /// Timeout for a single handler invocation. Default: 30s.
    ///
    /// `connection_total_timeout` is the hard ceiling: when the total
    /// connection lifetime expires first, the request is killed
    /// mid-flight regardless of this budget.
    pub handler_timeout: Duration,
    /// Timeout for reading the request body. Default: 30s.
    /// This is a total deadline for body consumption, not an idle timeout.
    ///
    /// `connection_total_timeout` is the hard ceiling: when the total
    /// connection lifetime expires first, body consumption is killed
    /// regardless of this budget.
    pub body_read_timeout: Duration,
    /// Graceful shutdown grace period. Default: 10s.
    pub graceful_shutdown_timeout: Duration,
    /// Final-boundary response privacy policy (Plan 165).
    ///
    /// Controls `Server` identification (suppressed by default), `Date`
    /// generation (system clock by default, EggServe is the sole authority;
    /// Hyper automatic `Date` is disabled), outbound header denylisting, and
    /// canonical error representation. See
    /// [`crate::server::response_policy::ResponsePolicy`].
    ///
    /// Migration from `server_header`: `None` (suppressed) is
    /// `response_policy.server_identification = None`; a fixed value is
    /// `Some(..)`. Use `RuntimeConfigBuilder::server_header(..)` or
    /// `RuntimeConfig::server_header_value()` for the common cases.
    pub response_policy: crate::server::response_policy::ResponsePolicy,
    /// TLS server configuration. If `Some`, connections are upgraded to TLS.
    /// Only available with the `tls` feature. Default: `None`.
    ///
    /// Plan 203: when `tls_reload_handle` is also `Some`, the reload handle
    /// snapshot wins for new handshakes (atomic reload); `tls_config` is the
    /// fallback/initial value. Single-identity compatibility callers keep
    /// using this field alone.
    #[cfg(feature = "tls")]
    pub tls_config: Option<Arc<rustls::ServerConfig>>,
    /// Atomic reload handle for TLS identity/trust state (Plan 203 Track F).
    ///
    /// When `Some`, new handshakes read the current snapshot atomically;
    /// established connections keep their session. Replace via
    /// [`crate::server::ServerHandle::replace_tls_config`] or
    /// [`crate::tls::TlsReloadHandle::replace`]. Failed builds never touch
    /// live state. Only available with the `tls` feature. Default: `None`.
    #[cfg(feature = "tls")]
    pub tls_reload_handle: Option<crate::tls::TlsReloadHandle>,
    /// Expose the verified peer DER chain in `TlsInfo` (Plan 203 Track D).
    ///
    /// Default `false` (opt-in, size-bounded to 8 certs × 64 KiB). When
    /// disabled, `peer_certificates_present`/`client_authenticated`/ALPN/SNI
    /// are still populated but `peer_certificate_chain` stays `None`.
    /// Only available with the `tls` feature.
    #[cfg(feature = "tls")]
    pub tls_expose_peer_chain: bool,
    /// Maximum allowed request body size in bytes. This is the hard ceiling
    /// that no service can exceed. Default: 0 (bodies rejected).
    pub max_request_body_bytes: u64,
    /// Maximum HTTP/1 parser/read buffer size in bytes. Set explicitly on
    /// Hyper so upgrades cannot silently widen parser memory. Minimum 8192
    /// (Hyper panics below). Default: 64 KiB.
    pub max_buf_size: usize,
    /// Maximum request header field count. Set explicitly on Hyper (which
    /// answers excess with 431). Default: 100.
    ///
    /// Note Hyper allocates header storage on the heap once a custom count
    /// is set, costing roughly 5% header-parse performance.
    pub max_headers: usize,
    /// Maximum aggregate post-parse request-header name+value bytes.
    /// Enforced before service invocation; excess fails with 431 without
    /// invoking the service. Default: 32 KiB.
    pub max_header_bytes: usize,
    /// Maximum request-target length in bytes. Enforced before service
    /// invocation; excess fails with 414. Default: 8192.
    pub max_request_target_bytes: usize,
    /// Maximum concurrent in-flight `Service::call()` executions,
    /// independent of idle keep-alive connections. Exhaustion produces a
    /// deterministic generic 503 without queuing unbounded work.
    /// Default: 64.
    pub max_in_flight_requests: usize,
    /// Keep-alive idle timeout. A connection with no in-flight request and
    /// no outstanding response body is gracefully closed after this much
    /// inactivity; the deadline resets on request/transport activity.
    /// Independent of `connection_total_timeout`, which remains the hard
    /// maximum connection lifetime. Default: 60s.
    pub keep_alive_idle_timeout: Duration,
    /// Maximum completed requests per connection. `None` disables the
    /// limit. When reached, the current response completes correctly with
    /// `Connection: close`. Every response counts, including HEAD and
    /// error responses and requests rejected before service invocation.
    /// Default: `None` (unlimited).
    pub max_requests_per_connection: Option<u64>,
    /// Response write no-progress timeout. A connection with an
    /// outstanding response body is closed after this much time with no
    /// forward socket-write progress; steady progress — however slow —
    /// never triggers it. Covers files, buffered bodies, and streams, on
    /// TCP, TLS, and caller-owned transports. Default: 30s.
    pub response_write_timeout: Duration,
    /// Maximum concurrent active tunnels (generic upgrade / CONNECT /
    /// Extended CONNECT duplex sessions). H1 tunnels hold the owning
    /// connection's lifetime as outer bound; H2/H3 tunnels are stream-scoped
    /// (siblings survive). Exhaustion fails new handshakes with 503.
    /// Default: 64.
    pub max_active_tunnels: usize,
    /// Trusted proxy policy (Plan 202).
    ///
    /// Defaults trust nothing: no peer is trusted (loopback included),
    /// Unix requires explicit `trust_unix`, PROXY parsing is disabled, and
    /// header-derived forwarding is disabled. When enabled, PROXY preambles
    /// are read before TLS/HTTP only from trusted peers, and `Forwarded` /
    /// `X-Forwarded-*` populate provenance-tagged effective fields without
    /// rewriting the canonical Host/target. H3 ignores this policy.
    pub trusted_proxy: crate::primitives::proxy::TrustedProxyConfig,
    /// HTTP/2 transport policy and resource limits. Present only in builds
    /// compiled with the `http2` feature.
    #[cfg(feature = "http2")]
    pub http2: Http2Config,
    /// HTTP/3/QUIC transport policy and resource limits.
    #[cfg(feature = "http3")]
    pub http3: Http3Config,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        use crate::runtime_limits as rl;
        Self {
            bind: "127.0.0.1:8000".parse().unwrap(),
            max_connections: rl::DEFAULT_MAX_CONNECTIONS,
            max_file_streams: rl::DEFAULT_MAX_FILE_STREAMS,
            stream_chunk_size: rl::DEFAULT_STREAM_CHUNK_SIZE,
            header_read_timeout: rl::DEFAULT_HEADER_READ_TIMEOUT,
            tls_handshake_timeout: rl::DEFAULT_TLS_HANDSHAKE_TIMEOUT,
            connection_total_timeout: rl::DEFAULT_CONNECTION_TOTAL_TIMEOUT,
            handler_timeout: rl::DEFAULT_HANDLER_TIMEOUT,
            body_read_timeout: rl::DEFAULT_BODY_READ_TIMEOUT,
            graceful_shutdown_timeout: rl::DEFAULT_GRACEFUL_SHUTDOWN_TIMEOUT,
            response_policy: crate::server::response_policy::ResponsePolicy::default(),
            #[cfg(feature = "tls")]
            tls_config: None,
            #[cfg(feature = "tls")]
            tls_reload_handle: None,
            #[cfg(feature = "tls")]
            tls_expose_peer_chain: false,
            max_request_body_bytes: rl::DEFAULT_MAX_REQUEST_BODY_BYTES,
            max_buf_size: rl::DEFAULT_MAX_BUF_SIZE,
            max_headers: rl::DEFAULT_MAX_HEADERS,
            max_header_bytes: rl::DEFAULT_MAX_HEADER_BYTES,
            max_request_target_bytes: rl::DEFAULT_MAX_REQUEST_TARGET_BYTES,
            max_in_flight_requests: rl::DEFAULT_MAX_IN_FLIGHT_REQUESTS,
            keep_alive_idle_timeout: rl::DEFAULT_KEEP_ALIVE_IDLE_TIMEOUT,
            max_requests_per_connection: None,
            response_write_timeout: rl::DEFAULT_RESPONSE_WRITE_TIMEOUT,
            max_active_tunnels: rl::DEFAULT_MAX_ACTIVE_TUNNELS,
            trusted_proxy: crate::primitives::proxy::TrustedProxyConfig::default(),
            #[cfg(feature = "http2")]
            http2: Http2Config::default(),
            #[cfg(feature = "http3")]
            http3: Http3Config::default(),
        }
    }
}

impl RuntimeConfig {
    /// Project the legacy-compatible fields into the HTTP/1 protocol config.
    pub(crate) fn http1_config(&self) -> Http1Config {
        Http1Config {
            max_buf_size: self.max_buf_size,
            max_headers: self.max_headers,
        }
    }

    /// Create a new builder with default values.
    pub fn builder() -> RuntimeConfigBuilder {
        RuntimeConfigBuilder {
            bind: None,
            max_connections: None,
            max_file_streams: None,
            stream_chunk_size: None,
            header_read_timeout: None,
            tls_handshake_timeout: None,
            connection_total_timeout: None,
            handler_timeout: None,
            body_read_timeout: None,
            graceful_shutdown_timeout: None,
            server_header: None,
            response_policy: None,
            date_policy: None,
            stripped_response_headers: None,
            error_policy: None,
            #[cfg(feature = "tls")]
            tls_config: None,
            #[cfg(feature = "tls")]
            tls_reload_handle: None,
            #[cfg(feature = "tls")]
            tls_expose_peer_chain: None,
            max_request_body_bytes: None,
            max_buf_size: None,
            max_headers: None,
            max_header_bytes: None,
            max_request_target_bytes: None,
            max_in_flight_requests: None,
            keep_alive_idle_timeout: None,
            max_requests_per_connection: None,
            response_write_timeout: None,
            max_active_tunnels: None,
            trusted_proxy: None,
            #[cfg(feature = "http2")]
            http2: None,
            #[cfg(feature = "http3")]
            http3: None,
        }
    }

    /// Returns the configured `Server` identification value, if any.
    ///
    /// `None` means suppressed (secure default). This is a convenience
    /// accessor for `response_policy.server_identification`.
    pub fn server_header_value(&self) -> Option<&str> {
        self.response_policy.server_identification.as_deref()
    }

    /// Validate a complete hand-constructed [`RuntimeConfig`] (Plan 179 Track C).
    ///
    /// Builder validation alone cannot protect `RuntimeConfig` because its
    /// fields are public: callers can hand-construct values that bypass the
    /// builder. Call this before semaphore/Hyper/runtime operations, or use
    /// [`RuntimeConfigBuilder::build`] / [`try_from_serve_config`] /
    /// [`crate::server::RuntimeState::try_new`] / [`crate::server::ServerBuilder::build`],
    /// which all enforce it.
    ///
    /// Checks the shared runtime kernel plus `ResponsePolicy`. Feature-gated
    /// TLS has no additional scalar invariants beyond presence; handshake
    /// timeout is part of the shared kernel.
    ///
    /// Services may lower request-body ceilings but cannot raise this runtime
    /// hard ceiling.
    pub fn validate(&self) -> Result<(), crate::server::errors::ServerError> {
        let shared = crate::runtime_limits::SharedRuntimeValues::from_runtime_config(self);
        let violations = shared.validate();
        if !violations.is_empty() {
            let msg = violations
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(crate::server::errors::ServerError::Config(msg));
        }
        self.response_policy.validate().map_err(|e| {
            crate::server::errors::ServerError::Config(format!("invalid response_policy: {e}"))
        })?;
        self.trusted_proxy.validate().map_err(|e| {
            crate::server::errors::ServerError::Config(format!("invalid trusted_proxy: {e}"))
        })?;
        #[cfg(feature = "http2")]
        if self.http2.enabled {
            self.http2.validate()?;
        }
        #[cfg(feature = "http3")]
        if self.http3.enabled {
            self.http3.validate()?;
        }
        Ok(())
    }

    /// Project shared runtime values into a [`RuntimeConfig`] (Plan 179 Track D).
    ///
    /// Single helper for the `ServeConfig` bridge so field-by-field
    /// correctness assumptions live in exactly one place. Static policy, root
    /// ownership, directory listing, MIME, and static response limits stay
    /// outside this type; response policy remains explicit so CLI/Python
    /// compatibility profiles do not silently inherit Rust-only privacy.
    pub(crate) fn from_shared_runtime(
        bind: SocketAddr,
        shared: &crate::runtime_limits::SharedRuntimeValues,
        response_policy: crate::server::response_policy::ResponsePolicy,
    ) -> Self {
        Self {
            bind,
            max_connections: shared.max_connections,
            max_file_streams: shared.max_file_streams,
            stream_chunk_size: shared.stream_chunk_size,
            header_read_timeout: shared.header_read_timeout,
            tls_handshake_timeout: shared.tls_handshake_timeout,
            connection_total_timeout: shared.connection_total_timeout,
            handler_timeout: shared.handler_timeout,
            body_read_timeout: shared.body_read_timeout,
            graceful_shutdown_timeout: shared.graceful_shutdown_timeout,
            response_policy,
            #[cfg(feature = "tls")]
            tls_config: None,
            #[cfg(feature = "tls")]
            tls_reload_handle: None,
            #[cfg(feature = "tls")]
            tls_expose_peer_chain: false,
            max_request_body_bytes: shared.max_request_body_bytes,
            max_buf_size: shared.max_buf_size,
            max_headers: shared.max_headers,
            max_header_bytes: shared.max_header_bytes,
            max_request_target_bytes: shared.max_request_target_bytes,
            max_in_flight_requests: shared.max_in_flight_requests,
            keep_alive_idle_timeout: shared.keep_alive_idle_timeout,
            max_requests_per_connection: shared.max_requests_per_connection,
            response_write_timeout: shared.response_write_timeout,
            max_active_tunnels: shared.max_active_tunnels,
            trusted_proxy: crate::primitives::proxy::TrustedProxyConfig::default(),
            #[cfg(feature = "http2")]
            http2: Http2Config::default(),
            #[cfg(feature = "http3")]
            http3: Http3Config::default(),
        }
    }
}
