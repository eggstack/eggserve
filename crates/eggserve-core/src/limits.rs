//! Resource limit types for connection and request constraints.

/// Limits applied to connections and request processing.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Maximum number of concurrent connections.
    pub max_connections: usize,
    /// Maximum request body size in bytes (should be 0 for GET/HEAD only).
    pub max_body_bytes: u64,
}
