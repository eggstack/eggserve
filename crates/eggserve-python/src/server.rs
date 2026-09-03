use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyIterator};
use tokio::sync::mpsc;
use tokio::sync::Semaphore;

use bytes::Bytes;
use eggserve_core::policy;
use eggserve_core::primitives::body::BodySource;
use eggserve_core::primitives::canonical::{
    normalize_response, NormalizeRequest, Response as CanonicalResponse, ResponseBody,
    ResponseStream, ResponseStreamError, StatusCode as CanonicalStatusCode,
};
use eggserve_core::primitives::header_block::{HeaderName, HeaderValue};
use eggserve_core::primitives::http::ReadOnlyMethod;
use eggserve_core::primitives::request_body::RequestBody;
use eggserve_core::primitives::request_body_error::RequestBodyError as RustBodyError;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::primitives::{
    resolve_and_plan, ConfinedPath, PathDotfilePolicy, PathPolicy, PathRejection,
    ResolveAndPlanError, SecureRoot, StaticPolicy,
};
use eggserve_core::server::config::RuntimeConfig;
use eggserve_core::server::errors::ShutdownResult;
use eggserve_core::server::lifecycle::LifecycleState;
use eggserve_core::server::service::{Service, ServiceError};
use eggserve_core::server::{Server, ServerHandle};

/// Maximum time to wait for the server to reach Running state during startup.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// Wait for a [`ServerHandle`] to reach `LifecycleState::Running`.
///
/// Returns `Ok(())` only after observing the `Running` state. All other
/// outcomes — timeout, `Failed`, or any non-running terminal state —
/// produce an error with a descriptive message.
///
/// The `timeout` parameter makes this testable without sleeping for the
/// production 30-second `STARTUP_TIMEOUT`.
async fn wait_until_running(
    handle: &ServerHandle,
    timeout: Duration,
) -> Result<(), PyErr> {
    // Fast path: already running.
    if handle.state() == LifecycleState::Running {
        return Ok(());
    }

    // Wait for the readiness signal with a deadline.
    let timed_out = tokio::time::timeout(timeout, handle.ready())
        .await
        .is_err();

    // Re-read the authoritative state regardless of timeout vs. signal.
    let state = handle.state();
    if state == LifecycleState::Running {
        Ok(())
    } else if state == LifecycleState::Failed {
        Err(pyo3::exceptions::PyRuntimeError::new_err(
            "server failed during startup",
        ))
    } else if timed_out {
        Err(crate::LifecycleError::new_err(format!(
            "startup readiness timeout: server is {state} after {}s",
            timeout.as_secs()
        )))
    } else {
        Err(crate::LifecycleError::new_err(format!(
            "server not running: unexpected state {state}"
        )))
    }
}

// ---------------------------------------------------------------------------
#[pyclass(frozen, name = "ServerRequestError")]
#[derive(Debug)]
pub enum ServerRequestError {
    MethodNotAllowed { allowed: String },
    TargetInvalid { reason: String },
    PathRejected { reason: String },
    BodyNotAllowed(),
}

impl std::fmt::Display for ServerRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MethodNotAllowed { allowed } => write!(f, "Method not allowed; use {allowed}"),
            Self::TargetInvalid { reason } => write!(f, "Invalid request target: {reason}"),
            Self::PathRejected { reason } => write!(f, "Path rejected: {reason}"),
            Self::BodyNotAllowed() => write!(f, "Request body not allowed"),
        }
    }
}

impl std::error::Error for ServerRequestError {}

impl ServerRequestError {
    fn into_py_err(self) -> PyErr {
        pyo3::exceptions::PyValueError::new_err(self.to_string())
    }
}

// ---------------------------------------------------------------------------
// Raw body error for channel communication (no Python objects)
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum RawBodyError {
    RejectedByPolicy,
    DeclaredLengthTooLarge {
        declared: u64,
        limit: u64,
    },
    LimitExceeded {
        limit: u64,
        received: u64,
    },
    ReadTimeout,
    PrematureEof {
        received: u64,
        expected: Option<u64>,
    },
    LengthMismatch {
        declared: u64,
        actual: u64,
    },
    InvalidChunkFraming(String),
    Cancelled,
    Disconnected,
    AlreadyConsumed,
    MixedConsumptionMode,
    Transport(String),
}

impl From<RustBodyError> for RawBodyError {
    fn from(err: RustBodyError) -> Self {
        match err {
            RustBodyError::RejectedByPolicy => Self::RejectedByPolicy,
            RustBodyError::DeclaredLengthTooLarge { declared, limit } => {
                Self::DeclaredLengthTooLarge { declared, limit }
            }
            RustBodyError::LimitExceeded { limit, received } => {
                Self::LimitExceeded { limit, received }
            }
            RustBodyError::ReadTimeout => Self::ReadTimeout,
            RustBodyError::PrematureEof { received, expected } => {
                Self::PrematureEof { received, expected }
            }
            RustBodyError::LengthMismatch { declared, actual } => {
                Self::LengthMismatch { declared, actual }
            }
            RustBodyError::InvalidChunkFraming(msg) => Self::InvalidChunkFraming(msg),
            RustBodyError::Cancelled => Self::Cancelled,
            RustBodyError::Disconnected => Self::Disconnected,
            RustBodyError::AlreadyConsumed => Self::AlreadyConsumed,
            RustBodyError::MixedConsumptionMode => Self::MixedConsumptionMode,
            RustBodyError::Transport(msg) => Self::Transport(msg),
        }
    }
}

