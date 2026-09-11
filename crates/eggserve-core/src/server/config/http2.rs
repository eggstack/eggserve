//! HTTP/2 transport configuration (Plan 206 Track E).
//!
//! Owns [`Http2Config`] (protocol-owned fields/defaults/validation).
//! Shared constraints stay in `crate::runtime_limits`; this module never
//! duplicates the shared default table. Validation is `pub(super)` for
//! the sibling `runtime` validator only.

use std::time::Duration;

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
    pub(super) fn validate(&self) -> Result<(), crate::server::errors::ServerError> {
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
