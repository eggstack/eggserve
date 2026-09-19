//! H1-generic runtime configuration (Plan 215: moved from compatibility core).
//!
//! [`RuntimeConfig`] controls transport-level concerns (connection limits,
//! timeouts, keep-alive) independently of service-level concerns. The direct
//! configuration covers the H1-generic subset: TLS identity, HTTP/2, and
//! HTTP/3 transport policy stay compatibility-owned (Plans 203/213/217)
//! behind the compatibility `RuntimeConfig`, which projects its H1 fields
//! onto this type and shares the same defaults/validation authority
//! ([`crate::runtime_limits`]).
//!
//! There is exactly one defaults table and one validation kernel: this
//! module never duplicates constant values.

use std::net::SocketAddr;
use std::time::Duration;

use eggserve_primitives::proxy::{IpPrefix, TrustedProxyConfig};

use crate::errors::ServerError;
use crate::response_policy::{DatePolicy, ResponsePolicy};
use crate::runtime_limits as rl;

/// HTTP/1-only parser and framing settings projected from [`RuntimeConfig`].
///
/// No second defaults table: values project from the shared authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Http1Config {
    pub(crate) max_buf_size: usize,
    pub(crate) max_headers: usize,
}

/// Transport-level runtime configuration (H1-generic subset).
///
/// All fields have safe defaults. Validation occurs at construction time via
/// the builder ([`RuntimeConfig::builder`]) or explicitly via
/// [`RuntimeConfig::validate`]; hand-constructed values must be validated
/// before semaphore/Hyper construction.
#[derive(Debug, Clone)]
#[must_use]
pub struct RuntimeConfig {
    /// Address to bind the listener to.
    pub bind: SocketAddr,
    /// Maximum concurrent connections. Default: 64.
    pub max_connections: usize,
    /// Maximum concurrent file-stream responses. Default: 32.
    pub max_file_streams: usize,
    /// File streaming read chunk size. Default: 128 KiB.
    pub stream_chunk_size: usize,
    /// Timeout for reading request headers. Default: 10s.
    pub header_read_timeout: Duration,
    /// Timeout for a TLS handshake. Default: 10s.
    ///
    /// Part of the shared kernel so direct and compatibility construction
    /// reject the same invalid values; the direct H1 driver performs no
    /// handshake itself.
    pub tls_handshake_timeout: Duration,
    /// Timeout wrapping the entire connection future. Default: 60s.
    ///
    /// Maximum connection lifetime shared across all requests on a
    /// keep-alive connection, not reset per request.
    pub connection_total_timeout: Duration,
    /// Timeout for a single handler invocation. Default: 30s.
    pub handler_timeout: Duration,
    /// Timeout for reading the request body (total deadline). Default: 30s.
    pub body_read_timeout: Duration,
    /// Graceful shutdown grace period. Default: 10s.
    pub graceful_shutdown_timeout: Duration,
    /// Final-boundary response privacy policy.
    pub response_policy: ResponsePolicy,
    /// Maximum allowed request body size in bytes. Hard ceiling no service
    /// can exceed. Default: 0 (bodies rejected).
    pub max_request_body_bytes: u64,
    /// Maximum HTTP/1 parser/read buffer size in bytes. Minimum 8192
    /// (Hyper panics below). Default: 64 KiB.
    pub max_buf_size: usize,
    /// Maximum request header field count. Default: 100.
    pub max_headers: usize,
    /// Maximum aggregate post-parse request-header name+value bytes.
    /// Default: 32 KiB.
    pub max_header_bytes: usize,
    /// Maximum request-target length in bytes. Default: 8192.
    pub max_request_target_bytes: usize,
    /// Maximum concurrent in-flight `Service::call()` executions.
    /// Default: 64.
    pub max_in_flight_requests: usize,
    /// Keep-alive idle timeout. Resets on activity; independent of the total
    /// lifetime. Default: 60s.
    pub keep_alive_idle_timeout: Duration,
    /// Maximum completed requests per connection. `None` disables the
    /// limit. Default: `None` (unlimited).
    pub max_requests_per_connection: Option<u64>,
    /// Response write no-progress timeout. Default: 30s.
    pub response_write_timeout: Duration,
    /// Maximum concurrent active tunnels (Plan 216 direct H1 authority).
    /// Long-lived tunnels hold a permit until close; exhaustion fails new
    /// handshakes with 503 without affecting ordinary HTTP. Default: 64.
    pub max_active_tunnels: usize,
    /// Trusted proxy policy. Defaults trust nothing. Header-derived
    /// forwarding populates provenance-tagged effective fields without
    /// rewriting the canonical Host/target.
    pub trusted_proxy: TrustedProxyConfig,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
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
            response_policy: ResponsePolicy::default(),
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
            trusted_proxy: TrustedProxyConfig::default(),
        }
    }
}

