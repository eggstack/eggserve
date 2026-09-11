//! Runtime configuration for the HTTP server.
//!
//! [`RuntimeConfig`] controls transport-level concerns (connection limits,
//! timeouts, keep-alive) independently of service-level concerns (filesystem
//! policy, root directory). The CLI and Python frontends translate their
//! respective configurations into a shared [`RuntimeConfig`] plus service
//! configuration.
//!
//! # Separation from service configuration
//!
//! Filesystem policy ([`StaticPolicy`]) and root directory belong to the
//! static service, not the runtime. This separation ensures the runtime
//! remains transport-agnostic and reusable for custom services.

use std::net::SocketAddr;
use std::time::Duration;

#[cfg(feature = "tls")]
use std::sync::Arc;

/// EggServe-owned HTTP/2 transport limits.
///
/// The values are sent to Hyper explicitly so an upgrade of Hyper or h2 does
/// not silently change the resource envelope. This configuration is only
/// available in builds with the `http2` feature; the Python compatibility
/// crate and H1-only builds therefore retain their existing surface.
#[cfg(feature = "http2")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Http2Config {
    /// Whether this runtime accepts cleartext prior-knowledge H2 and H2 on
    /// TLS connections whose ALPN negotiation selected `h2`.
    pub enabled: bool,
    /// Maximum concurrent request streams advertised per connection.
    pub max_concurrent_streams: u32,
    /// Maximum decoded header-list size accepted by Hyper/h2.
    pub max_header_list_size: u32,
    /// Maximum HTTP/2 frame size emitted by the server.
    pub max_frame_size: u32,
    /// Initial receive window for each H2 stream.
    pub initial_stream_window_size: u32,
    /// Initial receive window for the H2 connection.
    pub initial_connection_window_size: u32,
    /// Maximum pending outbound bytes per H2 stream.
    pub max_send_buf_size: usize,
    /// Maximum locally-reset streams retained before Hyper sends GOAWAY.
    pub max_local_error_reset_streams: usize,
    /// Maximum peer-reset streams pending acceptance before Hyper sends
    /// GOAWAY.
    pub max_pending_accept_reset_streams: usize,
    /// Whether Hyper may adapt flow-control windows. Disabled by default so
    /// the initial budgets above remain deterministic.
    pub adaptive_window: bool,
    /// Optional H2 PING interval. Disabled by default.
    pub keep_alive_interval: Option<Duration>,
    /// Timeout for an H2 keep-alive PING acknowledgement.
    pub keep_alive_timeout: Duration,
}

#[cfg(feature = "http2")]
impl Default for Http2Config {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent_streams: 100,
            max_header_list_size: 32 * 1024,
            max_frame_size: 16 * 1024,
            initial_stream_window_size: 256 * 1024,
            initial_connection_window_size: 1024 * 1024,
            max_send_buf_size: 256 * 1024,
            max_local_error_reset_streams: 1024,
            max_pending_accept_reset_streams: 20,
            adaptive_window: false,
            keep_alive_interval: None,
            keep_alive_timeout: Duration::from_secs(20),
        }
    }
}

#[cfg(feature = "http2")]
impl Http2Config {
    fn validate(&self) -> Result<(), crate::server::errors::ServerError> {
        let invalid = |field: &str, detail: &str| {
            crate::server::errors::ServerError::Config(format!("invalid http2.{field}: {detail}"))
        };
        if self.max_concurrent_streams == 0 {
            return Err(invalid(
                "max_concurrent_streams",
                "must be greater than zero",
            ));
        }
        if self.max_header_list_size < 1024 {
            return Err(invalid(
                "max_header_list_size",
                "must be at least 1024 bytes",
            ));
        }
        if self.max_frame_size < 16_384 || self.max_frame_size > 16_777_215 {
            return Err(invalid(
                "max_frame_size",
                "must be between 16384 and 16777215 bytes",
            ));
        }
        if self.initial_stream_window_size == 0 {
            return Err(invalid(
                "initial_stream_window_size",
                "must be greater than zero",
            ));
        }
        if self.initial_connection_window_size == 0 {
            return Err(invalid(
                "initial_connection_window_size",
                "must be greater than zero",
            ));
        }
        if self.max_send_buf_size == 0 || self.max_send_buf_size > u32::MAX as usize {
            return Err(invalid(
                "max_send_buf_size",
                "must be between 1 and u32::MAX bytes",
            ));
        }
        if self.max_local_error_reset_streams == 0 {
            return Err(invalid(
                "max_local_error_reset_streams",
                "must be greater than zero",
            ));
        }
        if self.max_pending_accept_reset_streams == 0 {
            return Err(invalid(
                "max_pending_accept_reset_streams",
                "must be greater than zero",
            ));
        }
        if self.keep_alive_timeout.is_zero() {
            return Err(invalid("keep_alive_timeout", "must be greater than zero"));
        }
        Ok(())
    }
}

/// EggServe-owned HTTP/3 and QUIC resource limits.
///
/// HTTP/3 is experimental and only available with the `http3` feature. The
/// defaults intentionally keep the product disabled until a caller supplies
/// a QUIC TLS identity through [`super::ServerBuilder::http3_identity`].
#[cfg(feature = "http3")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Http3Config {
    /// Whether the native server should bind a same-port UDP endpoint.
    pub enabled: bool,
    /// Maximum peer-created bidirectional request streams per connection.
    pub max_concurrent_bidi_streams: u32,
    /// Maximum peer-created unidirectional streams per connection.
    pub max_concurrent_uni_streams: u32,
    /// Per-stream receive window in bytes.
    pub stream_receive_window: u64,
    /// Per-connection receive window in bytes.
    pub connection_receive_window: u64,
    /// QUIC send window in bytes.
    pub send_window: u64,
    /// Maximum idle time for a QUIC connection.
    pub max_idle_timeout: Duration,
    /// Maximum number of handshakes admitted to application tasks at once.
    pub max_pending_handshakes: usize,
    /// Maximum decoded HTTP/3 field section size.
    pub max_field_section_size: u64,
    /// Maximum number of bytes buffered by one outgoing H3 response stream.
    pub max_send_buf_size: usize,
    /// Whether Quinn may issue stateless retry tokens.
    pub stateless_retry: bool,
    /// Whether TCP/H2 responses advertise this server's active H3 endpoint
    /// with a runtime-owned `Alt-Svc` field.
    pub advertise_alt_svc: bool,
}

