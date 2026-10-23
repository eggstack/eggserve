//! Canonical runtime/transport limit authority (Plan 215: moved from
//! compatibility core).
//!
//! One source of truth for shared runtime defaults and scalar/cross-field
//! validation consumed by the direct [`crate::config::RuntimeConfig`],
//! [`crate::config::RuntimeConfigBuilder`], and — via re-export — the
//! compatibility `Limits`/`RuntimeConfig` types in `eggserve-core`.
//!
//! # Ownership
//!
//! - **Runtime/transport (here):** connection/file-stream concurrency,
//!   request-body ceiling, HTTP/1 parser buffer/header/target limits,
//!   in-flight service admission, header/TLS/handshake/handler/body/total/
//!   shutdown/keep-alive/response-write timeouts, max requests per connection,
//!   file-stream chunk size.
//! - **Static-service-only (NOT here):** directory-listing budgets
//!   (`max_listing_entries`, `max_listing_response_bytes`), extra static
//!   response-header budgets. Those stay in the compatibility `Limits` type.
//! - **Frontend-only (NOT here):** bind exposure acknowledgements, CLI logging
//!   format, Python callback concurrency, compatibility-facade buffering.
//!
//! Services may lower request-body ceilings but cannot raise the runtime hard
//! ceiling (`max_request_body_bytes`).
//!
//! Compatibility adapters construct
//! [`crate::runtime_limits::SharedRuntimeValues`] field-by-field
//! (via `From` impls next to their own config types) so no constant or
//! constraint is duplicated. There is exactly one defaults table and one
//! validation kernel.

use std::fmt;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Authoritative defaults and bounds
// ---------------------------------------------------------------------------

/// Default maximum concurrent connections.
pub const DEFAULT_MAX_CONNECTIONS: usize = 64;
/// Default maximum concurrent file-stream responses.
pub const DEFAULT_MAX_FILE_STREAMS: usize = 32;
/// Default request-body ceiling: 0 rejects all bodies.
pub const DEFAULT_MAX_REQUEST_BODY_BYTES: u64 = 0;
/// Upper bound for `max_request_body_bytes` (1 GiB).
pub const MAX_REQUEST_BODY_BYTES: u64 = 1024 * 1024 * 1024;

/// Default header-read timeout.
pub const DEFAULT_HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Default TLS handshake timeout.
pub const DEFAULT_TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Default total connection lifetime.
pub const DEFAULT_CONNECTION_TOTAL_TIMEOUT: Duration = Duration::from_secs(60);
/// Default single handler invocation budget.
pub const DEFAULT_HANDLER_TIMEOUT: Duration = Duration::from_secs(30);
/// Default total request-body consumption deadline.
pub const DEFAULT_BODY_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Default graceful-shutdown drain deadline.
pub const DEFAULT_GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
/// Default keep-alive idle timeout (resets on activity; independent of total).
pub const DEFAULT_KEEP_ALIVE_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Default response-write no-progress timeout.
pub const DEFAULT_RESPONSE_WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// Default file-streaming read chunk size.
///
/// Plan 229 selected 128 KiB from the current-HEAD body matrix: it reduced
/// 16 MiB file-adapter frame/read overhead substantially while retaining the
/// best 1 MiB result on the qualification host. The bounded resident payload
/// is `max_file_streams * stream_chunk_size` (4 MiB at the default 32 streams),
/// excluding runtime and allocator overhead.
pub const DEFAULT_STREAM_CHUNK_SIZE: usize = 128 * 1024;
/// Minimum file-streaming chunk size.
pub const MIN_STREAM_CHUNK_SIZE: usize = 64;
/// Maximum file-streaming chunk size (1 MiB).
pub const MAX_STREAM_CHUNK_SIZE: usize = 1024 * 1024;

/// Default HTTP/1 parser read-buffer ceiling.
///
/// Hyper's own default is ~400 KiB and explicitly not stable; this
/// EggServe-owned default preserves ordinary browser/proxy compatibility
/// while bounding per-connection parser memory.
pub const DEFAULT_MAX_BUF_SIZE: usize = 64 * 1024;
/// Minimum parser buffer accepted by Hyper (`Builder::max_buf_size` panics below).
pub const MIN_MAX_BUF_SIZE: usize = 8192;
/// Legacy conservative parser-buffer guidance (4 MiB); not a runtime maximum.
pub const MAX_MAX_BUF_SIZE: usize = 4 * 1024 * 1024;

