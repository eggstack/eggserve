//! Canonical outbound conversion adapters (Plan 206 Track C).
//!
//! Owns the explicit low-level `Response -> hyper::Response` boundary
//! ([`to_hyper_response`] plus the crate-internal semaphore/chunk-size
//! overloads). Returns an opaque `http_body::Body`; downstream code must
//! not name `BoxBody`/`UnsyncBoxBody`. Normalization stays in `response`;
//! this module never invents framing.

use super::super::body::BodySource;
use super::super::header_block::HeaderError;
use super::super::response_stream::{ResponseStream, ResponseStreamError};
use super::response::Response;
use super::response_body::ResponseBody;
use super::status::ResponseConstructionError;

/// Convert a canonical [`Response`] into the explicit Hyper transport boundary.
///
/// This is the final step after normalization. The response body is consumed.
/// The returned body type is intentionally opaque: downstream code should
/// depend on its `http_body::Body` behavior, not on a concrete erasure type.
/// The one-owner response-stream model remains `Send` without requiring
/// producer `Sync`; concurrent body polling is unsupported.
///
/// Streaming and file-stream observability in this standalone adapter resolve
/// through the process-global default. Runtime pipeline conversions that own
/// an [`crate::ops::OpsContext`] use the contextual conversion path instead
/// so per-runtime counters stay isolated.
pub fn to_hyper_response(
    response: Response,
) -> Result<
    hyper::Response<impl http_body::Body<Data = bytes::Bytes, Error = std::io::Error>>,
    ResponseConstructionError,
> {
    to_hyper_response_with_optional_file_stream_semaphore(
        response,
        None,
        crate::limits::DEFAULT_STREAM_CHUNK_SIZE,
        None,
    )
}

/// Convert a canonical response while enforcing the runtime file-stream
/// admission limit for every file-backed body.
///
/// Streaming/file-stream observability resolves through the process-global
/// default; the runtime pipeline prefers
/// [`to_hyper_response_with_file_stream_semaphore_and_chunk_size`] with its
/// own context.
#[allow(dead_code)]
pub(crate) fn to_hyper_response_with_file_stream_semaphore(
    response: Response,
    semaphore: &std::sync::Arc<tokio::sync::Semaphore>,
) -> Result<
    hyper::Response<http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, std::io::Error>>,
    ResponseConstructionError,
> {
    to_hyper_response_with_optional_file_stream_semaphore(
        response,
        Some(semaphore),
        crate::limits::DEFAULT_STREAM_CHUNK_SIZE,
        None,
    )
}

/// Convert a canonical response using a configured file-stream chunk size.
///
/// `ops` carries the runtime observability context for streaming and
/// file-stream counters/events; `None` retains the process-global default
/// for standalone (non-runtime) conversions.
pub(crate) fn to_hyper_response_with_file_stream_semaphore_and_chunk_size(
    response: Response,
    semaphore: &std::sync::Arc<tokio::sync::Semaphore>,
    stream_chunk_size: usize,
    ops: Option<&crate::ops::OpsContext>,
) -> Result<
    hyper::Response<http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, std::io::Error>>,
    ResponseConstructionError,
> {
    to_hyper_response_with_optional_file_stream_semaphore(
        response,
        Some(semaphore),
        stream_chunk_size,
        ops,
    )
}

fn to_hyper_response_with_optional_file_stream_semaphore(
    response: Response,
    semaphore: Option<&std::sync::Arc<tokio::sync::Semaphore>>,
    stream_chunk_size: usize,
    ops: Option<&crate::ops::OpsContext>,
) -> Result<
    hyper::Response<http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, std::io::Error>>,
    ResponseConstructionError,
