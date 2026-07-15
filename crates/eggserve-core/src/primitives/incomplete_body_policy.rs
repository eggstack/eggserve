//! Policy for handling incomplete request bodies.
//!
//! When a handler returns without fully consuming the body, the runtime
//! must decide whether to drain the remaining body (for keep-alive) or
//! close the connection.

use std::time::Duration;

/// Policy for handling incomplete request bodies.
///
/// When a handler returns without fully consuming the request body,
/// the runtime applies this policy to determine the connection outcome.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IncompleteBodyPolicy {
    /// Drain the remaining body up to a byte and time limit, then allow
    /// connection reuse.
    ///
    /// The runtime reads and discards remaining body bytes, bounded by
    /// both `max_bytes` and `timeout`. If either limit is exceeded, the
    /// connection is closed.
    Drain {
        /// Maximum bytes to drain.
        max_bytes: u64,
        /// Maximum time to spend draining.
        timeout: Duration,
    },
    /// Close the connection immediately without draining.
    ///
    /// This is the strictest policy and the recommended default. It
    /// avoids wasting resources on unwanted body data.
    #[default]
    Close,
}

impl IncompleteBodyPolicy {
    /// Returns `true` if this policy drains before closing.
    pub fn is_drain(&self) -> bool {
        matches!(self, Self::Drain { .. })
    }

    /// Returns `true` if this policy closes immediately.
    pub fn is_close(&self) -> bool {
        matches!(self, Self::Close)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_close() {
        assert_eq!(IncompleteBodyPolicy::default(), IncompleteBodyPolicy::Close);
    }

    #[test]
    fn drain_is_drain() {
        let policy = IncompleteBodyPolicy::Drain {
            max_bytes: 1024,
            timeout: Duration::from_secs(5),
        };
        assert!(policy.is_drain());
        assert!(!policy.is_close());
    }

    #[test]
    fn close_is_close() {
        assert!(IncompleteBodyPolicy::Close.is_close());
        assert!(!IncompleteBodyPolicy::Close.is_drain());
    }
}