/// Default maximum request header field count (Hyper's default, pinned).
pub const DEFAULT_MAX_HEADERS: usize = 100;
/// Legacy conservative header-count guidance; not a runtime maximum.
pub const MAX_MAX_HEADERS: usize = 10_000;

/// Default post-parse aggregate request-header ceiling (name+value bytes).
pub const DEFAULT_MAX_HEADER_BYTES: usize = 32 * 1024;
/// Minimum aggregate header-byte ceiling.
pub const MIN_MAX_HEADER_BYTES: usize = 1024;
/// Maximum aggregate header-byte ceiling (1 MiB).
pub const MAX_MAX_HEADER_BYTES: usize = 1024 * 1024;

/// Default maximum request-target length in bytes.
pub const DEFAULT_MAX_REQUEST_TARGET_BYTES: usize = 8192;
/// Minimum request-target ceiling.
pub const MIN_MAX_REQUEST_TARGET_BYTES: usize = 128;
/// Maximum request-target ceiling (64 KiB).
pub const MAX_MAX_REQUEST_TARGET_BYTES: usize = 64 * 1024;

/// Default maximum concurrent in-flight service executions.
pub const DEFAULT_MAX_IN_FLIGHT_REQUESTS: usize = 64;

/// Default maximum concurrent active tunnels (generic upgrade / CONNECT /
/// Extended CONNECT duplex sessions). Long-lived tunnels hold a tunnel permit
/// until close; exhaustion fails new tunnel handshakes with 503 without
/// affecting ordinary HTTP. Default matches `max_connections` so one tunnel
/// per connection is possible by default.
pub const DEFAULT_MAX_ACTIVE_TUNNELS: usize = 64;

// ---------------------------------------------------------------------------
// Shared value group
// ---------------------------------------------------------------------------

/// Shared runtime/transport values validated by one kernel.
///
/// Frontends construct this field-by-field (or via `From` impls next to
/// their own config types) and call [`SharedRuntimeValues::validate`]; the
/// resulting [`Violation`] list renders into each frontend's existing public
/// error shape (`LimitsError`, `ServerError::Config`). No Hyper, Tokio,
/// Python, CLI, or FFI types appear here.
#[derive(Debug, Clone)]
pub struct SharedRuntimeValues {
    /// Maximum concurrent connections. Must be greater than zero and at most
    /// `Semaphore::MAX_PERMITS`.
    pub max_connections: usize,
    /// Maximum concurrent file-stream responses. Must be greater than zero and
    /// at most `Semaphore::MAX_PERMITS`.
    pub max_file_streams: usize,
    /// Hard request-body ceiling in bytes. `0` rejects every request body;
    /// values above [`MAX_REQUEST_BODY_BYTES`] (1 GiB) are rejected.
    pub max_request_body_bytes: u64,
    /// Deadline for reading the request headers. Must be greater than zero and
    /// no wider than `connection_total_timeout` when that total is enabled.
    pub header_read_timeout: Duration,
    /// Deadline for a TLS handshake. Must be greater than zero. The direct H1
    /// driver performs no handshake itself; the value is shared so direct and
    /// compatibility construction reject the same values.
    pub tls_handshake_timeout: Duration,
    /// Total lifetime of one connection, shared across keep-alive requests.
    /// `Duration::ZERO` disables only this ceiling; every independent
    /// timeout, admission, and shutdown bound stays active.
    pub connection_total_timeout: Duration,
    /// Deadline for one service invocation. Must be greater than zero and no
    /// wider than `connection_total_timeout` when that total is enabled.
    pub handler_timeout: Duration,
    /// Total deadline for consuming the request body. Must be greater than
    /// zero and no wider than `connection_total_timeout` when that total is
    /// enabled.
    pub body_read_timeout: Duration,
    /// Grace period for draining connections during shutdown. Must be greater
    /// than zero.
    pub graceful_shutdown_timeout: Duration,
    /// Read chunk size for file-streaming responses. Must be within
    /// [`MIN_STREAM_CHUNK_SIZE`]..=[`MAX_STREAM_CHUNK_SIZE`].
    pub stream_chunk_size: usize,
    /// HTTP/1 parser/read buffer ceiling in bytes. Must be at least
    /// [`MIN_MAX_BUF_SIZE`] (Hyper's minimum). [`MAX_MAX_BUF_SIZE`] is legacy
    /// guidance, not an enforced maximum.
    pub max_buf_size: usize,
    /// Maximum request header field count. Must be greater than zero.
    /// [`MAX_MAX_HEADERS`] is legacy guidance, not an enforced maximum.
    pub max_headers: usize,
    /// Aggregate post-parse request-header name+value byte ceiling. Must be
    /// within [`MIN_MAX_HEADER_BYTES`]..=[`MAX_MAX_HEADER_BYTES`].
    pub max_header_bytes: usize,
    /// Maximum request-target length in bytes. Must be within
    /// [`MIN_MAX_REQUEST_TARGET_BYTES`]..=[`MAX_MAX_REQUEST_TARGET_BYTES`].
    pub max_request_target_bytes: usize,
    /// Maximum concurrent in-flight service executions. Must be greater than
    /// zero and at most `Semaphore::MAX_PERMITS`.
    pub max_in_flight_requests: usize,
    /// Keep-alive idle timeout; it resets on activity and is independent of
    /// the total connection lifetime. Must be greater than zero.
    pub keep_alive_idle_timeout: Duration,
    /// Maximum completed requests per connection. `None` disables the limit;
    /// `Some(0)` is rejected because it would close the connection before the
    /// first response.
    pub max_requests_per_connection: Option<u64>,
    /// Deadline for response-write progress (no bytes written within the
    /// window). Must be greater than zero.
    pub response_write_timeout: Duration,
    /// Maximum concurrent active tunnels. Must be greater than zero and at
    /// most `Semaphore::MAX_PERMITS`.
    pub max_active_tunnels: usize,
}

