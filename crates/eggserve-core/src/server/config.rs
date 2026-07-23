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

use crate::primitives::request_body_policy::RequestBodyPolicy;

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
///     .build()?;
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
    /// Maximum allowed request body size in bytes. This is the hard ceiling
    /// that no service can exceed. Default: 0 (bodies rejected).
    pub max_request_body_bytes: u64,
    /// Request body acceptance policy. Default: `Reject`.
    ///
    /// Services declare their preferred policy (Reject, Buffer, or Stream),
    /// but the runtime enforces `max_request_body_bytes` as the absolute
    /// ceiling. A service cannot request a limit above this value.
    pub request_body_policy: RequestBodyPolicy,
    /// Policy for handling incomplete request bodies when a handler returns
    /// without fully consuming the body. Default: `Close`.
    pub incomplete_body_policy: crate::primitives::incomplete_body_policy::IncompleteBodyPolicy,
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
            keep_alive: true,
            max_in_flight_requests: None,
            server_header: None,
            #[cfg(feature = "tls")]
            tls_config: None,
            max_request_body_bytes: 0,
            request_body_policy: RequestBodyPolicy::Reject,
            incomplete_body_policy:
                crate::primitives::incomplete_body_policy::IncompleteBodyPolicy::Close,
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
            keep_alive: None,
            max_in_flight_requests: None,
            server_header: None,
            #[cfg(feature = "tls")]
            tls_config: None,
            max_request_body_bytes: None,
            request_body_policy: None,
            incomplete_body_policy: None,
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
    keep_alive: Option<bool>,
    max_in_flight_requests: Option<usize>,
    server_header: Option<String>,
    #[cfg(feature = "tls")]
    tls_config: Option<Arc<rustls::ServerConfig>>,
    max_request_body_bytes: Option<u64>,
    request_body_policy: Option<RequestBodyPolicy>,
    incomplete_body_policy: Option<crate::primitives::incomplete_body_policy::IncompleteBodyPolicy>,
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

    /// Set the maximum request body size in bytes.
    ///
    /// This is the hard ceiling that no service can exceed. Default: 0
    /// (bodies rejected). Set to a positive value to allow request bodies.
    pub fn max_request_body_bytes(mut self, max: u64) -> Self {
        self.max_request_body_bytes = Some(max);
        self
    }

    /// Set the request body acceptance policy.
    ///
    /// Services declare their preferred policy, but the runtime enforces
    /// `max_request_body_bytes` as the absolute ceiling. Default: `Reject`.
    pub fn request_body_policy(mut self, policy: RequestBodyPolicy) -> Self {
        self.request_body_policy = Some(policy);
        self
    }

    /// Set the policy for handling incomplete request bodies.
    ///
    /// When a handler returns without fully consuming the body, the runtime
    /// applies this policy. Default: `Close`.
    pub fn incomplete_body_policy(
        mut self,
        policy: crate::primitives::incomplete_body_policy::IncompleteBodyPolicy,
    ) -> Self {
        self.incomplete_body_policy = Some(policy);
        self
    }

    /// Build the runtime configuration.
    ///
    /// Returns an error if `max_connections`, `max_file_streams`, or any
    /// timeout duration is 0.
    pub fn build(self) -> Result<RuntimeConfig, crate::server::errors::ServerError> {
        let max_connections = self.max_connections.unwrap_or(64);
        let max_file_streams = self.max_file_streams.unwrap_or(32);
        if max_connections == 0 {
            return Err(crate::server::errors::ServerError::Config(
                "max_connections must be > 0".into(),
            ));
        }
        if max_file_streams == 0 {
            return Err(crate::server::errors::ServerError::Config(
                "max_file_streams must be > 0".into(),
            ));
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
            keep_alive: self.keep_alive.unwrap_or(true),
            max_in_flight_requests: self.max_in_flight_requests,
            server_header: self.server_header,
            #[cfg(feature = "tls")]
            tls_config: self.tls_config,
            max_request_body_bytes: self.max_request_body_bytes.unwrap_or(0),
            request_body_policy: self
                .request_body_policy
                .unwrap_or(RequestBodyPolicy::Reject),
            incomplete_body_policy: self
                .incomplete_body_policy
                .unwrap_or(crate::primitives::incomplete_body_policy::IncompleteBodyPolicy::Close),
        })
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
            connection_total_timeout: config.limits.connection_total_timeout,
            handler_timeout: Duration::from_secs(30),
            body_read_timeout: Duration::from_secs(30),
            graceful_shutdown_timeout: config.limits.graceful_shutdown_timeout,
            keep_alive: true,
            max_in_flight_requests: None,
            server_header: None,
            #[cfg(feature = "tls")]
            tls_config: None,
            max_request_body_bytes: config.limits.max_request_body_bytes,
            request_body_policy: RequestBodyPolicy::Reject,
            incomplete_body_policy:
                crate::primitives::incomplete_body_policy::IncompleteBodyPolicy::Close,
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
        assert_eq!(config.connection_total_timeout, Duration::from_secs(60));
        assert_eq!(config.handler_timeout, Duration::from_secs(30));
        assert_eq!(config.body_read_timeout, Duration::from_secs(30));
        assert_eq!(config.graceful_shutdown_timeout, Duration::from_secs(10));
        assert!(config.keep_alive);
        assert_eq!(config.max_in_flight_requests, None);
        assert_eq!(config.server_header, None);
        assert_eq!(config.max_request_body_bytes, 0);
        assert_eq!(config.request_body_policy, RequestBodyPolicy::Reject);
        assert_eq!(
            config.incomplete_body_policy,
            crate::primitives::incomplete_body_policy::IncompleteBodyPolicy::Close
        );
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
            .keep_alive(false)
            .max_in_flight_requests(8)
            .server_header("eggserve/0.1".into())
            .max_request_body_bytes(1024 * 1024)
            .request_body_policy(RequestBodyPolicy::Buffer { max_bytes: 512 })
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
        assert!(!config.keep_alive);
        assert_eq!(config.max_in_flight_requests, Some(8));
        assert_eq!(config.server_header.as_deref(), Some("eggserve/0.1"));
        assert_eq!(config.max_request_body_bytes, 1024 * 1024);
        assert_eq!(
            config.request_body_policy,
            RequestBodyPolicy::Buffer { max_bytes: 512 }
        );
        assert_eq!(
            config.incomplete_body_policy,
            crate::primitives::incomplete_body_policy::IncompleteBodyPolicy::Close
        );
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
        assert_eq!(
            runtime.max_request_body_bytes,
            serve_config.limits.max_request_body_bytes
        );
        assert_eq!(runtime.request_body_policy, RequestBodyPolicy::Reject);
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
}
