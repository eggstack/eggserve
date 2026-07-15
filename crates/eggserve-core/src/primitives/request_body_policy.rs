//! Request body policy for controlling body acceptance.
//!
//! [`RequestBodyPolicy`] determines whether a service accepts, buffers, or
//! streams request bodies, and at what limit. The runtime enforces the
//! effective policy before service invocation.

/// Policy controlling how request bodies are handled.
///
/// The runtime enforces this policy before service invocation. Services
/// declare their policy, and the runtime ensures no handler can exceed
/// the global ceiling.
///
/// # Variants
///
/// - [`Reject`](RequestBodyPolicy::Reject) — no body accepted (static service default)
/// - [`Buffer`](RequestBodyPolicy::Buffer) — buffer entire body in memory up to `max_bytes`
/// - [`Stream`](RequestBodyPolicy::Stream) — stream body chunks up to `max_bytes`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RequestBodyPolicy {
    /// Reject all request bodies. This is the static service default.
    #[default]
    Reject,
    /// Buffer the entire request body in memory.
    ///
    /// `max_bytes` is the hard limit. Bodies exceeding this are rejected
    /// before service invocation.
    Buffer { max_bytes: u64 },
    /// Stream the request body in chunks.
    ///
    /// `max_bytes` is the cumulative decoded-size limit across all chunks.
    /// The stream is terminated when the limit is reached.
    Stream { max_bytes: u64 },
}

impl RequestBodyPolicy {
    /// Returns `true` if this policy rejects all bodies.
    pub fn is_reject(&self) -> bool {
        matches!(self, Self::Reject)
    }

    /// Returns the effective byte limit, if any.
    pub fn max_bytes(&self) -> Option<u64> {
        match self {
            Self::Reject => None,
            Self::Buffer { max_bytes } | Self::Stream { max_bytes } => Some(*max_bytes),
        }
    }

    /// Returns `true` if this policy allows buffering.
    pub fn allows_buffer(&self) -> bool {
        matches!(self, Self::Buffer { .. })
    }

    /// Returns `true` if this policy allows streaming.
    pub fn allows_stream(&self) -> bool {
        matches!(self, Self::Stream { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_reject() {
        assert_eq!(RequestBodyPolicy::default(), RequestBodyPolicy::Reject);
    }

    #[test]
    fn reject_is_reject() {
        assert!(RequestBodyPolicy::Reject.is_reject());
        assert!(!RequestBodyPolicy::Buffer { max_bytes: 1024 }.is_reject());
    }

    #[test]
    fn max_bytes_returns_limit() {
        assert_eq!(RequestBodyPolicy::Reject.max_bytes(), None);
        assert_eq!(
            RequestBodyPolicy::Buffer { max_bytes: 1024 }.max_bytes(),
            Some(1024)
        );
        assert_eq!(
            RequestBodyPolicy::Stream { max_bytes: 4096 }.max_bytes(),
            Some(4096)
        );
    }

    #[test]
    fn allows_buffer_only_for_buffer() {
        assert!(!RequestBodyPolicy::Reject.allows_buffer());
        assert!(RequestBodyPolicy::Buffer { max_bytes: 1024 }.allows_buffer());
        assert!(!RequestBodyPolicy::Stream { max_bytes: 1024 }.allows_buffer());
    }

    #[test]
    fn allows_stream_only_for_stream() {
        assert!(!RequestBodyPolicy::Reject.allows_stream());
        assert!(!RequestBodyPolicy::Buffer { max_bytes: 1024 }.allows_stream());
        assert!(RequestBodyPolicy::Stream { max_bytes: 1024 }.allows_stream());
    }
}
