use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Limits {
    pub max_connections: usize,
    pub max_file_streams: usize,
    pub max_request_body_bytes: u64,
    pub header_read_timeout: Duration,
    pub response_write_timeout: Duration,
    pub graceful_shutdown_timeout: Duration,
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
        }
    }
}