impl RuntimeConfig {
    /// Project the H1 parser settings from the shared authority values.
    pub(crate) fn http1_config(&self) -> Http1Config {
        Http1Config {
            max_buf_size: self.max_buf_size,
            max_headers: self.max_headers,
        }
    }

    /// Create a new builder with default values.
    pub fn builder() -> RuntimeConfigBuilder {
        RuntimeConfigBuilder::default()
    }

    /// Returns the configured `Server` identification value, if any.
    ///
    /// `None` means suppressed (secure default).
    pub fn server_header_value(&self) -> Option<&str> {
        self.response_policy.server_identification.as_deref()
    }

    /// Validate a complete hand-constructed [`RuntimeConfig`].
    ///
    /// Builder validation alone cannot protect `RuntimeConfig` because its
    /// fields are public. Call this before semaphore/Hyper/runtime
    /// operations, or use [`RuntimeConfigBuilder::build`],
    /// [`crate::runtime::RuntimeState::try_new`], or `ServerBuilder::build`,
    /// which all enforce it.
    pub fn validate(&self) -> Result<(), ServerError> {
        let shared = rl::SharedRuntimeValues {
            max_connections: self.max_connections,
            max_file_streams: self.max_file_streams,
            max_request_body_bytes: self.max_request_body_bytes,
            header_read_timeout: self.header_read_timeout,
            tls_handshake_timeout: self.tls_handshake_timeout,
            connection_total_timeout: self.connection_total_timeout,
            handler_timeout: self.handler_timeout,
            body_read_timeout: self.body_read_timeout,
            graceful_shutdown_timeout: self.graceful_shutdown_timeout,
            stream_chunk_size: self.stream_chunk_size,
            max_buf_size: self.max_buf_size,
            max_headers: self.max_headers,
            max_header_bytes: self.max_header_bytes,
            max_request_target_bytes: self.max_request_target_bytes,
            max_in_flight_requests: self.max_in_flight_requests,
            keep_alive_idle_timeout: self.keep_alive_idle_timeout,
            max_requests_per_connection: self.max_requests_per_connection,
            response_write_timeout: self.response_write_timeout,
            max_active_tunnels: self.max_active_tunnels,
        };
        let violations = shared.validate();
        if !violations.is_empty() {
            let msg = violations
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ServerError::Config(msg));
        }
        self.response_policy
            .validate()
            .map_err(|e| ServerError::Config(format!("invalid response_policy: {e}")))?;
        self.trusted_proxy
            .validate()
            .map_err(|e| ServerError::Config(format!("invalid trusted_proxy: {e}")))?;
        Ok(())
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
    response_policy: Option<ResponsePolicy>,
    date_policy: Option<DatePolicy>,
    stripped_response_headers: Option<Vec<String>>,
    error_policy: Option<eggserve_primitives::policy::ErrorRepresentationPolicy>,
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
    trusted_proxy: Option<TrustedProxyConfig>,
}

impl RuntimeConfigBuilder {
    /// Set the bind address.
    pub fn bind(mut self, addr: SocketAddr) -> Self {
        self.bind = Some(addr);
        self
    }

    /// Set the maximum number of concurrent connections. Must be > 0.
    pub fn max_connections(mut self, max: usize) -> Self {
        self.max_connections = Some(max);
        self
    }

    /// Set the maximum number of concurrent file-stream responses.
    pub fn max_file_streams(mut self, max: usize) -> Self {
        self.max_file_streams = Some(max);
        self
    }

