//! Resource limits for connections, streams, and request sizes.

use std::fmt;
use std::time::Duration;

/// Default maximum number of entries to enumerate in a directory listing.
pub const DEFAULT_MAX_LISTING_ENTRIES: usize = 4096;
pub const MAX_LISTING_RESPONSE_BYTES: usize = 10 * 1024 * 1024;
/// Upper bound for `max_listing_entries`. This is an entry count, not a
/// byte size; it numerically matches the historical acceptance range.
pub const MAX_LISTING_ENTRIES: usize = 10 * 1024 * 1024;
/// Upper bound for `max_request_body_bytes`. The default of `0` rejects all
/// bodies; explicit values are capped so a single config value cannot become
/// an unbounded per-request buffering knob.
pub const MAX_REQUEST_BODY_BYTES: u64 = 1024 * 1024 * 1024;
pub const DEFAULT_STREAM_CHUNK_SIZE: usize = 8192;
pub const DEFAULT_MAX_EXTRA_HEADERS: usize = 32;
pub const DEFAULT_MAX_EXTRA_HEADER_BYTES: usize = 8 * 1024;
/// Default HTTP/1 parser read-buffer ceiling (Plan 164).
///
/// Hyper's own default is ~400 KiB and explicitly not stable; this
/// EggServe-owned default preserves ordinary browser/proxy compatibility
/// while bounding per-connection parser memory. Lower values reduce peak
/// memory under many concurrent slow connections.
pub const DEFAULT_MAX_BUF_SIZE: usize = 64 * 1024;
/// Minimum parser buffer accepted by Hyper (`Builder::max_buf_size` panics
/// below this).
pub const MIN_MAX_BUF_SIZE: usize = 8192;
/// Maximum parser buffer EggServe will configure (4 MiB).
pub const MAX_MAX_BUF_SIZE: usize = 4 * 1024 * 1024;
/// Default maximum request header field count (Plan 164).
///
/// Matches Hyper's default of 100 so setting it explicitly pins the policy
/// instead of inheriting a value Hyper documents as unstable. Note Hyper
/// allocates header storage on the heap (instead of the stack fast path)
/// once a custom count is set, costing roughly 5% header-parse performance.
/// The same bound also caps HTTP/1 trailers.
pub const DEFAULT_MAX_HEADERS: usize = 100;
/// Maximum header-field count EggServe will configure.
pub const MAX_MAX_HEADERS: usize = 10_000;
/// Default post-parse aggregate request-header ceiling in name+value bytes
/// (Plan 164). Hyper exposes no aggregate byte knob, so this is enforced by
/// EggServe after parsing and before service invocation; excess fails with
/// 431 without invoking the service.
pub const DEFAULT_MAX_HEADER_BYTES: usize = 32 * 1024;
pub const MIN_MAX_HEADER_BYTES: usize = 1024;
pub const MAX_MAX_HEADER_BYTES: usize = 1024 * 1024;
/// Default maximum request-target length in bytes (Plan 164).
///
/// Enforced after parsing and before service invocation; excess fails with
/// 414. This is distinct from the parser buffer: a short buffer already
/// rejects huge targets at parse time, but this bound gives operators an
/// explicit, observable application-level ceiling.
pub const DEFAULT_MAX_REQUEST_TARGET_BYTES: usize = 8192;
pub const MIN_MAX_REQUEST_TARGET_BYTES: usize = 128;
pub const MAX_MAX_REQUEST_TARGET_BYTES: usize = 64 * 1024;
/// Default maximum concurrent in-flight service (`Service::call`)
/// executions, independent of idle keep-alive connections (Plan 164).
///
/// Idle keep-alive sockets hold the connection budget only; handler
/// concurrency is governed here. The default matches `max_connections` so
/// existing single-request-per-connection behavior is preserved while the
/// knob remains available for high-concurrency deployments.
pub const DEFAULT_MAX_IN_FLIGHT_REQUESTS: usize = 64;

/// Error returned when a [`Limits`] field violates its constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitsError {
    /// The field that failed validation.
    pub field: &'static str,
    /// The rejected value.
    pub value: String,
    /// Human-readable constraint description.
    pub constraint: String,
}

impl fmt::Display for LimitsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} must be {}: got {}",
            self.field, self.constraint, self.value
        )
    }
}

impl std::error::Error for LimitsError {}

