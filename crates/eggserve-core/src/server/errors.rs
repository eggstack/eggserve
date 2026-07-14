//! Public runtime error types.
//!
//! Errors are classified into four categories:
//!
//! - **Startup errors** ([`ServerError::Bind`], [`ServerError::Config`],
//!   [`ServerError::TlsSetup`]) — returned to the caller before the listener
//!   is ready.
//! - **Lifecycle errors** ([`ServerError::AlreadyStarted`],
//!   [`ServerError::NotStarted`], [`ServerError::Startup`]) — indicate misuse
//!   of the server handle or lifecycle state violations.
//! - **Runtime errors** ([`ServerError::Accept`], [`ServerError::ShutdownTimeout`])
//!   — occur during serving and are logged, not returned to callers.
//! - **Transport errors** ([`ServerError::Transport`]) — failures in response
//!   normalization or body conversion.

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
    /// TLS certificate or configuration error.
    TlsSetup(String),
    /// Transport conversion failure (e.g., body conversion, response normalization).
    Transport(String),
    /// The graceful shutdown timed out.
    ShutdownTimeout,
    /// A fatal startup error occurred (bind failure, TLS error, etc.).
    Startup(String),
    /// The server encountered a terminal runtime error.
    Terminal(String),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bind(e) => write!(f, "failed to bind: {}", e),
            Self::Config(msg) => write!(f, "configuration error: {}", msg),
            Self::AlreadyStarted => write!(f, "server already started"),
            Self::NotStarted => write!(f, "server not started"),
            Self::Accept(e) => write!(f, "accept error: {}", e),
            Self::TlsSetup(msg) => write!(f, "TLS setup error: {}", msg),
            Self::Transport(msg) => write!(f, "transport error: {}", msg),
            Self::ShutdownTimeout => write!(f, "graceful shutdown timed out"),
            Self::Startup(msg) => write!(f, "startup error: {}", msg),
            Self::Terminal(msg) => write!(f, "terminal runtime error: {}", msg),
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

/// Outcome of a server shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownResult {
    /// All in-flight connections completed within the grace period.
    Clean,
    /// The grace period expired; some connections were forcibly cancelled.
    Timeout,
    /// The server was forcefully terminated.
    Forced,
}

impl fmt::Display for ShutdownResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Clean => write!(f, "clean shutdown"),
            Self::Timeout => write!(f, "shutdown timed out"),
            Self::Forced => write!(f, "forced shutdown"),
        }
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

        let err = ServerError::TlsSetup("invalid cert".into());
        assert!(err.to_string().contains("TLS setup error"));
        assert!(err.to_string().contains("invalid cert"));

        let err = ServerError::Transport("body conversion failed".into());
        assert!(err.to_string().contains("transport error"));
        assert!(err.to_string().contains("body conversion failed"));

        let err = ServerError::ShutdownTimeout;
        assert!(err.to_string().contains("timed out"));

        let err = ServerError::Startup("bind failed".into());
        assert!(err.to_string().contains("startup error"));
        assert!(err.to_string().contains("bind failed"));

        let err = ServerError::Terminal("runtime crashed".into());
        assert!(err.to_string().contains("terminal runtime error"));
        assert!(err.to_string().contains("runtime crashed"));
    }

    #[test]
    fn server_error_is_error() {
        let err: Box<dyn std::error::Error> = Box::new(ServerError::Config("test".into()));
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn shutdown_result_display() {
        assert_eq!(ShutdownResult::Clean.to_string(), "clean shutdown");
        assert_eq!(ShutdownResult::Timeout.to_string(), "shutdown timed out");
        assert_eq!(ShutdownResult::Forced.to_string(), "forced shutdown");
    }

    #[test]
    fn shutdown_result_equality() {
        assert_eq!(ShutdownResult::Clean, ShutdownResult::Clean);
        assert_ne!(ShutdownResult::Clean, ShutdownResult::Timeout);
        assert_ne!(ShutdownResult::Forced, ShutdownResult::Clean);
    }
}
