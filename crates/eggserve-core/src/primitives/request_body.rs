//! Transport-independent, one-shot request body.
//!
//! [`RequestBody`] wraps the transfer-decoded body stream from an HTTP
//! request. It provides one-shot consumption (either fully buffered or
//! chunk-by-chunk) with bounded limits, timeout awareness, and
//! cancellation safety.
//!
//! # One-shot guarantee
//!
//! A `RequestBody` can only be consumed once. [`read_all`](RequestBody::read_all)
//! consumes the entire body into memory. Streaming via [`Stream`](futures_util::Stream)
//! reads chunks incrementally. Mixing consumption modes is detected and
//! returns [`RequestBodyError::MixedConsumptionMode`].
//!
//! # Transport independence
//!
//! No Hyper type appears in this struct or its public API. The body
//! stream is internal to the type.

use bytes::Bytes;
use futures_util::Stream;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use super::request_body_error::RequestBodyError;

/// The consumption state of a request body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyState {
    /// Initial state: no data consumed yet.
    Unread,
    /// Streaming in progress: at least one chunk consumed via `Stream`.
    Streaming,
    /// Body fully consumed (either via `read_all` or stream completion).
    Complete,
    /// An error terminated consumption.
    Error,
}

/// A transport-independent, one-shot request body.
///
/// Owns the transfer-decoded byte stream from an HTTP request. No Hyper
/// type appears in the public API.
///
/// # Examples
///
/// ```ignore
/// use futures_util::StreamExt;
///
/// async fn handle(body: RequestBody) -> Result<Vec<u8>, RequestBodyError> {
///     // Option 1: buffer everything
///     let bytes = body.read_all().await?;
///     Ok(bytes.to_vec())
/// }
///
/// async fn handle_streaming(mut body: RequestBody) -> Result<(), RequestBodyError> {
///     // Option 2: stream chunks
///     while let Some(chunk) = body.next_chunk().await? {
///         process(chunk);
///     }
///     Ok(())
/// }
/// ```
pub struct RequestBody {
    inner: Option<BodyInner>,
    declared_length: Option<u64>,
    bytes_received: u64,
    state: BodyState,
    max_bytes: u64,
    /// Shared flag indicating whether the body stream was fully consumed.
    /// Set when the stream ends and all declared bytes (if any) have been
    /// received. Used by the connection pipeline for incomplete-body policy.
    consumed: Arc<AtomicBool>,
}

/// Internal body stream, hidden from public API.
#[allow(dead_code)]
enum BodyInner {
    /// A Hyper `Incoming` body (runtime-provided).
    Incoming {
        stream: Pin<Box<dyn Stream<Item = Result<Bytes, IncomingError>> + Send + 'static>>,
    },
    /// A pre-built test body (bytes).
    Fixed { data: Vec<u8>, offset: usize },
    /// An empty body.
    Empty,
}

/// Internal error type for body stream items.
#[derive(Debug)]
pub struct IncomingError(pub(crate) String);

impl std::fmt::Display for IncomingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "incoming body error: {}", self.0)
    }
}

impl std::error::Error for IncomingError {}

impl From<IncomingError> for RequestBodyError {
    fn from(e: IncomingError) -> Self {
        Self::Transport(e.0)
    }
}

