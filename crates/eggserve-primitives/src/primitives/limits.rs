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
    pub fn validate(&self) -> Result<(), LimitsError> {
        if self.max_request_body_bytes == 0 {
            Err(LimitsError::ZeroBodyLimit)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitsError {
    ZeroBodyLimit,
}

impl fmt::Display for LimitsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("max_request_body_bytes must be non-zero")
    }
}

impl std::error::Error for LimitsError {}