    /// Set the file streaming read chunk size (64 bytes – 1 MiB).
    pub fn stream_chunk_size(mut self, size: usize) -> Self {
        self.stream_chunk_size = Some(size);
        self
    }

    /// Set the header-read timeout.
    pub fn header_read_timeout(mut self, timeout: Duration) -> Self {
        self.header_read_timeout = Some(timeout);
        self
    }

    /// Set the TLS handshake timeout (shared-kernel parity; the direct H1
    /// driver performs no handshake itself).
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
    pub fn handler_timeout(mut self, timeout: Duration) -> Self {
        self.handler_timeout = Some(timeout);
        self
    }

    /// Set the body read timeout (total deadline, not idle).
    pub fn body_read_timeout(mut self, timeout: Duration) -> Self {
        self.body_read_timeout = Some(timeout);
        self
    }

    /// Set the graceful shutdown grace period.
    pub fn graceful_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.graceful_shutdown_timeout = Some(timeout);
        self
    }

    /// Set the server identification header value (`None` suppresses).
    pub fn server_header(mut self, header: String) -> Self {
        self.server_header = Some(header);
        self
    }

    /// Set the complete final-boundary response privacy policy.
    pub fn response_policy(mut self, policy: ResponsePolicy) -> Self {
        self.response_policy = Some(policy);
        self
    }

    /// Set the `Date` generation policy.
    pub fn date_policy(mut self, policy: DatePolicy) -> Self {
        self.date_policy = Some(policy);
        self
    }

    /// Set the validated denylist of outbound response header names.
    pub fn stripped_response_headers(mut self, headers: Vec<String>) -> Self {
        self.stripped_response_headers = Some(headers);
        self
    }

    /// Set the canonical runtime-error representation.
    pub fn error_policy(
        mut self,
        policy: eggserve_primitives::policy::ErrorRepresentationPolicy,
    ) -> Self {
        self.error_policy = Some(policy);
        self
    }

    /// Set the maximum request body size in bytes (0 rejects all bodies).
    pub fn max_request_body_bytes(mut self, max: u64) -> Self {
        self.max_request_body_bytes = Some(max);
        self
    }

    /// Set the HTTP/1 parser/read buffer ceiling (min 8192).
    pub fn max_buf_size(mut self, max: usize) -> Self {
        self.max_buf_size = Some(max);
        self
    }

    /// Set the maximum request header field count.
    pub fn max_headers(mut self, max: usize) -> Self {
        self.max_headers = Some(max);
        self
    }

    /// Set the aggregate post-parse request-header ceiling in bytes.
    pub fn max_header_bytes(mut self, max: usize) -> Self {
        self.max_header_bytes = Some(max);
        self
    }

    /// Set the maximum request-target length in bytes.
    pub fn max_request_target_bytes(mut self, max: usize) -> Self {
        self.max_request_target_bytes = Some(max);
        self
    }

    /// Set the maximum concurrent in-flight service executions.
    pub fn max_in_flight_requests(mut self, max: usize) -> Self {
        self.max_in_flight_requests = Some(max);
        self
    }

    /// Set the keep-alive idle timeout.
    pub fn keep_alive_idle_timeout(mut self, timeout: Duration) -> Self {
        self.keep_alive_idle_timeout = Some(timeout);
        self
    }

    /// Set the maximum completed requests per connection (`None` unlimited).
    pub fn max_requests_per_connection(mut self, max: Option<u64>) -> Self {
        self.max_requests_per_connection = Some(max);
        self
    }

    /// Set the response write no-progress timeout.
    pub fn response_write_timeout(mut self, timeout: Duration) -> Self {
        self.response_write_timeout = Some(timeout);
        self
    }

    /// Set the maximum concurrent active tunnels (Plan 216 direct authority).
    pub fn max_active_tunnels(mut self, max: usize) -> Self {
        self.max_active_tunnels = Some(max);
        self
    }

    /// Set the complete trusted-proxy policy (defaults trust nothing).
    pub fn trusted_proxy(mut self, config: TrustedProxyConfig) -> Self {
        self.trusted_proxy = Some(config);
        self
    }

    fn trusted_proxy_mut(&mut self) -> &mut TrustedProxyConfig {
        if self.trusted_proxy.is_none() {
            self.trusted_proxy = Some(TrustedProxyConfig::default());
        }
        self.trusted_proxy.as_mut().expect("just initialized")
    }

    /// Trust one immediate peer (`IP` or `IP/prefix`, no DNS).
    pub fn trusted_proxy_peer(mut self, prefix: IpPrefix) -> Self {
        self.trusted_proxy_mut().peers.push(prefix);
        self
    }

    /// Trust Unix-domain listeners for header-derived forwarding.
    pub fn trust_unix_local(mut self, trust: bool) -> Self {
        self.trusted_proxy_mut().trust_unix = trust;
        self
    }

    /// Enable or disable PROXY protocol preamble parsing (default disabled).
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

    /// Build the runtime configuration.
    ///
    /// Shared runtime checks delegate to the canonical kernel; the
    /// resulting error identifies the invalid field/constraint.
    pub fn build(self) -> Result<RuntimeConfig, ServerError> {
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
            return Err(ServerError::Config(msg));
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
        response_policy
            .validate()
            .map_err(|e| ServerError::Config(format!("invalid response_policy: {e}")))?;
        let trusted_proxy = self.trusted_proxy.unwrap_or_default();
        trusted_proxy
            .validate()
            .map_err(|e| ServerError::Config(format!("invalid trusted_proxy: {e}")))?;
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
        })
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
        assert_eq!(config.response_policy.server_identification, None);
        assert_eq!(config.server_header_value(), None);
        assert_eq!(config.max_request_body_bytes, 0);
        assert_eq!(config.max_buf_size, rl::DEFAULT_MAX_BUF_SIZE);
        assert_eq!(config.max_headers, rl::DEFAULT_MAX_HEADERS);
        assert_eq!(config.max_in_flight_requests, 64);
        assert_eq!(config.keep_alive_idle_timeout, Duration::from_secs(60));
        assert_eq!(config.max_requests_per_connection, None);
        assert_eq!(config.response_write_timeout, Duration::from_secs(30));
        assert_eq!(config.max_active_tunnels, 64);
    }

    #[test]
    fn builder_overrides() {
        let config = RuntimeConfig::builder()
            .bind("0.0.0.0:9000".parse().unwrap())
            .max_connections(128)
            .handler_timeout(Duration::from_secs(15))
            .body_read_timeout(Duration::from_secs(20))
            .server_header("eggserve/0.1".into())
            .max_request_body_bytes(1024 * 1024)
            .max_in_flight_requests(16)
            .max_requests_per_connection(Some(100))
            .build()
            .unwrap();
        assert_eq!(config.bind.port(), 9000);
        assert_eq!(config.max_connections, 128);
        assert_eq!(config.handler_timeout, Duration::from_secs(15));
        assert_eq!(
            config.response_policy.server_identification.as_deref(),
            Some("eggserve/0.1")
        );
        assert_eq!(config.server_header_value(), Some("eggserve/0.1"));
        assert_eq!(config.max_in_flight_requests, 16);
        assert_eq!(config.max_requests_per_connection, Some(100));
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
    fn zero_connections_returns_error() {
        let result = RuntimeConfig::builder().max_connections(0).build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("max_connections must be > 0"));
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
    fn direct_and_compat_defaults_agree() {
        // Plan 215 Workstream E: one defaults table. Spot-check the shared
        // kernel values routed through the direct builder.
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
        assert_eq!(config.max_buf_size, default.max_buf_size);
        assert_eq!(config.max_headers, default.max_headers);
        assert_eq!(config.max_header_bytes, default.max_header_bytes);
        assert_eq!(
            config.max_request_target_bytes,
            default.max_request_target_bytes
        );
        assert_eq!(
            config.max_in_flight_requests,
            default.max_in_flight_requests
        );
        assert_eq!(
            config.keep_alive_idle_timeout,
            default.keep_alive_idle_timeout
        );
        assert_eq!(
            config.max_requests_per_connection,
            default.max_requests_per_connection
        );
        assert_eq!(
            config.response_write_timeout,
            default.response_write_timeout
        );
        assert_eq!(config.max_active_tunnels, default.max_active_tunnels);
    }
}
