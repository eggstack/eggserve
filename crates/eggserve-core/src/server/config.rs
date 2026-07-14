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
/// ```ignore
/// use eggserve_core::server::RuntimeConfig;
///
/// let config = RuntimeConfig::builder()
///     .bind("127.0.0.1:8000".parse().unwrap())
///     .max_connections(128)
///     .build();
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
    /// Timeout for reading request headers. Default: 10s.
    pub header_read_timeout: Duration,
    /// Timeout for writing response bodies. Default: 60s.
    pub response_write_timeout: Duration,
    /// Timeout for a single handler invocation. Default: 30s.
    pub handler_timeout: Duration,
    /// Graceful shutdown grace period. Default: 10s.
    pub graceful_shutdown_timeout: Duration,
    /// Whether to enable HTTP keep-alive. Default: true.
    pub keep_alive: bool,
    /// Maximum concurrent in-flight requests per connection (HTTP/1.1
    /// pipelining limit). Default: `None` (no limit).
    pub max_in_flight_requests: Option<usize>,
    /// Server identification header value. If `Some`, added as `Server`
    /// header on responses. Default: `None`.
    pub server_header: Option<String>,
    /// TLS server configuration. If `Some`, connections are upgraded to TLS.
    /// Only available with the `tls` feature. Default: `None`.
    #[cfg(feature = "tls")]
    pub tls_config: Option<Arc<rustls::ServerConfig>>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8000".parse().unwrap(),
            max_connections: 64,
            max_file_streams: 32,
            header_read_timeout: Duration::from_secs(10),
            response_write_timeout: Duration::from_secs(60),
            handler_timeout: Duration::from_secs(30),
            graceful_shutdown_timeout: Duration::from_secs(10),
            keep_alive: true,
            max_in_flight_requests: None,
            server_header: None,
            #[cfg(feature = "tls")]
            tls_config: None,
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
            header_read_timeout: None,
            response_write_timeout: None,
            handler_timeout: None,
            graceful_shutdown_timeout: None,
            keep_alive: None,
            max_in_flight_requests: None,
            server_header: None,
            #[cfg(feature = "tls")]
            tls_config: None,
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
    header_read_timeout: Option<Duration>,
    response_write_timeout: Option<Duration>,
    handler_timeout: Option<Duration>,
    graceful_shutdown_timeout: Option<Duration>,
    keep_alive: Option<bool>,
    max_in_flight_requests: Option<usize>,
    server_header: Option<String>,
    #[cfg(feature = "tls")]
    tls_config: Option<Arc<rustls::ServerConfig>>,
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

    /// Set the header-read timeout.
    pub fn header_read_timeout(mut self, timeout: Duration) -> Self {
        self.header_read_timeout = Some(timeout);
        self
    }

    /// Set the response-write timeout.
    pub fn response_write_timeout(mut self, timeout: Duration) -> Self {
        self.response_write_timeout = Some(timeout);
        self
    }

    /// Set the handler invocation timeout.
    pub fn handler_timeout(mut self, timeout: Duration) -> Self {
        self.handler_timeout = Some(timeout);
        self
    }

    /// Set the graceful shutdown grace period.
    pub fn graceful_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.graceful_shutdown_timeout = Some(timeout);
        self
    }

    /// Enable or disable HTTP keep-alive.
    pub fn keep_alive(mut self, enabled: bool) -> Self {
        self.keep_alive = Some(enabled);
        self
    }

    /// Set the maximum concurrent in-flight requests per connection.
    ///
    /// Controls HTTP/1.1 pipelining depth. `None` means no limit.
    pub fn max_in_flight_requests(mut self, max: usize) -> Self {
        self.max_in_flight_requests = Some(max);
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

    /// Build the runtime configuration.
    ///
    /// # Panics
    ///
    /// Panics if `max_connections` or `max_file_streams` is 0.
    pub fn build(self) -> RuntimeConfig {
        let max_connections = self.max_connections.unwrap_or(64);
        let max_file_streams = self.max_file_streams.unwrap_or(32);
        assert!(max_connections > 0, "max_connections must be > 0");
        assert!(max_file_streams > 0, "max_file_streams must be > 0");
        RuntimeConfig {
            bind: self
                .bind
                .unwrap_or_else(|| "127.0.0.1:8000".parse().unwrap()),
            max_connections,
            max_file_streams,
            header_read_timeout: self.header_read_timeout.unwrap_or(Duration::from_secs(10)),
            response_write_timeout: self
                .response_write_timeout
                .unwrap_or(Duration::from_secs(60)),
            handler_timeout: self.handler_timeout.unwrap_or(Duration::from_secs(30)),
            graceful_shutdown_timeout: self
                .graceful_shutdown_timeout
                .unwrap_or(Duration::from_secs(10)),
            keep_alive: self.keep_alive.unwrap_or(true),
            max_in_flight_requests: self.max_in_flight_requests,
            server_header: self.server_header,
            #[cfg(feature = "tls")]
            tls_config: self.tls_config,
        }
    }
}