#[derive(Debug, Clone)]
#[must_use]
pub struct Limits {
    pub max_connections: usize,
    pub max_file_streams: usize,
    pub(crate) max_request_body_bytes: u64,
    pub header_read_timeout: Duration,
    /// Timeout for a TLS handshake. Default: 10s.
    pub tls_handshake_timeout: Duration,
    pub connection_total_timeout: Duration,
    /// Timeout for a single handler invocation. Default: 30s.
    pub handler_timeout: Duration,
    /// Timeout for reading the request body (total deadline, not idle).
    /// Default: 30s.
    pub body_read_timeout: Duration,
    pub graceful_shutdown_timeout: Duration,
    /// Maximum number of entries to enumerate in a directory listing.
    pub max_listing_entries: usize,
    /// Maximum size in bytes for a directory listing response body.
    pub max_listing_response_bytes: usize,
    /// Chunk size in bytes for file streaming reads.
    pub stream_chunk_size: usize,
    /// Maximum number of extra response headers per response.
    pub max_extra_headers: usize,
    /// Maximum combined name and value bytes for extra response headers.
    pub max_extra_header_bytes: usize,
    /// Maximum HTTP/1 parser/read buffer size in bytes. Minimum 8192
    /// (Hyper panics below). Default: 64 KiB.
    pub max_buf_size: usize,
    /// Maximum request header field count. Default: 100 (Hyper's default,
    /// pinned explicitly so upgrades cannot silently widen it).
    pub max_headers: usize,
    /// Maximum aggregate post-parse request-header name+value bytes.
    /// Enforced before service invocation; excess fails with 431.
    /// Default: 32 KiB.
    pub max_header_bytes: usize,
    /// Maximum request-target length in bytes. Enforced before service
    /// invocation; excess fails with 414. Default: 8192.
    pub max_request_target_bytes: usize,
    /// Maximum concurrent in-flight service executions, independent of
    /// idle keep-alive connections. Exhaustion fails with a deterministic
    /// generic 503. Default: 64.
    pub max_in_flight_requests: usize,
    /// Keep-alive idle timeout: a connection with no in-flight request
    /// and no outstanding response body is gracefully closed after this
    /// much inactivity. Resets on request/transport activity. Default: 60s.
    pub keep_alive_idle_timeout: Duration,
    /// Maximum completed requests per connection. `None` disables the
    /// limit. When reached, the current response completes correctly with
    /// `Connection: close`. Default: `None` (unlimited).
    pub max_requests_per_connection: Option<u64>,
    /// Response write no-progress timeout: a connection with an
    /// outstanding response body is closed after this much time with no
    /// forward socket-write progress. Steady progress — however slow —
    /// never triggers it. Default: 30s.
    pub response_write_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: 64,
            max_file_streams: 32,
            max_request_body_bytes: 0,
            header_read_timeout: Duration::from_secs(10),
            tls_handshake_timeout: Duration::from_secs(10),
            connection_total_timeout: Duration::from_secs(60),
            handler_timeout: Duration::from_secs(30),
            body_read_timeout: Duration::from_secs(30),
            graceful_shutdown_timeout: Duration::from_secs(10),
            max_listing_entries: DEFAULT_MAX_LISTING_ENTRIES,
            max_listing_response_bytes: 1024 * 1024, // 1 MiB
            stream_chunk_size: DEFAULT_STREAM_CHUNK_SIZE,
            max_extra_headers: DEFAULT_MAX_EXTRA_HEADERS,
            max_extra_header_bytes: DEFAULT_MAX_EXTRA_HEADER_BYTES,
            max_buf_size: DEFAULT_MAX_BUF_SIZE,
            max_headers: DEFAULT_MAX_HEADERS,
            max_header_bytes: DEFAULT_MAX_HEADER_BYTES,
            max_request_target_bytes: DEFAULT_MAX_REQUEST_TARGET_BYTES,
            max_in_flight_requests: DEFAULT_MAX_IN_FLIGHT_REQUESTS,
            keep_alive_idle_timeout: Duration::from_secs(60),
            max_requests_per_connection: None,
            response_write_timeout: Duration::from_secs(30),
        }
    }
}