fn raw_body_error_to_pyerr(err: RawBodyError) -> PyErr {
    match err {
        RawBodyError::RejectedByPolicy => {
            crate::RequestBodyRejectedError::new_err("request body rejected by policy")
        }
        RawBodyError::DeclaredLengthTooLarge { declared, limit } => {
            crate::RequestBodyTooLargeError::new_err(format!(
                "declared content-length {declared} exceeds limit {limit}"
            ))
        }
        RawBodyError::LimitExceeded { limit, received } => {
            crate::RequestBodyTooLargeError::new_err(format!(
                "body exceeded limit: received {received} bytes, limit is {limit}"
            ))
        }
        RawBodyError::ReadTimeout => crate::RequestBodyTimeoutError::new_err("body read timed out"),
        RawBodyError::PrematureEof { received, expected } => {
            let msg = match expected {
                Some(exp) => {
                    format!("premature EOF: received {received} of {exp} expected bytes")
                }
                None => format!("premature EOF after {received} bytes"),
            };
            crate::RequestBodyDisconnectedError::new_err(msg)
        }
        RawBodyError::Disconnected => {
            crate::RequestBodyDisconnectedError::new_err("client disconnected")
        }
        RawBodyError::AlreadyConsumed => {
            crate::RequestBodyConsumedError::new_err("body already consumed")
        }
        RawBodyError::MixedConsumptionMode => crate::RequestBodyConsumedError::new_err(
            "mixed consumption mode: cannot switch between read_all and streaming",
        ),
        RawBodyError::Cancelled => {
            crate::RequestBodyCancelledError::new_err("body consumption cancelled")
        }
        RawBodyError::LengthMismatch { declared, actual } => crate::RequestBodyError::new_err(
            format!("body length mismatch: declared {declared}, actual {actual}"),
        ),
        RawBodyError::InvalidChunkFraming(msg) => {
            crate::RequestBodyError::new_err(format!("invalid chunk framing: {msg}"))
        }
        RawBodyError::Transport(msg) => {
            crate::RequestBodyDisconnectedError::new_err(format!("transport error: {msg}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Bounded Python iterator -> Rust ResponseStream bridge (Plan 166)
// ---------------------------------------------------------------------------

/// Bound for the Python producer -> Rust transport channel.
///
/// The producer thread blocks on `blocking_send` when full, so client
/// backpressure eventually stops iterator advancement. No whole-body
/// buffering occurs: at most this many chunks are in flight.
const PYTHON_STREAM_CHANNEL_BOUND: usize = 16;

/// `Stream` adapter over the bounded producer channel.
///
/// Wraps the receiver in a `Mutex` so the adapter is `Sync` as required by
/// `ResponseStream::new` (tokio's `Receiver` is `Send` but single-consumer
/// `!Sync`). Polls are sequential from the transport, so the lock is
/// uncontended.
struct PythonReceiverStream {
    rx: std::sync::Mutex<mpsc::Receiver<Result<Bytes, ResponseStreamError>>>,
}

impl futures_util::Stream for PythonReceiverStream {
    type Item = Result<Bytes, ResponseStreamError>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.rx.lock() {
            Ok(mut guard) => guard.poll_recv(cx),
            Err(_) => std::task::Poll::Ready(Some(Err(ResponseStreamError::new(
                "response stream lock failed",
            )))),
        }
    }
}

/// Drive a Python iterable of bytes-like chunks into the bounded channel.
///
/// Runs on a dedicated `std` thread (never a Tokio worker): iterator `next()`
/// calls acquire the GIL per item, chunk bytes are copied to Rust while the
/// GIL is held, then the GIL is released while blocking on channel capacity
/// so slow clients apply backpressure without stalling the interpreter.
/// Dropping the stream (HEAD suppression, disconnect, shutdown) drops the
/// receiver; the next send fails and this thread exits, releasing all
/// `PyObject` references promptly.
///
/// Non-bytes items and iterator exceptions become stream errors: the wire
/// sees a truncated/closed connection and diagnostics carry only the
/// sanitized exception type name, never request/response content.
fn spawn_python_stream_producer(
    iterable: Py<PyAny>,
    sender: mpsc::Sender<Result<Bytes, ResponseStreamError>>,
) {
    std::thread::spawn(move || {
        let iterator = Python::with_gil(|py| {
            let bound = iterable.bind(py);
            PyIterator::from_object(&bound)
                .map(|it| it.into_any().unbind())
                .map_err(|e| {
                    let type_name = e
                        .get_type(py)
                        .name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| "<unknown>".to_string());
                    (type_name, e.to_string())
                })
        });
        let iterator_obj: Py<PyAny> = match iterator {
            Ok(obj) => obj,
            Err((type_name, _)) => {
                eggserve_core::ops::Logger::global().emit(eggserve_core::ops::Event::new(
                    eggserve_core::ops::Severity::Error,
                    eggserve_core::ops::EventKind::ServiceError,
                    format!("Python response iterator is not iterable ({type_name})"),
                ));
                let _ = sender.blocking_send(Err(ResponseStreamError::new(
                    "python response iterator is not iterable",
                )));
                return;
            }
        };
        loop {
            // Pull one item under the GIL and copy bytes to Rust.
            enum Pulled {
                Chunk(Vec<u8>),
                Empty,
                Finished,
                ItemError(String),
                NonBytes,
            }
            let pulled = Python::with_gil(|py| {
                let bound = iterator_obj.bind(py);
                // `iterator_obj` is the single iterator created at thread
                // start; advancing it via `__next__` preserves one-shot
                // generator semantics (re-calling `iter()` on a list each
                // lap would restart from the beginning).
                match bound.call_method0("__next__") {
                    Ok(item) => {
                        if let Ok(data) = item.extract::<Vec<u8>>() {
                            if data.is_empty() {
                                Pulled::Empty
                            } else {
                                Pulled::Chunk(data)
                            }
                        } else {
                            Pulled::NonBytes
                        }
                    }
                    Err(e) => {
                        if e.is_instance_of::<pyo3::exceptions::PyStopIteration>(py) {
                            Pulled::Finished
                        } else {
                            let type_name = e
                                .get_type(py)
                                .name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|_| "<unknown>".to_string());
                            Pulled::ItemError(type_name)
                        }
                    }
                }
            });
            match pulled {
                Pulled::Finished => break,
                Pulled::Empty => continue,
                Pulled::Chunk(data) => {
                    let bytes = Bytes::from(data);
                    if sender.blocking_send(Ok(bytes)).is_err() {
                        break;
                    }
                }
                Pulled::NonBytes => {
                    eggserve_core::ops::Logger::global().emit(eggserve_core::ops::Event::new(
                        eggserve_core::ops::Severity::Error,
                        eggserve_core::ops::EventKind::ServiceError,
                        "Python response iterator yielded non-bytes chunk",
                    ));
                    let _ = sender.blocking_send(Err(ResponseStreamError::new(
                        "python response iterator yielded non-bytes",
                    )));
                    break;
                }
                Pulled::ItemError(type_name) => {
                    eggserve_core::ops::Logger::global().emit(eggserve_core::ops::Event::new(
                        eggserve_core::ops::Severity::Error,
                        eggserve_core::ops::EventKind::ServiceError,
                        format!("Python response iterator failed ({type_name})"),
                    ));
                    let _ = sender.blocking_send(Err(ResponseStreamError::new(
                        "python response iterator failed",
                    )));
                    break;
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Python RequestBody — wraps Rust RequestBody
// ---------------------------------------------------------------------------

#[pyclass(frozen, name = "RequestBody")]
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PyRequestBody {
    inner: Arc<std::sync::Mutex<Option<RequestBody>>>,
    handle: tokio::runtime::Handle,
    declared_length: Option<u64>,
    final_bytes_received: Arc<AtomicU64>,
    final_complete: Arc<AtomicBool>,
}

#[pymethods]
impl PyRequestBody {
    #[getter]
    fn declared_length(&self) -> Option<u64> {
        self.declared_length
    }

    #[getter]
    fn bytes_received(&self) -> u64 {
        if let Ok(guard) = self.inner.lock() {
            if let Some(body) = guard.as_ref() {
                return body.bytes_received();
            }
        }
        self.final_bytes_received.load(Ordering::Acquire)
    }

    #[getter]
    fn complete(&self) -> bool {
        if let Ok(guard) = self.inner.lock() {
            if let Some(body) = guard.as_ref() {
                return body.is_complete();
            }
        }
        self.final_complete.load(Ordering::Acquire)
    }

    fn read<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let body = {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            guard
                .take()
                .ok_or_else(|| crate::RequestBodyConsumedError::new_err("body already consumed"))?
        };

        let handle = self.handle.clone();
        let data = py.allow_threads(|| {
            handle.block_on(async {
                let mut body = body;
                let mut data = Vec::new();
                loop {
                    match body.next_chunk().await {
                        Ok(Some(chunk)) => data.extend_from_slice(&chunk),
                        Ok(None) => return Ok((data, body.bytes_received())),
                        Err(error) => return Err((error, body.bytes_received())),
                    }
                }
            })
        });

        match data {
            Ok((bytes, received)) => {
                self.final_bytes_received.store(received, Ordering::Release);
                self.final_complete.store(true, Ordering::Release);
                Ok(PyBytes::new(py, &bytes))
            }
            Err((e, received)) => {
                self.final_bytes_received.store(received, Ordering::Release);
                let raw: RawBodyError = e.into();
                Err(raw_body_error_to_pyerr(raw))
            }
        }
    }

    #[pyo3(signature = (chunk_size=None))]
    fn iter_chunks(
        &self,
        _py: Python<'_>,
        chunk_size: Option<usize>,
    ) -> PyResult<PyBodyChunkIterator> {
        if let Some(size) = chunk_size {
            if size == 0 {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "chunk_size must be greater than zero",
                ));
            }
        }
        let body = {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            guard
                .take()
                .ok_or_else(|| crate::RequestBodyConsumedError::new_err("body already consumed"))?
        };

        let (sender, receiver) = mpsc::channel::<Result<Vec<u8>, RawBodyError>>(16);
        let handle = self.handle.clone();
        let final_bytes = Arc::clone(&self.final_bytes_received);
        let final_complete = Arc::clone(&self.final_complete);

        // Dropping the iterator before EOF leaves `complete` False for both
        // exit paths (consumer abandonment and transport error): the body is
        // genuinely incomplete in both cases, and observers cannot rely on
        // completion unless iteration ran to exhaustion or `read()` finished.
        handle.spawn(async move {
            let mut body = body;
            // When a chunk size is requested, buffer native chunks and emit
            // exactly-sized chunks; the final partial chunk is flushed at EOF.
            let mut pending: Vec<u8> = Vec::new();
            'producer: loop {
                // Race each read against receiver-drop: when the Python
                // consumer abandons iteration, stop reading immediately
                // instead of lingering on `next_chunk()` (which can park on a
                // slow client upload) until the next send would fail.
                let chunk = tokio::select! {
                    biased;
                    _ = sender.closed() => break 'producer,
                    chunk = body.next_chunk() => chunk,
                };
                match chunk {
                    Ok(Some(chunk)) => {
                        final_bytes.store(body.bytes_received(), Ordering::Release);
                        match chunk_size {
                            None => {
                                let data = chunk.to_vec();
                                if sender.send(Ok(data)).await.is_err() {
                                    break 'producer;
                                }
                            }
                            Some(size) => {
                                pending.extend_from_slice(&chunk);
                                while pending.len() >= size {
                                    let rest = pending.split_off(size);
                                    if sender.send(Ok(pending)).await.is_err() {
                                        break 'producer;
                                    }
                                    pending = rest;
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        if !pending.is_empty() && sender.send(Ok(pending)).await.is_err() {
                            break 'producer;
                        }
                        final_bytes.store(body.bytes_received(), Ordering::Release);
                        final_complete.store(true, Ordering::Release);
                        break 'producer;
                    }
                    Err(e) => {
                        if !pending.is_empty()
                            && sender.send(Ok(std::mem::take(&mut pending))).await.is_err()
                        {
                            break 'producer;
                        }
                        let bytes = body.bytes_received();
                        final_bytes.store(bytes, Ordering::Release);
                        let _ = sender.send(Err(e.into())).await;
                        break 'producer;
                    }
                }
            }
        });

        Ok(PyBodyChunkIterator {
            receiver,
            final_bytes_received: Arc::clone(&self.final_bytes_received),
        })
    }

    fn __repr__(&self) -> String {
        match self.inner.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(body) => format!(
                    "<RequestBody declared_length={:?} bytes_received={}>",
                    body.declared_length(),
                    body.bytes_received()
                ),
                None => "<RequestBody consumed>".to_string(),
            },
            Err(_) => "<RequestBody lock error>".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Python BodyChunkIterator — synchronous iterator over body chunks
// ---------------------------------------------------------------------------

/// PyBodyChunkIterator is not thread-safe; concurrent __next__ calls are not supported.
#[pyclass(name = "BodyChunkIterator")]
#[allow(dead_code)]
pub struct PyBodyChunkIterator {
    receiver: mpsc::Receiver<Result<Vec<u8>, RawBodyError>>,
    final_bytes_received: Arc<AtomicU64>,
}

#[pymethods]
impl PyBodyChunkIterator {
    fn __iter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    fn __next__<'py>(&mut self, py: Python<'py>) -> PyResult<PyObject> {
        let result = py.allow_threads(|| self.receiver.blocking_recv());
        match result {
            Some(Ok(data)) => {
                Ok(PyBytes::new(py, &data).into_any().unbind())
            }
            Some(Err(e)) => Err(raw_body_error_to_pyerr(e)),
            None => Err(pyo3::exceptions::PyStopIteration::new_err(())),
        }
    }

    fn __repr__(&self) -> String {
        "<BodyChunkIterator>".to_string()
    }
}

// ---------------------------------------------------------------------------
// Python Request — request envelope for handler callbacks
// ---------------------------------------------------------------------------

#[pyclass(frozen, name = "Request")]
#[derive(Debug, Clone)]
pub struct PyRequest {
    #[pyo3(get)]
    method: String,
    #[pyo3(get)]
    path: String,
    #[pyo3(get)]
    query: String,
    /// First-wins semantics; for duplicate-sensitive headers use `header_items`.
    #[pyo3(get)]
    headers: HashMap<String, String>,
    header_items: Vec<(String, String)>,
    #[pyo3(get)]
    remote_addr: Option<String>,
    #[pyo3(get)]
    remote_address: Option<(String, u16)>,
    #[pyo3(get)]
    local_addr: Option<String>,
    #[pyo3(get)]
    local_address: Option<(String, u16)>,
    #[pyo3(get)]
    scheme: Option<String>,
    #[pyo3(get)]
    http_version: String,
    #[pyo3(get)]
    body: Option<PyRequestBody>,
}

#[pymethods]
impl PyRequest {
    #[getter]
    fn header_items(&self) -> Vec<(String, String)> {
        self.header_items.clone()
    }

    #[getter]
    fn has_body(&self) -> bool {
        self.body.is_some()
    }

    fn __repr__(&self) -> String {
        format!("<Request {} {}>", self.method, self.path)
    }
}

#[pyclass(frozen, name = "Response")]
pub struct PyResponse {
    #[pyo3(get)]
    status: u16,
    #[pyo3(get)]
    headers: HashMap<String, String>,
    pub(crate) body: std::sync::Mutex<PyResponseBody>,
    pub(crate) extra_headers: Vec<(String, String)>,
}

impl std::fmt::Debug for PyResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PyResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body", &self.body)
            .finish_non_exhaustive()
    }
}

pub(crate) enum PyResponseBody {
    Empty,
    Bytes(Vec<u8>),
    BodySource(BodySource),
    Stream {
        iterable: Py<PyAny>,
        content_length: Option<u64>,
    },
    Consumed,
}

impl std::fmt::Debug for PyResponseBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "Empty"),
            Self::Bytes(b) => f.debug_tuple("Bytes").field(&b.len()).finish(),
            Self::BodySource(s) => f.debug_tuple("BodySource").field(&s.kind()).finish(),
            Self::Stream {
                content_length, ..
            } => f
                .debug_struct("Stream")
                .field("content_length", content_length)
                .finish_non_exhaustive(),
            Self::Consumed => write!(f, "Consumed"),
        }
    }
}

#[pymethods]
impl PyResponse {
    #[staticmethod]
    fn empty(status: u16) -> PyResult<Self> {
        validate_response_status(status)?;
        Ok(Self {
            status,
            headers: HashMap::new(),
            body: std::sync::Mutex::new(PyResponseBody::Empty),
            extra_headers: Vec::new(),
        })
    }

    #[staticmethod]
    #[pyo3(signature = (status, data, headers=None))]
    fn bytes(
        status: u16,
        data: Vec<u8>,
        headers: Option<HashMap<String, String>>,
    ) -> PyResult<Self> {
        validate_response_status(status)?;
        Ok(Self {
            status,
            headers: headers.unwrap_or_default(),
            body: std::sync::Mutex::new(PyResponseBody::Bytes(data)),
            extra_headers: Vec::new(),
        })
    }

    #[staticmethod]
    #[pyo3(signature = (status, text, headers=None))]
    fn text(
        status: u16,
        text: String,
        headers: Option<HashMap<String, String>>,
    ) -> PyResult<Self> {
        validate_response_status(status)?;
        let mut h = headers.unwrap_or_default();
        h.entry("content-type".to_string())
            .or_insert_with(|| "text/plain; charset=utf-8".to_string());
        Ok(Self {
            status,
            headers: h,
            body: std::sync::Mutex::new(PyResponseBody::Bytes(text.into_bytes())),
            extra_headers: Vec::new(),
        })
    }

    #[staticmethod]
    fn body_source(
        status: u16,
        body: &ServerBodySource,
        headers: Option<HashMap<String, String>>,
    ) -> PyResult<Self> {
        validate_response_status(status)?;
        let mut taken = body
            .inner
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        let source = taken.take().ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err("BodySource already consumed")
        })?;
        Ok(Self {
            status,
            headers: headers.unwrap_or_default(),
            body: std::sync::Mutex::new(PyResponseBody::BodySource(source)),
            extra_headers: Vec::new(),
        })
    }

    /// Incrementally produced response body from a synchronous iterable.
    ///
    /// The iterable must yield bytes-like chunks (`bytes`/`bytearray`;
    /// empty chunks are skipped). It is consumed incrementally on a
    /// dedicated producer thread through a bounded (16-chunk) channel, so
    /// client backpressure eventually stops iterator advancement and the
    /// full body is never buffered. `content_length`, when given, is the
    /// exact representation length (Plan 162 known-length validation:
    /// underrun/overrun closes the connection after commitment); when
    /// omitted the runtime uses HTTP/1 chunked framing.
    ///
    /// HEAD and body-forbidden responses never advance the iterator. Raw
    /// `Transfer-Encoding` cannot be set by the service (rejected as a
    /// runtime-owned header). Non-bytes items and iterator exceptions
    /// become stream failures: the wire sees a truncated connection and
    /// diagnostics carry only the sanitized exception type. Async
    /// generators/coroutines are not supported; keep asyncio ownership in
    /// the downstream app server.
    #[staticmethod]
    #[pyo3(signature = (status, iterable, headers=None, content_length=None))]
    fn stream(
        py: Python<'_>,
        status: u16,
        iterable: Py<PyAny>,
        headers: Option<HashMap<String, String>>,
        content_length: Option<u64>,
    ) -> PyResult<Self> {
        validate_response_status(status)?;
        // Fail fast on non-iterables so caller mistakes surface as
        // TypeError at construction rather than truncated streams.
        if PyIterator::from_object(iterable.bind(py)).is_err() {
            // Coroutine/async-generator producers are explicitly unsupported.
            let is_awaitable = iterable
                .bind(py)
                .hasattr("__await__")
                .unwrap_or(false)
                || iterable.bind(py).hasattr("__anext__").unwrap_or(false);
            if is_awaitable {
                return Err(pyo3::exceptions::PyTypeError::new_err(
                    "async response producers are not supported; use a synchronous iterable of bytes",
                ));
            }
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "response iterable must be a synchronous iterable of bytes-like chunks",
            ));
        }
        Ok(Self {
            status,
            headers: headers.unwrap_or_default(),
            body: std::sync::Mutex::new(PyResponseBody::Stream {
                iterable,
                content_length,
            }),
            extra_headers: Vec::new(),
        })
    }

    /// Returns a clone of the body. The first call extracts; subsequent
    /// calls re-clone from internal state. For hot paths, use the
    /// internal conversion directly.
    #[getter]
    fn body(&self) -> PyResult<ServerBodySource> {
        let body = self.body.lock().map_err(|_| {
            pyo3::exceptions::PyRuntimeError::new_err("response body lock poisoned")
        })?;
        let source = match &*body {
            PyResponseBody::Empty => BodySource::Empty,
            PyResponseBody::Bytes(data) => BodySource::Bytes(data.clone()),
            PyResponseBody::Stream { .. } => {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "streamed response body is one-shot and cannot be cloned",
                ))
            }
            PyResponseBody::BodySource(source) => match source {
                BodySource::Empty => BodySource::Empty,
                BodySource::Bytes(data) => BodySource::Bytes(data.clone()),
                BodySource::FileFull { file, len, mime } => match file.try_clone() {
                    Ok(cloned) => BodySource::FileFull {
                        file: cloned,
                        len: *len,
                        mime,
                    },
                    Err(_) => {
                        return Err(pyo3::exceptions::PyIOError::new_err(
                            "response file body could not be cloned",
                        ))
                    }
                },
                BodySource::FileRange {
                    file,
                    range,
                    total_len,
                    mime,
                } => match file.try_clone() {
                    Ok(cloned) => BodySource::FileRange {
                        file: cloned,
                        range: *range,
                        total_len: *total_len,
                        mime,
                    },
                    Err(_) => {
                        return Err(pyo3::exceptions::PyIOError::new_err(
                            "response file body could not be cloned",
                        ))
                    }
                },
            },
            PyResponseBody::Consumed => {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "response body already consumed",
                ))
            }
        };
        Ok(ServerBodySource {
            inner: std::sync::Mutex::new(Some(source)),
        })
    }

    fn __repr__(&self) -> String {
        format!("<Response {}>", self.status)
    }
}

