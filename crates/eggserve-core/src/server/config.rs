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
    /// Server identification header value. If `Some`, added as `Server`
    /// header on responses. Default: `None`.
    pub server_header: Option<String>,
    /// TLS server configuration. If `Some`, connections are upgraded to TLS.
    /// Only available with the `tls` feature. Default: `None`.
    #[cfg(feature = "tls")]
    pub tls_config: Option<Arc<rustls::ServerConfig>>,
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
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8000".parse().unwrap(),
            max_connections: 64,
            max_file_streams: 32,
            stream_chunk_size: crate::limits::DEFAULT_STREAM_CHUNK_SIZE,
            header_read_timeout: Duration::from_secs(10),
            tls_handshake_timeout: Duration::from_secs(10),
            connection_total_timeout: Duration::from_secs(60),
            handler_timeout: Duration::from_secs(30),
            body_read_timeout: Duration::from_secs(30),
            graceful_shutdown_timeout: Duration::from_secs(10),
            server_header: None,
            #[cfg(feature = "tls")]
            tls_config: None,
            max_request_body_bytes: 0,
            max_buf_size: crate::limits::DEFAULT_MAX_BUF_SIZE,
            max_headers: crate::limits::DEFAULT_MAX_HEADERS,
            max_header_bytes: crate::limits::DEFAULT_MAX_HEADER_BYTES,
            max_request_target_bytes: crate::limits::DEFAULT_MAX_REQUEST_TARGET_BYTES,
            max_in_flight_requests: crate::limits::DEFAULT_MAX_IN_FLIGHT_REQUESTS,
            keep_alive_idle_timeout: Duration::from_secs(60),
            max_requests_per_connection: None,
            response_write_timeout: Duration::from_secs(30),
        }
    }
}

impl RuntimeConfig {
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
            #[cfg(feature = "tls")]
            tls_config: None,
            max_request_body_bytes: None,
            max_buf_size: None,
            max_headers: None,
            max_header_bytes: None,
            max_request_target_bytes: None,
            max_in_flight_requests: None,
            keep_alive_idle_timeout: None,
            max_requests_per_connection: None,
            response_write_timeout: None,
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
    #[cfg(feature = "tls")]
    tls_config: Option<Arc<rustls::ServerConfig>>,
    max_request_body_bytes: Option<u64>,
    max_buf_size: Option<usize>,
    max_headers: Option<usize>,
    max_header_bytes: Option<usize>,
    max_request_target_bytes: Option<usize>,
    max_in_flight_requests: Option<usize>,
    keep_alive_idle_timeout: Option<Duration>,
    max_requests_per_connection: Option<Option<u64>>,
    response_write_timeout: Option<Duration>,
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
    /// If set, added as `Server` header on all responses.
    pub fn server_header(mut self, header: String) -> Self {
        self.server_header = Some(header);
        self
    }