> {
    use bytes::Bytes;
    use http_body_util::BodyExt;
    use http_body_util::Full;

    let status = response.status();
    let code = status.as_u16();
    let hyper_status = hyper::StatusCode::from_u16(code)
        .map_err(|_| ResponseConstructionError::InvalidStatus(code))?;

    let mut builder = hyper::Response::builder().status(hyper_status);
    for field in response.headers().iter() {
        // Byte-preserving outbound conversion: canonical bytes are already in
        // the transport-accepted domain, so this preserves exact octets
        // without UTF-8 coercion. Framing/privacy stripping already ran in
        // normalization; this step does not bypass response policy.
        let name = hyper::header::HeaderName::from_bytes(field.name.as_str().as_bytes())
            .map_err(|_| ResponseConstructionError::InvalidHeader(HeaderError::InvalidName))?;
        let value = hyper::header::HeaderValue::from_bytes(field.value.as_bytes())
            .map_err(|_| ResponseConstructionError::InvalidHeader(HeaderError::InvalidValue))?;
        builder = builder.header(name, value);
    }

    let body = match response.body {
        Some(ResponseBody::Empty) => Full::new(Bytes::new())
            .map_err(|never| match never {})
            .boxed_unsync(),
        Some(ResponseBody::Bytes(b)) => Full::new(Bytes::from(b))
            .map_err(|never| match never {})
            .boxed_unsync(),
        Some(ResponseBody::File(source)) => {
            let permit = semaphore
                .map(|s| s.clone().try_acquire_owned())
                .transpose()
                .map_err(|_| ResponseConstructionError::FileStreamLimit)?;
            let permit = permit.map(|p| CountingFileStreamPermit::new(p, ops));
            file_body(source, permit, stream_chunk_size)
        }
        Some(ResponseBody::Stream(stream)) => {
            // Defense-in-depth: body-forbidden statuses must never emit
            // application bytes even if a caller forgot `normalize_response`.
            // HEAD suppression requires request context and must happen in
            // `normalize_response`/pipeline; here we only guard statuses.
            if !status.permits_payload_body() {
                drop(stream);
                Full::new(Bytes::new())
                    .map_err(|never| match never {})
                    .boxed_unsync()
            } else {
                stream_body(stream, stream_chunk_size, ops)
            }
        }
        Some(ResponseBody::EmptyWithLength(_)) => Full::new(Bytes::new())
            .map_err(|never| match never {})
            .boxed_unsync(),
        None => Full::new(Bytes::new())
            .map_err(|never| match never {})
            .boxed_unsync(),
    };

    let mut response = builder
        .body(body)
        .map_err(|_| ResponseConstructionError::InvalidHeader(HeaderError::InvalidValue))?;
    crate::response::finalize_origin_headers(&mut response, std::time::SystemTime::now());
    Ok(response)
}