#[pyclass(frozen, name = "StaticResponder")]
#[derive(Debug, Clone)]
pub struct PyStaticResponder {
    root: SecureRoot,
    policy: StaticPolicy,
}

#[pymethods]
impl PyStaticResponder {
    #[new]
    fn new(root: &ServerSecureRoot) -> Self {
        Self {
            root: root.inner.clone(),
            policy: root.policy.clone(),
        }
    }

    #[pyo3(signature = (method, target, headers=None, has_body=false, remote_addr=None, http_version=None, index_pages=None, mime_overrides=None, default_content_type=None, extra_response_headers=None))]
    fn respond(
        &self,
        method: &str,
        target: &str,
        headers: Option<HashMap<String, String>>,
        has_body: bool,
        remote_addr: Option<String>,
        http_version: Option<String>,
        index_pages: Option<Vec<String>>,
        mime_overrides: Option<HashMap<String, String>>,
        default_content_type: Option<String>,
        extra_response_headers: Option<Vec<(String, String)>>,
    ) -> PyResult<PyResponse> {
        let _ = remote_addr;
        let _http_version = http_version.unwrap_or_else(|| "1.1".to_string());
        let ro_method = match method {
            "GET" => ReadOnlyMethod::Get,
            "HEAD" => ReadOnlyMethod::Head,
            _ => {
                return Err(ServerRequestError::MethodNotAllowed {
                    allowed: "GET, HEAD".to_string(),
                }
                .into_py_err())
            }
        };

        if !target.starts_with('/') {
            return Err(ServerRequestError::TargetInvalid {
                reason: "target must start with '/'".to_string(),
            }
            .into_py_err());
        }

        if has_body {
            return Err(ServerRequestError::BodyNotAllowed().into_py_err());
        }

        let path_policy = PathPolicy {
            dotfiles: match self.root.policy().dotfiles {
                policy::DotfilePolicy::Denied => PathPolicy::default().dotfiles,
                policy::DotfilePolicy::Serve => PathDotfilePolicy::Allow,
            },
            reject_backslash: true,
        };
        let (raw_path, query) = target.split_once('?').unwrap_or((target, ""));
        let path = match ConfinedPath::parse(raw_path, &path_policy) {
            Ok(p) => p,
            Err(e) => {
                let is_malformed = matches!(
                    e,
                    PathRejection::MalformedPercentEncoding
                        | PathRejection::InvalidUtf8
                        | PathRejection::NulByte
                        | PathRejection::ControlCharacter
                        | PathRejection::Empty
                        | PathRejection::UnsupportedUriForm
                        | PathRejection::TooLong
                );
                if is_malformed {
                    return Err(ServerRequestError::TargetInvalid {
                        reason: e.to_string(),
                    }
                    .into_py_err());
                }
                return build_error_response(403, "Forbidden");
            }
        };

        let hdrs = headers.unwrap_or_default();
        let if_match = hdrs.get("if-match").map(|s| s.as_str());
        let if_unmodified_since = hdrs.get("if-unmodified-since").map(|s| s.as_str());
        let if_none_match = hdrs.get("if-none-match").map(|s| s.as_str());
        let if_modified_since = hdrs.get("if-modified-since").map(|s| s.as_str());
        let range = hdrs.get("range").map(|s| s.as_str());
        let if_range = hdrs.get("if-range").map(|s| s.as_str());

        let default_content_type = default_content_type
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let extra_response_headers = extra_response_headers.unwrap_or_default();
        validate_extra_response_headers(&default_content_type, &extra_response_headers)?;
        let plan_file = |file: eggserve_core::primitives::ResolvedFile| -> PyResult<PyResponse> {
            let plan = file.plan_response(
                ro_method,
                if_match,
                if_unmodified_since,
                if_none_match,
                if_modified_since,
                range,
                if_range,
            );
            let body = file.into_body(&plan).map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("body error: {e}"))
            })?;
            let mut response = build_response(plan, body)?;
            apply_static_metadata(
                &mut response,
                &default_content_type,
                &extra_response_headers,
            )?;
            Ok(response)
        };

        if let eggserve_core::primitives::ResolvedResource::Directory(dir) =
            self.root.resolve(&path)
        {
            // Keep the low-level StaticResponder contract (directories are
            // not responses) unless the compatibility facade explicitly
            // supplies index metadata.
            if index_pages.is_none() {
                return build_error_response(403, "Forbidden");
            }
            if !raw_path.ends_with('/') {
                let mut location = path.as_str().to_string();
                if !location.ends_with('/') {
                    location.push('/');
                }
                if !query.is_empty() {
                    location.push('?');
                    location.push_str(query);
                }
                let mut response = PyResponse::empty(301)?;
                response.headers.insert("location".to_string(), location);
                return Ok(response);
            }

            for index in index_pages.expect("checked above") {
                match dir.resolve_child(&index, &self.root) {
                    eggserve_core::primitives::ResolvedResource::File(file) => {
                        if let Ok(response) = plan_file(file) {
                            let mut response = response;
                            if let Some(overrides) = &mime_overrides {
                                let suffix = file_suffix(&index);
                                if let Some(mime) = overrides.get(&suffix) {
                                    response.headers.insert("content-type".into(), mime.clone());
                                }
                            }
                            return Ok(response);
                        }
                    }
                    eggserve_core::primitives::ResolvedResource::Denied(_)
                    | eggserve_core::primitives::ResolvedResource::NotFound
                    | eggserve_core::primitives::ResolvedResource::Directory(_) => continue,
                    eggserve_core::primitives::ResolvedResource::IoError(error) => {
                        return Err(pyo3::exceptions::PyRuntimeError::new_err(format!(
                            "filesystem resolution failed: {error}"
                        )))
                    }
                }
            }

            if matches!(
                self.policy.directory_listing,
                policy::DirectoryListingPolicy::Enabled
            ) {
                let entries = dir
                    .list(&self.root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES)
                    .map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!(
                        "directory listing failed: {e}"
                    ))
                })?;
                let body = directory_listing_bytes(&entries);
                let body_len = body.len();
                let mut response = PyResponse::bytes(200, body, None)?;
                response
                    .headers
                    .insert("content-type".into(), "text/html; charset=utf-8".into());
                response
                    .headers
                    .insert("x-content-type-options".into(), "nosniff".into());
                response.headers.insert(
                    "content-security-policy".into(),
                    "default-src 'none'; base-uri 'none'; form-action 'none'".into(),
                );
                response
                    .headers
                    .insert("referrer-policy".into(), "no-referrer".into());
                if ro_method == ReadOnlyMethod::Head {
                    response
                        .headers
                        .insert("content-length".into(), body_len.to_string());
                    *response.body.lock().map_err(|_| {
                        pyo3::exceptions::PyRuntimeError::new_err("lock poisoned")
                    })? = PyResponseBody::Empty;
                }
                apply_static_metadata(
                    &mut response,
                    &default_content_type,
                    &extra_response_headers,
                )?;
                return Ok(response);
            }
            return build_error_response(403, "Forbidden");
        }

        match resolve_and_plan(
            &self.root,
            &path,
            ro_method,
            if_match,
            if_unmodified_since,
            if_none_match,
            if_modified_since,
            range,
            if_range,
        ) {
            Ok((plan, body_source)) => {
                let mut response = build_response(plan, body_source)?;
                if let Some(overrides) = &mime_overrides {
                    if let Some(mime) = overrides.get(&file_suffix(raw_path)) {
                        response.headers.insert("content-type".into(), mime.clone());
                    }
                }
                apply_static_metadata(
                    &mut response,
                    &default_content_type,
                    &extra_response_headers,
                )?;
                Ok(response)
            }
            Err(ResolveAndPlanError::NotFound) => build_error_response(404, "Not Found"),
            Err(ResolveAndPlanError::IsDirectory) => build_error_response(403, "Forbidden"),
            Err(ResolveAndPlanError::Denied(_)) => build_error_response(403, "Forbidden"),
            Err(ResolveAndPlanError::Io(e)) => Err(pyo3::exceptions::PyRuntimeError::new_err(
                format!("filesystem resolution failed: {e}"),
            )),
            Err(ResolveAndPlanError::Body(e)) => Err(pyo3::exceptions::PyRuntimeError::new_err(
                format!("body error: {e}"),
            )),
        }
    }
}