#[cfg(feature = "http3")]
impl Default for Http3Config {
    fn default() -> Self {
        Self {
            enabled: false,
            max_concurrent_bidi_streams: 100,
            // Three unidirectional streams are needed for H3 control/QPACK;
            // leave additional bounded room for peer protocol state.
            max_concurrent_uni_streams: 16,
            stream_receive_window: 256 * 1024,
            connection_receive_window: 4 * 1024 * 1024,
            send_window: 4 * 1024 * 1024,
            max_idle_timeout: Duration::from_secs(60),
            max_pending_handshakes: 64,
            max_field_section_size: 32 * 1024,
            max_send_buf_size: 256 * 1024,
            stateless_retry: false,
            advertise_alt_svc: false,
        }
    }
}

#[cfg(feature = "http3")]
impl Http3Config {
    pub(crate) fn validate(&self) -> Result<(), crate::server::errors::ServerError> {
        let invalid = |field: &str, detail: &str| {
            crate::server::errors::ServerError::Config(format!("invalid http3.{field}: {detail}"))
        };
        if self.max_concurrent_bidi_streams == 0 {
            return Err(invalid(
                "max_concurrent_bidi_streams",
                "must be greater than zero",
            ));
        }
        if self.max_concurrent_uni_streams < 3 {
            return Err(invalid(
                "max_concurrent_uni_streams",
                "must leave room for H3 control and QPACK streams",
            ));
        }
        if self.stream_receive_window == 0 || self.connection_receive_window == 0 {
            return Err(invalid("receive_window", "must be greater than zero"));
        }
        if self.connection_receive_window < self.stream_receive_window {
            return Err(invalid(
                "connection_receive_window",
                "must be at least stream_receive_window",
            ));
        }
        if self.send_window == 0 || self.max_send_buf_size == 0 {
            return Err(invalid("send_window", "must be greater than zero"));
        }
        if self.max_idle_timeout.is_zero() {
            return Err(invalid("max_idle_timeout", "must be greater than zero"));
        }
        if self.max_pending_handshakes == 0 {
            return Err(invalid(
                "max_pending_handshakes",
                "must be greater than zero",
            ));
        }
        if self.max_field_section_size < 1024 {
            return Err(invalid(
                "max_field_section_size",
                "must be at least 1024 bytes",
            ));
        }
        Ok(())
    }
}

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

/// HTTP/1-only parser and framing settings projected from the compatibility
/// fields on [`RuntimeConfig`]. Keeping this internal projection lets future
/// protocol configs own their knobs without duplicating Plan 179 defaults or
/// changing every existing `RuntimeConfig` literal before the 0.2 transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Http1Config {
    pub(crate) max_buf_size: usize,
    pub(crate) max_headers: usize,
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

/// Builder for [`RuntimeConfig`].
#[derive(Debug, Default)]
#[must_use]
pub struct RuntimeConfigBuilder {
    bind: Option<SocketAddr>,
    max_connections: Option<usize>,
    max_file_streams: Option<usize>,
    stream_chunk_size: Option<usize>,
    header_read_timeout: Option<Duration>,
    tls_handshake_timeout: Option<Duration>,
    connection_total_timeout: Option<Duration>,
    handler_timeout: Option<Duration>,
    body_read_timeout: Option<Duration>,
    graceful_shutdown_timeout: Option<Duration>,
    server_header: Option<String>,
    response_policy: Option<crate::server::response_policy::ResponsePolicy>,
    date_policy: Option<crate::server::response_policy::DatePolicy>,
    stripped_response_headers: Option<Vec<String>>,
    error_policy: Option<crate::policy::ErrorRepresentationPolicy>,
    #[cfg(feature = "tls")]
    tls_config: Option<Arc<rustls::ServerConfig>>,
    #[cfg(feature = "tls")]
    tls_reload_handle: Option<crate::tls::TlsReloadHandle>,
    #[cfg(feature = "tls")]
    tls_expose_peer_chain: Option<bool>,
    max_request_body_bytes: Option<u64>,
    max_buf_size: Option<usize>,
    max_headers: Option<usize>,
    max_header_bytes: Option<usize>,
    max_request_target_bytes: Option<usize>,
    max_in_flight_requests: Option<usize>,
    keep_alive_idle_timeout: Option<Duration>,
    max_requests_per_connection: Option<Option<u64>>,
    response_write_timeout: Option<Duration>,
    max_active_tunnels: Option<usize>,
    trusted_proxy: Option<crate::primitives::proxy::TrustedProxyConfig>,
    #[cfg(feature = "http2")]
    http2: Option<Http2Config>,
    #[cfg(feature = "http3")]
    http3: Option<Http3Config>,
}

impl RuntimeConfigBuilder {
    /// Set the bind address.
    pub fn bind(mut self, addr: SocketAddr) -> Self {
        self.bind = Some(addr);
        self
    }

    /// Set the maximum number of concurrent connections.
    ///
    /// Must be > 0. Default: 64.
    pub fn max_connections(mut self, max: usize) -> Self {
        self.max_connections = Some(max);
        self
    }

    /// Set the maximum number of concurrent file-stream responses.
    ///
    /// Must be > 0. Default: 32.
    pub fn max_file_streams(mut self, max: usize) -> Self {
        self.max_file_streams = Some(max);
        self
    }

    /// Set the file streaming read chunk size.
    ///
    /// Must be between 64 bytes and 1 MiB. Default: 8 KiB.
    pub fn stream_chunk_size(mut self, size: usize) -> Self {
        self.stream_chunk_size = Some(size);
        self
    }

    /// Set the header-read timeout.
    pub fn header_read_timeout(mut self, timeout: Duration) -> Self {
        self.header_read_timeout = Some(timeout);
        self
    }