impl Limits {
    /// Validate all fields and return every constraint violation.
    ///
    /// Returns `Ok(())` if all fields satisfy their invariants. Returns `Err`
    /// with one [`LimitsError`] per violated field.
    pub fn validate(&self) -> Result<(), Vec<LimitsError>> {
        let mut errors = Vec::new();
        let max_semaphore_permits = tokio::sync::Semaphore::MAX_PERMITS;
        if self.max_connections == 0 {
            errors.push(LimitsError {
                field: "max_connections",
                value: "0".into(),
                constraint: "> 0".into(),
            });
        } else if self.max_connections > max_semaphore_permits {
            errors.push(LimitsError {
                field: "max_connections",
                value: self.max_connections.to_string(),
                constraint: format!("<= {} (Semaphore::MAX_PERMITS)", max_semaphore_permits),
            });
        }
        if self.max_file_streams == 0 {
            errors.push(LimitsError {
                field: "max_file_streams",
                value: "0".into(),
                constraint: "> 0".into(),
            });
        } else if self.max_file_streams > max_semaphore_permits {
            errors.push(LimitsError {
                field: "max_file_streams",
                value: self.max_file_streams.to_string(),
                constraint: format!("<= {} (Semaphore::MAX_PERMITS)", max_semaphore_permits),
            });
        }
        if self.header_read_timeout.is_zero() {
            errors.push(LimitsError {
                field: "header_read_timeout",
                value: "0s".into(),
                constraint: "> 0".into(),
            });
        }
        if self.tls_handshake_timeout.is_zero() {
            errors.push(LimitsError {
                field: "tls_handshake_timeout",
                value: "0s".into(),
                constraint: "> 0".into(),
            });
        }
        if self.connection_total_timeout.is_zero() {
            errors.push(LimitsError {
                field: "connection_total_timeout",
                value: "0s".into(),
                constraint: "> 0".into(),
            });
        }
        if self.header_read_timeout > self.connection_total_timeout {
            errors.push(LimitsError {
                field: "header_read_timeout",
                value: format!("{}s", self.header_read_timeout.as_secs()),
                constraint: "<= connection_total_timeout".into(),
            });
        }
        if self.handler_timeout.is_zero() {
            errors.push(LimitsError {
                field: "handler_timeout",
                value: "0s".into(),
                constraint: "> 0".into(),
            });
        }
        if self.body_read_timeout.is_zero() {
            errors.push(LimitsError {
                field: "body_read_timeout",
                value: "0s".into(),
                constraint: "> 0".into(),
            });
        }
        // A handler or body budget wider than the total connection
        // lifetime is dead configuration: the connection budget always
        // fires first and kills the request mid-flight. This mirrors the
        // RuntimeConfigBuilder::build() cross-field checks so the
        // ServeConfig bridge cannot bypass them.
        if self.handler_timeout > self.connection_total_timeout {
            errors.push(LimitsError {
                field: "handler_timeout",
                value: format!("{}s", self.handler_timeout.as_secs()),
                constraint: "<= connection_total_timeout".into(),
            });
        }
        if self.body_read_timeout > self.connection_total_timeout {
            errors.push(LimitsError {
                field: "body_read_timeout",
                value: format!("{}s", self.body_read_timeout.as_secs()),
                constraint: "<= connection_total_timeout".into(),
            });
        }
        if self.graceful_shutdown_timeout.is_zero() {
            errors.push(LimitsError {
                field: "graceful_shutdown_timeout",
                value: "0s".into(),
                constraint: "> 0".into(),
            });
        }
        if self.stream_chunk_size < 64 {
            errors.push(LimitsError {
                field: "stream_chunk_size",
                value: self.stream_chunk_size.to_string(),
                constraint: ">= 64".into(),
            });
        }
        if self.stream_chunk_size > 1024 * 1024 {
            errors.push(LimitsError {
                field: "stream_chunk_size",
                value: self.stream_chunk_size.to_string(),
                constraint: "<= 1048576 (1 MiB)".into(),
            });
        }
        if self.max_listing_response_bytes == 0 {
            errors.push(LimitsError {
                field: "max_listing_response_bytes",
                value: "0".into(),
                constraint: "> 0".into(),
            });
        } else if self.max_listing_response_bytes > MAX_LISTING_RESPONSE_BYTES {
            errors.push(LimitsError {
                field: "max_listing_response_bytes",
                value: self.max_listing_response_bytes.to_string(),
                constraint: format!("<= {} (10 MiB)", MAX_LISTING_RESPONSE_BYTES),
            });
        }
        // Zero silently renders every directory listing empty; entries above
        // the response-byte budget can never be fully rendered anyway.
        if self.max_listing_entries == 0 {
            errors.push(LimitsError {
                field: "max_listing_entries",
                value: "0".into(),
                constraint: "> 0".into(),
            });
        } else if self.max_listing_entries > MAX_LISTING_ENTRIES {
            errors.push(LimitsError {
                field: "max_listing_entries",
                value: self.max_listing_entries.to_string(),
                constraint: format!("<= {MAX_LISTING_ENTRIES} (entries)"),
            });
        }
        if self.max_request_body_bytes > MAX_REQUEST_BODY_BYTES {
            errors.push(LimitsError {
                field: "max_request_body_bytes",
                value: self.max_request_body_bytes.to_string(),
                constraint: format!(
                    "<= {} (1 GiB), or 0 to reject bodies",
                    MAX_REQUEST_BODY_BYTES
                ),
            });
        }
        if self.max_buf_size < MIN_MAX_BUF_SIZE {
            errors.push(LimitsError {
                field: "max_buf_size",
                value: self.max_buf_size.to_string(),
                constraint: format!(">= {MIN_MAX_BUF_SIZE} (Hyper minimum)"),
            });
        } else if self.max_buf_size > MAX_MAX_BUF_SIZE {
            errors.push(LimitsError {
                field: "max_buf_size",
                value: self.max_buf_size.to_string(),
                constraint: format!("<= {MAX_MAX_BUF_SIZE} (4 MiB)"),
            });
        }
        if self.max_headers == 0 {
            errors.push(LimitsError {
                field: "max_headers",
                value: "0".into(),
                constraint: "> 0".into(),
            });
        } else if self.max_headers > MAX_MAX_HEADERS {
            errors.push(LimitsError {
                field: "max_headers",
                value: self.max_headers.to_string(),
                constraint: format!("<= {MAX_MAX_HEADERS}"),
            });
        }
        if self.max_header_bytes < MIN_MAX_HEADER_BYTES {
            errors.push(LimitsError {
                field: "max_header_bytes",
                value: self.max_header_bytes.to_string(),
                constraint: format!(">= {MIN_MAX_HEADER_BYTES}"),
            });
        } else if self.max_header_bytes > MAX_MAX_HEADER_BYTES {
            errors.push(LimitsError {
                field: "max_header_bytes",
                value: self.max_header_bytes.to_string(),
                constraint: format!("<= {MAX_MAX_HEADER_BYTES} (1 MiB)"),
            });
        }
        if self.max_request_target_bytes < MIN_MAX_REQUEST_TARGET_BYTES {
            errors.push(LimitsError {
                field: "max_request_target_bytes",
                value: self.max_request_target_bytes.to_string(),
                constraint: format!(">= {MIN_MAX_REQUEST_TARGET_BYTES}"),
            });
        } else if self.max_request_target_bytes > MAX_MAX_REQUEST_TARGET_BYTES {
            errors.push(LimitsError {
                field: "max_request_target_bytes",
                value: self.max_request_target_bytes.to_string(),
                constraint: format!("<= {MAX_MAX_REQUEST_TARGET_BYTES} (64 KiB)"),
            });
        }
        if self.max_in_flight_requests == 0 {
            errors.push(LimitsError {
                field: "max_in_flight_requests",
                value: "0".into(),
                constraint: "> 0".into(),
            });
        } else if self.max_in_flight_requests > max_semaphore_permits {
            errors.push(LimitsError {
                field: "max_in_flight_requests",
                value: self.max_in_flight_requests.to_string(),
                constraint: format!("<= {} (Semaphore::MAX_PERMITS)", max_semaphore_permits),
            });
        }
        if self.keep_alive_idle_timeout.is_zero() {
            errors.push(LimitsError {
                field: "keep_alive_idle_timeout",
                value: "0s".into(),
                constraint: "> 0".into(),
            });
        }
        if self.max_requests_per_connection == Some(0) {
            errors.push(LimitsError {
                field: "max_requests_per_connection",
                value: "0".into(),
                constraint: ">= 1 or None (unlimited)".into(),
            });
        }
        if self.response_write_timeout.is_zero() {
            errors.push(LimitsError {
                field: "response_write_timeout",
                value: "0s".into(),
                constraint: "> 0".into(),
            });
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits_are_valid() {
        let limits = Limits::default();
        assert!(limits.validate().is_ok());
    }

    #[test]
    fn zero_max_connections_is_invalid() {
        let limits = Limits {
            max_connections: 0,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_connections"));
    }

    #[test]
    fn zero_max_file_streams_is_invalid() {
        let limits = Limits {
            max_file_streams: 0,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_file_streams"));
    }

    #[test]
    fn zero_header_read_timeout_is_invalid() {
        let limits = Limits {
            header_read_timeout: Duration::ZERO,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "header_read_timeout"));
    }

    #[test]
    fn zero_tls_handshake_timeout_is_invalid() {
        let limits = Limits {
            tls_handshake_timeout: Duration::ZERO,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "tls_handshake_timeout"));
    }

    #[test]
    fn zero_connection_total_timeout_is_invalid() {
        let limits = Limits {
            connection_total_timeout: Duration::ZERO,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "connection_total_timeout"));
    }

    #[test]
    fn header_timeout_cannot_exceed_connection_timeout() {
        let limits = Limits {
            header_read_timeout: Duration::from_secs(2),
            connection_total_timeout: Duration::from_secs(1),
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "header_read_timeout"));
    }

