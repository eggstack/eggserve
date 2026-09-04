//! Transport-independent streaming response bodies.
//!
//! [`ResponseStream`] is the canonical one-shot byte stream for application
//! responses. It carries no Hyper types: it yields [`bytes::Bytes`] chunks or
//! a small EggServe-owned [`ResponseStreamError`]. The runtime remains the
//! only authority for `Content-Length`, `Transfer-Encoding`, and connection
//! reuse.
//!
//! # Chunk contract
//!
//! - Producers should keep individual chunks bounded (advisory 64 KiB,
//!   hard-split at the runtime `stream_chunk_size` for framing). Chunks larger
//!   than the runtime chunk size are split by the transport rather than
//!   rejected, so downstream framing stays bounded.
//! - Empty (`len == 0`) chunks are skipped by the transport and never produce
//!   an empty DATA frame. They do not count toward a known length.
//! - The stream is pull/backpressure driven: the transport polls only when
//!   downstream write capacity exists. No unbounded channel sits between
//!   producer and socket. Cross-thread adapters must use a bounded channel.
//! - Dropping the stream releases producer resources promptly. Client
//!   disconnect and shutdown drop the transport body, which drops this stream.
//!
//! # Length contract
//!
//! - `ResponseStream::new` declares unknown length: the runtime omits
//!   `Content-Length` and lets HTTP/1 select chunked framing.
//! - `ResponseStream::with_known_length` declares the exact representation
//!   length. Fewer or more bytes is a stream/protocol failure: after response
//!   commitment the connection is closed and structured diagnostics are
//!   emitted. No second HTTP error response is attempted.
//!
//! # Error privacy
//!
//! Producer failure details never reach the client. The wire sees only a
//! truncated/closed connection; diagnostics are emitted via `ops` events with
//! sanitized text. See [`ResponseStreamError`].

use bytes::Bytes;
use futures_util::Stream;
use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Maximum advisory application chunk size.
///
/// The transport splits larger chunks into `stream_chunk_size` pieces rather
/// than rejecting them. Producers should stay well below this (64 KiB
/// advisory) to keep per-chunk allocation bounded. The 1 MiB ceiling matches
/// the maximum `stream_chunk_size` so a single producer chunk never forces
/// more than one framing split unit beyond the configured transport size.
pub const MAX_RESPONSE_STREAM_CHUNK_BYTES: usize = 1024 * 1024;

/// Transport-neutral error for streaming response producers.
///
/// Carries no Hyper types and no framing state. The `Display` impl is
/// intentionally generic (`"response stream failed"`) so producer details are
/// never serialized to the client. Use [`ResponseStreamError::detail`] for
/// sanitized internal diagnostics only.
#[derive(Debug)]
pub struct ResponseStreamError {
    #[allow(dead_code)]
    detail: String,
}

impl ResponseStreamError {
    /// Create a producer failure.
    ///
    /// The message is for internal diagnostics only and is sanitized at the
    /// log site. It is never sent on the wire.
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }

    /// Create a known-length mismatch error.
    #[allow(dead_code)]
    pub(crate) fn length_mismatch(declared: u64, emitted: u64) -> Self {
        Self {
            detail: format!(
                "known-length mismatch: declared {} emitted {}",
                declared, emitted
            ),
        }
    }

    /// Returns the internal detail for sanitized logging.
    ///
    /// Callers must pass this through `ops::sanitize_text_field` before
    /// emitting. Never write it to the client.
    #[allow(dead_code)]
    pub(crate) fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for ResponseStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "response stream failed")
    }
}

impl std::error::Error for ResponseStreamError {}

impl From<std::io::Error> for ResponseStreamError {
    fn from(e: std::io::Error) -> Self {
        Self::new(e.to_string())
    }
}

/// A one-shot, transport-independent byte stream for responses.
///
/// Wraps any `Stream<Item = Result<Bytes, ResponseStreamError>>` without
/// exposing Hyper. The producer must be `Send`, but need not be `Sync`: it is
/// owned and polled by one connection task. The optional `known_length` is
/// the exact representation length when known; `None` means unknown length
/// (chunked framing).
///
/// The stream is one-shot: it is consumed once by transport conversion.
/// Dropping it (HEAD/body-forbidden suppression, client disconnect, shutdown)
/// releases producer resources promptly without polling.
pub struct ResponseStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, ResponseStreamError>> + Send>>,
    known_length: Option<u64>,
}

impl ResponseStream {
    /// Create an unknown-length stream.
    ///
    /// The runtime will omit `Content-Length` and let HTTP/1 select chunked
    /// framing. Callers must not attempt chunked coding themselves.
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<Bytes, ResponseStreamError>> + Send + 'static,
    {
        Self {
            inner: Box::pin(stream),
            known_length: None,
        }
    }

    /// Create a known-length stream.
    ///
    /// `len` is the exact number of payload bytes the stream will yield
    /// (empty chunks excluded). Fewer or more bytes is a protocol failure
    /// that closes the connection after commitment.
    pub fn with_known_length<S>(stream: S, len: u64) -> Self
    where
        S: Stream<Item = Result<Bytes, ResponseStreamError>> + Send + 'static,
    {
        Self {
            inner: Box::pin(stream),
            known_length: Some(len),
        }
    }

    /// Returns the declared representation length, if known.
    pub fn known_length(&self) -> Option<u64> {
        self.known_length
    }

    /// Returns `true` when a known length was declared.
    pub fn is_known_length(&self) -> bool {
        self.known_length.is_some()
    }

    /// Create an empty known-length (0) stream.
    pub fn empty() -> Self {
        Self::with_known_length(futures_util::stream::empty(), 0)
    }

    pub(crate) fn into_inner(
        self,
    ) -> Pin<Box<dyn Stream<Item = Result<Bytes, ResponseStreamError>> + Send>> {
        self.inner
    }
}

impl fmt::Debug for ResponseStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseStream")
            .field("known_length", &self.known_length)
            .finish_non_exhaustive()
    }
}

impl Stream for ResponseStream {
    type Item = Result<Bytes, ResponseStreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    #[test]
    fn unknown_stream_has_no_length() {
        let s = ResponseStream::new(stream::empty::<Result<Bytes, ResponseStreamError>>());
        assert_eq!(s.known_length(), None);
        assert!(!s.is_known_length());
    }

    #[test]
    fn known_stream_reports_length() {
        let s = ResponseStream::with_known_length(
            stream::empty::<Result<Bytes, ResponseStreamError>>(),
            42,
        );
        assert_eq!(s.known_length(), Some(42));
        assert!(s.is_known_length());
    }

    #[test]
    fn error_display_is_generic() {
        let e = ResponseStreamError::new("/secret/path leaked");
        assert_eq!(e.to_string(), "response stream failed");
        assert!(e.detail().contains("/secret/path"));
    }

    #[test]
    fn debug_does_not_leak_contents() {
        let s = ResponseStream::with_known_length(
            stream::once(async { Ok::<_, ResponseStreamError>(Bytes::from("secret")) }),
            6,
        );
        let dbg = format!("{:?}", s);
        assert!(dbg.contains("known_length"));
        assert!(!dbg.contains("secret"));
    }

    #[tokio::test]
    async fn stream_polls_through() {
        use futures_util::StreamExt;
        let mut s = ResponseStream::new(stream::once(async {
            Ok::<_, ResponseStreamError>(Bytes::from("hi"))
        }));
        let chunk = s.next().await.unwrap().unwrap();
        assert_eq!(&chunk[..], b"hi");
    }
}