impl Default for SharedRuntimeValues {
    fn default() -> Self {
        Self {
            max_connections: DEFAULT_MAX_CONNECTIONS,
            max_file_streams: DEFAULT_MAX_FILE_STREAMS,
            max_request_body_bytes: DEFAULT_MAX_REQUEST_BODY_BYTES,
            header_read_timeout: DEFAULT_HEADER_READ_TIMEOUT,
            tls_handshake_timeout: DEFAULT_TLS_HANDSHAKE_TIMEOUT,
            connection_total_timeout: DEFAULT_CONNECTION_TOTAL_TIMEOUT,
            handler_timeout: DEFAULT_HANDLER_TIMEOUT,
            body_read_timeout: DEFAULT_BODY_READ_TIMEOUT,
            graceful_shutdown_timeout: DEFAULT_GRACEFUL_SHUTDOWN_TIMEOUT,
            stream_chunk_size: DEFAULT_STREAM_CHUNK_SIZE,
            max_buf_size: DEFAULT_MAX_BUF_SIZE,
            max_headers: DEFAULT_MAX_HEADERS,
            max_header_bytes: DEFAULT_MAX_HEADER_BYTES,
            max_request_target_bytes: DEFAULT_MAX_REQUEST_TARGET_BYTES,
            max_in_flight_requests: DEFAULT_MAX_IN_FLIGHT_REQUESTS,
            keep_alive_idle_timeout: DEFAULT_KEEP_ALIVE_IDLE_TIMEOUT,
            max_requests_per_connection: None,
            response_write_timeout: DEFAULT_RESPONSE_WRITE_TIMEOUT,
            max_active_tunnels: DEFAULT_MAX_ACTIVE_TUNNELS,
        }
    }
}

/// One structured constraint violation.
///
/// Adapters preserve actionable wording: `LimitsError` carries the triple
/// directly; `ServerError::Config` renders it via `Violation::to_string`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The field that failed validation.
    pub field: &'static str,
    /// The rejected value (human-readable).
    pub value: String,
    /// Human-readable constraint (the text after "must be").
    pub constraint: String,
}

impl Violation {
    fn new(field: &'static str, value: String, constraint: String) -> Self {
        Self {
            field,
            value,
            constraint,
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} must be {}: got {}",
            self.field, self.constraint, self.value
        )
    }
}