fn file_body(
    source: BodySource,
    permit: Option<CountingFileStreamPermit>,
    stream_chunk_size: usize,
) -> http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, std::io::Error> {
    // B-03 note: each chunk iteration allocates a fresh `vec![0; chunk_len]`
    // (moved into `Bytes`). For the default 8 KiB chunk size the cost is modest;
    // for large `stream_chunk_size` (up to 1 MiB) the allocation rate scales with
    // `file_len / chunk_size`. `BytesMut` reuse would reduce this but is a
    // pure optimization with no correctness impact (see `benchmarks/088-baseline`).
    use bytes::Bytes;
    use futures_util::stream;
    use http_body_util::{BodyExt, StreamBody};
    use hyper::body::Frame;
    use tokio::io::AsyncSeekExt;

    let (file, start, remaining) = match source {
        BodySource::FileFull { file, len, .. } => (tokio::fs::File::from_std(file), 0, len),
        BodySource::FileRange { file, range, .. } => {
            (tokio::fs::File::from_std(file), range.start(), range.len())
        }
        BodySource::Empty => {
            return http_body_util::Full::new(Bytes::new())
                .map_err(|never| match never {})
                .boxed_unsync();
        }
        BodySource::Bytes(bytes) => {
            return http_body_util::Full::new(Bytes::from(bytes))
                .map_err(|never| match never {})
                .boxed_unsync();
        }
    };

    let stream = stream::unfold(
        (file, start, remaining, start > 0, permit),
        move |(mut file, offset, remaining, needs_seek, permit)| async move {
            if remaining == 0 {
                return None;
            }
            if needs_seek {
                if let Err(error) = file.seek(std::io::SeekFrom::Start(offset)).await {
                    return Some((Err(error), (file, offset, 0, false, permit)));
                }
            }
            let chunk_len = remaining.min(stream_chunk_size as u64) as usize;
            let mut buffer = vec![0; chunk_len];
            match read_file_chunk(&mut file, &mut buffer).await {
                Ok(0) => Some((
                    Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "file ended before the advertised response length",
                    )),
                    (file, offset, 0, false, permit),
                )),
                Ok(bytes_read) => {
                    if bytes_read < chunk_len {
                        return Some((
                            Err(std::io::Error::new(
                                std::io::ErrorKind::UnexpectedEof,
                                "file ended before the advertised response length",
                            )),
                            (file, offset, 0, false, permit),
                        ));
                    }
                    let next_remaining = remaining - bytes_read as u64;
                    buffer.truncate(bytes_read);
                    Some((
                        Ok(Frame::data(Bytes::from(buffer))),
                        (
                            file,
                            offset + bytes_read as u64,
                            next_remaining,
                            false,
                            permit,
                        ),
                    ))
                }
                Err(error) => Some((Err(error), (file, offset, 0, false, permit))),
            }
        },
    );
    StreamBody::new(stream).boxed_unsync()
}

async fn read_file_chunk(file: &mut tokio::fs::File, buffer: &mut [u8]) -> std::io::Result<usize> {
    use tokio::io::AsyncReadExt;

    // `tokio::io::AsyncReadExt::read` retries `Interrupted` internally, so no
    // explicit handling is needed here. `Ok(0)` is treated as EOF per the
    // `AsyncRead` contract and bubbled as `UnexpectedEof` by the caller.
    let mut bytes_read = 0;
    while bytes_read < buffer.len() {
        let count = file.read(&mut buffer[bytes_read..]).await?;
        if count == 0 {
            break;
        }
        bytes_read += count;
    }
    Ok(bytes_read)
}

/// Convert a transport-independent [`ResponseStream`] into a Hyper body.
///
/// Contract:
/// - pull/backpressure driven: polls the producer only when downstream is
///   ready; no unbounded channel;
/// - empty chunks skipped (never emit empty DATA frames);
/// - chunks larger than `stream_chunk_size` split zero-copy via `Bytes`
///   (not rejected), keeping downstream framing bounded;
/// - known-length overrun/underrun and producer failure close the connection
///   after commitment (Hyper closes on body error); no second HTTP error is
///   attempted and no producer detail reaches the client;
/// - panics while polling are contained at this task boundary, counted
///   separately, and close deterministically with a sanitized event;
/// - cancellation (client disconnect/shutdown) drops the producer promptly
///   and is counted via `Drop` when the stream never completed.
fn stream_body(
    stream: ResponseStream,
    stream_chunk_size: usize,
    ops: Option<&crate::ops::OpsContext>,
) -> http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, std::io::Error> {
    use http_body_util::{BodyExt, StreamBody};
    let owner = resolve_ops(ops);
    owner
        .counters()
        .streaming_started
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    owner.emit(crate::ops::Event::new(
        crate::ops::Severity::Debug,
        crate::ops::EventKind::ResponseStreamStarted,
        "streaming response started",
    ));
    let adapter = ResponseStreamAdapter::new(stream, stream_chunk_size, ops);
    StreamBody::new(adapter).boxed_unsync()
}