fn file_suffix(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .rsplit_once('.')
        .map(|(_, suffix)| format!(".{suffix}"))
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn build_response(
    plan: eggserve_core::primitives::response::StaticResponsePlan,
    body_source: BodySource,
) -> PyResult<PyResponse> {
    let mut headers = HashMap::new();
    for header in plan.headers.iter() {
        headers.insert(header.name.clone(), header.value.clone());
    }

    Ok(PyResponse {
        status: plan.status.as_u16(),
        headers,
        body: std::sync::Mutex::new(PyResponseBody::BodySource(body_source)),
        extra_headers: Vec::new(),
    })
}

fn validate_extra_response_headers(
    default_content_type: &str,
    headers: &[(String, String)],
) -> PyResult<()> {
    eggserve_core::config::validate_static_metadata(
        default_content_type,
        headers,
    )
    .map_err(pyo3::exceptions::PyValueError::new_err)
}

fn apply_static_metadata(
    response: &mut PyResponse,
    default_content_type: &str,
    extra_headers: &[(String, String)],
) -> PyResult<()> {
    if response.status != 200 {
        return Ok(());
    }
    if response
        .headers
        .get("content-type")
        .is_some_and(|value| value == "application/octet-stream")
    {
        response
            .headers
            .insert("content-type".to_string(), default_content_type.to_string());
    }
    for (name, value) in extra_headers {
        if !response
            .headers
            .keys()
            .any(|existing| existing.eq_ignore_ascii_case(name))
        {
            // Canonicalize via `HeaderValue` (trims SP/HTAB OWS) so validation
            // and wire value agree — mirrors `static_service::append_extra_headers`.
            let canonical = eggserve_core::primitives::header_block::HeaderValue::new(
                value.clone(),
            )
            .map(|v| v.as_str().to_owned())
            .unwrap_or_else(|_| value.clone());
            response.extra_headers.push((name.clone(), canonical));
        }
    }
    Ok(())
}

fn validate_response_status(status: u16) -> PyResult<()> {
    if (100..600).contains(&status) {
        Ok(())
    } else {
        Err(pyo3::exceptions::PyValueError::new_err(format!(
            "status code {status} is outside 100-599"
        )))
    }
}

fn build_error_response(status: u16, reason: &str) -> PyResult<PyResponse> {
    let mut headers = HashMap::new();
    headers.insert(
        "content-type".to_string(),
        "text/plain; charset=utf-8".to_string(),
    );
    Ok(PyResponse {
        status,
        headers,
        body: std::sync::Mutex::new(PyResponseBody::Bytes(reason.as_bytes().to_vec())),
        extra_headers: Vec::new(),
    })
}

fn directory_listing_bytes(entries: &[(String, bool)]) -> Vec<u8> {
    fn escape(value: &str) -> String {
        use std::fmt::Write;

        let mut out = String::with_capacity(value.len());
        for c in value.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&#x27;"),
                c if !c.is_control() => out.push(c),
                c => write!(&mut out, "&#x{:X};", c as u32)
                    .expect("writing to String cannot fail"),
            }
        }
        out
    }
    fn segment(value: &str) -> String {
        value.bytes().fold(String::new(), |mut out, byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                out.push(byte as char);
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
            out
        })
    }

    let mut html = String::from(
        "<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\"><title>Directory listing</title></head>\n<body><h1>Directory listing</h1><ul>\n",
    );
    for (name, is_dir) in entries {
        let visible = escape(name);
        let href = escape(&segment(name));
        if *is_dir {
            html.push_str(&format!("<li><a href=\"{href}/\">{visible}/</a></li>\n"));
        } else {
            html.push_str(&format!("<li><a href=\"{href}\">{visible}</a></li>\n"));
        }
    }
    html.push_str("</ul>\n</body>\n</html>\n");
    html.into_bytes()
}

#[pyclass(frozen, name = "StaticPolicyWrapper")]
#[derive(Debug, Clone)]
pub struct PyStaticPolicyWrapper {
    inner: StaticPolicy,
}

#[pymethods]
impl PyStaticPolicyWrapper {
    #[new]
    #[pyo3(signature = (directory_listing=false, follow_symlinks=false, allow_dotfiles=false))]
    fn new(directory_listing: bool, follow_symlinks: bool, allow_dotfiles: bool) -> Self {
        let mut policy = StaticPolicy::safe_default();
        if directory_listing {
            policy.directory_listing = policy::DirectoryListingPolicy::Enabled;
        }
        if follow_symlinks {
            policy.symlinks = policy::SymlinkPolicy::Follow;
        }
        if allow_dotfiles {
            policy.dotfiles = policy::DotfilePolicy::Serve;
        }
        Self { inner: policy }
    }

    #[getter]
    fn directory_listing(&self) -> bool {
        matches!(
            self.inner.directory_listing,
            policy::DirectoryListingPolicy::Enabled
        )
    }

    #[getter]
    fn follow_symlinks(&self) -> bool {
        matches!(self.inner.symlinks, policy::SymlinkPolicy::Follow)
    }

    #[getter]
    fn allow_dotfiles(&self) -> bool {
        matches!(self.inner.dotfiles, policy::DotfilePolicy::Serve)
    }
}

#[pyclass(frozen, name = "ServerSecureRoot")]
#[derive(Debug, Clone)]
pub struct ServerSecureRoot {
    pub(crate) inner: SecureRoot,
    policy: StaticPolicy,
}

#[pymethods]
impl ServerSecureRoot {
    #[new]
    #[pyo3(signature = (path, policy=None))]
    fn new(path: String, policy: Option<PyStaticPolicyWrapper>) -> PyResult<Self> {
        let static_policy = policy
            .map(|p| p.inner)
            .unwrap_or_else(StaticPolicy::safe_default);
        let root = SecureRoot::new(path, static_policy.clone()).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("failed to create secure root: {e}"))
        })?;
        Ok(Self {
            inner: root,
            policy: static_policy,
        })
    }

    #[getter]
    fn root_path(&self) -> String {
        self.inner.root_path().to_string_lossy().to_string()
    }
}

#[pyclass(frozen, name = "ServerBodySource")]
pub struct ServerBodySource {
    pub(crate) inner: std::sync::Mutex<Option<BodySource>>,
}