fn duration_value(d: Duration) -> String {
    if d.is_zero() {
        "0s".to_owned()
    } else {
        format!("{d:?}")
    }
}

impl SharedRuntimeValues {
    /// Check every shared scalar and cross-field constraint.
    ///
    /// Returns one [`Violation`] per violated field in stable field order.
    /// Static-only budgets (listing, extra headers) are intentionally absent;
    /// response-policy and TLS-identity checks live with their owners and are
    /// composed by `RuntimeConfig::validate`.
    pub fn validate(&self) -> Vec<Violation> {
        let mut errors = Vec::new();
        let max_semaphore_permits = tokio::sync::Semaphore::MAX_PERMITS;

        if self.max_connections == 0 {
            errors.push(Violation::new("max_connections", "0".into(), "> 0".into()));
        } else if self.max_connections > max_semaphore_permits {
            errors.push(Violation::new(
                "max_connections",
                self.max_connections.to_string(),
                format!("<= {max_semaphore_permits} (Semaphore::MAX_PERMITS)"),
            ));
        }

        if self.max_file_streams == 0 {
            errors.push(Violation::new("max_file_streams", "0".into(), "> 0".into()));
        } else if self.max_file_streams > max_semaphore_permits {
            errors.push(Violation::new(
                "max_file_streams",
                self.max_file_streams.to_string(),
                format!("<= {max_semaphore_permits} (Semaphore::MAX_PERMITS)"),
            ));
        }

        if self.header_read_timeout.is_zero() {
            errors.push(Violation::new(
                "header_read_timeout",
                "0s".into(),
                "> 0".into(),
            ));
        }
        if self.tls_handshake_timeout.is_zero() {
            errors.push(Violation::new(
                "tls_handshake_timeout",
                "0s".into(),
                "> 0".into(),
            ));
        }
        if !self.connection_total_timeout.is_zero()
            && self.header_read_timeout > self.connection_total_timeout
        {
            errors.push(Violation::new(
                "header_read_timeout",
                duration_value(self.header_read_timeout),
                "<= connection_total_timeout".into(),
            ));
        }

        if self.handler_timeout.is_zero() {
            errors.push(Violation::new("handler_timeout", "0s".into(), "> 0".into()));
        }
        if self.body_read_timeout.is_zero() {
            errors.push(Violation::new(
                "body_read_timeout",
                "0s".into(),
                "> 0".into(),
            ));
        }
        // A handler or body budget wider than the total connection lifetime
        // is dead configuration: the connection budget always fires first.
        if !self.connection_total_timeout.is_zero()
            && self.handler_timeout > self.connection_total_timeout
        {
            errors.push(Violation::new(
                "handler_timeout",
                duration_value(self.handler_timeout),
                "<= connection_total_timeout".into(),
            ));
        }
        if !self.connection_total_timeout.is_zero()
            && self.body_read_timeout > self.connection_total_timeout
        {
            errors.push(Violation::new(
                "body_read_timeout",
                duration_value(self.body_read_timeout),
                "<= connection_total_timeout".into(),
            ));
        }

        if self.graceful_shutdown_timeout.is_zero() {
            errors.push(Violation::new(
                "graceful_shutdown_timeout",
                "0s".into(),
                "> 0".into(),
            ));
        }

        if self.stream_chunk_size < MIN_STREAM_CHUNK_SIZE {
            errors.push(Violation::new(
                "stream_chunk_size",
                self.stream_chunk_size.to_string(),
                format!(">= {MIN_STREAM_CHUNK_SIZE}"),
            ));
        } else if self.stream_chunk_size > MAX_STREAM_CHUNK_SIZE {
            errors.push(Violation::new(
                "stream_chunk_size",
                self.stream_chunk_size.to_string(),
                format!("<= {MAX_STREAM_CHUNK_SIZE} (1 MiB)"),
            ));
        }

        if self.max_request_body_bytes > MAX_REQUEST_BODY_BYTES {
            errors.push(Violation::new(
                "max_request_body_bytes",
                self.max_request_body_bytes.to_string(),
                format!("<= {MAX_REQUEST_BODY_BYTES} (1 GiB), or 0 to reject bodies"),
            ));
        }

        if self.max_buf_size < MIN_MAX_BUF_SIZE {
            errors.push(Violation::new(
                "max_buf_size",
                self.max_buf_size.to_string(),
                format!(">= {MIN_MAX_BUF_SIZE} (Hyper minimum)"),
            ));
        }

        if self.max_headers == 0 {
            errors.push(Violation::new("max_headers", "0".into(), "> 0".into()));
        }

        if self.max_header_bytes < MIN_MAX_HEADER_BYTES {
            errors.push(Violation::new(
                "max_header_bytes",
                self.max_header_bytes.to_string(),
                format!(">= {MIN_MAX_HEADER_BYTES}"),
            ));
        } else if self.max_header_bytes > MAX_MAX_HEADER_BYTES {
            errors.push(Violation::new(
                "max_header_bytes",
                self.max_header_bytes.to_string(),
                format!("<= {MAX_MAX_HEADER_BYTES} (1 MiB)"),
            ));
        }

        if self.max_request_target_bytes < MIN_MAX_REQUEST_TARGET_BYTES {
            errors.push(Violation::new(
                "max_request_target_bytes",
                self.max_request_target_bytes.to_string(),
                format!(">= {MIN_MAX_REQUEST_TARGET_BYTES}"),
            ));
        } else if self.max_request_target_bytes > MAX_MAX_REQUEST_TARGET_BYTES {
            errors.push(Violation::new(
                "max_request_target_bytes",
                self.max_request_target_bytes.to_string(),
                format!("<= {MAX_MAX_REQUEST_TARGET_BYTES} (64 KiB)"),
            ));
        }

        if self.max_in_flight_requests == 0 {
            errors.push(Violation::new(
                "max_in_flight_requests",
                "0".into(),
                "> 0".into(),
            ));
        } else if self.max_in_flight_requests > max_semaphore_permits {
            errors.push(Violation::new(
                "max_in_flight_requests",
                self.max_in_flight_requests.to_string(),
                format!("<= {max_semaphore_permits} (Semaphore::MAX_PERMITS)"),
            ));
        }

        if self.keep_alive_idle_timeout.is_zero() {
            errors.push(Violation::new(
                "keep_alive_idle_timeout",
                "0s".into(),
                "> 0".into(),
            ));
        }
        if self.max_requests_per_connection == Some(0) {
            errors.push(Violation::new(
                "max_requests_per_connection",
                "0".into(),
                ">= 1 or None (unlimited)".into(),
            ));
        }
        if self.response_write_timeout.is_zero() {
            errors.push(Violation::new(
                "response_write_timeout",
                "0s".into(),
                "> 0".into(),
            ));
        }

        if self.max_active_tunnels == 0 {
            errors.push(Violation::new(
                "max_active_tunnels",
                "0".into(),
                "> 0".into(),
            ));
        } else if self.max_active_tunnels > max_semaphore_permits {
            errors.push(Violation::new(
                "max_active_tunnels",
                self.max_active_tunnels.to_string(),
                format!("<= {max_semaphore_permits} (Semaphore::MAX_PERMITS)"),
            ));
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(SharedRuntimeValues::default().validate().is_empty());
    }

    #[test]
    fn parser_guidance_constants_are_not_validation_maxima() {
        let values = SharedRuntimeValues {
            max_buf_size: MAX_MAX_BUF_SIZE + 1,
            max_headers: MAX_MAX_HEADERS + 1,
            ..SharedRuntimeValues::default()
        };
        assert!(values.validate().is_empty());

        let below_minimum = SharedRuntimeValues {
            max_buf_size: MIN_MAX_BUF_SIZE - 1,
            ..SharedRuntimeValues::default()
        };
        assert!(below_minimum
            .validate()
            .iter()
            .any(|v| v.field == "max_buf_size"));
        let zero_headers = SharedRuntimeValues {
            max_buf_size: MIN_MAX_BUF_SIZE,
            max_headers: 0,
            ..SharedRuntimeValues::default()
        };
        assert!(zero_headers
            .validate()
            .iter()
            .any(|v| v.field == "max_headers"));
    }

    #[test]
    fn violation_display_is_actionable() {
        let v = Violation::new("max_connections", "0".into(), "> 0".into());
        let msg = v.to_string();
        assert!(msg.contains("max_connections"));
        assert!(msg.contains("> 0"));
        assert!(msg.contains("0"));
    }
}
