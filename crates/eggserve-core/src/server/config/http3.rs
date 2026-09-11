//! HTTP/3/QUIC transport configuration (Plan 206 Track E).
//!
//! Owns [`Http3Config`] (protocol-owned fields/defaults/validation).
//! Shared constraints stay in `crate::runtime_limits`. Validation is
//! `pub(super)` for the sibling `runtime` validator only.

use std::time::Duration;

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
