//! Public runtime error types.
//!
//! Errors are classified into three categories:
//!
//! - **Startup errors** ([`ServerError::Bind`], [`ServerError::Config`]) —
//!   returned to the caller before the listener is ready.
//! - **Lifecycle errors** ([`ServerError::AlreadyStarted`],
//!   [`ServerError::NotStarted`]) — indicate misuse of the server handle.
//! - **Runtime errors** ([`ServerError::Accept`], [`ServerError::ShutdownTimeout`])
//!   — occur during serving and are logged, not returned to callers.

use std::fmt;

/// Errors from server startup and lifecycle operations.
#[derive(Debug)]
pub enum ServerError {
    /// Failed to bind the TCP listener.
    Bind(std::io::Error),
    /// Invalid or inconsistent configuration.
    Config(String),
    /// The server was already started.
    AlreadyStarted,
    /// The server has not been started.
    NotStarted,
    /// An error occurred during connection acceptance.
    Accept(std::io::Error),
    /// The graceful shutdown timed out.
    ShutdownTimeout,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bind(e) => write!(f, "failed to bind: {}", e),
            Self::Config(msg) => write!(f, "configuration error: {}", msg),
            Self::AlreadyStarted => write!(f, "server already started"),
            Self::NotStarted => write!(f, "server not started"),
            Self::Accept(e) => write!(f, "accept error: {}", e),
            Self::ShutdownTimeout => write!(f, "graceful shutdown timed out"),
        }
    }
}

impl std::error::Error for ServerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Bind(e) => Some(e),
            Self::Accept(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ServerError {
    fn from(e: std::io::Error) -> Self {
        ServerError::Bind(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_error_display() {
        let err = ServerError::Config("bad value".into());
        assert!(err.to_string().contains("bad value"));

        let err = ServerError::AlreadyStarted;
        assert!(err.to_string().contains("already started"));

        let err = ServerError::NotStarted;
        assert!(err.to_string().contains("not started"));

        let err = ServerError::ShutdownTimeout;
        assert!(err.to_string().contains("timed out"));
    }

    #[test]
    fn server_error_is_error() {
        let err: Box<dyn std::error::Error> = Box::new(ServerError::Config("test".into()));
        assert!(!err.to_string().is_empty());
    }
}
