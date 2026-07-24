//! Resource limits for connections, streams, and request sizes.

use std::fmt;
use std::time::Duration;

/// Default maximum number of entries to enumerate in a directory listing.
pub const DEFAULT_MAX_LISTING_ENTRIES: usize = 4096;

/// Error returned when a [`Limits`] field violates its constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitsError {
    /// The field that failed validation.
    pub field: &'static str,
    /// The rejected value.
    pub value: String,
    /// Human-readable constraint description.
    pub constraint: &'static str,
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
    /// Maximum size in bytes for a single encoded filename in a listing.
    pub max_listing_filename_bytes: usize,
    /// Timeout for directory enumeration operations.
    pub listing_enumeration_timeout: Duration,
    /// Chunk size in bytes for file streaming reads.
    pub stream_chunk_size: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: 64,
            max_file_streams: 32,
            max_request_body_bytes: 0,
            header_read_timeout: Duration::from_secs(10),
            connection_total_timeout: Duration::from_secs(60),
            handler_timeout: Duration::from_secs(30),
            body_read_timeout: Duration::from_secs(30),
            graceful_shutdown_timeout: Duration::from_secs(10),
            max_listing_entries: DEFAULT_MAX_LISTING_ENTRIES,
            max_listing_response_bytes: 1024 * 1024, // 1 MiB
            max_listing_filename_bytes: 255,
            listing_enumeration_timeout: Duration::from_secs(30),
            stream_chunk_size: 8192,
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
        if self.max_connections == 0 {
            errors.push(LimitsError {
                field: "max_connections",
                value: "0".into(),
                constraint: "> 0",
            });
        }
        if self.max_file_streams == 0 {
            errors.push(LimitsError {
                field: "max_file_streams",
                value: "0".into(),
                constraint: "> 0",
            });
        }
        if self.header_read_timeout.is_zero() {
            errors.push(LimitsError {
                field: "header_read_timeout",
                value: "0s".into(),
                constraint: "> 0",
            });
        }
        if self.connection_total_timeout.is_zero() {
            errors.push(LimitsError {
                field: "connection_total_timeout",
                value: "0s".into(),
                constraint: "> 0",
            });
        }
        if self.handler_timeout.is_zero() {
            errors.push(LimitsError {
                field: "handler_timeout",
                value: "0s".into(),
                constraint: "> 0",
            });
        }
        if self.body_read_timeout.is_zero() {
            errors.push(LimitsError {
                field: "body_read_timeout",
                value: "0s".into(),
                constraint: "> 0",
            });
        }
        if self.graceful_shutdown_timeout.is_zero() {
            errors.push(LimitsError {
                field: "graceful_shutdown_timeout",
                value: "0s".into(),
                constraint: "> 0",
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
    fn zero_connection_total_timeout_is_invalid() {
        let limits = Limits {
            connection_total_timeout: Duration::ZERO,
            ..Default::default()
        };
        let errs = limits.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "connection_total_timeout"));
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
            constraint: "> 0",
        };
        let msg = err.to_string();
        assert!(msg.contains("max_connections"));
        assert!(msg.contains("> 0"));
        assert!(msg.contains("0"));
    }

    #[test]
    fn large_concurrency_values_are_valid() {
        let limits = Limits {
            max_connections: usize::MAX,
            max_file_streams: usize::MAX,
            ..Default::default()
        };
        assert!(limits.validate().is_ok());
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
    fn validate_all_fields_simultaneously() {
        let limits = Limits {
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
        let fields: Vec<&str> = errs.iter().map(|e| e.field).collect();
        assert!(fields.contains(&"max_connections"));
        assert!(fields.contains(&"max_file_streams"));
        assert!(fields.contains(&"header_read_timeout"));
        assert!(fields.contains(&"connection_total_timeout"));
        assert!(fields.contains(&"handler_timeout"));
        assert!(fields.contains(&"body_read_timeout"));
        assert!(fields.contains(&"graceful_shutdown_timeout"));
    }
}