#[pymethods]
impl ServerBodySource {
    #[pyo3(signature = (status=200))]
    fn to_response(&self, status: u16) -> PyResult<PyResponse> {
        validate_response_status(status)?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        let source = inner.take().ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err("BodySource already consumed")
        })?;
        Ok(PyResponse {
            status,
            headers: HashMap::new(),
            body: std::sync::Mutex::new(PyResponseBody::BodySource(source)),
            extra_headers: Vec::new(),
        })
    }

    #[getter]
    fn kind(&self) -> PyResult<String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        Ok(match inner.as_ref() {
            Some(s) => match s {
                BodySource::Empty => "empty",
                BodySource::Bytes(_) => "bytes",
                BodySource::FileFull { .. } => "file_full",
                BodySource::FileRange { .. } => "file_range",
            }
            .to_string(),
            None => "consumed".to_string(),
        })
    }

    #[getter]
    fn length(&self) -> PyResult<Option<u64>> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        Ok(inner.as_ref().map(|s| s.len()))
    }

    #[getter]
    fn range(&self) -> PyResult<Option<(u64, u64)>> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        Ok(inner
            .as_ref()
            .and_then(|s| s.range())
            .map(|r| (r.start(), r.end_inclusive())))
    }

    fn read_all<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        let mut source = inner.take().ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err("BodySource already consumed")
        })?;
        drop(inner);
        let data = py
            .allow_threads(|| source.read_all())
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
        Ok(PyBytes::new(py, &data))
    }

    fn read_range<'py>(
        &self,
        py: Python<'py>,
        start: u64,
        end_inclusive: u64,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        let mut source = inner.take().ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err("BodySource already consumed")
        })?;
        drop(inner);
        let data = py
            .allow_threads(|| source.read_range(start, end_inclusive))
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
        Ok(PyBytes::new(py, &data))
    }

    fn __repr__(&self) -> String {
        let inner = self.inner.lock().map_err(|_| "lock poisoned".to_string());
        match inner {
            Ok(inner) => match inner.as_ref() {
                Some(s) => format!("<BodySource {:?}>", s.kind()),
                None => "<BodySource consumed>".to_string(),
            },
            Err(e) => format!("<BodySource {e}>"),
        }
    }
}

// ---------------------------------------------------------------------------
// Python callback service adapter
// ---------------------------------------------------------------------------

struct PythonCallbackService {
    handler: Arc<std::sync::Mutex<Option<Py<PyAny>>>>,
    callback_semaphore: Arc<Semaphore>,
    body_policy: RequestBodyPolicy,
}

impl PythonCallbackService {
    fn call_python_callback(
        handler: &Arc<std::sync::Mutex<Option<Py<PyAny>>>>,
        py_request: PyRequest,
    ) -> Result<CanonicalResponse, ServiceError> {
        Python::with_gil(|py| {
            let handler_gil = handler
                .lock()
                .map_err(|_| ServiceError::internal("handler lock poisoned"))?;
            let handler_py = handler_gil
                .as_ref()
                .ok_or_else(|| ServiceError::internal("handler already consumed"))?
                .clone_ref(py);
            drop(handler_gil);

            let is_head = py_request.method == "HEAD";
            let py_req_obj = py_request
                .into_pyobject(py)
                .map_err(|e| ServiceError::internal(format!("failed to create request: {e}")))?;

            let result = handler_py.bind(py).call1((py_req_obj,)).map_err(|err| {
                // Log the exception type only; exception text may carry
                // untrusted request data and must not reach logs.
                let type_name = err
                    .value(py)
                    .get_type()
                    .name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "<unknown>".to_string());
                eggserve_core::ops::Logger::global().emit(eggserve_core::ops::Event::new(
                    eggserve_core::ops::Severity::Error,
                    eggserve_core::ops::EventKind::ServiceError,
                    format!("Python handler raised an exception ({type_name})"),
                ));
                ServiceError::internal("handler raised an exception")
            })?;

            if result
                .hasattr("__await__")
                .map_err(|_| ServiceError::internal("Python handler response inspection failed"))?
            {
                return Err(ServiceError::internal(
                    "handler returned a coroutine; async handlers are not supported",
                ));
            }

            convert_python_response_to_canonical(py, &result, is_head)
        })
    }

    fn build_py_request(
        head: RequestHead,
        body: RequestBody,
        body_policy: RequestBodyPolicy,
        connection: eggserve_core::primitives::connection_info::ConnectionInfo,
    ) -> PyRequest {
        use eggserve_core::primitives::connection_info::Scheme;

        let method_str = head.method().as_str().to_string();
        let target = head.target().path().to_string();
        let query = head.target().query().unwrap_or("").to_string();
        let header_items: Vec<(String, String)> = head
            .headers()
            .iter()
            .map(|f| (f.name.to_string(), f.value.to_string()))
            .collect();
        let mut headers = HashMap::new();
        for (name, value) in &header_items {
            headers
                .entry(name.to_ascii_lowercase())
                .or_insert_with(|| value.clone());
        }
        let http_version = head.version().to_string();

        // Non-socket transports expose no fabricated addresses: map absent
        // endpoints to None rather than placeholder values.
        let remote_addr = connection.remote_addr.map(|a| a.to_string());
        let local_addr = connection.local_addr.map(|a| a.to_string());
        let remote_address = connection
            .remote_addr
            .map(|a| (a.ip().to_string(), a.port()));
        let local_address = connection
            .local_addr
            .map(|a| (a.ip().to_string(), a.port()));
        let scheme = Some(match connection.scheme {
            Scheme::Http => "http".to_string(),
            Scheme::Https => "https".to_string(),
        });

        let (py_body, has_body) = if body_policy.is_reject() {
            (None, false)
        } else {
            // Expose the body only when there is actual content to read.
            // Empty bodies (Content-Length: 0 or no Content-Length with no
            // Transfer-Encoding) are treated as bodyless for the Python
            // handler, regardless of method.
            let has_content = body.declared_length().is_some_and(|len| len > 0)
                || head.headers().contains("transfer-encoding")
                || body.bytes_received() > 0;
            if has_content {
                let declared_length = body.declared_length();
                let py_body = PyRequestBody {
                    inner: Arc::new(std::sync::Mutex::new(Some(body))),
                    handle: tokio::runtime::Handle::current(),
                    declared_length,
                    final_bytes_received: Arc::new(AtomicU64::new(0)),
                    final_complete: Arc::new(AtomicBool::new(false)),
                };
                (Some(py_body), true)
            } else {
                (None, false)
            }
        };

        PyRequest {
            method: method_str,
            path: target,
            query,
            headers,
            header_items,
            remote_addr,
            remote_address,
            local_addr,
            local_address,
            scheme,
            http_version,
            body: if has_body { py_body } else { None },
        }
    }
}

fn convert_python_response_to_canonical<'py>(
    _py: Python<'py>,
    obj: &Bound<'py, PyAny>,
    is_head: bool,
) -> Result<CanonicalResponse, ServiceError> {
    let status: u16 = obj
        .getattr("status")
        .map_err(|_| ServiceError::internal("Python handler response status is missing"))?
        .extract()
        .map_err(|_| ServiceError::internal("Python handler response status is invalid"))?;
    let code = CanonicalStatusCode::new(status)
        .map_err(|_| ServiceError::internal("Python handler response status is outside 100-599"))?;

    let mut headers: Vec<(String, String)> = obj
        .getattr("headers")
        .map_err(|_| ServiceError::internal("Python handler response headers are missing"))?
        .extract()
        .or_else(|_| {
            // Native Response exposes a dict; structural responses may
            // provide an ordered list of header pairs.
            obj.getattr("headers")
                .and_then(|v| v.extract::<HashMap<String, String>>())
                .map(|map| map.into_iter().collect())
        })
        .map_err(|_| ServiceError::internal("Python handler response headers are invalid"))?;

    if let Ok(py_resp) = obj.extract::<pyo3::Bound<'py, PyResponse>>() {
        headers.extend(py_resp.borrow().extra_headers.iter().cloned());
    }

    // Validate every header into temporary canonical values before constructing
    // a response. This keeps a later body or framing failure from exposing a
    // partially validated response.
    let mut validated_headers = Vec::with_capacity(headers.len());
    for (name, value) in headers {
        if eggserve_core::primitives::canonical::is_hop_by_hop_header(&name) {
            return Err(ServiceError::internal(
                "Python handler response header validation failed",
            ));
        }
        if value.trim().is_empty() {
            return Err(ServiceError::internal(
                "Python handler response header validation failed",
            ));
        }
        let n = HeaderName::new(name.as_str()).map_err(|_| {
            ServiceError::internal("Python handler response header validation failed")
        })?;
        let v = HeaderValue::new(value.as_str()).map_err(|_| {
            ServiceError::internal("Python handler response header validation failed")
        })?;
        let content_length = if name.eq_ignore_ascii_case("content-length") {
            Some(value.parse::<u64>().map_err(|_| {
                ServiceError::internal("Python handler response length validation failed")
            })?)
        } else {
            None
        };
        validated_headers.push((n, v, content_length));
    }

    let representation_length = validated_headers
        .iter()
        .find_map(|(_, _, declared)| *declared);
    let body = extract_python_response_body(obj, code, is_head, representation_length)?;

    let body_len = body.len();
    for (_, _, declared) in &validated_headers {
        if let Some(declared) = declared {
            if *declared != body_len {
                return Err(ServiceError::internal(
                    "Python handler response length validation failed",
                ));
            }
        }
    }

    let mut response = CanonicalResponse::builder()
        .status(code)
        .body(body)
        .map_err(|_| ServiceError::internal("Python handler response construction failed"))?;

    for (n, v, _) in validated_headers {
        response.head_mut().headers_mut().push(n, v);
    }

    let norm_req = NormalizeRequest::new(is_head);
    normalize_response(response, &norm_req)
        .map_err(|_| ServiceError::internal("Python handler response normalization failed"))
}