    /// Set the TLS handshake timeout.
    pub fn tls_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.tls_handshake_timeout = Some(timeout);
        self
    }

    /// Set the connection total timeout.
    pub fn connection_total_timeout(mut self, timeout: Duration) -> Self {
        self.connection_total_timeout = Some(timeout);
        self
    }

    /// Set the handler invocation timeout.
    ///
    /// Must be <= `connection_total_timeout` when both are set explicitly;
    /// the total connection lifetime is the hard ceiling.
    pub fn handler_timeout(mut self, timeout: Duration) -> Self {
        self.handler_timeout = Some(timeout);
        self
    }

    /// Set the body read timeout.
    ///
    /// This is a total deadline for body consumption, not an idle timeout.
    /// Must be <= `connection_total_timeout` when both are set explicitly;
    /// the total connection lifetime is the hard ceiling.
    pub fn body_read_timeout(mut self, timeout: Duration) -> Self {
        self.body_read_timeout = Some(timeout);
        self
    }

    /// Set the graceful shutdown grace period.
    pub fn graceful_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.graceful_shutdown_timeout = Some(timeout);
        self
    }

    /// Set the server identification header value.
    ///
    /// If set, added as `Server` header on all responses. Default is
    /// suppressed (`None`). Never emits implementation versions automatically.
    pub fn server_header(mut self, header: String) -> Self {
        self.server_header = Some(header);
        self
    }

    /// Set the complete final-boundary response privacy policy.
    ///
    /// Individual `date_policy` / `stripped_response_headers` / `error_policy`
    /// / `server_header` settings override the corresponding fields of this
    /// policy when both are supplied.
    pub fn response_policy(
        mut self,
        policy: crate::server::response_policy::ResponsePolicy,
    ) -> Self {
        self.response_policy = Some(policy);
        self
    }

    /// Set the `Date` generation policy.
    ///
    /// Default is [`crate::server::response_policy::DatePolicy::SystemClock`].
    /// Use `Suppress` only as an explicit RFC 9110 tradeoff, or `Custom`
    /// with a trusted time source for anonymity-sensitive origins.
    pub fn date_policy(mut self, policy: crate::server::response_policy::DatePolicy) -> Self {
        self.date_policy = Some(policy);
        self
    }

    /// Set the validated denylist of outbound response header names.
    ///
    /// Applied after service construction; framing/hop-by-hop headers and
    /// `date` cannot be denylisted (see
    /// [`crate::server::response_policy::validate_stripped_header_name`]).
    pub fn stripped_response_headers(mut self, headers: Vec<String>) -> Self {
        self.stripped_response_headers = Some(headers);
        self
    }

    /// Set the canonical runtime-error representation.
    ///
    /// `Minimal` (default) emits fixed generic plain-text bodies;
    /// `Empty` emits no body bytes for runtime-generated errors. Application
    /// `Ok` bodies are never rewritten.
    pub fn error_policy(mut self, policy: crate::policy::ErrorRepresentationPolicy) -> Self {
        self.error_policy = Some(policy);
        self
    }

    /// Set the TLS server configuration.
    #[cfg(feature = "tls")]
    pub fn tls_config(mut self, config: Arc<rustls::ServerConfig>) -> Self {
        self.tls_config = Some(config);
        self
    }

    /// Set the atomic reload handle for TLS identity/trust (Plan 203 Track F).
    ///
    /// When `Some`, new handshakes read the current snapshot; `tls_config`
    /// is the fallback. Share the same handle with `ServerHandle` for live
    /// reload (see `TlsReloadHandle::replace`).
    #[cfg(feature = "tls")]
    pub fn tls_reload_handle(mut self, handle: crate::tls::TlsReloadHandle) -> Self {
        self.tls_reload_handle = Some(handle);
        self
    }

    /// Opt into bounded peer DER chain exposure in `TlsInfo` (Plan 203 D).
    ///
    /// Default `false`. When enabled, verified chains up to 8 × 64 KiB are
    /// cloned into `TlsInfo::peer_certificate_chain`; otherwise only
    /// presence/authenticated flags are populated.
    #[cfg(feature = "tls")]
    pub fn tls_expose_peer_chain(mut self, expose: bool) -> Self {
        self.tls_expose_peer_chain = Some(expose);
        self
    }

    /// Set the maximum request body size in bytes.
    ///
    /// This is the hard ceiling that no service can exceed. Default: 0
    /// (bodies rejected). Set to a positive value to allow request bodies.
    pub fn max_request_body_bytes(mut self, max: u64) -> Self {
        self.max_request_body_bytes = Some(max);
        self
    }

    /// Set the HTTP/1 parser/read buffer ceiling in bytes.
    ///
    /// Must be between 8192 (Hyper minimum) and 4 MiB. Default: 64 KiB.
    pub fn max_buf_size(mut self, max: usize) -> Self {
        self.max_buf_size = Some(max);
        self
    }

    /// Set the maximum request header field count.
    ///
    /// Must be between 1 and 10_000. Default: 100 (Hyper's default, pinned
    /// explicitly). Hyper answers excess with 431.
    pub fn max_headers(mut self, max: usize) -> Self {
        self.max_headers = Some(max);
        self
    }

    /// Set the aggregate post-parse request-header ceiling in name+value
    /// bytes. Must be between 1 KiB and 1 MiB. Default: 32 KiB.
    pub fn max_header_bytes(mut self, max: usize) -> Self {
        self.max_header_bytes = Some(max);
        self
    }

    /// Set the maximum request-target length in bytes. Must be between 128
    /// and 64 KiB. Default: 8192.
    pub fn max_request_target_bytes(mut self, max: usize) -> Self {
        self.max_request_target_bytes = Some(max);
        self
    }

    /// Set the maximum concurrent in-flight service executions.
    ///
    /// Must be > 0. Default: 64.
    pub fn max_in_flight_requests(mut self, max: usize) -> Self {
        self.max_in_flight_requests = Some(max);
        self
    }

    /// Set the keep-alive idle timeout.
    ///
    /// Independent of `connection_total_timeout`: this deadline resets on
    /// request/transport activity, while the total lifetime never resets.
    pub fn keep_alive_idle_timeout(mut self, timeout: Duration) -> Self {
        self.keep_alive_idle_timeout = Some(timeout);
        self
    }

    /// Set the maximum completed requests per connection.
    ///
    /// Pass `None` for unlimited (default). `Some(0)` is rejected.
    pub fn max_requests_per_connection(mut self, max: Option<u64>) -> Self {
        self.max_requests_per_connection = Some(max);
        self
    }

    /// Set the response write no-progress timeout.
    ///
    /// Fires only after the configured interval with zero forward socket
    /// progress while a response body is outstanding.
    pub fn response_write_timeout(mut self, timeout: Duration) -> Self {
        self.response_write_timeout = Some(timeout);
        self
    }

    /// Set the maximum concurrent active tunnels.
    ///
    /// Must be > 0. Default: 64. Exhaustion fails new handshakes with 503.
    pub fn max_active_tunnels(mut self, max: usize) -> Self {
        self.max_active_tunnels = Some(max);
        self
    }

    /// Set the complete trusted-proxy policy (Plan 202).
    ///
    /// Defaults trust nothing. Use this for explicit `TrustedProxyConfig`
    /// values; the convenience setters below mutate the same policy.
    pub fn trusted_proxy(mut self, config: crate::primitives::proxy::TrustedProxyConfig) -> Self {
        self.trusted_proxy = Some(config);
        self
    }

    fn trusted_proxy_mut(&mut self) -> &mut crate::primitives::proxy::TrustedProxyConfig {
        if self.trusted_proxy.is_none() {
            self.trusted_proxy = Some(crate::primitives::proxy::TrustedProxyConfig::default());
        }
        self.trusted_proxy.as_mut().expect("just initialized")
    }

    /// Trust one immediate peer (`IP` or `IP/prefix`, no DNS).
    ///
    /// Repeatable via chaining. Loopback must be listed explicitly; it is
    /// never implicitly trusted. Invalid entries fail at [`Self::build`].
    pub fn trusted_proxy_peer(mut self, prefix: crate::primitives::proxy::IpPrefix) -> Self {
        self.trusted_proxy_mut().peers.push(prefix);
        self
    }

    /// Trust Unix-domain listeners for header-derived forwarding.
    ///
    /// Never implicit. PROXY preambles are not read from Unix streams.
    pub fn trust_unix_local(mut self, trust: bool) -> Self {
        self.trusted_proxy_mut().trust_unix = trust;
        self
    }

    /// Enable or disable PROXY protocol preamble parsing (default disabled).
    ///
    /// When enabled, only explicitly trusted peers may send a preamble;
    /// all other peers close before TLS/HTTP. Disabled listeners interpret
    /// bytes normally with no auto-detection.
    pub fn proxy_protocol_enabled(mut self, enabled: bool) -> Self {
        self.trusted_proxy_mut().proxy_protocol.enabled = enabled;
        self
    }

    /// Set the PROXY preamble read timeout (default 5s, max 60s).
    pub fn proxy_protocol_timeout(mut self, timeout: Duration) -> Self {
        self.trusted_proxy_mut().proxy_protocol.timeout = timeout;
        self
    }

    /// Honor the standardized `Forwarded` header from trusted peers.
    pub fn forwarded_standard(mut self, enabled: bool) -> Self {
        self.trusted_proxy_mut().forwarded.standard_enabled = enabled;
        self
    }

    /// Honor legacy `X-Forwarded-*` headers from trusted peers.
    pub fn forwarded_legacy(mut self, enabled: bool) -> Self {
        self.trusted_proxy_mut().forwarded.legacy_enabled = enabled;
        self
    }

    /// Set the HTTP/2 transport policy and resource limits.
    #[cfg(feature = "http2")]
    pub fn http2(mut self, config: Http2Config) -> Self {
        self.http2 = Some(config);
        self
    }

    /// Set the HTTP/3 and QUIC transport policy and resource limits.
    #[cfg(feature = "http3")]
    pub fn http3(mut self, config: Http3Config) -> Self {
        self.http3 = Some(config);
        self
    }

    /// Build the runtime configuration.
    ///
    /// Shared runtime checks delegate to the canonical Plan 179 kernel; the
    /// resulting error identifies the invalid field/constraint. Returns an
    /// error if any shared value or the composed `ResponsePolicy` is invalid.
    pub fn build(self) -> Result<RuntimeConfig, crate::server::errors::ServerError> {
        use crate::runtime_limits as rl;
        let shared = rl::SharedRuntimeValues {
            max_connections: self.max_connections.unwrap_or(rl::DEFAULT_MAX_CONNECTIONS),
            max_file_streams: self
                .max_file_streams
                .unwrap_or(rl::DEFAULT_MAX_FILE_STREAMS),
            max_request_body_bytes: self
                .max_request_body_bytes
                .unwrap_or(rl::DEFAULT_MAX_REQUEST_BODY_BYTES),
            header_read_timeout: self
                .header_read_timeout
                .unwrap_or(rl::DEFAULT_HEADER_READ_TIMEOUT),
            tls_handshake_timeout: self
                .tls_handshake_timeout
                .unwrap_or(rl::DEFAULT_TLS_HANDSHAKE_TIMEOUT),
            connection_total_timeout: self
                .connection_total_timeout
                .unwrap_or(rl::DEFAULT_CONNECTION_TOTAL_TIMEOUT),
            handler_timeout: self.handler_timeout.unwrap_or(rl::DEFAULT_HANDLER_TIMEOUT),
            body_read_timeout: self
                .body_read_timeout
                .unwrap_or(rl::DEFAULT_BODY_READ_TIMEOUT),
            graceful_shutdown_timeout: self
                .graceful_shutdown_timeout
                .unwrap_or(rl::DEFAULT_GRACEFUL_SHUTDOWN_TIMEOUT),
            stream_chunk_size: self
                .stream_chunk_size
                .unwrap_or(rl::DEFAULT_STREAM_CHUNK_SIZE),
            max_buf_size: self.max_buf_size.unwrap_or(rl::DEFAULT_MAX_BUF_SIZE),
            max_headers: self.max_headers.unwrap_or(rl::DEFAULT_MAX_HEADERS),
            max_header_bytes: self
                .max_header_bytes
                .unwrap_or(rl::DEFAULT_MAX_HEADER_BYTES),
            max_request_target_bytes: self
                .max_request_target_bytes
                .unwrap_or(rl::DEFAULT_MAX_REQUEST_TARGET_BYTES),
            max_in_flight_requests: self
                .max_in_flight_requests
                .unwrap_or(rl::DEFAULT_MAX_IN_FLIGHT_REQUESTS),
            keep_alive_idle_timeout: self
                .keep_alive_idle_timeout
                .unwrap_or(rl::DEFAULT_KEEP_ALIVE_IDLE_TIMEOUT),
            max_requests_per_connection: self.max_requests_per_connection.unwrap_or(None),
            response_write_timeout: self
                .response_write_timeout
                .unwrap_or(rl::DEFAULT_RESPONSE_WRITE_TIMEOUT),
            max_active_tunnels: self
                .max_active_tunnels
                .unwrap_or(rl::DEFAULT_MAX_ACTIVE_TUNNELS),
        };
        let violations = shared.validate();
        if !violations.is_empty() {
            let msg = violations
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(crate::server::errors::ServerError::Config(msg));
        }
        let mut response_policy = self.response_policy.unwrap_or_default();
        if let Some(server_header) = self.server_header {
            response_policy.server_identification = Some(server_header);
        }
        if let Some(date_policy) = self.date_policy {
            response_policy.date_policy = date_policy;
        }
        if let Some(stripped) = self.stripped_response_headers {
            response_policy.stripped_response_headers = stripped;
        }
        if let Some(error_policy) = self.error_policy {
            response_policy.error_policy = error_policy;
        }
        response_policy.validate().map_err(|e| {
            crate::server::errors::ServerError::Config(format!("invalid response_policy: {e}"))
        })?;
        let trusted_proxy = self.trusted_proxy.unwrap_or_default();
        trusted_proxy.validate().map_err(|e| {
            crate::server::errors::ServerError::Config(format!("invalid trusted_proxy: {e}"))
        })?;
        #[cfg(feature = "http2")]
        let http2 = self.http2.unwrap_or_default();
        #[cfg(feature = "http2")]
        if http2.enabled {
            http2.validate()?;
        }
        #[cfg(feature = "http3")]
        let http3 = self.http3.unwrap_or_default();
        #[cfg(feature = "http3")]
        if http3.enabled {
            http3.validate()?;
        }
        Ok(RuntimeConfig {
            bind: self
                .bind
                .unwrap_or_else(|| "127.0.0.1:8000".parse().unwrap()),
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
            tls_config: self.tls_config,
            #[cfg(feature = "tls")]
            tls_reload_handle: self.tls_reload_handle,
            #[cfg(feature = "tls")]
            tls_expose_peer_chain: self.tls_expose_peer_chain.unwrap_or(false),
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
            trusted_proxy,
            #[cfg(feature = "http2")]
            http2,
            #[cfg(feature = "http3")]
            http3,
        })
    }
}

