//! Transport-independent streaming response bodies with trailers.
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
//! # Trailers (Plan 198 Track C)
//!
//! One terminal trailer source may be attached via
//! [`ResponseStream::with_trailers`] (unknown length) or
//! [`ResponseStream::with_known_length_and_trailers`] (known length):
//!
//! - exactly one terminal trailer block; no data after trailers (enforced by
//!   the transport adapter, which polls the byte stream to completion, then
//!   polls the trailer future once, then ends);
//! - `HEAD`/body-forbidden responses never poll the body or trailer producer
//!   (dropping releases both promptly);
//! - producer cancellation/drop remains deterministic (dropping the
//!   `ResponseStream` drops both the byte stream and the trailer future);
//! - known-length semantics stay coherent: the declared length counts data
//!   bytes only, trailers never count toward it;
//! - adapters map trailers without buffering the entire body (incremental
//!   body polling, then one trailer future poll);
//! - ordinary byte-only streams stay ergonomic via [`ResponseStream::new`].
//!
//! Trailer validation reuses the single canonical [`Trailers`](super::trailers::Trailers)
//! validator; adapters never maintain a second policy.
//!
//! # Error privacy
//!
//! Producer failure details never reach the client. The wire sees only a
//! truncated/closed connection; diagnostics are emitted via `ops` events with
//! sanitized text. See [`ResponseStreamError`].

use bytes::Bytes;
use futures_util::Stream;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::trailers::Trailers;

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
            detail: format!("known-length mismatch: declared {declared} emitted {emitted}"),
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

/// Terminal trailer future: polled once after bytes, yielding one block or none.
#[allow(clippy::type_complexity)]
pub(crate) type TrailerFuture =
    Pin<Box<dyn Future<Output = Result<Option<Trailers>, ResponseStreamError>> + Send>>;

/// Byte-stream half of a response stream.
#[allow(clippy::type_complexity)]
pub(crate) type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, ResponseStreamError>> + Send>>;

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
/// releases producer resources promptly without polling (both byte and trailer
/// producers are dropped together).
pub struct ResponseStream {
    inner: ByteStream,
    known_length: Option<u64>,
    trailers: Option<TrailerFuture>,
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
            trailers: None,
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
            trailers: None,
        }
    }

    /// Create an unknown-length stream with one terminal trailer source.
    ///
    /// The trailer future is polled exactly once after the byte stream ends;
    /// `Ok(None)` means no trailers, `Ok(Some(block))` emits one terminal
    /// block, `Err` fails the stream after commitment (truncated close, no
    /// second HTTP error). No data may follow trailers by construction.
    pub fn with_trailers<S, F>(stream: S, trailer_future: F) -> Self
    where
        S: Stream<Item = Result<Bytes, ResponseStreamError>> + Send + 'static,
        F: Future<Output = Result<Option<Trailers>, ResponseStreamError>> + Send + 'static,
    {
        Self {
            inner: Box::pin(stream),
            known_length: None,
            trailers: Some(Box::pin(trailer_future)),
        }
    }

    /// Create a known-length stream with one terminal trailer source.
    ///
    /// The declared length counts data bytes only; trailers never count
    /// toward it. Length validation runs on data bytes before the trailer
    /// future is polled.
    pub fn with_known_length_and_trailers<S, F>(stream: S, len: u64, trailer_future: F) -> Self
    where
        S: Stream<Item = Result<Bytes, ResponseStreamError>> + Send + 'static,
        F: Future<Output = Result<Option<Trailers>, ResponseStreamError>> + Send + 'static,
    {
        Self {
            inner: Box::pin(stream),
            known_length: Some(len),
            trailers: Some(Box::pin(trailer_future)),
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

    /// Returns `true` when a terminal trailer source is attached.
    pub fn has_trailers(&self) -> bool {
        self.trailers.is_some()
    }

    /// Create an empty known-length (0) stream.
    pub fn empty() -> Self {
        Self::with_known_length(futures_util::stream::empty(), 0)
    }

    #[allow(dead_code)]
    pub(crate) fn into_inner(self) -> ByteStream {
        self.inner
    }

    /// Take the terminal trailer future, if attached.
    pub(crate) fn take_trailer_future(&mut self) -> Option<TrailerFuture> {
        self.trailers.take()
    }

    /// Take both the byte stream and the trailer future.
    pub(crate) fn into_parts(self) -> (ByteStream, Option<TrailerFuture>) {
        (self.inner, self.trailers)
    }
}

impl fmt::Debug for ResponseStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseStream")
            .field("known_length", &self.known_length)
            .field("has_trailers", &self.has_trailers())
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
        let dbg = format!("{s:?}");
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
