//! Transport-neutral request limits.

use std::fmt;

/// Bounds owned by an application-facing service contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum request body bytes accepted by a service.
    pub max_request_body_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_request_body_bytes: 8 * 1024 * 1024,
        }
    }
}

impl Limits {
    /// Checks that the configured limits are usable.
    ///
    /// # Errors
    ///
    /// Returns [`LimitsError::ZeroBodyLimit`] when
    /// `max_request_body_bytes` is zero, which would reject every request
    /// body including empty ones.
    pub fn validate(&self) -> Result<(), LimitsError> {
        if self.max_request_body_bytes == 0 {
            Err(LimitsError::ZeroBodyLimit)
        } else {
            Ok(())
        }
    }
}

/// Reasons a [`Limits`] configuration is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitsError {
    /// `max_request_body_bytes` was zero.
    ZeroBodyLimit,
}

impl fmt::Display for LimitsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("max_request_body_bytes must be non-zero")
    }
}

impl std::error::Error for LimitsError {}
