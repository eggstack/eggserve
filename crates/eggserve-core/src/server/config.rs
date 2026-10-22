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

//! Module layout (Plan 206 Track E): `runtime` owns the single
//! [`RuntimeConfig`] validation authority; `http1`/`http2`/`http3`/`tls`
//! own protocol-specific fields/defaults/validation. Shared constraints
//! stay in `crate::runtime_limits`; no duplicate tables, no new knobs.
//! Public construction paths are re-exported here.

pub mod http1;
#[cfg(feature = "http2")]
pub mod http2;
#[cfg(feature = "http3")]
pub mod http3;
pub mod runtime;
pub mod tls;

#[cfg(feature = "http2")]
pub use http2::Http2Config;
#[cfg(feature = "http3")]
pub use http3::Http3Config;
pub use runtime::RuntimeConfig;

/// Direct-H1 ownership vocabulary reachable through the compatibility
/// configuration.
///
/// These are the direct crate's own types (no compatibility duplicates), so
/// a compatibility consumer and a direct embedder configure the same
/// values with the same names.
pub use eggserve_server::{
    AdmissionOwner, AdmissionOwnership, H1PolicyOwnership, Http1RequestTargetMode, PolicyOwner,
    RuntimeErrorPresentation, RuntimeRejection, RuntimeRejectionKind, RuntimeRejectionPresenter,
};

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

/// Builder for [`RuntimeConfig`].
///
/// The shared H1 kernel is owned by the direct builder
/// ([`eggserve_server::RuntimeConfigBuilder`]): every shared setter below
/// delegates to it instead of re-implementing the value plumbing, so the
/// compatibility builder cannot drift from the direct one. The
/// compatibility-only fields (TLS, HTTP/2, HTTP/3) stay local because their
/// orchestration remains compatibility-owned.
#[derive(Debug, Default)]
#[must_use]
pub struct RuntimeConfigBuilder {
    direct: eggserve_server::RuntimeConfigBuilder,
    #[cfg(feature = "tls")]
    tls_config: Option<Arc<rustls::ServerConfig>>,
    #[cfg(feature = "tls")]
    tls_reload_handle: Option<crate::tls::TlsReloadHandle>,
    #[cfg(feature = "tls")]
    tls_expose_peer_chain: Option<bool>,
    #[cfg(feature = "http2")]
    http2: Option<Http2Config>,
    #[cfg(feature = "http3")]
    http3: Option<Http3Config>,
}

impl RuntimeConfigBuilder {
    /// Select the HTTP/1 request-target forms delivered to the service.
    ///
    /// Delegates to the direct runtime builder. Default: `OriginOnly`
    /// (absolute-form request targets are rejected before service dispatch).
    pub fn http1_request_target_mode(
        mut self,
        mode: eggserve_server::Http1RequestTargetMode,
    ) -> Self {
        self.direct = self.direct.http1_request_target_mode(mode);
        self
    }

    /// Select explicit ownership for H1 deadlines and semantic ceilings.
    ///
    /// Delegates to the direct runtime builder. Default: every selected
    /// policy stays EggServe-owned.
    pub fn policy_ownership(mut self, ownership: eggserve_server::H1PolicyOwnership) -> Self {
        self.direct = self.direct.policy_ownership(ownership);
        self
    }

    /// Select explicit ownership for H1 service-call and tunnel admission.
    ///
    /// Delegates to the direct runtime builder. Default: admission stays
    /// EggServe-owned.
    pub fn admission_ownership(mut self, ownership: eggserve_server::AdmissionOwnership) -> Self {
        self.direct = self.direct.admission_ownership(ownership);
        self
    }

    /// Install a synchronous, bounded presenter for runtime-generated
    /// rejections.
    ///
    /// Delegates to the direct runtime builder. Response framing, status
    /// selection, and the stripped-header denylist stay runtime-owned.
    pub fn runtime_rejection_presenter(
        mut self,
        presenter: Arc<dyn eggserve_server::rejection::RuntimeRejectionPresenter>,
    ) -> Self {
        self.direct = self.direct.runtime_rejection_presenter(presenter);
        self
    }

    /// Set the bind address.
    pub fn bind(mut self, addr: SocketAddr) -> Self {
        self.direct = self.direct.bind(addr);
        self
    }

    /// Set the maximum number of concurrent connections.
    ///
    /// Must be > 0. Default: 64.
    pub fn max_connections(mut self, max: usize) -> Self {
        self.direct = self.direct.max_connections(max);
        self
    }