    #[test]
    fn handler_timeout_cannot_exceed_connection_timeout() {
        let limits = Limits {
            handler_timeout: Duration::from_secs(2),
            connection_total_timeout: Duration::from_secs(1),
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "handler_timeout"));
    }

    #[test]
    fn body_read_timeout_cannot_exceed_connection_timeout() {
        let limits = Limits {
            body_read_timeout: Duration::from_secs(2),
            connection_total_timeout: Duration::from_secs(1),
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "body_read_timeout"));
    }

    #[test]
    fn timeouts_equal_to_connection_timeout_are_valid() {
        let limits = Limits {
            header_read_timeout: Duration::from_secs(5),
            handler_timeout: Duration::from_secs(5),
            body_read_timeout: Duration::from_secs(5),
            connection_total_timeout: Duration::from_secs(5),
            ..Default::default()
        };
        assert!(limits.validate().is_ok());
    }

    #[test]
    fn zero_handler_timeout_is_invalid() {
        let limits = Limits {
            handler_timeout: Duration::ZERO,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "handler_timeout"));
    }

    #[test]
    fn zero_body_read_timeout_is_invalid() {
        let limits = Limits {
            body_read_timeout: Duration::ZERO,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "body_read_timeout"));
    }

    #[test]
    fn zero_graceful_shutdown_timeout_is_invalid() {
        let limits = Limits {
            graceful_shutdown_timeout: Duration::ZERO,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "graceful_shutdown_timeout"));
    }

    #[test]
    fn multiple_errors_reported() {
        let limits = Limits {
            max_connections: 0,
            max_file_streams: 0,
            handler_timeout: Duration::ZERO,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert_eq!(errs.len(), 3);
    }

    #[test]
    fn non_default_valid_values() {
        let limits = Limits {
            max_connections: 1,
            max_file_streams: 1,
            header_read_timeout: Duration::from_millis(1),
            connection_total_timeout: Duration::from_millis(1),
            handler_timeout: Duration::from_millis(1),
            body_read_timeout: Duration::from_millis(1),
            graceful_shutdown_timeout: Duration::from_millis(1),
            ..Default::default()
        };
        assert!(limits.validate().is_ok());
    }

    #[test]
    fn limits_error_display() {
        let err = LimitsError {
            field: "max_connections",
            value: "0".into(),
            constraint: "> 0".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("max_connections"));
        assert!(msg.contains("> 0"));
        assert!(msg.contains("0"));
    }

    #[test]
    fn large_concurrency_values_are_valid() {
        let max = tokio::sync::Semaphore::MAX_PERMITS;
        let limits = Limits {
            max_connections: max,
            max_file_streams: max,
            ..Default::default()
        };
        assert!(limits.validate().is_ok());
    }

    #[test]
    fn exceeding_semaphore_max_permits_is_invalid() {
        let limits = Limits {
            max_connections: tokio::sync::Semaphore::MAX_PERMITS + 1,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_connections"));
    }

    #[test]
    fn usizemax_concurrency_is_invalid() {
        let limits = Limits {
            max_connections: usize::MAX,
            max_file_streams: usize::MAX,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_connections"));
        assert!(errs.iter().any(|e| e.field == "max_file_streams"));
    }

    #[test]
    fn large_duration_values_are_valid() {
        let limits = Limits {
            header_read_timeout: Duration::from_secs(u64::MAX),
            connection_total_timeout: Duration::from_secs(u64::MAX),
            handler_timeout: Duration::from_secs(u64::MAX),
            body_read_timeout: Duration::from_secs(u64::MAX),
            graceful_shutdown_timeout: Duration::from_secs(u64::MAX),
            ..Default::default()
        };
        assert!(limits.validate().is_ok());
    }

    #[test]
    fn limits_error_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<LimitsError>();
    }

    #[test]
    fn limits_is_clone() {
        let limits = Limits::default();
        let cloned = limits.clone();
        assert_eq!(limits.max_connections, cloned.max_connections);
    }

    #[test]
    fn zero_stream_chunk_size_is_invalid() {
        let limits = Limits {
            stream_chunk_size: 0,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "stream_chunk_size"));
    }

    #[test]
    fn small_stream_chunk_size_below_minimum_is_invalid() {
        let limits = Limits {
            stream_chunk_size: 63,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "stream_chunk_size"));
    }

    #[test]
    fn minimum_stream_chunk_size_is_valid() {
        let limits = Limits {
            stream_chunk_size: 64,
            ..Default::default()
        };
        assert!(limits.validate().is_ok());
    }

    #[test]
    fn excessive_stream_chunk_size_is_invalid() {
        let limits = Limits {
            stream_chunk_size: 1024 * 1024 + 1,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "stream_chunk_size"));
    }

    #[test]
    fn listing_response_limit_is_bounded() {
        let limits = Limits {
            max_listing_response_bytes: MAX_LISTING_RESPONSE_BYTES + 1,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_listing_response_bytes"));
    }

    #[test]
    fn zero_max_listing_entries_is_invalid() {
        let limits = Limits {
            max_listing_entries: 0,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_listing_entries"));
    }

    #[test]
    fn excessive_max_listing_entries_is_invalid() {
        let limits = Limits {
            max_listing_entries: MAX_LISTING_ENTRIES + 1,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_listing_entries"));
    }

    #[test]
    fn maximum_request_body_bytes_are_valid() {
        let limits = Limits {
            max_request_body_bytes: MAX_REQUEST_BODY_BYTES,
            ..Default::default()
        };
        assert!(limits.validate().is_ok());
    }

    #[test]
    fn excessive_max_request_body_bytes_is_invalid() {
        let limits = Limits {
            max_request_body_bytes: u64::MAX,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_request_body_bytes"));
    }

    #[test]
    fn maximum_stream_chunk_size_is_valid() {
        let limits = Limits {
            stream_chunk_size: 1024 * 1024,
            ..Default::default()
        };
        assert!(limits.validate().is_ok());
    }

    #[test]
    fn validate_all_fields_simultaneously() {
        let limits = Limits {
            max_connections: 0,
            max_file_streams: 0,
            header_read_timeout: Duration::ZERO,
            connection_total_timeout: Duration::ZERO,
            handler_timeout: Duration::ZERO,
            body_read_timeout: Duration::ZERO,
            graceful_shutdown_timeout: Duration::ZERO,
            stream_chunk_size: 0,
            max_listing_entries: 0,
            max_request_body_bytes: u64::MAX,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert_eq!(errs.len(), 10);
        let fields: Vec<&str> = errs.iter().map(|e| e.field).collect();
        assert!(fields.contains(&"max_connections"));
        assert!(fields.contains(&"max_file_streams"));
        assert!(fields.contains(&"header_read_timeout"));
        assert!(fields.contains(&"connection_total_timeout"));
        assert!(fields.contains(&"handler_timeout"));
        assert!(fields.contains(&"body_read_timeout"));
        assert!(fields.contains(&"graceful_shutdown_timeout"));
        assert!(fields.contains(&"stream_chunk_size"));
        assert!(fields.contains(&"max_listing_entries"));
        assert!(fields.contains(&"max_request_body_bytes"));
    }

    #[test]
    fn plan164_parser_defaults_are_valid() {
        let limits = Limits::default();
        assert_eq!(limits.max_buf_size, DEFAULT_MAX_BUF_SIZE);
        assert_eq!(limits.max_headers, DEFAULT_MAX_HEADERS);
        assert_eq!(limits.max_header_bytes, DEFAULT_MAX_HEADER_BYTES);
        assert_eq!(
            limits.max_request_target_bytes,
            DEFAULT_MAX_REQUEST_TARGET_BYTES
        );
        assert_eq!(
            limits.max_in_flight_requests,
            DEFAULT_MAX_IN_FLIGHT_REQUESTS
        );
        assert_eq!(limits.keep_alive_idle_timeout, Duration::from_secs(60));
        assert_eq!(limits.max_requests_per_connection, None);
        assert_eq!(limits.response_write_timeout, Duration::from_secs(30));
        assert!(limits.validate().is_ok());
    }

    #[test]
    fn buf_size_below_hyper_minimum_is_invalid() {
        let limits = Limits {
            max_buf_size: MIN_MAX_BUF_SIZE - 1,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_buf_size"));
    }

    #[test]
    fn buf_size_above_maximum_is_invalid() {
        let limits = Limits {
            max_buf_size: MAX_MAX_BUF_SIZE + 1,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_buf_size"));
    }

    #[test]
    fn buf_size_boundaries_are_valid() {
        for size in [MIN_MAX_BUF_SIZE, DEFAULT_MAX_BUF_SIZE, MAX_MAX_BUF_SIZE] {
            let limits = Limits {
                max_buf_size: size,
                ..Default::default()
            };
            assert!(limits.validate().is_ok(), "size {size} should be valid");
        }
    }

    #[test]
    fn zero_max_headers_is_invalid() {
        let limits = Limits {
            max_headers: 0,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_headers"));
    }

    #[test]
    fn excessive_max_headers_is_invalid() {
        let limits = Limits {
            max_headers: MAX_MAX_HEADERS + 1,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_headers"));
    }

    #[test]
    fn header_byte_bounds_are_enforced() {
        for bytes in [MIN_MAX_HEADER_BYTES - 1, MAX_MAX_HEADER_BYTES + 1] {
            let limits = Limits {
                max_header_bytes: bytes,
                ..Default::default()
            };
            let errs = limits.validate().unwrap_err();
            assert!(errs.iter().any(|e| e.field == "max_header_bytes"));
        }
        for bytes in [MIN_MAX_HEADER_BYTES, MAX_MAX_HEADER_BYTES] {
            let limits = Limits {
                max_header_bytes: bytes,
                ..Default::default()
            };
            assert!(limits.validate().is_ok());
        }
    }

    #[test]
    fn request_target_byte_bounds_are_enforced() {
        for bytes in [
            MIN_MAX_REQUEST_TARGET_BYTES - 1,
            MAX_MAX_REQUEST_TARGET_BYTES + 1,
        ] {
            let limits = Limits {
                max_request_target_bytes: bytes,
                ..Default::default()
            };
            let errs = limits.validate().unwrap_err();
            assert!(errs.iter().any(|e| e.field == "max_request_target_bytes"));
        }
    }

    #[test]
    fn zero_in_flight_requests_is_invalid() {
        let limits = Limits {
            max_in_flight_requests: 0,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_in_flight_requests"));
    }

    #[test]
    fn in_flight_requests_above_semaphore_permits_is_invalid() {
        let limits = Limits {
            max_in_flight_requests: tokio::sync::Semaphore::MAX_PERMITS + 1,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "max_in_flight_requests"));
    }

    #[test]
    fn zero_keep_alive_idle_timeout_is_invalid() {
        let limits = Limits {
            keep_alive_idle_timeout: Duration::ZERO,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "keep_alive_idle_timeout"));
    }

    #[test]
    fn zero_max_requests_per_connection_is_invalid() {
        let limits = Limits {
            max_requests_per_connection: Some(0),
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs
            .iter()
            .any(|e| e.field == "max_requests_per_connection"));
    }

    #[test]
    fn max_requests_per_connection_none_and_positive_are_valid() {
        for value in [None, Some(1), Some(1000)] {
            let limits = Limits {
                max_requests_per_connection: value,
                ..Default::default()
            };
            assert!(limits.validate().is_ok(), "value {value:?} should be valid");
        }
    }

    #[test]
    fn zero_response_write_timeout_is_invalid() {
        let limits = Limits {
            response_write_timeout: Duration::ZERO,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "response_write_timeout"));
    }
}
