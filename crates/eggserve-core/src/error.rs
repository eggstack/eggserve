//! Error and result types for eggserve operations.

/// Errors that can occur in eggserve.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The requested path is outside the allowed root.
    #[error("path escapes configured root")]
    PathEscape,

    /// The requested path could not be resolved (missing, permissions, etc.).
    #[error("path not accessible: {0}")]
    PathNotAccessible(String),
}

/// Convenience result alias.
pub type Result<T> = std::result::Result<T, Error>;
