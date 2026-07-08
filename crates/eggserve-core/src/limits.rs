use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Limits {
    pub max_connections: usize,
    pub max_header_bytes: usize,
    pub max_request_target_bytes: usize,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub idle_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: 64,
            max_header_bytes: 32 * 1024,
            max_request_target_bytes: 8 * 1024,
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(120),
        }
    }
}