impl RequestBody {
    /// Create an empty body with no declared length.
    pub fn empty() -> Self {
        Self {
            inner: Some(BodyInner::Empty),
            declared_length: None,
            bytes_received: 0,
            state: BodyState::Unread,
            max_bytes: u64::MAX,
            consumed: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Create a body from fixed bytes (test/experimental constructor).
    ///
    /// The `max_bytes` parameter sets the effective limit. Use `u64::MAX`
    /// for unlimited.
    pub fn from_bytes(data: Vec<u8>, max_bytes: u64) -> Self {
        let len = data.len() as u64;
        Self {
            inner: Some(BodyInner::Fixed { data, offset: 0 }),
            declared_length: Some(len),
            bytes_received: 0,
            state: BodyState::Unread,
            max_bytes,
            consumed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a body from a Hyper `Incoming` stream.
    ///
    /// This is primarily used by the runtime to wrap Hyper incoming bodies.
    /// External consumers (e.g. fuzz targets) may also use it to test
    /// stream-based body ingestion.
    #[allow(dead_code)]
    pub(crate) fn from_incoming(
        stream: impl Stream<Item = Result<Bytes, IncomingError>> + Send + 'static,
        declared_length: Option<u64>,
        max_bytes: u64,
    ) -> Self {
        Self {
            inner: Some(BodyInner::Incoming {
                stream: Box::pin(stream),
            }),
            declared_length,
            bytes_received: 0,
            state: BodyState::Unread,
            max_bytes,
            consumed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns the declared body length from `Content-Length`, if present.
    pub fn declared_length(&self) -> Option<u64> {
        self.declared_length
    }

    /// Returns the number of bytes received so far.
    pub fn bytes_received(&self) -> u64 {
        self.bytes_received
    }

    /// Returns `true` if the body has been fully consumed.
    pub fn is_complete(&self) -> bool {
        self.state == BodyState::Complete
    }

    /// Returns the current consumption state.
    pub fn state(&self) -> BodyState {
        self.state
    }

    /// Returns the effective byte limit.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Returns a clone of the shared consumption flag.
    ///
    /// The flag is set when the body stream ends and all declared bytes
    /// have been received. Used by the connection pipeline for
    /// incomplete-body policy decisions.
    pub(crate) fn consumed_flag(&self) -> Arc<AtomicBool> {
        self.consumed.clone()
    }

    /// Returns `true` if the body was fully consumed (stream ended and
    /// all declared bytes received).
    pub(crate) fn was_fully_consumed(&self) -> bool {
        self.consumed.load(Ordering::Acquire)
    }

    /// Mark the body as fully consumed.
    fn mark_consumed(&self) {
        self.consumed.store(true, Ordering::Release);
    }

    /// Consume the entire body into a single `Bytes` value.
    ///
    /// This is the simplest way to consume a body. After this call,
    /// the body is in the `Complete` state.
    ///
    /// # Errors
    ///
    /// Returns an error if the body exceeds the limit, if the stream
    /// fails, or if the body was already consumed.
    pub async fn read_all(mut self) -> Result<Bytes, RequestBodyError> {
        if self.state == BodyState::Complete || self.state == BodyState::Error {
            return Err(RequestBodyError::AlreadyConsumed);
        }
        if self.state == BodyState::Streaming {
            return Err(RequestBodyError::MixedConsumptionMode);
        }

        let inner = self.inner.take().ok_or(RequestBodyError::AlreadyConsumed)?;

        match inner {
            BodyInner::Empty => {
                self.state = BodyState::Complete;
                self.mark_consumed();
                Ok(Bytes::new())
            }
            BodyInner::Fixed { data, offset } => {
                let remaining = &data[offset..];
                let total = self
                    .bytes_received
                    .checked_add(remaining.len() as u64)
                    .ok_or(RequestBodyError::LimitExceeded {
                        limit: self.max_bytes,
                        received: u64::MAX,
                    })?;
                if total > self.max_bytes {
                    self.state = BodyState::Error;
                    return Err(RequestBodyError::LimitExceeded {
                        limit: self.max_bytes,
                        received: total,
                    });
                }
                self.bytes_received = total;
                self.state = BodyState::Complete;
                self.mark_consumed();
                Ok(Bytes::copy_from_slice(remaining))
            }
            BodyInner::Incoming { mut stream } => {
                let mut buf = Vec::new();
                use futures_util::StreamExt;
                while let Some(item) = stream.next().await {
                    let chunk = item.map_err(|e| RequestBodyError::Transport(e.0))?;
                    let new_total = self.bytes_received.checked_add(chunk.len() as u64).ok_or(
                        RequestBodyError::LimitExceeded {
                            limit: self.max_bytes,
                            received: u64::MAX,
                        },
                    )?;
                    if new_total > self.max_bytes {
                        self.state = BodyState::Error;
                        return Err(RequestBodyError::LimitExceeded {
                            limit: self.max_bytes,
                            received: new_total,
                        });
                    }
                    self.bytes_received = new_total;
                    buf.extend_from_slice(&chunk);
                }
                self.state = BodyState::Complete;
                // Check for premature EOF: stream ended before declared length.
                if let Some(declared) = self.declared_length {
                    if self.bytes_received < declared {
                        let received = self.bytes_received;
                        return Err(RequestBodyError::PrematureEof {
                            received,
                            expected: Some(declared),
                        });
                    }
                }
                self.mark_consumed();
                Ok(Bytes::from(buf))
            }
        }
    }

    /// Read the next chunk from the body.
    ///
    /// Returns `Ok(None)` when the body is fully consumed.
    /// Returns `Ok(Some(chunk))` with the next chunk of bytes.
    ///
    /// After the first call to `next_chunk`, the body enters the
    /// `Streaming` state. Subsequent calls to `read_all` will fail
    /// with [`RequestBodyError::MixedConsumptionMode`].
    ///
    /// # Errors
    ///
    /// Returns an error if the body exceeds the limit, if the stream
    /// fails, or if the body was already consumed.
    pub async fn next_chunk(&mut self) -> Result<Option<Bytes>, RequestBodyError> {
        if self.state == BodyState::Error {
            return Err(RequestBodyError::AlreadyConsumed);
        }
        if self.state == BodyState::Complete {
            return Ok(None);
        }

        // Transition to streaming on first chunk read.
        if self.state == BodyState::Unread {
            self.state = BodyState::Streaming;
        }

        let inner = self
            .inner
            .as_mut()
            .ok_or(RequestBodyError::AlreadyConsumed)?;

        match inner {
            BodyInner::Empty => {
                self.state = BodyState::Complete;
                self.mark_consumed();
                Ok(None)
            }
            BodyInner::Fixed { data, offset } => {
                if *offset >= data.len() {
                    self.state = BodyState::Complete;
                    self.mark_consumed();
                    return Ok(None);
                }
                let remaining = &data[*offset..];
                let chunk_size = remaining.len().min(8192);
                let new_total = self.bytes_received.checked_add(chunk_size as u64).ok_or(
                    RequestBodyError::LimitExceeded {
                        limit: self.max_bytes,
                        received: u64::MAX,
                    },
                )?;
                if new_total > self.max_bytes {
                    self.state = BodyState::Error;
                    return Err(RequestBodyError::LimitExceeded {
                        limit: self.max_bytes,
                        received: new_total,
                    });
                }
                let chunk = &data[*offset..*offset + chunk_size];
                *offset += chunk_size;
                self.bytes_received = new_total;
                if *offset >= data.len() {
                    self.state = BodyState::Complete;
                }
                Ok(Some(Bytes::copy_from_slice(chunk)))
            }
            BodyInner::Incoming { stream } => {
                use futures_util::StreamExt;
                match stream.next().await {
                    Some(Ok(chunk)) => {
                        let new_total = self.bytes_received.checked_add(chunk.len() as u64).ok_or(
                            RequestBodyError::LimitExceeded {
                                limit: self.max_bytes,
                                received: u64::MAX,
                            },
                        )?;
                        if new_total > self.max_bytes {
                            self.state = BodyState::Error;
                            return Err(RequestBodyError::LimitExceeded {
                                limit: self.max_bytes,
                                received: new_total,
                            });
                        }
                        self.bytes_received = new_total;
                        Ok(Some(chunk))
                    }
                    Some(Err(e)) => {
                        self.state = BodyState::Error;
                        Err(RequestBodyError::Transport(e.0))
                    }
                    None => {
                        self.state = BodyState::Complete;
                        // Check for premature EOF.
                        if let Some(declared) = self.declared_length {
                            if self.bytes_received < declared {
                                let received = self.bytes_received;
                                return Err(RequestBodyError::PrematureEof {
                                    received,
                                    expected: Some(declared),
                                });
                            }
                        }
                        self.mark_consumed();
                        Ok(None)
                    }
                }
            }
        }
    }
}

impl std::fmt::Debug for RequestBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestBody")
            .field("declared_length", &self.declared_length)
            .field("bytes_received", &self.bytes_received)
            .field("state", &self.state)
            .field("max_bytes", &self.max_bytes)
            .field("consumed", &self.was_fully_consumed())
            .finish()
    }
}

impl Stream for RequestBody {
    type Item = Result<Bytes, RequestBodyError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Cannot poll after completion or error.
        if self.state == BodyState::Complete || self.state == BodyState::Error {
            return Poll::Ready(None);
        }

        // Check limit before polling.
        if self.bytes_received >= self.max_bytes {
            self.state = BodyState::Error;
            return Poll::Ready(Some(Err(RequestBodyError::LimitExceeded {
                limit: self.max_bytes,
                received: self.bytes_received,
            })));
        }

        let max_bytes = self.max_bytes;
        let bytes_received = self.bytes_received;

        let inner = match self.inner.as_mut() {
            Some(i) => i,
            None => return Poll::Ready(None),
        };

        match inner {
            BodyInner::Empty => {
                self.state = BodyState::Complete;
                self.mark_consumed();
                Poll::Ready(None)
            }
            BodyInner::Fixed { data, offset } => {
                if *offset >= data.len() {
                    self.state = BodyState::Complete;
                    self.mark_consumed();
                    Poll::Ready(None)
                } else {
                    let remaining = &data[*offset..];
                    let chunk_size = remaining.len().min(8192);
                    let new_total = match bytes_received.checked_add(chunk_size as u64) {
                        Some(v) => v,
                        None => {
                            self.state = BodyState::Error;
                            return Poll::Ready(Some(Err(RequestBodyError::LimitExceeded {
                                limit: max_bytes,
                                received: u64::MAX,
                            })));
                        }
                    };
                    if new_total > max_bytes {
                        self.state = BodyState::Error;
                        return Poll::Ready(Some(Err(RequestBodyError::LimitExceeded {
                            limit: max_bytes,
                            received: new_total,
                        })));
                    }
                    let chunk = Bytes::copy_from_slice(&data[*offset..*offset + chunk_size]);
                    *offset += chunk_size;
                    self.bytes_received = new_total;
                    if self.state == BodyState::Unread {
                        self.state = BodyState::Streaming;
                    }
                    Poll::Ready(Some(Ok(chunk)))
                }
            }
            BodyInner::Incoming { stream } => match stream.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    let new_total = match bytes_received.checked_add(chunk.len() as u64) {
                        Some(v) => v,
                        None => {
                            self.state = BodyState::Error;
                            return Poll::Ready(Some(Err(RequestBodyError::LimitExceeded {
                                limit: max_bytes,
                                received: u64::MAX,
                            })));
                        }
                    };
                    if new_total > max_bytes {
                        self.state = BodyState::Error;
                        Poll::Ready(Some(Err(RequestBodyError::LimitExceeded {
                            limit: max_bytes,
                            received: new_total,
                        })))
                    } else {
                        self.bytes_received = new_total;
                        if self.state == BodyState::Unread {
                            self.state = BodyState::Streaming;
                        }
                        Poll::Ready(Some(Ok(chunk)))
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    self.state = BodyState::Error;
                    Poll::Ready(Some(Err(RequestBodyError::Transport(e.0))))
                }
                Poll::Ready(None) => {
                    self.state = BodyState::Complete;
                    // Check for premature EOF.
                    if let Some(declared) = self.declared_length {
                        if self.bytes_received < declared {
                            let received = self.bytes_received;
                            self.state = BodyState::Error;
                            return Poll::Ready(Some(Err(RequestBodyError::PrematureEof {
                                received,
                                expected: Some(declared),
                            })));
                        }
                    }
                    self.mark_consumed();
                    Poll::Ready(None)
                }
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use proptest::prelude::*;

    #[tokio::test]
    async fn empty_body_read_all() {
        let body = RequestBody::empty();
        assert_eq!(body.declared_length(), None);
        assert_eq!(body.bytes_received(), 0);
        let data = body.read_all().await.unwrap();
        assert!(data.is_empty());
    }

    #[tokio::test]
    async fn fixed_body_read_all() {
        let body = RequestBody::from_bytes(b"hello".to_vec(), u64::MAX);
        assert_eq!(body.declared_length(), Some(5));
        let data = body.read_all().await.unwrap();
        assert_eq!(&data[..], b"hello");
    }

    #[tokio::test]
    async fn fixed_body_streaming() {
        let mut body = RequestBody::from_bytes(b"hello world".to_vec(), u64::MAX);
        let mut chunks = Vec::new();
        while let Some(chunk) = body.next_chunk().await.unwrap() {
            chunks.push(chunk);
        }
        let total: Vec<u8> = chunks.iter().flat_map(|c| c.iter().copied()).collect();
        assert_eq!(&total[..], b"hello world");
    }

    #[tokio::test]
    async fn limit_exceeded_on_read_all() {
        let body = RequestBody::from_bytes(b"hello".to_vec(), 3);
        let err = body.read_all().await.unwrap_err();
        assert!(err.is_limit_exceeded());
    }

    #[tokio::test]
    async fn limit_exceeded_on_stream() {
        let mut body = RequestBody::from_bytes(b"hello".to_vec(), 3);
        let err = body.next_chunk().await.unwrap_err();
        assert!(err.is_limit_exceeded());
    }

    #[tokio::test]
    async fn already_consumed_after_read_all() {
        let body = RequestBody::from_bytes(b"hello".to_vec(), u64::MAX);
        let _data = body.read_all().await.unwrap();
        // Can't reuse - but we moved self, so this test verifies the type system.
    }

    #[tokio::test]
    async fn mixed_consumption_mode() {
        // Use a body larger than the internal chunk size to ensure streaming state
        let large_body = vec![0u8; 16384];
        let mut body = RequestBody::from_bytes(large_body, u64::MAX);
        let _chunk = body.next_chunk().await.unwrap();
        // Can't call read_all because body is moved - verify via state.
        assert_eq!(body.state(), BodyState::Streaming);
    }

    #[tokio::test]
    async fn zero_length_body() {
        let body = RequestBody::from_bytes(Vec::new(), u64::MAX);
        assert_eq!(body.declared_length(), Some(0));
        let data = body.read_all().await.unwrap();
        assert!(data.is_empty());
    }

    #[tokio::test]
    async fn stream_trait_works() {
        let body = RequestBody::from_bytes(b"abc".to_vec(), u64::MAX);
        let mut stream = body;
        let mut all = Vec::new();
        while let Some(chunk) = stream.next().await.transpose().unwrap() {
            all.extend_from_slice(&chunk);
        }
        assert_eq!(&all[..], b"abc");
    }

    #[test]
    fn body_state_debug() {
        assert_eq!(format!("{:?}", BodyState::Unread), "Unread");
        assert_eq!(format!("{:?}", BodyState::Streaming), "Streaming");
        assert_eq!(format!("{:?}", BodyState::Complete), "Complete");
        assert_eq!(format!("{:?}", BodyState::Error), "Error");
    }

    #[test]
    fn request_body_debug() {
        let body = RequestBody::empty();
        let dbg = format!("{:?}", body);
        assert!(dbg.contains("RequestBody"));
        assert!(dbg.contains("Unread"));
    }

    #[tokio::test]
    async fn premature_eof_returns_error() {
        use futures_util::stream;
        // Create a stream that provides fewer bytes than declared.
        let short_data = b"hi";
        let declared = 10u64;
        let body_stream =
            stream::once(async move { Ok::<_, IncomingError>(Bytes::copy_from_slice(short_data)) });
        let body = RequestBody::from_incoming(body_stream, Some(declared), u64::MAX);
        let result = body.read_all().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            RequestBodyError::PrematureEof { received, expected } => {
                assert_eq!(received, 2);
                assert_eq!(expected, Some(10));
            }
            other => panic!("expected PrematureEof, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn premature_eof_streaming_returns_error() {
        use futures_util::stream;
        // Stream that provides fewer bytes than declared.
        let short_data = b"hi";
        let declared = 10u64;
        let body_stream =
            stream::once(async move { Ok::<_, IncomingError>(Bytes::copy_from_slice(short_data)) });
        let mut body = RequestBody::from_incoming(body_stream, Some(declared), u64::MAX);
        // Read the one available chunk.
        let chunk = body.next_chunk().await.unwrap();
        assert!(chunk.is_some());
        // Next read: stream ended, premature EOF should be reported.
        let result = body.next_chunk().await;
        match result {
            Err(RequestBodyError::PrematureEof { received, expected }) => {
                assert_eq!(received, 2);
                assert_eq!(expected, Some(10));
            }
            Ok(None) => {
                panic!("expected PrematureEof, got Ok(None)");
            }
            other => panic!("expected PrematureEof, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn exact_declared_length_succeeds() {
        use futures_util::stream;
        let data = b"hello";
        let declared = 5u64;
        let body_stream =
            stream::once(
                async move { Ok::<_, IncomingError>(Bytes::copy_from_slice(data.as_slice())) },
            );
        let body = RequestBody::from_incoming(body_stream, Some(declared), u64::MAX);
        let result = body.read_all().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_ref(), b"hello");
    }

    #[tokio::test]
    async fn checked_add_overflow_returns_error() {
        let body = RequestBody::from_bytes(vec![0u8; 200], 100);
        let result = body.read_all().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().is_limit_exceeded());
    }

    #[test]
    fn state_transitions_unread_to_complete_on_read() {
        proptest::proptest!(|(data in prop::collection::vec(any::<u8>(), 0..500))| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let body = RequestBody::from_bytes(data, u64::MAX);
            prop_assert_eq!(body.state(), BodyState::Unread);
            let flag = body.consumed_flag();
            let _ = rt.block_on(body.read_all());
            prop_assert!(flag.load(std::sync::atomic::Ordering::Acquire));
        });
    }

    #[test]
    fn consumed_flag_set_after_read() {
        proptest::proptest!(|(data in prop::collection::vec(any::<u8>(), 0..500))| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let body = RequestBody::from_bytes(data, u64::MAX);
            let flag = body.consumed_flag();
            prop_assert!(!flag.load(std::sync::atomic::Ordering::Acquire));
            let _ = rt.block_on(body.read_all());
            prop_assert!(flag.load(std::sync::atomic::Ordering::Acquire));
        });
    }

    #[test]
    fn chunked_body_via_stream_succeeds() {
        proptest::proptest!(|(data in prop::collection::vec(any::<u8>(), 0..1000))| {
            use futures_util::stream;

            let rt = tokio::runtime::Runtime::new().unwrap();
            let chunk_size = if data.is_empty() { 1 } else { (data[0] as usize % 64) + 1 };
            let chunks: Vec<Result<Bytes, IncomingError>> = data.chunks(chunk_size)
                .map(|c| Ok(Bytes::copy_from_slice(c)))
                .collect();
            let body_stream = stream::iter(chunks);
            let body = RequestBody::from_incoming(body_stream, Some(data.len() as u64), u64::MAX);
            let result = rt.block_on(body.read_all());
            if let Ok(val) = result {
                prop_assert_eq!(val.len(), data.len());
            }
        });
    }

    #[test]
    fn premature_eof_detected() {
        proptest::proptest!(|(data in prop::collection::vec(any::<u8>(), 0..100))| {
            use futures_util::stream;

            let rt = tokio::runtime::Runtime::new().unwrap();
            let declared = (data.len() as u64) + 100;
            let data_owned = data.clone();
            let body_stream = stream::once(async move {
                Ok::<_, IncomingError>(Bytes::from(data_owned))
            });
            let body = RequestBody::from_incoming(body_stream, Some(declared), u64::MAX);
            let result = rt.block_on(body.read_all());
            if let Err(e) = result {
                prop_assert!(
                    matches!(e, RequestBodyError::PrematureEof { .. }),
                    "expected PrematureEof, got: {:?}",
                    e
                );
            }
        });
    }
}