    /// Set the maximum number of concurrent file-stream responses.
    ///
    /// Must be > 0. Default: 32.
    pub fn max_file_streams(mut self, max: usize) -> Self {
        self.direct = self.direct.max_file_streams(max);
        self
    }

    /// Set the file streaming read chunk size.
    ///
    /// Must be between 64 bytes and 1 MiB. Default: 128 KiB.
    pub fn stream_chunk_size(mut self, size: usize) -> Self {
        self.direct = self.direct.stream_chunk_size(size);
        self
    }

    /// Set the header-read timeout.
    pub fn header_read_timeout(mut self, timeout: Duration) -> Self {
        self.direct = self.direct.header_read_timeout(timeout);
        self
    }

    /// Set the TLS handshake timeout.
    pub fn tls_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.direct = self.direct.tls_handshake_timeout(timeout);
        self
    }

    /// Set the maximum connection lifetime.
    ///
    /// `Duration::ZERO` disables the total lifetime ceiling while the
    /// independent runtime bounds stay active.
    pub fn connection_total_timeout(mut self, timeout: Duration) -> Self {
        self.direct = self.direct.connection_total_timeout(timeout);
        self
    }

    /// Disable the total connection lifetime ceiling.
    ///
    /// Idle, header, handler, body, response-write, admission, and shutdown
    /// bounds remain active.
    pub fn disable_connection_total_timeout(mut self) -> Self {
        self.direct = self.direct.disable_connection_total_timeout();
        self
    }

    /// Set the handler invocation timeout.
    pub fn handler_timeout(mut self, timeout: Duration) -> Self {
        self.direct = self.direct.handler_timeout(timeout);
        self
    }

    /// Set the body read timeout (total deadline, not idle).
    pub fn body_read_timeout(mut self, timeout: Duration) -> Self {
        self.direct = self.direct.body_read_timeout(timeout);
        self
    }

    /// Set the graceful shutdown grace period.
    pub fn graceful_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.direct = self.direct.graceful_shutdown_timeout(timeout);
        self
    }

    /// Set the server identification header value.
    ///
    /// Composed into the response policy at build time.
    pub fn server_header(mut self, header: String) -> Self {
        self.direct = self.direct.server_header(header);
        self
    }

    /// Set the complete final-boundary response privacy policy.
    pub fn response_policy(
        mut self,
        policy: crate::server::response_policy::ResponsePolicy,
    ) -> Self {
        self.direct = self.direct.response_policy(policy);
        self
    }

    /// Set the `Date` generation policy.
    ///
    /// Composed into the response policy at build time.
    pub fn date_policy(mut self, policy: crate::server::response_policy::DatePolicy) -> Self {
        self.direct = self.direct.date_policy(policy);
        self
    }

    /// Set the validated denylist of outbound response header names.
    ///
    /// Applied after service construction; framing/hop-by-hop headers and
    /// `date` cannot be denylisted (see
    /// [`crate::server::response_policy::validate_stripped_header_name`]).
    pub fn stripped_response_headers(mut self, headers: Vec<String>) -> Self {
        self.direct = self.direct.stripped_response_headers(headers);
        self
    }

    /// Set the canonical runtime-error representation.
    ///
    /// `Minimal` (default) emits fixed generic plain-text bodies;
    /// `Empty` emits no body bytes for runtime-generated errors. Application
    /// `Ok` bodies are never rewritten.
    pub fn error_policy(mut self, policy: crate::policy::ErrorRepresentationPolicy) -> Self {
        self.direct = self.direct.error_policy(policy);
        self
    }

    /// Set the maximum request body size in bytes.
    ///
    /// This is the hard ceiling that no service can exceed. Default: 0
    /// (bodies rejected). Set to a positive value to allow request bodies.
    pub fn max_request_body_bytes(mut self, max: u64) -> Self {
        self.direct = self.direct.max_request_body_bytes(max);
        self
    }

    /// Set the HTTP/1 parser/read buffer ceiling in bytes.
    ///
    /// Must be between 8192 (Hyper minimum) and 4 MiB. Default: 64 KiB.
    pub fn max_buf_size(mut self, max: usize) -> Self {
        self.direct = self.direct.max_buf_size(max);
        self
    }

    /// Set the maximum request header field count.
    ///
    /// Must be between 1 and 10_000. Default: 100 (Hyper's default, pinned
    /// explicitly). Hyper answers excess with 431.
    pub fn max_headers(mut self, max: usize) -> Self {
        self.direct = self.direct.max_headers(max);
        self
    }

    /// Set the aggregate post-parse request-header ceiling in name+value
    /// bytes. Must be between 1 KiB and 1 MiB. Default: 32 KiB.
    pub fn max_header_bytes(mut self, max: usize) -> Self {
        self.direct = self.direct.max_header_bytes(max);
        self
    }

    /// Set the maximum request-target length in bytes. Must be between 128
    /// and 64 KiB. Default: 8192.
    pub fn max_request_target_bytes(mut self, max: usize) -> Self {
        self.direct = self.direct.max_request_target_bytes(max);
        self
    }

    /// Set the maximum concurrent in-flight service executions.
    ///
    /// Must be > 0. Default: 64.
    pub fn max_in_flight_requests(mut self, max: usize) -> Self {
        self.direct = self.direct.max_in_flight_requests(max);
        self
    }

    /// Set the keep-alive idle timeout.
    ///
    /// Independent of `connection_total_timeout`: this deadline resets on
    /// request/transport activity, while the total lifetime never resets.
    pub fn keep_alive_idle_timeout(mut self, timeout: Duration) -> Self {
        self.direct = self.direct.keep_alive_idle_timeout(timeout);
        self
    }

    /// Set the maximum completed requests per connection.
    ///
    /// Pass `None` for unlimited (default). `Some(0)` is rejected.
    pub fn max_requests_per_connection(mut self, max: Option<u64>) -> Self {
        self.direct = self.direct.max_requests_per_connection(max);
        self
    }

    /// Set the response write no-progress timeout.
    ///
    /// Fires only after the configured interval with zero forward socket
    /// progress while a response body is outstanding.
    pub fn response_write_timeout(mut self, timeout: Duration) -> Self {
        self.direct = self.direct.response_write_timeout(timeout);
        self
    }

    /// Set the maximum concurrent active tunnels.
    ///
    /// Must be > 0. Default: 64. Exhaustion fails new handshakes with 503.
    pub fn max_active_tunnels(mut self, max: usize) -> Self {
        self.direct = self.direct.max_active_tunnels(max);
        self
    }

    /// Set the complete trusted-proxy policy (Plan 202).
    ///
    /// Defaults trust nothing. Use this for explicit `TrustedProxyConfig`
    /// values; the convenience setters below mutate the same policy.
    pub fn trusted_proxy(mut self, config: crate::primitives::proxy::TrustedProxyConfig) -> Self {
        self.direct = self.direct.trusted_proxy(config);
        self
    }

    /// Trust one immediate peer (`IP` or `IP/prefix`, no DNS).
    ///
    /// Repeatable via chaining. Loopback must be listed explicitly; it is
    /// never implicitly trusted. Invalid entries fail at [`Self::build`].
    pub fn trusted_proxy_peer(mut self, prefix: crate::primitives::proxy::IpPrefix) -> Self {
        self.direct = self.direct.trusted_proxy_peer(prefix);
        self
    }

    /// Trust Unix-domain listeners for header-derived forwarding.
    ///
    /// Never implicit. PROXY preambles are not read from Unix streams.
    pub fn trust_unix_local(mut self, trust: bool) -> Self {
        self.direct = self.direct.trust_unix_local(trust);
        self
    }

    /// Enable or disable PROXY protocol preamble parsing (default disabled).
    ///
    /// When enabled, only explicitly trusted peers may send a preamble;
    /// all other peers close before TLS/HTTP. Disabled listeners interpret
    /// bytes normally with no auto-detection.
    pub fn proxy_protocol_enabled(mut self, enabled: bool) -> Self {
        self.direct = self.direct.proxy_protocol_enabled(enabled);
        self
    }

    /// Set the PROXY preamble read timeout (default 5s, max 60s).
    pub fn proxy_protocol_timeout(mut self, timeout: Duration) -> Self {
        self.direct = self.direct.proxy_protocol_timeout(timeout);
        self
    }

    /// Honor the standardized `Forwarded` header from trusted peers.
    pub fn forwarded_standard(mut self, enabled: bool) -> Self {
        self.direct = self.direct.forwarded_standard(enabled);
        self
    }

    /// Honor legacy `X-Forwarded-*` headers from trusted peers.
    pub fn forwarded_legacy(mut self, enabled: bool) -> Self {
        self.direct = self.direct.forwarded_legacy(enabled);
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
    /// The shared H1 kernel, the composed `ResponsePolicy`, and the
    /// trusted-proxy policy are validated by the direct builder, which is
    /// the single authority for those constraints. The compatibility-only
    /// HTTP/2 and HTTP/3 policies are validated locally.
    pub fn build(self) -> Result<RuntimeConfig, crate::server::errors::ServerError> {
        let direct = self.direct.build()?;
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
        Ok(RuntimeConfig::from_direct_config(
            direct,
            #[cfg(feature = "tls")]
            self.tls_config,
            #[cfg(feature = "tls")]
            self.tls_reload_handle,
            #[cfg(feature = "tls")]
            self.tls_expose_peer_chain.unwrap_or(false),
            #[cfg(feature = "http2")]
            http2,
            #[cfg(feature = "http3")]
            http3,
        ))
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
    let shared = eggserve_server::runtime_limits::SharedRuntimeValues::from(&config.limits);
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
    fn zero_connection_total_timeout_is_accepted() {
        let result = RuntimeConfig::builder()
            .connection_total_timeout(Duration::ZERO)
            .build();
        assert_eq!(result.unwrap().connection_total_timeout, Duration::ZERO);
        assert_eq!(
            RuntimeConfig::builder()
                .disable_connection_total_timeout()
                .build()
                .unwrap()
                .connection_total_timeout,
            Duration::ZERO
        );
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
        assert_eq!(errs.len(), 6);
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

    // ------------------------------------------------------------------
    // Plan 308: compatibility/direct projection parity
    // ------------------------------------------------------------------

    /// The compatibility projection must equal the direct default for every
    /// shared field and every ownership knob.
    #[test]
    fn default_compatibility_projection_matches_the_direct_default() {
        let projected = RuntimeConfig::default().direct_h1_config();
        let direct = eggserve_server::RuntimeConfig::default();
        assert_eq!(
            projected.http1_request_target_mode,
            direct.http1_request_target_mode
        );
        assert_eq!(projected.policy_ownership, direct.policy_ownership);
        assert_eq!(projected.admission_ownership, direct.admission_ownership);
        assert!(projected.runtime_rejection_presenter.is_none());
        assert_eq!(projected.bind, direct.bind);
        assert_eq!(projected.max_connections, direct.max_connections);
        assert_eq!(projected.max_file_streams, direct.max_file_streams);
        assert_eq!(projected.stream_chunk_size, direct.stream_chunk_size);
        assert_eq!(projected.header_read_timeout, direct.header_read_timeout);
        assert_eq!(
            projected.tls_handshake_timeout,
            direct.tls_handshake_timeout
        );
        assert_eq!(
            projected.connection_total_timeout,
            direct.connection_total_timeout
        );
        assert_eq!(projected.handler_timeout, direct.handler_timeout);
        assert_eq!(projected.body_read_timeout, direct.body_read_timeout);
        assert_eq!(
            projected.graceful_shutdown_timeout,
            direct.graceful_shutdown_timeout
        );
        assert_eq!(
            projected.response_policy.server_identification,
            direct.response_policy.server_identification
        );
        assert!(matches!(
            (
                &projected.response_policy.date_policy,
                &direct.response_policy.date_policy
            ),
            (
                crate::server::response_policy::DatePolicy::SystemClock,
                crate::server::response_policy::DatePolicy::SystemClock
            )
        ));
        assert_eq!(
            projected.response_policy.error_policy,
            direct.response_policy.error_policy
        );
        assert_eq!(
            projected.response_policy.stripped_response_headers,
            direct.response_policy.stripped_response_headers
        );
        assert_eq!(
            projected.max_request_body_bytes,
            direct.max_request_body_bytes
        );
        assert_eq!(projected.max_buf_size, direct.max_buf_size);
        assert_eq!(projected.max_headers, direct.max_headers);
        assert_eq!(projected.max_header_bytes, direct.max_header_bytes);
        assert_eq!(
            projected.max_request_target_bytes,
            direct.max_request_target_bytes
        );
        assert_eq!(
            projected.max_in_flight_requests,
            direct.max_in_flight_requests
        );
        assert_eq!(
            projected.keep_alive_idle_timeout,
            direct.keep_alive_idle_timeout
        );
        assert_eq!(
            projected.max_requests_per_connection,
            direct.max_requests_per_connection
        );
        assert_eq!(
            projected.response_write_timeout,
            direct.response_write_timeout
        );
        assert_eq!(projected.max_active_tunnels, direct.max_active_tunnels);
        assert_eq!(projected.trusted_proxy.peers, direct.trusted_proxy.peers);
    }

    /// Each of the four ownership knobs must reach the projected direct
    /// configuration unchanged.
    #[test]
    fn ownership_knobs_round_trip_into_the_projected_direct_config() {
        let config = RuntimeConfig::builder()
            .http1_request_target_mode(Http1RequestTargetMode::OriginOrAbsolute)
            .policy_ownership(H1PolicyOwnership {
                handler_deadline: PolicyOwner::External,
                ..H1PolicyOwnership::default()
            })
            .admission_ownership(AdmissionOwnership {
                service_calls: AdmissionOwner::External,
                ..AdmissionOwnership::default()
            })
            .build()
            .unwrap();
        let projected = config.direct_h1_config();
        assert_eq!(
            projected.http1_request_target_mode,
            Http1RequestTargetMode::OriginOrAbsolute
        );
        assert_eq!(
            projected.policy_ownership.handler_deadline,
            PolicyOwner::External
        );
        assert_eq!(
            projected.admission_ownership.service_calls,
            AdmissionOwner::External
        );
        assert!(projected.runtime_rejection_presenter.is_none());
        // The compatibility configuration keeps the same values.
        assert_eq!(
            config.http1_request_target_mode,
            Http1RequestTargetMode::OriginOrAbsolute
        );
        assert_eq!(
            config.policy_ownership.handler_deadline,
            PolicyOwner::External
        );
        assert_eq!(
            config.admission_ownership.service_calls,
            AdmissionOwner::External
        );
    }

    /// The rejection presenter must survive the projection.
    #[test]
    fn runtime_rejection_presenter_round_trips_into_the_projected_direct_config() {
        #[derive(Debug)]
        struct NoopPresenter;
        impl RuntimeRejectionPresenter for NoopPresenter {
            fn present(&self, _rejection: &RuntimeRejection) -> Option<RuntimeErrorPresentation> {
                None
            }
        }
        let config = RuntimeConfig::builder()
            .runtime_rejection_presenter(Arc::new(NoopPresenter))
            .build()
            .unwrap();
        assert!(config.runtime_rejection_presenter.is_some());
        assert!(config
            .direct_h1_config()
            .runtime_rejection_presenter
            .is_some());
    }

    /// The six setters whose semantics differ from a plain value copy keep
    /// their exact behavior after the delegation change.
    #[test]
    fn non_duplicated_setters_keep_their_semantics() {
        // `response_policy` replaces the whole policy.
        let policy = crate::server::response_policy::ResponsePolicy {
            server_identification: Some("example/1".to_owned()),
            ..Default::default()
        };
        let config = RuntimeConfig::builder()
            .response_policy(policy)
            .build()
            .unwrap();
        assert_eq!(
            config.response_policy.server_identification.as_deref(),
            Some("example/1")
        );
        assert_eq!(config.server_header_value(), Some("example/1"));

        // `date_policy` and `error_policy` compose into the response policy.
        let config = RuntimeConfig::builder()
            .response_policy(crate::server::response_policy::ResponsePolicy {
                server_identification: Some("kept/1".to_owned()),
                ..Default::default()
            })
            .date_policy(crate::server::response_policy::DatePolicy::Suppress)
            .error_policy(crate::policy::ErrorRepresentationPolicy::Empty)
            .build()
            .unwrap();
        assert_eq!(
            config.response_policy.server_identification.as_deref(),
            Some("kept/1")
        );
        assert!(matches!(
            config.response_policy.date_policy,
            crate::server::response_policy::DatePolicy::Suppress
        ));
        assert_eq!(
            config.response_policy.error_policy,
            crate::policy::ErrorRepresentationPolicy::Empty
        );

        // `trusted_proxy` replaces the policy; `trusted_proxy_peer` mutates it.
        let config = RuntimeConfig::builder()
            .trusted_proxy(crate::primitives::proxy::TrustedProxyConfig {
                trust_unix: true,
                ..Default::default()
            })
            .trusted_proxy_peer(crate::primitives::proxy::IpPrefix::parse("127.0.0.1/32").unwrap())
            .build()
            .unwrap();
        assert!(config.trusted_proxy.trust_unix);
        assert_eq!(config.trusted_proxy.peers.len(), 1);

        // `build` still validates the shared kernel and the response policy.
        let err = RuntimeConfig::builder()
            .max_connections(0)
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("max_connections"));
        let err = RuntimeConfig::builder()
            .stripped_response_headers(vec!["content-length".to_owned()])
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("runtime-owned"));
    }
}
