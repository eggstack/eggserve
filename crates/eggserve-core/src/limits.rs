//! Resource limits for connections, streams, and request sizes.

use std::time::Duration;

/// Default maximum number of entries to enumerate in a directory listing.
pub const DEFAULT_MAX_LISTING_ENTRIES: usize = 4096;

#[derive(Debug, Clone)]
#[must_use]
pub struct Limits {
    pub max_connections: usize,
    pub max_file_streams: usize,
    pub(crate) max_request_body_bytes: u64,
    pub header_read_timeout: Duration,
    pub response_write_timeout: Duration,
    pub graceful_shutdown_timeout: Duration,
    /// Maximum number of entries to enumerate in a directory listing.
    pub max_listing_entries: usize,
    /// Maximum size in bytes for a directory listing response body.
    pub max_listing_response_bytes: usize,
    /// Maximum size in bytes for a single encoded filename in a listing.
    pub max_listing_filename_bytes: usize,
    /// Timeout for directory enumeration operations.
    pub listing_enumeration_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: 64,
            max_file_streams: 32,
            max_request_body_bytes: 0,
            header_read_timeout: Duration::from_secs(10),
            response_write_timeout: Duration::from_secs(60),
            graceful_shutdown_timeout: Duration::from_secs(10),
            max_listing_entries: DEFAULT_MAX_LISTING_ENTRIES,
            max_listing_response_bytes: 1024 * 1024, // 1 MiB
            max_listing_filename_bytes: 255,
            listing_enumeration_timeout: Duration::from_secs(30),
        }
    }
}