/// Convert a [`crate::config::ServeConfig`] into a [`RuntimeConfig`].
///
/// This bridges the CLI/Python configuration model into the runtime model.
/// Filesystem policy and root directory are NOT transferred — they belong
/// to the service, not the runtime.
impl From<&crate::config::ServeConfig> for RuntimeConfig {
    fn from(config: &crate::config::ServeConfig) -> Self {
        Self {
            bind: config.bind,
            max_connections: config.limits.max_connections,
            max_file_streams: config.limits.max_file_streams,
            header_read_timeout: config.limits.header_read_timeout,
            response_write_timeout: config.limits.response_write_timeout,
            handler_timeout: Duration::from_secs(30),
            graceful_shutdown_timeout: config.limits.graceful_shutdown_timeout,
            keep_alive: true,
            max_in_flight_requests: None,
            server_header: None,
            #[cfg(feature = "tls")]
            tls_config: None,
        }
    }
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
        assert_eq!(config.header_read_timeout, Duration::from_secs(10));
        assert_eq!(config.response_write_timeout, Duration::from_secs(60));
        assert_eq!(config.handler_timeout, Duration::from_secs(30));
        assert_eq!(config.graceful_shutdown_timeout, Duration::from_secs(10));
        assert!(config.keep_alive);
        assert_eq!(config.max_in_flight_requests, None);
        assert_eq!(config.server_header, None);
    }

    #[test]
    fn builder_overrides() {
        let config = RuntimeConfig::builder()
            .bind("0.0.0.0:9000".parse().unwrap())
            .max_connections(128)
            .max_file_streams(64)
            .header_read_timeout(Duration::from_secs(5))
            .response_write_timeout(Duration::from_secs(30))
            .handler_timeout(Duration::from_secs(15))
            .graceful_shutdown_timeout(Duration::from_secs(5))
            .keep_alive(false)
            .max_in_flight_requests(8)
            .server_header("eggserve/0.1".into())
            .build();
        assert_eq!(config.bind.port(), 9000);
        assert_eq!(config.max_connections, 128);
        assert_eq!(config.max_file_streams, 64);
        assert_eq!(config.header_read_timeout, Duration::from_secs(5));
        assert_eq!(config.response_write_timeout, Duration::from_secs(30));
        assert_eq!(config.handler_timeout, Duration::from_secs(15));
        assert_eq!(config.graceful_shutdown_timeout, Duration::from_secs(5));
        assert!(!config.keep_alive);
        assert_eq!(config.max_in_flight_requests, Some(8));
        assert_eq!(config.server_header.as_deref(), Some("eggserve/0.1"));
    }

    #[test]
    fn from_serve_config() {
        let serve_config = crate::config::ServeConfig::default();
        let runtime = RuntimeConfig::from(&serve_config);
        assert_eq!(runtime.bind, serve_config.bind);
        assert_eq!(runtime.max_connections, serve_config.limits.max_connections);
        assert_eq!(
            runtime.max_file_streams,
            serve_config.limits.max_file_streams
        );
    }

    #[test]
    #[should_panic(expected = "max_connections must be > 0")]
    fn zero_connections_panics() {
        let _ = RuntimeConfig::builder().max_connections(0).build();
    }

    #[test]
    #[should_panic(expected = "max_file_streams must be > 0")]
    fn zero_file_streams_panics() {
        let _ = RuntimeConfig::builder().max_file_streams(0).build();
    }
}
