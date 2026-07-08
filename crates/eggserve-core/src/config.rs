//! Configuration types for eggserve (bind address, root directory, options).

/// Top-level configuration for an eggserve serving session.
#[derive(Debug, Clone)]
pub struct Config {
    /// The filesystem root to serve content from.
    pub root: std::path::PathBuf,
    /// Address to bind the listener to.
    pub bind: std::net::SocketAddr,
}
