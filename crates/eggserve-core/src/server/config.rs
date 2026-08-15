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
    /// Timeout for reading request headers. Default: 10s.
    pub header_read_timeout: Duration,
    /// Timeout wrapping the entire Hyper connection future. Default: 60s.
    pub connection_total_timeout: Duration,
    /// Timeout for a single handler invocation. Default: 30s.
    pub handler_timeout: Duration,
    /// Timeout for reading the request body. Default: 30s.
    /// This is a total deadline for body consumption, not an idle timeout.
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
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8000".parse().unwrap(),
            max_connections: 64,
            max_file_streams: 32,
            header_read_timeout: Duration::from_secs(10),
            connection_total_timeout: Duration::from_secs(60),
            handler_timeout: Duration::from_secs(30),
            body_read_timeout: Duration::from_secs(30),
            graceful_shutdown_timeout: Duration::from_secs(10),
            server_header: None,
            #[cfg(feature = "tls")]
            tls_config: None,
            max_request_body_bytes: 0,
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
            connection_total_timeout: None,
            handler_timeout: None,
            body_read_timeout: None,
            graceful_shutdown_timeout: None,
            server_header: None,
            #[cfg(feature = "tls")]
            tls_config: None,
            max_request_body_bytes: None,
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
    connection_total_timeout: Option<Duration>,
    handler_timeout: Option<Duration>,
    body_read_timeout: Option<Duration>,
    graceful_shutdown_timeout: Option<Duration>,
    server_header: Option<String>,
    #[cfg(feature = "tls")]
    tls_config: Option<Arc<rustls::ServerConfig>>,
    max_request_body_bytes: Option<u64>,
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

    /// Set the connection total timeout.
    pub fn connection_total_timeout(mut self, timeout: Duration) -> Self {
        self.connection_total_timeout = Some(timeout);
        self
    }

    /// Set the handler invocation timeout.
    pub fn handler_timeout(mut self, timeout: Duration) -> Self {
        self.handler_timeout = Some(timeout);
        self
    }

    /// Set the body read timeout.
    ///
    /// This is a total deadline for body consumption, not an idle timeout.
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

        let header_read_timeout = self.header_read_timeout.unwrap_or(Duration::from_secs(10));
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
        if connection_total_timeout.is_zero() {
            return Err(crate::server::errors::ServerError::Config(
                "connection_total_timeout must be > 0".into(),
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
            header_read_timeout,
            connection_total_timeout,
            handler_timeout,
            body_read_timeout,
            graceful_shutdown_timeout,
            server_header: self.server_header,
            #[cfg(feature = "tls")]
            tls_config: self.tls_config,
            max_request_body_bytes: self.max_request_body_bytes.unwrap_or(0),
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
        header_read_timeout: config.limits.header_read_timeout,
        connection_total_timeout: config.limits.connection_total_timeout,
        handler_timeout: config.limits.handler_timeout,
        body_read_timeout: config.limits.body_read_timeout,
        graceful_shutdown_timeout: config.limits.graceful_shutdown_timeout,
        server_header: None,
        #[cfg(feature = "tls")]
        tls_config: None,
        max_request_body_bytes: config.limits.max_request_body_bytes,
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
        assert_eq!(config.header_read_timeout, Duration::from_secs(10));
        assert_eq!(config.connection_total_timeout, Duration::from_secs(60));
        assert_eq!(config.handler_timeout, Duration::from_secs(30));
        assert_eq!(config.body_read_timeout, Duration::from_secs(30));
        assert_eq!(config.graceful_shutdown_timeout, Duration::from_secs(10));
        assert_eq!(config.server_header, None);
        assert_eq!(config.max_request_body_bytes, 0);
    }

    #[test]
    fn builder_overrides() {
        let config = RuntimeConfig::builder()
            .bind("0.0.0.0:9000".parse().unwrap())
            .max_connections(128)
            .max_file_streams(64)
            .header_read_timeout(Duration::from_secs(5))
            .connection_total_timeout(Duration::from_secs(30))
            .handler_timeout(Duration::from_secs(15))
            .body_read_timeout(Duration::from_secs(20))
            .graceful_shutdown_timeout(Duration::from_secs(5))
            .server_header("eggserve/0.1".into())
            .max_request_body_bytes(1024 * 1024)
            .build()
            .unwrap();
        assert_eq!(config.bind.port(), 9000);
        assert_eq!(config.max_connections, 128);
        assert_eq!(config.max_file_streams, 64);
        assert_eq!(config.header_read_timeout, Duration::from_secs(5));
        assert_eq!(config.connection_total_timeout, Duration::from_secs(30));
        assert_eq!(config.handler_timeout, Duration::from_secs(15));
        assert_eq!(config.body_read_timeout, Duration::from_secs(20));
        assert_eq!(config.graceful_shutdown_timeout, Duration::from_secs(5));
        assert_eq!(config.server_header.as_deref(), Some("eggserve/0.1"));
        assert_eq!(config.max_request_body_bytes, 1024 * 1024);
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
    }

    #[test]
    fn serve_config_to_runtime_preserves_limits() {
        let limits = crate::limits::Limits {
            max_connections: 99,
            max_file_streams: 77,
            handler_timeout: Duration::from_secs(42),
            body_read_timeout: Duration::from_secs(99),
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
}