/// Try to convert a [`crate::config::ServeConfig`] into a [`RuntimeConfig`].
///
/// This bridges the CLI/Python configuration model into the runtime model.
/// Filesystem policy and root directory are NOT transferred — they belong
/// to the service, not the runtime.
///
/// Returns an error if the `Limits` contain invalid values (zero concurrency,
/// zero timeouts).
pub fn try_from_serve_config(
    config: &crate::config::ServeConfig,
) -> Result<RuntimeConfig, crate::server::errors::ServerError> {
    config.limits.validate().map_err(|errs| {
        crate::server::errors::ServerError::Config(
            errs.iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    // CLI/Python keep standards-compliant defaults: Server suppressed,
    // system-clock Date, no denylist, minimal errors. Advanced privacy
    // policy is Rust-only; the stdlib facade must not silently diverge.
    // `ServeConfig.error_policy` (for static errors) is transferred so
    // `serve_config()` static errors share the runtime error profile.
    // Static policy, root, listing budgets, and MIME stay with the service.
    let response_policy = crate::server::response_policy::ResponsePolicy {
        error_policy: config.error_policy,
        ..Default::default()
    };
    let shared = crate::runtime_limits::SharedRuntimeValues::from_limits(&config.limits);
    // `Limits::validate` already passed, so this projection is infallible;
    // route through the single shared helper rather than reproducing fields.
    Ok(RuntimeConfig::from_shared_runtime(
        config.bind,
        &shared,
        response_policy,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_runtime_config() {
        let config = RuntimeConfig::default();
        assert!(config.bind.ip().is_loopback());
        assert_eq!(config.bind.port(), 8000);
        assert_eq!(config.max_connections, 64);
        assert_eq!(config.max_file_streams, 32);
        assert_eq!(
            config.stream_chunk_size,
            crate::limits::DEFAULT_STREAM_CHUNK_SIZE
        );
        assert_eq!(config.header_read_timeout, Duration::from_secs(10));
        assert_eq!(config.tls_handshake_timeout, Duration::from_secs(10));
        assert_eq!(config.connection_total_timeout, Duration::from_secs(60));
        assert_eq!(config.handler_timeout, Duration::from_secs(30));
        assert_eq!(config.body_read_timeout, Duration::from_secs(30));
        assert_eq!(config.graceful_shutdown_timeout, Duration::from_secs(10));
        assert_eq!(config.response_policy.server_identification, None);
        assert_eq!(config.server_header_value(), None);
        assert_eq!(config.max_request_body_bytes, 0);
        assert_eq!(config.max_buf_size, crate::limits::DEFAULT_MAX_BUF_SIZE);
        assert_eq!(config.max_headers, crate::limits::DEFAULT_MAX_HEADERS);
        assert_eq!(
            config.max_header_bytes,
            crate::limits::DEFAULT_MAX_HEADER_BYTES
        );
        assert_eq!(
            config.max_request_target_bytes,
            crate::limits::DEFAULT_MAX_REQUEST_TARGET_BYTES
        );
        assert_eq!(
            config.max_in_flight_requests,
            crate::limits::DEFAULT_MAX_IN_FLIGHT_REQUESTS
        );
        assert_eq!(config.keep_alive_idle_timeout, Duration::from_secs(60));
        assert_eq!(config.max_requests_per_connection, None);
        assert_eq!(config.response_write_timeout, Duration::from_secs(30));
        #[cfg(feature = "http2")]
        {
            assert_eq!(config.http2.max_concurrent_streams, 100);
            assert_eq!(config.http2.max_header_list_size, 32 * 1024);
            assert_eq!(config.http2.max_frame_size, 16 * 1024);
            assert!(config.http2.validate().is_ok());
        }
    }

    #[test]
    fn builder_overrides() {
        let config = RuntimeConfig::builder()
            .bind("0.0.0.0:9000".parse().unwrap())
            .max_connections(128)
            .max_file_streams(64)
            .stream_chunk_size(64)
            .header_read_timeout(Duration::from_secs(5))
            .tls_handshake_timeout(Duration::from_secs(7))
            .connection_total_timeout(Duration::from_secs(30))
            .handler_timeout(Duration::from_secs(15))
            .body_read_timeout(Duration::from_secs(20))
            .graceful_shutdown_timeout(Duration::from_secs(5))
            .server_header("eggserve/0.1".into())
            .max_request_body_bytes(1024 * 1024)
            .max_buf_size(8192)
            .max_headers(50)
            .max_header_bytes(4096)
            .max_request_target_bytes(2048)
            .max_in_flight_requests(16)
            .keep_alive_idle_timeout(Duration::from_secs(25))
            .max_requests_per_connection(Some(100))
            .response_write_timeout(Duration::from_secs(12))
            .build()
            .unwrap();
        assert_eq!(config.bind.port(), 9000);
        assert_eq!(config.max_connections, 128);
        assert_eq!(config.max_file_streams, 64);
        assert_eq!(config.stream_chunk_size, 64);
        assert_eq!(config.header_read_timeout, Duration::from_secs(5));
        assert_eq!(config.tls_handshake_timeout, Duration::from_secs(7));
        assert_eq!(config.connection_total_timeout, Duration::from_secs(30));
        assert_eq!(config.handler_timeout, Duration::from_secs(15));
        assert_eq!(config.body_read_timeout, Duration::from_secs(20));
        assert_eq!(config.graceful_shutdown_timeout, Duration::from_secs(5));
        assert_eq!(
            config.response_policy.server_identification.as_deref(),
            Some("eggserve/0.1")
        );
        assert_eq!(config.server_header_value(), Some("eggserve/0.1"));
        assert_eq!(config.max_request_body_bytes, 1024 * 1024);
        assert_eq!(config.max_buf_size, 8192);
        assert_eq!(config.max_headers, 50);
        assert_eq!(config.max_header_bytes, 4096);
        assert_eq!(config.max_request_target_bytes, 2048);
        assert_eq!(config.max_in_flight_requests, 16);
        assert_eq!(config.keep_alive_idle_timeout, Duration::from_secs(25));
        assert_eq!(config.max_requests_per_connection, Some(100));
        assert_eq!(config.response_write_timeout, Duration::from_secs(12));
    }

    #[test]
    fn invalid_server_header_is_rejected() {
        let err = RuntimeConfig::builder()
            .server_header("bad\r\nvalue".into())
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("invalid server"));
    }

    #[test]
    fn zero_tls_handshake_timeout_is_rejected() {
        let err = RuntimeConfig::builder()
            .tls_handshake_timeout(Duration::ZERO)
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("tls_handshake_timeout"));
    }

    #[test]
    fn invalid_stream_chunk_size_is_rejected() {
        let err = RuntimeConfig::builder()
            .stream_chunk_size(63)
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("stream_chunk_size"));
    }

    #[test]
    fn excessive_max_request_body_bytes_is_rejected() {
        let err = RuntimeConfig::builder()
            .max_request_body_bytes(u64::MAX)
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("max_request_body_bytes"));
    }

    #[cfg(feature = "http2")]
    #[test]
    fn invalid_http2_transport_limits_are_rejected() {
        let h2 = Http2Config {
            max_concurrent_streams: 0,
            ..Http2Config::default()
        };
        assert!(RuntimeConfig::builder().http2(h2).build().is_err());

        let h2 = Http2Config {
            max_frame_size: 1024,
            ..Http2Config::default()
        };
        assert!(RuntimeConfig::builder().http2(h2).build().is_err());

        let h2 = Http2Config {
            keep_alive_timeout: Duration::ZERO,
            ..Http2Config::default()
        };
        assert!(RuntimeConfig::builder().http2(h2).build().is_err());
    }

    #[test]
    fn from_serve_config() {
        let serve_config = crate::config::ServeConfig::default();
        let runtime = try_from_serve_config(&serve_config).unwrap();
        assert_eq!(runtime.bind, serve_config.bind);
        assert_eq!(runtime.max_connections, serve_config.limits.max_connections);
        assert_eq!(
            runtime.max_file_streams,
            serve_config.limits.max_file_streams
        );
        assert_eq!(
            runtime.stream_chunk_size,
            serve_config.limits.stream_chunk_size
        );
        assert_eq!(
            runtime.tls_handshake_timeout,
            serve_config.limits.tls_handshake_timeout
        );
        assert_eq!(
            runtime.max_request_body_bytes,
            serve_config.limits.max_request_body_bytes
        );
    }

    #[test]
    fn zero_connections_returns_error() {
        let result = RuntimeConfig::builder().max_connections(0).build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("max_connections must be > 0"));
    }

    #[test]
    fn zero_file_streams_returns_error() {
        let result = RuntimeConfig::builder().max_file_streams(0).build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("max_file_streams must be > 0"));
    }

    #[test]
    fn zero_header_read_timeout_returns_error() {
        let result = RuntimeConfig::builder()
            .header_read_timeout(Duration::ZERO)
            .build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("header_read_timeout must be > 0"));
    }

    #[test]
    fn zero_connection_total_timeout_returns_error() {
        let result = RuntimeConfig::builder()
            .connection_total_timeout(Duration::ZERO)
            .build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err
            .to_string()
            .contains("connection_total_timeout must be > 0"));
    }

    #[test]
    fn header_timeout_cannot_exceed_connection_total_timeout() {
        let result = RuntimeConfig::builder()
            .header_read_timeout(Duration::from_secs(2))
            .connection_total_timeout(Duration::from_secs(1))
            .build();
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("header_read_timeout must be <= connection_total_timeout"));
    }

    #[test]
    fn handler_timeout_cannot_exceed_connection_total_timeout() {
        let result = RuntimeConfig::builder()
            .handler_timeout(Duration::from_secs(60))
            .connection_total_timeout(Duration::from_secs(30))
            .build();
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("handler_timeout must be <= connection_total_timeout"));
    }

    #[test]
    fn body_read_timeout_cannot_exceed_connection_total_timeout() {
        let result = RuntimeConfig::builder()
            .body_read_timeout(Duration::from_secs(60))
            .connection_total_timeout(Duration::from_secs(30))
            .build();
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("body_read_timeout must be <= connection_total_timeout"));
    }

    #[test]
    fn zero_handler_timeout_returns_error() {
        let result = RuntimeConfig::builder()
            .handler_timeout(Duration::ZERO)
            .build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("handler_timeout must be > 0"));
    }

    #[test]
    fn zero_body_read_timeout_returns_error() {
        let result = RuntimeConfig::builder()
            .body_read_timeout(Duration::ZERO)
            .build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("body_read_timeout must be > 0"));
    }

    #[test]
    fn zero_graceful_shutdown_timeout_returns_error() {
        let result = RuntimeConfig::builder()
            .graceful_shutdown_timeout(Duration::ZERO)
            .build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err
            .to_string()
            .contains("graceful_shutdown_timeout must be > 0"));
    }

    #[test]
    fn limits_defaults_match_runtime_config_defaults() {
        let limits = crate::limits::Limits::default();
        let runtime = RuntimeConfig::default();
        assert_eq!(limits.max_connections, runtime.max_connections);
        assert_eq!(limits.max_file_streams, runtime.max_file_streams);
        assert_eq!(limits.stream_chunk_size, runtime.stream_chunk_size);
        assert_eq!(limits.header_read_timeout, runtime.header_read_timeout);
        assert_eq!(
            limits.connection_total_timeout,
            runtime.connection_total_timeout
        );
        assert_eq!(limits.handler_timeout, runtime.handler_timeout);
        assert_eq!(limits.body_read_timeout, runtime.body_read_timeout);
        assert_eq!(
            limits.graceful_shutdown_timeout,
            runtime.graceful_shutdown_timeout
        );
        assert_eq!(limits.max_buf_size, runtime.max_buf_size);
        assert_eq!(limits.max_headers, runtime.max_headers);
        assert_eq!(limits.max_header_bytes, runtime.max_header_bytes);
        assert_eq!(
            limits.max_request_target_bytes,
            runtime.max_request_target_bytes
        );
        assert_eq!(
            limits.max_in_flight_requests,
            runtime.max_in_flight_requests
        );
        assert_eq!(
            limits.keep_alive_idle_timeout,
            runtime.keep_alive_idle_timeout
        );
        assert_eq!(
            limits.max_requests_per_connection,
            runtime.max_requests_per_connection
        );
        assert_eq!(
            limits.response_write_timeout,
            runtime.response_write_timeout
        );
    }

    #[test]
    fn serve_config_to_runtime_preserves_limits() {
        let limits = crate::limits::Limits {
            max_connections: 99,
            max_file_streams: 77,
            handler_timeout: Duration::from_secs(42),
            body_read_timeout: Duration::from_secs(99),
            connection_total_timeout: Duration::from_secs(120),
            ..Default::default()
        };
        let serve = crate::config::ServeConfig {
            limits,
            ..Default::default()
        };
        let runtime = try_from_serve_config(&serve).unwrap();
        assert_eq!(runtime.max_connections, 99);
        assert_eq!(runtime.max_file_streams, 77);
        assert_eq!(runtime.handler_timeout, Duration::from_secs(42));
        assert_eq!(runtime.body_read_timeout, Duration::from_secs(99));
    }

    #[test]
    fn try_from_serve_config_rejects_handler_wider_than_total() {
        let limits = crate::limits::Limits {
            handler_timeout: Duration::from_secs(60),
            connection_total_timeout: Duration::from_secs(30),
            ..Default::default()
        };
        let serve = crate::config::ServeConfig {
            limits,
            ..Default::default()
        };
        let err = try_from_serve_config(&serve).unwrap_err();
        assert!(err
            .to_string()
            .contains("handler_timeout must be <= connection_total_timeout"));
    }

    #[test]
    fn try_from_serve_config_rejects_body_wider_than_total() {
        let limits = crate::limits::Limits {
            body_read_timeout: Duration::from_secs(60),
            connection_total_timeout: Duration::from_secs(30),
            ..Default::default()
        };
        let serve = crate::config::ServeConfig {
            limits,
            ..Default::default()
        };
        let err = try_from_serve_config(&serve).unwrap_err();
        assert!(err
            .to_string()
            .contains("body_read_timeout must be <= connection_total_timeout"));
    }

    #[test]
    fn try_from_serve_config_rejects_invalid_limits() {
        let limits = crate::limits::Limits {
            max_connections: 0,
            ..Default::default()
        };
        let serve = crate::config::ServeConfig {
            limits,
            ..Default::default()
        };
        let err = try_from_serve_config(&serve).unwrap_err();
        assert!(err.to_string().contains("max_connections"));
    }

    #[test]
    fn limits_validate_rejects_all_zero_fields() {
        let limits = crate::limits::Limits {
            max_connections: 0,
            max_file_streams: 0,
            header_read_timeout: Duration::ZERO,
            connection_total_timeout: Duration::ZERO,
            handler_timeout: Duration::ZERO,
            body_read_timeout: Duration::ZERO,
            graceful_shutdown_timeout: Duration::ZERO,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert_eq!(errs.len(), 7);
    }

    #[test]
    fn builder_no_overrides_uses_defaults() {
        let config = RuntimeConfig::builder().build().unwrap();
        let default = RuntimeConfig::default();
        assert_eq!(config.max_connections, default.max_connections);
        assert_eq!(config.max_file_streams, default.max_file_streams);
        assert_eq!(config.header_read_timeout, default.header_read_timeout);
        assert_eq!(
            config.connection_total_timeout,
            default.connection_total_timeout
        );
        assert_eq!(config.handler_timeout, default.handler_timeout);
        assert_eq!(config.body_read_timeout, default.body_read_timeout);
        assert_eq!(
            config.graceful_shutdown_timeout,
            default.graceful_shutdown_timeout
        );
    }

    #[test]
    fn builder_is_consumed_by_build() {
        let builder = RuntimeConfig::builder().max_connections(128);
        let _config = builder.build().unwrap();
        // builder is moved, cannot use again
    }

    #[test]
    fn try_from_does_not_panic_on_invalid_input() {
        let limits = crate::limits::Limits {
            max_connections: 0,
            max_file_streams: 0,
            header_read_timeout: Duration::ZERO,
            connection_total_timeout: Duration::ZERO,
            handler_timeout: Duration::ZERO,
            body_read_timeout: Duration::ZERO,
            graceful_shutdown_timeout: Duration::ZERO,
            ..Default::default()
        };
        let serve = crate::config::ServeConfig {
            limits,
            ..Default::default()
        };
        let result = try_from_serve_config(&serve);
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Error message contains all invalid field names
        let msg = err.to_string();
        assert!(msg.contains("max_connections"));
        assert!(msg.contains("max_file_streams"));
        assert!(msg.contains("header_read_timeout"));
    }

    #[test]
    fn large_concurrency_valuesaccepted() {
        let max = tokio::sync::Semaphore::MAX_PERMITS;
        let config = RuntimeConfig::builder()
            .max_connections(max)
            .max_file_streams(max)
            .build()
            .unwrap();
        assert_eq!(config.max_connections, max);
        assert_eq!(config.max_file_streams, max);
    }

    #[test]
    fn exceeding_semaphore_max_permits_rejected() {
        let result = RuntimeConfig::builder()
            .max_connections(tokio::sync::Semaphore::MAX_PERMITS + 1)
            .build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Semaphore::MAX_PERMITS"));
    }

    #[test]
    fn large_timeout_values_accepted() {
        let config = RuntimeConfig::builder()
            .header_read_timeout(Duration::from_secs(u64::MAX))
            .connection_total_timeout(Duration::from_secs(u64::MAX))
            .handler_timeout(Duration::from_secs(u64::MAX))
            .body_read_timeout(Duration::from_secs(u64::MAX))
            .graceful_shutdown_timeout(Duration::from_secs(u64::MAX))
            .build()
            .unwrap();
        assert_eq!(config.header_read_timeout, Duration::from_secs(u64::MAX));
    }

    #[test]
    fn try_from_serve_config_multiple_invalid_fields() {
        let limits = crate::limits::Limits {
            max_connections: 0,
            handler_timeout: Duration::ZERO,
            ..Default::default()
        };
        let serve = crate::config::ServeConfig {
            limits,
            ..Default::default()
        };
        let err = try_from_serve_config(&serve).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("max_connections"));
        assert!(msg.contains("handler_timeout"));
    }

    #[test]
    fn try_from_serve_config_preserves_bind_address() {
        let serve = crate::config::ServeConfig {
            bind: "0.0.0.0:9000".parse().unwrap(),
            ..Default::default()
        };
        let runtime = try_from_serve_config(&serve).unwrap();
        assert_eq!(runtime.bind.port(), 9000);
        assert!(runtime.bind.ip().is_unspecified());
    }

    #[test]
    fn try_from_serve_config_sets_safe_defaults() {
        let serve = crate::config::ServeConfig::default();
        let runtime = try_from_serve_config(&serve).unwrap();
        assert_eq!(runtime.max_request_body_bytes, 0);
    }

    #[test]
    fn builder_rejects_buf_size_below_hyper_minimum() {
        let err = RuntimeConfig::builder()
            .max_buf_size(8191)
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("max_buf_size"));
    }

    #[test]
    fn builder_rejects_zero_max_headers() {
        let err = RuntimeConfig::builder().max_headers(0).build().unwrap_err();
        assert!(err.to_string().contains("max_headers"));
    }

    #[test]
    fn builder_rejects_small_max_header_bytes() {
        let err = RuntimeConfig::builder()
            .max_header_bytes(512)
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("max_header_bytes"));
    }

    #[test]
    fn builder_rejects_small_max_target_bytes() {
        let err = RuntimeConfig::builder()
            .max_request_target_bytes(64)
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("max_request_target_bytes"));
    }

    #[test]
    fn builder_rejects_zero_in_flight_requests() {
        let err = RuntimeConfig::builder()
            .max_in_flight_requests(0)
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("max_in_flight_requests"));
    }

    #[test]
    fn builder_rejects_zero_keep_alive_idle_timeout() {
        let err = RuntimeConfig::builder()
            .keep_alive_idle_timeout(Duration::ZERO)
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("keep_alive_idle_timeout"));
    }

    #[test]
    fn builder_rejects_zero_max_requests_per_connection() {
        let err = RuntimeConfig::builder()
            .max_requests_per_connection(Some(0))
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("max_requests_per_connection"));
    }

    #[test]
    fn builder_rejects_zero_response_write_timeout() {
        let err = RuntimeConfig::builder()
            .response_write_timeout(Duration::ZERO)
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("response_write_timeout"));
    }

    #[test]
    fn builder_accepts_unlimited_max_requests_per_connection() {
        let config = RuntimeConfig::builder()
            .max_requests_per_connection(None)
            .build()
            .unwrap();
        assert_eq!(config.max_requests_per_connection, None);
    }

    #[test]
    fn keep_alive_idle_is_independent_of_connection_total() {
        // An idle deadline wider than the hard lifetime is accepted: the
        // total lifetime simply fires first. Operators raising the total
        // for persistent connections must not be forced to raise it here.
        let config = RuntimeConfig::builder()
            .connection_total_timeout(Duration::from_secs(60))
            .keep_alive_idle_timeout(Duration::from_secs(3600))
            .response_write_timeout(Duration::from_secs(3600))
            .build()
            .unwrap();
        assert_eq!(config.keep_alive_idle_timeout, Duration::from_secs(3600));
    }

    #[test]
    fn try_from_serve_config_preserves_plan164_limits() {
        let limits = crate::limits::Limits {
            max_buf_size: 16384,
            max_headers: 50,
            max_header_bytes: 4096,
            max_request_target_bytes: 2048,
            max_in_flight_requests: 16,
            keep_alive_idle_timeout: Duration::from_secs(25),
            max_requests_per_connection: Some(100),
            response_write_timeout: Duration::from_secs(12),
            ..Default::default()
        };
        let serve = crate::config::ServeConfig {
            limits,
            ..Default::default()
        };
        let runtime = try_from_serve_config(&serve).unwrap();
        assert_eq!(runtime.max_buf_size, 16384);
        assert_eq!(runtime.max_headers, 50);
        assert_eq!(runtime.max_header_bytes, 4096);
        assert_eq!(runtime.max_request_target_bytes, 2048);
        assert_eq!(runtime.max_in_flight_requests, 16);
        assert_eq!(runtime.keep_alive_idle_timeout, Duration::from_secs(25));
        assert_eq!(runtime.max_requests_per_connection, Some(100));
        assert_eq!(runtime.response_write_timeout, Duration::from_secs(12));
    }

    #[test]
    fn try_from_serve_config_rejects_invalid_plan164_limits() {
        let limits = crate::limits::Limits {
            max_buf_size: 1024,
            max_in_flight_requests: 0,
            ..Default::default()
        };
        let serve = crate::config::ServeConfig {
            limits,
            ..Default::default()
        };
        let err = try_from_serve_config(&serve).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("max_buf_size"));
        assert!(msg.contains("max_in_flight_requests"));
    }

    #[cfg(feature = "http3")]
    #[test]
    fn http3_defaults_are_disabled_and_bounded() {
        let config = RuntimeConfig::builder().build().unwrap();
        assert!(!config.http3.enabled);
        assert_eq!(config.http3.max_concurrent_bidi_streams, 100);
        assert_eq!(config.http3.max_pending_handshakes, 64);
        assert_eq!(config.http3.max_field_section_size, 32 * 1024);
    }

    #[cfg(feature = "http3")]
    #[test]
    fn http3_rejects_invalid_window_and_stream_limits() {
        let err = RuntimeConfig::builder()
            .http3(Http3Config {
                enabled: true,
                max_concurrent_uni_streams: 2,
                ..Http3Config::default()
            })
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("max_concurrent_uni_streams"));

        let err = RuntimeConfig::builder()
            .http3(Http3Config {
                enabled: true,
                stream_receive_window: 4,
                connection_receive_window: 2,
                ..Http3Config::default()
            })
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("connection_receive_window"));
    }
}
