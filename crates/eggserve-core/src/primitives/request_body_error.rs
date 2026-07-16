//! Errors from request body consumption.
//!
//! [`RequestBodyError`] distinguishes policy, limit, timeout, disconnect,
//! and consumption-state failures. The runtime maps these to appropriate
//! HTTP responses without leaking internal details.

use std::fmt;

/// Errors that can occur during request body consumption.
///
/// Callers can distinguish policy rejections, limit violations, timeouts,
/// disconnects, and consumption-state failures. Client-visible responses
/// never include internal error details.
#[derive(Debug)]
pub enum RequestBodyError {
    /// Body rejected by policy (e.g. static service rejects all bodies).
    RejectedByPolicy,
    /// The declared Content-Length exceeds the effective limit.
    DeclaredLengthTooLarge { declared: u64, limit: u64 },
    /// The body exceeded the effective byte limit during consumption.
    LimitExceeded { limit: u64, received: u64 },
    /// The body read timed out.
    ReadTimeout,
    /// The connection was closed before the body was fully received.
    PrematureEof {
        received: u64,
        expected: Option<u64>,
    },
    /// The actual body length did not match the declared Content-Length.
    LengthMismatch { declared: u64, actual: u64 },
    /// The body transport framing is invalid.
    InvalidChunkFraming(String),
    /// The body consumption was cancelled.
    Cancelled,
    /// The client disconnected.
    Disconnected,
    /// The body has already been consumed.
    AlreadyConsumed,
    /// Mixed consumption modes (e.g. read_all after next_chunk).
    MixedConsumptionMode,
    /// A transport-level error occurred (mapped to 500 since eggserve is an origin server).
    Transport(String),
}

impl fmt::Display for RequestBodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RejectedByPolicy => write!(f, "request body rejected by policy"),
            Self::DeclaredLengthTooLarge { declared, limit } => {
                write!(
                    f,
                    "declared content-length {declared} exceeds limit {limit}"
                )
            }
            Self::LimitExceeded { limit, received } => {
                write!(
                    f,
                    "body exceeded limit: received {received} bytes, limit is {limit}"
                )
            }
            Self::ReadTimeout => write!(f, "body read timed out"),
            Self::PrematureEof { received, expected } => match expected {
                Some(exp) => write!(
                    f,
                    "premature EOF: received {received} of {exp} expected bytes"
                ),
                None => write!(f, "premature EOF after {received} bytes"),
            },
            Self::LengthMismatch { declared, actual } => {
                write!(
                    f,
                    "body length mismatch: declared {declared}, actual {actual}"
                )
            }
            Self::InvalidChunkFraming(msg) => {
                write!(f, "invalid chunk framing: {msg}")
            }
            Self::Cancelled => write!(f, "body consumption cancelled"),
            Self::Disconnected => write!(f, "client disconnected"),
            Self::AlreadyConsumed => write!(f, "body already consumed"),
            Self::MixedConsumptionMode => {
                write!(
                    f,
                    "mixed consumption mode: cannot switch between read_all and streaming"
                )
            }
            Self::Transport(msg) => write!(f, "transport error: {msg}"),
        }
    }
}

impl std::error::Error for RequestBodyError {}

impl RequestBodyError {
    /// Returns `true` if this error is a policy rejection.
    pub fn is_policy_rejection(&self) -> bool {
        matches!(self, Self::RejectedByPolicy)
    }

    /// Returns `true` if this error is a limit violation.
    pub fn is_limit_exceeded(&self) -> bool {
        matches!(
            self,
            Self::LimitExceeded { .. } | Self::DeclaredLengthTooLarge { .. }
        )
    }

    /// Returns `true` if this error is a timeout.
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::ReadTimeout)
    }

    /// Returns `true` if this error indicates a disconnect.
    pub fn is_disconnect(&self) -> bool {
        matches!(self, Self::Disconnected | Self::PrematureEof { .. })
    }

    /// Returns `true` if this error is a consumption-state error.
    pub fn is_consumption_state(&self) -> bool {
        matches!(self, Self::AlreadyConsumed | Self::MixedConsumptionMode)
    }

    /// Returns the appropriate HTTP status code for this error.
    pub fn to_status_code(&self) -> u16 {
        match self {
            Self::RejectedByPolicy => 400,
            Self::DeclaredLengthTooLarge { .. } => 413,
            Self::LimitExceeded { .. } => 413,
            Self::ReadTimeout => 408,
            Self::PrematureEof { .. } => 400,
            Self::LengthMismatch { .. } => 400,
            Self::InvalidChunkFraming(_) => 400,
            Self::Cancelled => 499,
            Self::Disconnected => 499,
            Self::AlreadyConsumed => 500,
            Self::MixedConsumptionMode => 500,
            Self::Transport(_) => 500,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_messages() {
        let err = RequestBodyError::RejectedByPolicy;
        assert!(err.to_string().contains("rejected by policy"));

        let err = RequestBodyError::LimitExceeded {
            limit: 1024,
            received: 2048,
        };
        assert!(err.to_string().contains("2048"));
        assert!(err.to_string().contains("1024"));

        let err = RequestBodyError::PrematureEof {
            received: 5,
            expected: Some(10),
        };
        assert!(err.to_string().contains("5"));
        assert!(err.to_string().contains("10"));
    }

    #[test]
    fn classification() {
        assert!(RequestBodyError::RejectedByPolicy.is_policy_rejection());
        assert!(!RequestBodyError::RejectedByPolicy.is_limit_exceeded());

        assert!(RequestBodyError::LimitExceeded {
            limit: 100,
            received: 200
        }
        .is_limit_exceeded());

        assert!(RequestBodyError::ReadTimeout.is_timeout());
        assert!(RequestBodyError::Disconnected.is_disconnect());
        assert!(RequestBodyError::AlreadyConsumed.is_consumption_state());
    }

    #[test]
    fn status_codes() {
        assert_eq!(RequestBodyError::RejectedByPolicy.to_status_code(), 400);
        assert_eq!(
            RequestBodyError::LimitExceeded {
                limit: 100,
                received: 200
            }
            .to_status_code(),
            413
        );
        assert_eq!(RequestBodyError::ReadTimeout.to_status_code(), 408);
        assert_eq!(
            RequestBodyError::PrematureEof {
                received: 0,
                expected: Some(10)
            }
            .to_status_code(),
            400
        );
        assert_eq!(
            RequestBodyError::Transport("oops".into()).to_status_code(),
            500
        );
    }
}