/// Resolve a runtime observability context.
///
/// Falls back to the process-global default for standalone conversions.
fn resolve_ops(ops: Option<&crate::ops::OpsContext>) -> &crate::ops::OpsContext {
    ops.unwrap_or_else(|| crate::ops::OpsContext::global())
}

#[allow(clippy::type_complexity)]
struct ResponseStreamAdapter {
    inner: Option<
        StdPin<
            Box<dyn futures_util::Stream<Item = Result<bytes::Bytes, ResponseStreamError>> + Send>,
        >,
    >,
    trailers: Option<
        StdPin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            Option<crate::primitives::trailers::Trailers>,
                            ResponseStreamError,
                        >,
                    > + Send,
            >,
        >,
    >,
    declared: Option<u64>,
    emitted: u64,
    chunk_size: usize,
    pending_split: Option<bytes::Bytes>,
    finished: bool,
    ops: Option<crate::ops::OpsContext>,
}

use std::pin::Pin as StdPin;
use std::task::{Context as TaskContext, Poll as TaskPoll};

impl ResponseStreamAdapter {
    fn new(
        stream: ResponseStream,
        chunk_size: usize,
        ops: Option<&crate::ops::OpsContext>,
    ) -> Self {
        let declared = stream.known_length();
        let chunk_size = chunk_size.max(1);
        let (inner, trailers) = stream.into_parts();
        Self {
            inner: Some(inner),
            trailers,
            declared,
            emitted: 0,
            chunk_size,
            pending_split: None,
            finished: false,
            ops: ops.cloned(),
        }
    }

    fn owner(&self) -> &crate::ops::OpsContext {
        resolve_ops(self.ops.as_ref())
    }

    fn fail_length_mismatch(&mut self, emitted: u64) -> std::io::Error {
        self.finished = true;
        self.owner()
            .counters()
            .stream_length_mismatches
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.owner().emit(
            crate::ops::Event::new(
                crate::ops::Severity::Warn,
                crate::ops::EventKind::ResponseStreamLengthMismatch,
                "streaming response length mismatch; closing connection",
            )
            .field(crate::ops::Field::U64(
                "declared_bytes".into(),
                self.declared.unwrap_or(0),
            ))
            .field(crate::ops::Field::U64("emitted_bytes".into(), emitted)),
        );
        std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "response stream length mismatch",
        )
    }

    fn fail_producer(&mut self) -> std::io::Error {
        self.finished = true;
        self.owner()
            .counters()
            .stream_producer_errors
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.owner().emit(crate::ops::Event::new(
            crate::ops::Severity::Warn,
            crate::ops::EventKind::ResponseStreamProducerError,
            "streaming response producer failed; closing connection",
        ));
        std::io::Error::other("response stream failed")
    }

    fn fail_panic(&mut self) -> std::io::Error {
        self.finished = true;
        self.owner()
            .counters()
            .stream_producer_panics
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.owner().emit(crate::ops::Event::new(
            crate::ops::Severity::Error,
            crate::ops::EventKind::ResponseStreamProducerPanic,
            "streaming response producer panicked; closing connection",
        ));
        std::io::Error::other("response stream failed")
    }

    fn complete_ok(&mut self) {
        self.finished = true;
        self.owner()
            .counters()
            .streaming_completed
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.owner().emit(crate::ops::Event::new(
            crate::ops::Severity::Debug,
            crate::ops::EventKind::ResponseStreamCompleted,
            "streaming response completed",
        ));
    }

    fn next_split_piece(&mut self) -> Option<bytes::Bytes> {
        let mut pending = self.pending_split.take()?;
        if pending.len() <= self.chunk_size {
            return Some(pending);
        }
        // `Bytes::split_off(at)`: self keeps [..at], returned is [at..].
        let remainder = pending.split_off(self.chunk_size);
        self.pending_split = Some(remainder);
        Some(pending)
    }
}