fn extract_python_response_body<'py>(
    obj: &Bound<'py, PyAny>,
    status: CanonicalStatusCode,
    is_head: bool,
    representation_length: Option<u64>,
) -> Result<ResponseBody, ServiceError> {
    if let Ok(py_resp) = obj.extract::<pyo3::Bound<'py, PyResponse>>() {
        let response = py_resp.borrow();
        let mut body = response.body.lock().map_err(|_| {
            ServiceError::internal("Python handler response body conversion failed")
        })?;
        return match std::mem::replace(&mut *body, PyResponseBody::Consumed) {
            PyResponseBody::Consumed => Err(ServiceError::internal(
                "Python handler response body conversion failed",
            )),
            PyResponseBody::Empty => {
                if is_head {
                    if let Some(length) = representation_length {
                        Ok(ResponseBody::EmptyWithLength(length))
                    } else {
                        Ok(ResponseBody::Empty)
                    }
                } else {
                    Ok(ResponseBody::Empty)
                }
            }
            PyResponseBody::Bytes(data) => {
                if is_head {
                    if let Some(length) = representation_length {
                        Ok(ResponseBody::EmptyWithLength(length))
                    } else {
                        Ok(ResponseBody::Bytes(data))
                    }
                } else {
                    Ok(ResponseBody::Bytes(data))
                }
            }
            PyResponseBody::BodySource(source) => match source {
                BodySource::Empty => {
                    if is_head {
                        if let Some(length) = representation_length {
                            Ok(ResponseBody::EmptyWithLength(length))
                        } else {
                            Ok(ResponseBody::Empty)
                        }
                    } else {
                        Ok(ResponseBody::Empty)
                    }
                }
                BodySource::Bytes(data) => {
                    if is_head {
                        if let Some(length) = representation_length {
                            Ok(ResponseBody::EmptyWithLength(length))
                        } else {
                            Ok(ResponseBody::Bytes(data))
                        }
                    } else {
                        Ok(ResponseBody::Bytes(data))
                    }
                }
                file @ BodySource::FileFull { .. } | file @ BodySource::FileRange { .. } => {
                    Ok(ResponseBody::File(file))
                }
            },
            PyResponseBody::Stream {
                iterable,
                content_length,
            } => {
                // HEAD and body-forbidden statuses must not advance the
                // iterator: drop the iterable (releasing Python references
                // promptly) and retain only framing-relevant length.
                // `normalize_response` drops streams without polling, but
                // spawning the producer would still pull one item before
                // observing the drop, so suppress here.
                let suppress_forbidden = !status.permits_payload_body();
                if is_head || suppress_forbidden {
                    drop(iterable);
                    // 304 may retain a matching representation length;
                    // 1xx/204/205 are forced to zero by normalization.
                    // HEAD with known length preserves it; HEAD unknown
                    // omits Content-Length via an empty unknown stream
                    // (Empty would invent `Content-Length: 0`).
                    if status == CanonicalStatusCode::NOT_MODIFIED {
                        if let Some(length) = content_length.or(representation_length) {
                            Ok(ResponseBody::EmptyWithLength(length))
                        } else {
                            let empty =
                                ResponseStream::new(futures_util::stream::empty::<
                                    Result<Bytes, ResponseStreamError>,
                                >());
                            Ok(ResponseBody::Stream(empty))
                        }
                    } else if is_head {
                        if let Some(length) = content_length.or(representation_length) {
                            // Known HEAD length: preserve for framing. When
                            // only the header supplied the length for an
                            // unknown stream, the header is authoritative
                            // (validation below already compared it against
                            // the would-be body only for non-suppressed
                            // paths; here the iterator never ran so accept
                            // the declared header length).
                            Ok(ResponseBody::EmptyWithLength(length))
                        } else {
                            // Unknown HEAD length: omit Content-Length.
                            let empty =
                                ResponseStream::new(futures_util::stream::empty::<
                                    Result<Bytes, ResponseStreamError>,
                                >());
                            Ok(ResponseBody::Stream(empty))
                        }
                    } else {
                        Ok(ResponseBody::Empty)
                    }
                } else {
                    let (sender, receiver) =
                        mpsc::channel::<Result<Bytes, ResponseStreamError>>(
                            PYTHON_STREAM_CHANNEL_BOUND,
                        );
                    spawn_python_stream_producer(iterable, sender);
                    let adapter = PythonReceiverStream {
                        rx: std::sync::Mutex::new(receiver),
                    };
                    let stream = match content_length {
                        Some(len) => ResponseStream::with_known_length(adapter, len),
                        None => ResponseStream::new(adapter),
                    };
                    Ok(ResponseBody::Stream(stream))
                }
            }
        };
    }

    let body = obj
        .getattr("body")
        .map_err(|_| ServiceError::internal("Python handler response body is missing"))?;
    if let Ok(data) = body.extract::<Vec<u8>>() {
        if is_head {
            if let Some(length) = representation_length {
                return Ok(ResponseBody::EmptyWithLength(length));
            }
        }
        return Ok(ResponseBody::Bytes(data));
    }

    let kind: String = body
        .getattr("kind")
        .map_err(|_| ServiceError::internal("Python handler response body is unsupported"))?
        .extract()
        .map_err(|_| ServiceError::internal("Python handler response body kind is invalid"))?;
    match kind.as_str() {
        "empty" => Ok(ResponseBody::Empty),
        "bytes" => {
            let data = body
                .call_method0("read_all")
                .map_err(|_| {
                    ServiceError::internal("Python handler response body conversion failed")
                })?
                .extract::<Vec<u8>>()
                .map_err(|_| {
                    ServiceError::internal("Python handler response body conversion failed")
                })?;
            if is_head {
                if let Some(length) = representation_length {
                    Ok(ResponseBody::EmptyWithLength(length))
                } else {
                    Ok(ResponseBody::Bytes(data))
                }
            } else {
                Ok(ResponseBody::Bytes(data))
            }
        }
        _ => Err(ServiceError::internal(
            "Python handler response body kind is unsupported",
        )),
    }
}

impl Service for PythonCallbackService {
    fn request_body_policy(
        &self,
        _head: &eggserve_core::primitives::request_head::RequestHead,
    ) -> RequestBodyPolicy {
        self.body_policy
    }

