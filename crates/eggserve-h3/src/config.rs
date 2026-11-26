//! HTTP/3/QUIC transport configuration (Plan 220 authority).
//!
//! Owns [`Http3Config`] (protocol-owned fields/defaults/validation).
//! Shared runtime constraints stay in `eggserve-server::runtime_limits`.
//! The compatibility core re-exports this type; this crate is the single
//! implementation authority. H3 remains experimental.

use std::time::Duration;

/// Largest value representable by a QUIC VarInt (2^62 - 1).
const QUIC_VARINT_MAX: u64 = (1 << 62) - 1;

/// EggServe-owned HTTP/3 and QUIC resource limits.
///
/// HTTP/3 is experimental. The defaults keep the product disabled until a
/// caller supplies a QUIC TLS identity (see `quic` endpoint helpers and
/// the compatibility `ServerBuilder::http3_identity`).
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

impl Http3Config {
    /// Validate H3/QUIC transport bounds.
    ///
    /// Called by the compatibility `RuntimeConfig` builder when `enabled`,
    /// and directly by H3 consumers. Returns `ServerError::Config` naming
    /// the invalid field.
    pub fn validate(&self) -> Result<(), eggserve_server::errors::ServerError> {
        let invalid = |field: &str, detail: &str| {
            eggserve_server::errors::ServerError::Config(format!("invalid http3.{field}: {detail}"))
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
        if self.stream_receive_window == 0 || self.stream_receive_window > QUIC_VARINT_MAX {
            return Err(invalid(
                "stream_receive_window",
                "must be between 1 and 2^62-1 bytes",
            ));
        }
        if self.connection_receive_window == 0 || self.connection_receive_window > QUIC_VARINT_MAX {
            return Err(invalid(
                "connection_receive_window",
                "must be between 1 and 2^62-1 bytes",
            ));
        }
        if self.connection_receive_window < self.stream_receive_window {
            return Err(invalid(
                "connection_receive_window",
                "must be at least stream_receive_window",
            ));
        }
        if self.send_window == 0 {
            return Err(invalid("send_window", "must be greater than zero"));
        }
        if self.max_send_buf_size == 0 {
            return Err(invalid("max_send_buf_size", "must be greater than zero"));
        }
        if self.max_idle_timeout.is_zero()
            || self.max_idle_timeout.as_millis() > u128::from(QUIC_VARINT_MAX)
        {
            return Err(invalid(
                "max_idle_timeout",
                "must be between 1ms and 2^62-1ms",
            ));
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

#[cfg(test)]
mod tests {
    use super::{Http3Config, QUIC_VARINT_MAX};
    use std::time::Duration;

    #[test]
    fn quic_window_bounds_are_validated() {
        let too_large = Http3Config {
            stream_receive_window: QUIC_VARINT_MAX + 1,
            ..Default::default()
        };
        let error = too_large.validate().unwrap_err().to_string();
        assert!(error.contains("http3.stream_receive_window"), "{error}");

        let too_large = Http3Config {
            connection_receive_window: QUIC_VARINT_MAX + 1,
            ..Default::default()
        };
        let error = too_large.validate().unwrap_err().to_string();
        assert!(error.contains("http3.connection_receive_window"), "{error}");

        let at_limit = Http3Config {
            stream_receive_window: QUIC_VARINT_MAX,
            connection_receive_window: QUIC_VARINT_MAX,
            ..Default::default()
        };
        assert!(at_limit.validate().is_ok());
    }

    /// A zero send buffer must be reported against `max_send_buf_size`.
    /// The earlier combined check blamed `send_window` for either field.
    #[test]
    fn zero_send_buf_size_names_its_own_field() {
        let zero_buffer = Http3Config {
            max_send_buf_size: 0,
            ..Default::default()
        };
        let error = zero_buffer.validate().unwrap_err().to_string();
        assert!(error.contains("http3.max_send_buf_size"), "{error}");
        assert!(!error.contains("send_window"), "{error}");

        let zero_window = Http3Config {
            send_window: 0,
            ..Default::default()
        };
        let error = zero_window.validate().unwrap_err().to_string();
        assert!(error.contains("http3.send_window"), "{error}");
    }

    #[test]
    fn quic_idle_timeout_bound_is_validated() {
        let too_large = Http3Config {
            max_idle_timeout: Duration::from_millis(QUIC_VARINT_MAX + 1),
            ..Default::default()
        };
        let error = too_large.validate().unwrap_err().to_string();
        assert!(error.contains("http3.max_idle_timeout"), "{error}");

        let at_limit = Http3Config {
            max_idle_timeout: Duration::from_millis(QUIC_VARINT_MAX),
            ..Default::default()
        };
        assert!(at_limit.validate().is_ok());
    }
}