    /// Set the TLS server configuration.
    #[cfg(feature = "tls")]
    pub fn tls_config(mut self, config: Arc<rustls::ServerConfig>) -> Self {
        self.tls_config = Some(config);
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

    /// Build the runtime configuration.
    ///
    /// Returns an error if `max_connections`, `max_file_streams`, or any
    /// timeout duration is 0.
    pub fn build(self) -> Result<RuntimeConfig, crate::server::errors::ServerError> {
        let max_connections = self.max_connections.unwrap_or(64);
        let max_file_streams = self.max_file_streams.unwrap_or(32);
        let max_semaphore_permits = tokio::sync::Semaphore::MAX_PERMITS;
        if max_connections == 0 {
            return Err(crate::server::errors::ServerError::Config(
                "max_connections must be > 0".into(),
            ));
        }
        if max_connections > max_semaphore_permits {
            return Err(crate::server::errors::ServerError::Config(format!(
                "max_connections must be <= {} (Semaphore::MAX_PERMITS): got {}",
                max_semaphore_permits, max_connections
            )));
        }
        if max_file_streams == 0 {
            return Err(crate::server::errors::ServerError::Config(
                "max_file_streams must be > 0".into(),
            ));
        }
        if max_file_streams > max_semaphore_permits {
            return Err(crate::server::errors::ServerError::Config(format!(
                "max_file_streams must be <= {} (Semaphore::MAX_PERMITS): got {}",
                max_semaphore_permits, max_file_streams
            )));
        }
        let stream_chunk_size = self
            .stream_chunk_size
            .unwrap_or(crate::limits::DEFAULT_STREAM_CHUNK_SIZE);
        if stream_chunk_size < 64 {
            return Err(crate::server::errors::ServerError::Config(
                "stream_chunk_size must be >= 64".into(),
            ));
        }
        if stream_chunk_size > 1024 * 1024 {
            return Err(crate::server::errors::ServerError::Config(
                "stream_chunk_size must be <= 1048576 (1 MiB)".into(),
            ));
        }

        let max_request_body_bytes = self.max_request_body_bytes.unwrap_or(0);
        if max_request_body_bytes > crate::limits::MAX_REQUEST_BODY_BYTES {
            return Err(crate::server::errors::ServerError::Config(format!(
                "max_request_body_bytes must be <= {} (1 GiB), or 0 to reject bodies: got {}",
                crate::limits::MAX_REQUEST_BODY_BYTES,
                max_request_body_bytes
            )));
        }

        let max_buf_size = self
            .max_buf_size
            .unwrap_or(crate::limits::DEFAULT_MAX_BUF_SIZE);
        if max_buf_size < crate::limits::MIN_MAX_BUF_SIZE {
            return Err(crate::server::errors::ServerError::Config(format!(
                "max_buf_size must be >= {} (Hyper minimum): got {}",
                crate::limits::MIN_MAX_BUF_SIZE,
                max_buf_size
            )));
        }
        if max_buf_size > crate::limits::MAX_MAX_BUF_SIZE {
            return Err(crate::server::errors::ServerError::Config(format!(
                "max_buf_size must be <= {} (4 MiB): got {}",
                crate::limits::MAX_MAX_BUF_SIZE,
                max_buf_size
            )));
        }
        let max_headers = self
            .max_headers
            .unwrap_or(crate::limits::DEFAULT_MAX_HEADERS);
        if max_headers == 0 {
            return Err(crate::server::errors::ServerError::Config(
                "max_headers must be > 0".into(),
            ));
        }
        if max_headers > crate::limits::MAX_MAX_HEADERS {
            return Err(crate::server::errors::ServerError::Config(format!(
                "max_headers must be <= {}: got {}",
                crate::limits::MAX_MAX_HEADERS,
                max_headers
            )));
        }
        let max_header_bytes = self
            .max_header_bytes
            .unwrap_or(crate::limits::DEFAULT_MAX_HEADER_BYTES);
        if max_header_bytes < crate::limits::MIN_MAX_HEADER_BYTES {
            return Err(crate::server::errors::ServerError::Config(format!(
                "max_header_bytes must be >= {}: got {}",
                crate::limits::MIN_MAX_HEADER_BYTES,
                max_header_bytes
            )));
        }
        if max_header_bytes > crate::limits::MAX_MAX_HEADER_BYTES {
            return Err(crate::server::errors::ServerError::Config(format!(
                "max_header_bytes must be <= {} (1 MiB): got {}",
                crate::limits::MAX_MAX_HEADER_BYTES,
                max_header_bytes
            )));
        }
        let max_request_target_bytes = self
            .max_request_target_bytes
            .unwrap_or(crate::limits::DEFAULT_MAX_REQUEST_TARGET_BYTES);
        if max_request_target_bytes < crate::limits::MIN_MAX_REQUEST_TARGET_BYTES {
            return Err(crate::server::errors::ServerError::Config(format!(
                "max_request_target_bytes must be >= {}: got {}",
                crate::limits::MIN_MAX_REQUEST_TARGET_BYTES,
                max_request_target_bytes
            )));
        }
        if max_request_target_bytes > crate::limits::MAX_MAX_REQUEST_TARGET_BYTES {
            return Err(crate::server::errors::ServerError::Config(format!(
                "max_request_target_bytes must be <= {} (64 KiB): got {}",
                crate::limits::MAX_MAX_REQUEST_TARGET_BYTES,
                max_request_target_bytes
            )));
        }
        let max_in_flight_requests = self.max_in_flight_requests.unwrap_or(64);
        if max_in_flight_requests == 0 {
            return Err(crate::server::errors::ServerError::Config(
                "max_in_flight_requests must be > 0".into(),
            ));
        }
        if max_in_flight_requests > max_semaphore_permits {
            return Err(crate::server::errors::ServerError::Config(format!(
                "max_in_flight_requests must be <= {} (Semaphore::MAX_PERMITS): got {}",
                max_semaphore_permits, max_in_flight_requests
            )));
        }
        let keep_alive_idle_timeout = self
            .keep_alive_idle_timeout
            .unwrap_or(Duration::from_secs(60));
        if keep_alive_idle_timeout.is_zero() {
            return Err(crate::server::errors::ServerError::Config(
                "keep_alive_idle_timeout must be > 0".into(),
            ));
        }
        let max_requests_per_connection = self.max_requests_per_connection.unwrap_or(None);
        if max_requests_per_connection == Some(0) {
            return Err(crate::server::errors::ServerError::Config(
                "max_requests_per_connection must be >= 1 or None (unlimited)".into(),
            ));
        }
        let response_write_timeout = self
            .response_write_timeout
            .unwrap_or(Duration::from_secs(30));
        if response_write_timeout.is_zero() {
            return Err(crate::server::errors::ServerError::Config(
                "response_write_timeout must be > 0".into(),
            ));
        }

        let header_read_timeout = self.header_read_timeout.unwrap_or(Duration::from_secs(10));
        let tls_handshake_timeout = self
            .tls_handshake_timeout
            .unwrap_or(Duration::from_secs(10));
        let connection_total_timeout = self
            .connection_total_timeout
            .unwrap_or(Duration::from_secs(60));
        let handler_timeout = self.handler_timeout.unwrap_or(Duration::from_secs(30));
        let body_read_timeout = self.body_read_timeout.unwrap_or(Duration::from_secs(30));
        let graceful_shutdown_timeout = self
            .graceful_shutdown_timeout
            .unwrap_or(Duration::from_secs(10));

        if header_read_timeout.is_zero() {
            return Err(crate::server::errors::ServerError::Config(
                "header_read_timeout must be > 0".into(),
            ));
        }
        if tls_handshake_timeout.is_zero() {
            return Err(crate::server::errors::ServerError::Config(
                "tls_handshake_timeout must be > 0".into(),
            ));
        }
        if connection_total_timeout.is_zero() {
            return Err(crate::server::errors::ServerError::Config(
                "connection_total_timeout must be > 0".into(),
            ));
        }
        if header_read_timeout > connection_total_timeout {
            return Err(crate::server::errors::ServerError::Config(
                "header_read_timeout must be <= connection_total_timeout".into(),
            ));
        }
        if handler_timeout.is_zero() {
            return Err(crate::server::errors::ServerError::Config(
                "handler_timeout must be > 0".into(),
            ));
        }
        if body_read_timeout.is_zero() {
            return Err(crate::server::errors::ServerError::Config(
                "body_read_timeout must be > 0".into(),
            ));
        }
        // A handler or body budget wider than the total connection
        // lifetime is dead configuration: the connection budget always
        // fires first and kills the request mid-flight.
        if handler_timeout > connection_total_timeout {
            return Err(crate::server::errors::ServerError::Config(
                "handler_timeout must be <= connection_total_timeout".into(),
            ));
        }
        if body_read_timeout > connection_total_timeout {
            return Err(crate::server::errors::ServerError::Config(
                "body_read_timeout must be <= connection_total_timeout".into(),
            ));
        }
        if graceful_shutdown_timeout.is_zero() {
            return Err(crate::server::errors::ServerError::Config(
                "graceful_shutdown_timeout must be > 0".into(),
            ));
        }
        if let Some(server_header) = &self.server_header {
            crate::primitives::header_block::HeaderValue::new(server_header.clone()).map_err(
                |e| {
                    crate::server::errors::ServerError::Config(format!(
                        "invalid server_header: {e}"
                    ))
                },
            )?;
        }
        Ok(RuntimeConfig {
            bind: self
                .bind
                .unwrap_or_else(|| "127.0.0.1:8000".parse().unwrap()),
            max_connections,
            max_file_streams,
            stream_chunk_size,
            header_read_timeout,
            tls_handshake_timeout,
            connection_total_timeout,
            handler_timeout,
            body_read_timeout,
            graceful_shutdown_timeout,
            server_header: self.server_header,
            #[cfg(feature = "tls")]
            tls_config: self.tls_config,
            max_request_body_bytes,
            max_buf_size,
            max_headers,
            max_header_bytes,
            max_request_target_bytes,
            max_in_flight_requests,
            keep_alive_idle_timeout,
            max_requests_per_connection,
            response_write_timeout,
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
    Ok(RuntimeConfig {
        bind: config.bind,
        max_connections: config.limits.max_connections,
        max_file_streams: config.limits.max_file_streams,
        stream_chunk_size: config.limits.stream_chunk_size,
        header_read_timeout: config.limits.header_read_timeout,
        tls_handshake_timeout: config.limits.tls_handshake_timeout,
        connection_total_timeout: config.limits.connection_total_timeout,
        handler_timeout: config.limits.handler_timeout,
        body_read_timeout: config.limits.body_read_timeout,
        graceful_shutdown_timeout: config.limits.graceful_shutdown_timeout,
        server_header: None,
        #[cfg(feature = "tls")]
        tls_config: None,
        max_request_body_bytes: config.limits.max_request_body_bytes,
        max_buf_size: config.limits.max_buf_size,
        max_headers: config.limits.max_headers,
        max_header_bytes: config.limits.max_header_bytes,
        max_request_target_bytes: config.limits.max_request_target_bytes,
        max_in_flight_requests: config.limits.max_in_flight_requests,
        keep_alive_idle_timeout: config.limits.keep_alive_idle_timeout,
        max_requests_per_connection: config.limits.max_requests_per_connection,
        response_write_timeout: config.limits.response_write_timeout,
    })
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
        assert_eq!(config.server_header, None);
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
        assert_eq!(config.server_header.as_deref(), Some("eggserve/0.1"));
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
        assert!(err.to_string().contains("invalid server_header"));
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
}