    fn call(
        &self,
        request: eggserve_core::primitives::request::Request,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<CanonicalResponse, ServiceError>> + Send + '_>,
    > {
        let handler = self.handler.clone();
        let callback_semaphore = self.callback_semaphore.clone();
        let body_policy = self.body_policy;

        Box::pin(async move {
            let callback_permit = callback_semaphore
                .acquire_owned()
                .await
                .map_err(|_| ServiceError::internal("callback semaphore closed"))?;

            let (head, body, connection) = request.into_parts();
            let py_request = Self::build_py_request(head, body, body_policy, connection);

            tokio::task::spawn_blocking(move || {
                let _callback_permit = callback_permit;
                Self::call_python_callback(&handler, py_request)
            })
            .await
                .map_err(|e| ServiceError::internal(format!("callback task failed: {e}")))?
        })
    }
}

// ---------------------------------------------------------------------------
// Python Server — delegates to Rust runtime
// ---------------------------------------------------------------------------

/// Dropping a started `Server` without calling `stop()` (or leaving the
/// context manager) drops the native tokio runtime synchronously when the
/// last Python reference disappears; that teardown waits for in-flight
/// tasks, so interpreter shutdown or GC can stall while connections drain.
/// Always stop a running server explicitly.
#[pyclass(frozen, name = "Server")]
#[allow(dead_code)]
pub struct PyServer {
    bind: String,
    port: u16,
    bind_address: SocketAddr,
    public: bool,
    addr: std::sync::Mutex<Option<String>>,
    static_root: Option<std::path::PathBuf>,
    static_policy: StaticPolicy,
    handler: Option<std::sync::Mutex<Option<Py<PyAny>>>>,
    handle: std::sync::Mutex<Option<ServerHandle>>,
    runtime: std::sync::Mutex<Option<tokio::runtime::Runtime>>,
    has_been_started: std::sync::atomic::AtomicBool,
    starting: std::sync::atomic::AtomicBool,
    max_connections: usize,
    max_file_streams: usize,
    max_python_callbacks: usize,
    header_timeout: Duration,
    connection_total_timeout: Duration,
    handler_timeout: Duration,
    graceful_shutdown_timeout: Duration,
    body_policy: RequestBodyPolicy,
    max_request_body_bytes: u64,
    body_read_timeout: Duration,
    tls_config: Option<std::sync::Arc<rustls::ServerConfig>>,
    default_content_type: String,
    extra_response_headers: Vec<(String, String)>,
    // Plan 164 production controls (operator-meaningful subset).
    max_in_flight_requests: usize,
    max_buf_size: usize,
    max_headers: usize,
    max_header_bytes: usize,
    max_request_target_bytes: usize,
    keep_alive_idle_timeout: Duration,
    max_requests_per_connection: Option<u64>,
    response_write_timeout: Duration,
    // Plan 165 response privacy subset (safe for Python embedding).
    server_header: Option<String>,
    date_suppressed: bool,
    stripped_response_headers: Vec<String>,
    error_empty: bool,
}

#[pymethods]
impl PyServer {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (root=None, bind="127.0.0.1", port=8000, policy=None, handler=None, public=false, max_connections=64, max_file_streams=32, max_python_callbacks=8, header_timeout_secs=10, connection_total_timeout_secs=60, handler_timeout_secs=30, graceful_shutdown_timeout_secs=10, request_body_mode="reject", max_request_body_bytes=0, body_timeout_secs=30, tls_certfile=None, tls_keyfile=None, default_content_type="application/octet-stream", extra_response_headers=None, max_in_flight_requests=64, max_buf_size=65536, max_headers=100, max_header_bytes=32768, max_request_target_bytes=8192, keep_alive_idle_timeout_secs=60, max_requests_per_connection=None, response_write_timeout_secs=30, server_header=None, date_policy="system", stripped_response_headers=None, error_policy="minimal"))]
    fn new(
        root: Option<String>,
        bind: &str,
        port: u16,
        policy: Option<PyStaticPolicyWrapper>,
        handler: Option<Py<PyAny>>,
        public: bool,
        max_connections: usize,
        max_file_streams: usize,
        max_python_callbacks: usize,
        header_timeout_secs: u64,
        connection_total_timeout_secs: u64,
        handler_timeout_secs: u64,
        graceful_shutdown_timeout_secs: u64,
        request_body_mode: &str,
        max_request_body_bytes: u64,
        body_timeout_secs: u64,
        tls_certfile: Option<String>,
        tls_keyfile: Option<String>,
        default_content_type: &str,
        extra_response_headers: Option<Vec<(String, String)>>,
        max_in_flight_requests: usize,
        max_buf_size: usize,
        max_headers: usize,
        max_header_bytes: usize,
        max_request_target_bytes: usize,
        keep_alive_idle_timeout_secs: u64,
        max_requests_per_connection: Option<u64>,
        response_write_timeout_secs: u64,
        server_header: Option<String>,
        date_policy: &str,
        stripped_response_headers: Option<Vec<String>>,
        error_policy: &str,
    ) -> PyResult<Self> {
        // rustls can be built with more than one provider through the
        // workspace's feature-unified dependency graph. Select the same
        // ring provider used by the CLI before constructing TLS config.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let bind_addr: SocketAddr = (bind, port)
            .to_socket_addrs()
            .map_err(|_| {
                pyo3::exceptions::PyOSError::new_err("invalid or unresolved bind address")
            })?
            .next()
            .ok_or_else(|| pyo3::exceptions::PyOSError::new_err("bind address did not resolve"))?;
        if !public && bind_addr.ip().is_unspecified() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "binding to 0.0.0.0 or :: requires public=True",
            ));
        }
        if max_connections == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "max_connections must be greater than zero",
            ));
        }
        if max_file_streams == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "max_file_streams must be greater than zero",
            ));
        }
        if max_python_callbacks == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "max_python_callbacks must be greater than zero",
            ));
        }
        if header_timeout_secs == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "header_timeout_secs must be greater than zero",
            ));
        }
        if connection_total_timeout_secs == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "connection_total_timeout_secs must be greater than zero",
            ));
        }
        if handler_timeout_secs == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "handler_timeout_secs must be greater than zero",
            ));
        }
        if graceful_shutdown_timeout_secs == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "graceful_shutdown_timeout_secs must be greater than zero",
            ));
        }
        if body_timeout_secs == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "body_timeout_secs must be greater than zero",
            ));
        }
        // Plan 164 production controls: operator-meaningful subset with the
        // same bounds as RuntimeConfig/Limits. `None` disables
        // max_requests_per_connection; zero is rejected (no zero-means-
        // unlimited overload).
        if max_in_flight_requests == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "max_in_flight_requests must be greater than zero",
            ));
        }
        if max_buf_size < eggserve_core::limits::MIN_MAX_BUF_SIZE {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "max_buf_size must be >= {} (Hyper minimum)",
                eggserve_core::limits::MIN_MAX_BUF_SIZE
            )));
        }
        if max_buf_size > eggserve_core::limits::MAX_MAX_BUF_SIZE {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "max_buf_size must be <= {} (4 MiB)",
                eggserve_core::limits::MAX_MAX_BUF_SIZE
            )));
        }
        if max_headers == 0 || max_headers > eggserve_core::limits::MAX_MAX_HEADERS {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "max_headers must be between 1 and {}",
                eggserve_core::limits::MAX_MAX_HEADERS
            )));
        }
        if max_header_bytes < eggserve_core::limits::MIN_MAX_HEADER_BYTES
            || max_header_bytes > eggserve_core::limits::MAX_MAX_HEADER_BYTES
        {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "max_header_bytes must be between {} and {}",
                eggserve_core::limits::MIN_MAX_HEADER_BYTES,
                eggserve_core::limits::MAX_MAX_HEADER_BYTES
            )));
        }
        if max_request_target_bytes < eggserve_core::limits::MIN_MAX_REQUEST_TARGET_BYTES
            || max_request_target_bytes > eggserve_core::limits::MAX_MAX_REQUEST_TARGET_BYTES
        {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "max_request_target_bytes must be between {} and {}",
                eggserve_core::limits::MIN_MAX_REQUEST_TARGET_BYTES,
                eggserve_core::limits::MAX_MAX_REQUEST_TARGET_BYTES
            )));
        }
        if keep_alive_idle_timeout_secs == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "keep_alive_idle_timeout_secs must be greater than zero",
            ));
        }
        if max_requests_per_connection == Some(0) {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "max_requests_per_connection must be >= 1 or None (unlimited)",
            ));
        }
        if response_write_timeout_secs == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "response_write_timeout_secs must be greater than zero",
            ));
        }
        // Plan 165 response privacy subset. Custom Rust clock providers stay
        // Rust-only: Python selects the standards clock or explicit
        // suppression, never a per-response GIL clock callback.
        let date_suppressed = match date_policy {
            "system" => false,
            "suppress" => true,
            _ => {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "date_policy must be 'system' or 'suppress'",
                ))
            }
        };
        let error_empty = match error_policy {
            "minimal" => false,
            "empty" => true,
            _ => {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "error_policy must be 'minimal' or 'empty'",
                ))
            }
        };
        let stripped_response_headers = stripped_response_headers.unwrap_or_default();
        // Validate privacy fields eagerly via the canonical validators so
        // misconfiguration fails before listener startup.
        {
            let mut policy = eggserve_core::server::response_policy::ResponsePolicy::default();
            if let Some(ref h) = server_header {
                policy.server_identification = Some(h.clone());
            }
            policy.stripped_response_headers = stripped_response_headers.clone();
            policy
                .validate()
                .map_err(pyo3::exceptions::PyValueError::new_err)?;
            for name in &stripped_response_headers {
                eggserve_core::server::response_policy::validate_stripped_header_name(name)
                    .map_err(pyo3::exceptions::PyValueError::new_err)?;
            }
        }
        // Handler-only mode requires no static root: custom services run
        // without a filesystem root. Static mode still requires one.
        let static_root = match (&root, &handler) {
            (None, None) => {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "static root is required when no handler is given (handler-only servers may omit root)",
                ))
            }
            (Some(r), None) => Some(std::path::PathBuf::from(r)),
            (_, Some(_)) => None,
        };

        // Parse body policy
        let body_policy = match request_body_mode {
            "reject" => RequestBodyPolicy::Reject,
            "buffer" => {
                if max_request_body_bytes == 0 {
                    return Err(pyo3::exceptions::PyValueError::new_err(
                        "buffer mode requires max_request_body_bytes > 0",
                    ));
                }
                RequestBodyPolicy::Buffer {
                    max_bytes: max_request_body_bytes,
                }
            }
            "stream" => {
                if max_request_body_bytes == 0 {
                    return Err(pyo3::exceptions::PyValueError::new_err(
                        "stream mode requires max_request_body_bytes > 0",
                    ));
                }
                RequestBodyPolicy::Stream {
                    max_bytes: max_request_body_bytes,
                }
            }
            _ => {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "request_body_mode must be 'reject', 'buffer', or 'stream'",
                ));
            }
        };

        let static_policy = policy
            .map(|p| p.inner)
            .unwrap_or_else(StaticPolicy::safe_default);
        let extra_response_headers = extra_response_headers.unwrap_or_default();
        eggserve_core::config::validate_static_metadata(
            default_content_type,
            &extra_response_headers,
        )
        .map_err(pyo3::exceptions::PyValueError::new_err)?;

        let tls_config = match (tls_certfile, tls_keyfile) {
            (None, None) => None,
            (Some(cert), Some(key)) => Some(
                eggserve_core::tls::load_tls_config(
                    std::path::Path::new(&cert),
                    std::path::Path::new(&key),
                )
                .map_err(|e| {
                    pyo3::exceptions::PyValueError::new_err(format!(
                        "TLS configuration failed: {e}"
                    ))
                })?,
            ),
            _ => {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "TLS requires both tls_certfile and tls_keyfile",
                ));
            }
        };

        Ok(Self {
            bind: bind.to_string(),
            port,
            bind_address: bind_addr,
            public,
            addr: std::sync::Mutex::new(None),
            static_root,
            static_policy,
            handler: handler.map(|h| std::sync::Mutex::new(Some(h))),
            handle: std::sync::Mutex::new(None),
            runtime: std::sync::Mutex::new(None),
            has_been_started: std::sync::atomic::AtomicBool::new(false),
            starting: std::sync::atomic::AtomicBool::new(false),
            max_connections,
            max_file_streams,
            max_python_callbacks,
            header_timeout: Duration::from_secs(header_timeout_secs),
            connection_total_timeout: Duration::from_secs(connection_total_timeout_secs),
            handler_timeout: Duration::from_secs(handler_timeout_secs),
            graceful_shutdown_timeout: Duration::from_secs(graceful_shutdown_timeout_secs),
            body_policy,
            max_request_body_bytes,
            body_read_timeout: Duration::from_secs(body_timeout_secs),
            tls_config,
            default_content_type: default_content_type.to_string(),
            extra_response_headers,
            max_in_flight_requests,
            max_buf_size,
            max_headers,
            max_header_bytes,
            max_request_target_bytes,
            keep_alive_idle_timeout: Duration::from_secs(keep_alive_idle_timeout_secs),
            max_requests_per_connection,
            response_write_timeout: Duration::from_secs(response_write_timeout_secs),
            server_header,
            date_suppressed,
            stripped_response_headers,
            error_empty,
        })
    }

    #[getter]
    fn addr(&self) -> PyResult<Option<String>> {
        let guard = self
            .addr
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        Ok(guard.clone())
    }

    #[getter]
    fn state(&self) -> PyResult<String> {
        let handle_guard = self
            .handle
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        if let Some(handle) = handle_guard.as_ref() {
            Ok(handle.state().to_string())
        } else if self
            .has_been_started
            .load(std::sync::atomic::Ordering::Acquire)
        {
            Ok("stopped".to_string())
        } else {
            Ok("created".to_string())
        }
    }

    fn start(slf: Py<Self>, py: Python<'_>) -> PyResult<()> {
        {
            let this = slf.borrow(py);
            let handle_guard = this
                .handle
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            let runtime_guard = this
                .runtime
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            if handle_guard.is_some()
                || runtime_guard.is_some()
                || this.starting.swap(true, std::sync::atomic::Ordering::AcqRel)
            {
                return Err(crate::LifecycleError::new_err("Server already started"));
            }
        }

        let result = Self::start_reserved(slf.clone_ref(py), py);
        // Reset the concurrency guard unconditionally: its only purpose is
        // to prevent concurrent starts, not to record failure. Leaving it
        // set after a failed startup would permanently brick the object
        // ("Server already started" on every retry).
        slf.borrow(py)
            .starting
            .store(false, std::sync::atomic::Ordering::Release);
        result
    }

    fn start_reserved(slf: Py<Self>, py: Python<'_>) -> PyResult<()> {
        let (
            bind_addr,
            max_connections,
            max_file_streams,
            max_python_callbacks,
            header_timeout,
            connection_total_timeout,
            handler_timeout,
            graceful_shutdown_timeout,
            max_request_body_bytes,
            body_read_timeout,
            tls_config,
            handler,
            static_root,
            static_policy,
            body_policy,
            default_content_type,
            extra_response_headers,
            max_in_flight_requests,
            max_buf_size,
            max_headers,
            max_header_bytes,
            max_request_target_bytes,
            keep_alive_idle_timeout,
            max_requests_per_connection,
            response_write_timeout,
            server_header,
            date_suppressed,
            stripped_response_headers,
            error_empty,
        ) = {
            let this = slf.borrow(py);
            let handler = this
                .handler
                .as_ref()
                .map(|handler| {
                    let guard = handler.lock().map_err(|_| {
                        pyo3::exceptions::PyRuntimeError::new_err("handler lock poisoned")
                    })?;
                    guard
                        .as_ref()
                        .map(|handler| handler.clone_ref(py))
                        .ok_or_else(|| {
                            pyo3::exceptions::PyRuntimeError::new_err("handler already consumed")
                        })
                })
                .transpose()?;
            (
                this.bind_address,
                this.max_connections,
                this.max_file_streams,
                this.max_python_callbacks,
                this.header_timeout,
                this.connection_total_timeout,
                this.handler_timeout,
                this.graceful_shutdown_timeout,
                this.max_request_body_bytes,
                this.body_read_timeout,
                this.tls_config.clone(),
                handler,
                this.static_root.clone(),
                this.static_policy.clone(),
                this.body_policy,
                this.default_content_type.clone(),
                this.extra_response_headers.clone(),
                this.max_in_flight_requests,
                this.max_buf_size,
                this.max_headers,
                this.max_header_bytes,
                this.max_request_target_bytes,
                this.keep_alive_idle_timeout,
                this.max_requests_per_connection,
                this.response_write_timeout,
                this.server_header.clone(),
                this.date_suppressed,
                this.stripped_response_headers.clone(),
                this.error_empty,
            )
        };

        // The connection total timeout is the hard ceiling on each
        // connection's lifetime. Cap handler/body budgets to it so the
        // total budget can never fire first and kill requests a wider
        // budget promised to allow.
        let capped_handler_timeout = handler_timeout.min(connection_total_timeout);
        let capped_body_read_timeout = body_read_timeout.min(connection_total_timeout);
        if capped_handler_timeout != handler_timeout || capped_body_read_timeout != body_read_timeout
        {
            eggserve_core::ops::Logger::global().emit(eggserve_core::ops::Event::new(
                eggserve_core::ops::Severity::Warn,
                eggserve_core::ops::EventKind::ProcessStarting,
                "handler/body timeout exceeds connection_total_timeout; capped to connection_total_timeout",
            ));
        }
        let handler_timeout = capped_handler_timeout;
        let body_read_timeout = capped_body_read_timeout;

        let mut runtime_builder = RuntimeConfig::builder()
            .bind(bind_addr)
            .max_connections(max_connections)
            .max_file_streams(max_file_streams)
            .header_read_timeout(header_timeout)
            .connection_total_timeout(connection_total_timeout)
            .handler_timeout(handler_timeout)
            .graceful_shutdown_timeout(graceful_shutdown_timeout)
            .max_request_body_bytes(max_request_body_bytes)
            .body_read_timeout(body_read_timeout)
            .max_in_flight_requests(max_in_flight_requests)
            .max_buf_size(max_buf_size)
            .max_headers(max_headers)
            .max_header_bytes(max_header_bytes)
            .max_request_target_bytes(max_request_target_bytes)
            .keep_alive_idle_timeout(keep_alive_idle_timeout)
            .max_requests_per_connection(max_requests_per_connection)
            .response_write_timeout(response_write_timeout);
        if let Some(tls_config) = &tls_config {
            runtime_builder = runtime_builder.tls_config(tls_config.clone());
        }
        // Plan 165 privacy subset: fixed server value, system/suppressed
        // date, validated denylist, minimal/empty errors. Custom clocks stay
        // Rust-only so no per-response Python callback is introduced.
        if let Some(header) = server_header {
            runtime_builder = runtime_builder.server_header(header);
        }
        runtime_builder = runtime_builder.date_policy(if date_suppressed {
            eggserve_core::server::response_policy::DatePolicy::Suppress
        } else {
            eggserve_core::server::response_policy::DatePolicy::SystemClock
        });
        if !stripped_response_headers.is_empty() {
            runtime_builder =
                runtime_builder.stripped_response_headers(stripped_response_headers);
        }
        runtime_builder = runtime_builder.error_policy(if error_empty {
            eggserve_core::policy::ErrorRepresentationPolicy::Empty
        } else {
            eggserve_core::policy::ErrorRepresentationPolicy::Minimal
        });
        let runtime_config = runtime_builder
            .build()
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;

        let (server_handle, rt) = py.allow_threads(|| -> PyResult<_> {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

            let server_handle = rt.block_on(async {
                if let Some(handler) = handler {
                    let service = PythonCallbackService {
                        handler: Arc::new(std::sync::Mutex::new(Some(handler))),
                        callback_semaphore: Arc::new(Semaphore::new(max_python_callbacks)),
                        body_policy,
                    };
                    let server = Server::builder()
                        .runtime(runtime_config)
                        .bind(bind_addr)
                        .build()
                        .map_err(|e| {
                            pyo3::exceptions::PyRuntimeError::new_err(format!(
                                "failed to build server: {e}"
                            ))
                        })?;
                    let handle = server.start_with_service(service).await.map_err(|e| {
                        pyo3::exceptions::PyRuntimeError::new_err(format!(
                            "failed to start server: {e}"
                        ))
                    })?;
                    wait_until_running(&handle, STARTUP_TIMEOUT).await?;
                    Ok::<ServerHandle, PyErr>(handle)
                } else {
                    let root = static_root.ok_or_else(|| {
                        pyo3::exceptions::PyRuntimeError::new_err(
                            "static configuration is unavailable for custom handler",
                        )
                    })?;
                    let serve_config = Arc::new(eggserve_core::config::ServeConfig {
                        root,
                        static_policy,
                        default_content_type,
                        extra_response_headers,
                        ..eggserve_core::config::ServeConfig::default()
                    });
                    let server = Server::builder()
                        .runtime(runtime_config)
                        .serve_config(serve_config)
                        .bind(bind_addr)
                        .build()
                        .map_err(|e| {
                            pyo3::exceptions::PyRuntimeError::new_err(format!(
                                "failed to build server: {e}"
                            ))
                        })?;
                    let handle = server.start().await.map_err(|e| {
                        pyo3::exceptions::PyRuntimeError::new_err(format!(
                            "failed to start server: {e}"
                        ))
                    })?;
                    wait_until_running(&handle, STARTUP_TIMEOUT).await?;
                    Ok::<ServerHandle, PyErr>(handle)
                }
            })?;
            Ok((server_handle, rt))
        })?;

        let local_addr = server_handle.local_addr();
        let this = slf.borrow(py);
        *this
            .addr
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))? =
            Some(local_addr.to_string());
        *this
            .runtime
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))? = Some(rt);
        *this
            .handle
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))? =
            Some(server_handle);
        this.has_been_started
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    fn stop(&self, py: Python<'_>) -> PyResult<()> {
        // Take the handle and release the mutex immediately: the blocking
        // drain below must not stall other threads calling state(),
        // wait_ready(), start(), or stop().
        let handle = {
            let mut handle_guard = self
                .handle
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            handle_guard.take()
        };
        if let Some(handle) = handle {
            handle.shutdown();
            let runtime_guard = self
                .runtime
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            if let Some(rt) = runtime_guard.as_ref() {
                let deadline = self.graceful_shutdown_timeout + Duration::from_secs(2);
                py.allow_threads(|| {
                    rt.block_on(async {
                        let _ = tokio::time::timeout(deadline, handle.wait()).await;
                    });
                });
            }
            drop(runtime_guard);
        }

        let mut runtime_guard = self
            .runtime
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        if let Some(rt) = runtime_guard.take() {
            py.allow_threads(|| {
                drop(rt);
            });
        }
        drop(runtime_guard);

        *self
            .addr
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))? = None;
        Ok(())
    }

    fn wait_ready(&self, py: Python<'_>) -> PyResult<()> {
        // Poll the lifecycle state through short lock acquisitions: a
        // blocking readiness wait must not hold the handle mutex, or
        // concurrent state()/start()/stop() calls on other threads would
        // stall for up to STARTUP_TIMEOUT (the same contract stop()
        // observes when it releases the handle lock before draining).
        let deadline = std::time::Instant::now() + STARTUP_TIMEOUT;
        loop {
            let state = {
                let handle_guard = self
                    .handle
                    .lock()
                    .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
                let handle = handle_guard
                    .as_ref()
                    .ok_or_else(|| crate::LifecycleError::new_err("server not started"))?;
                handle.state()
            };
            match state {
                LifecycleState::Running => return Ok(()),
                LifecycleState::Failed => {
                    return Err(pyo3::exceptions::PyRuntimeError::new_err(
                        "server failed during startup",
                    ));
                }
                LifecycleState::Starting => {}
                other => {
                    return Err(crate::LifecycleError::new_err(format!(
                        "server not running: unexpected state {other}"
                    )));
                }
            }
            if std::time::Instant::now() >= deadline {
                return Err(crate::LifecycleError::new_err(format!(
                    "startup readiness timeout: server is starting after {}s",
                    STARTUP_TIMEOUT.as_secs()
                )));
            }
            py.allow_threads(|| std::thread::sleep(Duration::from_millis(10)));
        }
    }

    fn shutdown(&self) -> PyResult<()> {
        let handle_guard = self
            .handle
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        if let Some(handle) = handle_guard.as_ref() {
            handle.shutdown();
        }
        Ok(())
    }

    #[pyo3(signature = (timeout_secs=10.0))]
    fn force_shutdown(&self, py: Python<'_>, timeout_secs: f64) -> PyResult<String> {
        let timeout = Duration::from_secs_f64(timeout_secs);

        // Take the handle and release the mutex immediately: the blocking
        // drain below must not stall other threads calling state(),
        // wait_ready(), stop(), shutdown(), or wait().
        let handle = {
            let mut handle_guard = self
                .handle
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            handle_guard.take()
        };
        if let Some(handle) = handle {
            let runtime_guard = self
                .runtime
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            let result = if let Some(rt) = runtime_guard.as_ref() {
                py.allow_threads(|| {
                    rt.block_on(async {
                        let result =
                            tokio::time::timeout(timeout, handle.force_shutdown(timeout)).await;
                        match result {
                            Ok(Ok(shutdown_result)) => Some(shutdown_result),
                            _ => None,
                        }
                    })
                })
            } else {
                None
            };
            drop(runtime_guard);

            // The handle has been consumed either way, so tear the runtime
            // down exactly as stop() does: force_shutdown() must be a
            // complete teardown path, not a runtime leak. Shutdown runs in
            // the background because connection/callback tasks can be
            // parked in uninterruptible synchronous work; a blocking drop
            // here would stall callers past their requested deadline.
            let mut runtime_guard = self
                .runtime
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            if let Some(rt) = runtime_guard.take() {
                rt.shutdown_background();
            }
            drop(runtime_guard);
            *self
                .addr
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))? = None;

            match result {
                Some(ShutdownResult::Clean) => Ok("clean".to_string()),
                _ => Ok("timeout".to_string()),
            }
        } else {
            Ok("clean".to_string())
        }
    }

    fn wait(&self, py: Python<'_>) -> PyResult<String> {
        // Take the handle and release the mutex immediately: the blocking
        // drain below must not stall other threads calling state(),
        // wait_ready(), start(), stop(), or force_shutdown().
        let handle = {
            let mut handle_guard = self
                .handle
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            handle_guard.take()
        };
        if let Some(handle) = handle {
            let runtime_guard = self
                .runtime
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            if let Some(rt) = runtime_guard.as_ref() {
                let deadline = self.graceful_shutdown_timeout + Duration::from_secs(2);
                py.allow_threads(|| {
                    rt.block_on(async {
                        let _ = tokio::time::timeout(deadline, handle.wait()).await;
                    });
                    Ok::<(), PyErr>(())
                })?;
            }
            drop(runtime_guard);

            // Consuming the handle ends this server instance, so tear the
            // runtime down exactly as stop() does: post-wait() must not
            // leave a runtime alive or report a listening address.
            let mut runtime_guard = self
                .runtime
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            if let Some(rt) = runtime_guard.take() {
                py.allow_threads(|| {
                    drop(rt);
                });
            }
            drop(runtime_guard);

            *self
                .addr
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))? = None;
        }
        Ok("stopped".to_string())
    }

    fn __enter__(slf: Py<Self>, py: Python<'_>) -> PyResult<Py<Self>> {
        Self::start(slf.clone_ref(py), py)?;
        Ok(slf)
    }

    fn __exit__(
        &self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
        py: Python<'_>,
    ) -> PyResult<bool> {
        self.stop(py)?;
        Ok(false)
    }

    fn __repr__(&self) -> String {
        match self.addr.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(addr) => format!("<Server {addr}>"),
                None => "<Server not started>".to_string(),
            },
            Err(_) => "<Server not started>".to_string(),
        }
    }
}