impl Drop for ResponseStreamAdapter {
    fn drop(&mut self) {
        if !self.finished {
            let owner = resolve_ops(self.ops.as_ref());
            owner
                .counters()
                .stream_cancelled
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            owner.emit(crate::ops::Event::new(
                crate::ops::Severity::Debug,
                crate::ops::EventKind::ResponseStreamCancelled,
                "streaming response cancelled",
            ));
        }
    }
}

impl futures_util::Stream for ResponseStreamAdapter {
    type Item = Result<hyper::body::Frame<bytes::Bytes>, std::io::Error>;

    fn poll_next(
        mut self: StdPin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> TaskPoll<Option<Self::Item>> {
        // Emit any pending split remainder first (no producer poll → no busy loop).
        if self.pending_split.is_some() {
            if let Some(piece) = self.next_split_piece() {
                let len = piece.len() as u64;
                let emitted = self.emitted.saturating_add(len);
                if let Some(declared) = self.declared {
                    if emitted > declared {
                        let err = self.fail_length_mismatch(emitted);
                        return TaskPoll::Ready(Some(Err(err)));
                    }
                }
                self.emitted = emitted;
                return TaskPoll::Ready(Some(Ok(hyper::body::Frame::data(piece))));
            }
        }
        if self.finished {
            return TaskPoll::Ready(None);
        }
        // Body phase: poll byte stream while present.
        if self.inner.is_some() {
            // Borrow dance: take inner temporarily to allow trailer handling
            // after EOF without holding the borrow across `self` mutation.
            let polled = {
                let inner = self.inner.as_mut().expect("checked is_some");
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    inner.as_mut().poll_next(cx)
                }))
            };
            let next = match polled {
                Ok(n) => n,
                Err(_) => {
                    let err = self.fail_panic();
                    return TaskPoll::Ready(Some(Err(err)));
                }
            };
            match next {
                TaskPoll::Pending => return TaskPoll::Pending,
                TaskPoll::Ready(None) => {
                    // End of producer stream: validate known length, then
                    // transition to trailer phase (no data after trailers by
                    // construction: body already ended).
                    if let Some(declared) = self.declared {
                        if self.emitted != declared {
                            let emitted = self.emitted;
                            let err = self.fail_length_mismatch(emitted);
                            return TaskPoll::Ready(Some(Err(err)));
                        }
                    }
                    self.inner = None;
                    // Fall through to trailer handling below.
                }
                TaskPoll::Ready(Some(Ok(chunk))) => {
                    if chunk.is_empty() {
                        cx.waker().wake_by_ref();
                        return TaskPoll::Pending;
                    }
                    let mut chunk = chunk;
                    if chunk.len() > self.chunk_size {
                        let remainder = chunk.split_off(self.chunk_size);
                        self.pending_split = Some(remainder);
                    }
                    let len = chunk.len() as u64;
                    let emitted = self.emitted.saturating_add(len);
                    if let Some(declared) = self.declared {
                        if emitted > declared {
                            self.pending_split = None;
                            let err = self.fail_length_mismatch(emitted);
                            return TaskPoll::Ready(Some(Err(err)));
                        }
                    }
                    self.emitted = emitted;
                    return TaskPoll::Ready(Some(Ok(hyper::body::Frame::data(chunk))));
                }
                TaskPoll::Ready(Some(Err(_detail))) => {
                    let err = self.fail_producer();
                    return TaskPoll::Ready(Some(Err(err)));
                }
            }
        }
        // Trailer phase: body ended, poll the single terminal future once.
        let Some(trailer_fut) = self.trailers.as_mut() else {
            self.complete_ok();
            return TaskPoll::Ready(None);
        };
        let polled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            trailer_fut.as_mut().poll(cx)
        }));
        let next = match polled {
            Ok(n) => n,
            Err(_) => {
                let err = self.fail_panic();
                return TaskPoll::Ready(Some(Err(err)));
            }
        };
        match next {
            TaskPoll::Pending => TaskPoll::Pending,
            TaskPoll::Ready(Ok(None)) => {
                // No trailers: terminal, no extra frame.
                self.trailers = None;
                self.complete_ok();
                TaskPoll::Ready(None)
            }
            TaskPoll::Ready(Ok(Some(trailers))) => {
                // Exactly one terminal block; no data may follow by
                // construction (body already ended, future polled once).
                match trailers_to_header_map(&trailers) {
                    Ok(map) => {
                        self.trailers = None;
                        // Emit one trailer frame; next poll completes.
                        // Mark `finished` only after the terminal EOF so
                        // `Drop` accounting distinguishes cancel vs complete.
                        // Store a sentinel: clear trailers, keep finished
                        // false until next poll returns None.
                        // Use pending_split as terminal marker? Instead track
                        // via inner=None + trailers=None + emitted trailer
                        // pending: set a flag by marking trailers None and
                        // returning the frame; next poll will complete_ok.
                        // To avoid a second trailer poll, we already cleared.
                        // Remember that we emitted trailers so completion is
                        // still counted once.
                        self.pending_split = None;
                        // Temporarily store completion intent: set emitted
                        // trailer flag via `finished` dance — emit frame now,
                        // complete on next poll.
                        // We use a two-step: return trailer frame now, and on
                        // next poll (inner None, trailers None) complete_ok.
                        // To distinguish "trailer emitted, awaiting EOF" from
                        // "no trailers", set `finished` to false but record
                        // via a private marker: reuse `declared`? No — use a
                        // dedicated bool via `pending_split`? Simplest: set a
                        // flag in `emitted`? Instead, push a one-shot state by
                        // setting `trailers` to None and returning frame; next
                        // poll sees both None and completes. No extra flag
                        // needed because completion happens on next poll
                        // uniformly. The only difference is we must not
                        // complete_ok twice: completion happens once on the
                        // following None poll, not here.
                        TaskPoll::Ready(Some(Ok(hyper::body::Frame::trailers(map))))
                    }
                    Err(_) => {
                        let err = self.fail_producer();
                        TaskPoll::Ready(Some(Err(err)))
                    }
                }
            }
            TaskPoll::Ready(Err(_detail)) => {
                // Trailer producer failure after commitment: truncated close,
                // no second HTTP error, sanitized diagnostics only.
                let err = self.fail_producer();
                TaskPoll::Ready(Some(Err(err)))
            }
        }
    }
}

