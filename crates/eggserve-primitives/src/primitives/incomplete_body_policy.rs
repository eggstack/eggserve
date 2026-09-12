//! Policy for handling incomplete request bodies.
//!
//! When a handler returns without fully consuming the body, the runtime
//! closes the connection. Active drain is not safely implementable
//! because the body stream is consumed into the `Request` envelope by
//! value and is no longer accessible from the connection pipeline after
//! service invocation. Hyper handles cleanup of unconsumed body bytes.

/// Policy for handling incomplete request bodies.
///
/// When a handler returns without fully consuming the request body,
/// the runtime applies this policy to determine the connection outcome.
///
/// Only [`Close`](IncompleteBodyPolicy::Close) is supported. The body
/// stream is owned by the `Request` envelope passed to `Service::call`
/// by value. After the service returns, the body is no longer accessible
/// from the connection pipeline, making active drain architecturally
/// unsafe. Hyper cleans up unconsumed bytes by closing the connection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IncompleteBodyPolicy {
    /// Close the connection immediately without draining.
    ///
    /// This is the only supported policy and the default. It avoids
    /// wasting resources on unwanted body data and prevents request
    /// smuggling through unconsumed bytes.
    #[default]
    Close,
}

impl IncompleteBodyPolicy {
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
    fn close_is_close() {
        assert!(IncompleteBodyPolicy::Close.is_close());
    }
}