/// Convert validated canonical trailers to a Hyper trailer map.
///
/// Validation already ran at [`Trailers`] construction via the single
/// canonical validator; this conversion never re-implements policy. Failures
/// here are transport-conversion bugs (should be unreachable) and surface as
/// producer errors that close the connection.
fn trailers_to_header_map(
    trailers: &crate::primitives::trailers::Trailers,
) -> Result<hyper::HeaderMap, ()> {
    let mut map = hyper::HeaderMap::new();
    for field in trailers.iter() {
        let name = hyper::header::HeaderName::from_bytes(field.name.as_str().as_bytes())
            .map_err(|_| ())?;
        let value =
            hyper::header::HeaderValue::from_bytes(field.value.as_bytes()).map_err(|_| ())?;
        map.append(name, value);
    }
    Ok(map)
}

struct CountingFileStreamPermit {
    _permit: tokio::sync::OwnedSemaphorePermit,
    ops: Option<crate::ops::OpsContext>,
}

impl CountingFileStreamPermit {
    fn new(
        permit: tokio::sync::OwnedSemaphorePermit,
        ops: Option<&crate::ops::OpsContext>,
    ) -> Self {
        resolve_ops(ops)
            .counters()
            .active_file_streams
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            _permit: permit,
            ops: ops.cloned(),
        }
    }
}

impl Drop for CountingFileStreamPermit {
    fn drop(&mut self) {
        resolve_ops(self.ops.as_ref())
            .counters()
            .active_file_streams
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}
