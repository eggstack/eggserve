//! Python request-body bridge (Plan 206 Track B).
//!
//! Owns the body channel state machine (`PythonReceiverStream`,
//! `spawn_python_stream_producer`, `PyRequestBody`, `BodyChunkIterator`).
//! One owner for body streaming; response bridging lives in
//! `response_bridge`. Shared with the Python-side `AsyncServer` shim via
//! the same bounded 16-chunk bridge (no duplication).

#![allow(unused_imports)]
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
use eggserve_core::primitives::request_context::RequestContext;
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

use super::*;
#[allow(unused_imports)]
use super::errors::{RawBodyError, raw_body_error_to_pyerr};

pub(super) const PYTHON_STREAM_CHANNEL_BOUND: usize = 16;

/// `Stream` adapter over the bounded producer channel.
///
/// Wraps the receiver in a `Mutex` so the adapter is `Sync` as required by
/// `ResponseStream::new` (tokio's `Receiver` is `Send` but single-consumer
/// `!Sync`). Polls are sequential from the transport, so the lock is
/// uncontended.
pub(super) struct PythonReceiverStream {
    pub(super) rx: std::sync::Mutex<mpsc::Receiver<Result<Bytes, ResponseStreamError>>>,
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
pub(super) fn spawn_python_stream_producer(
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
    pub(super) inner: Arc<std::sync::Mutex<Option<RequestBody>>>,
    pub(super) handle: tokio::runtime::Handle,
    pub(super) declared_length: Option<u64>,
    pub(super) final_bytes_received: Arc<AtomicU64>,
    pub(super) final_complete: Arc<AtomicBool>,
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

    /// Blocking incremental chunk read (GIL released during wait).
    ///
    /// Plan 204 async substrate: returns `bytes` on data, `None` at EOF.
    /// Unlike `read()` (which consumes via `read_all` semantics) and
    /// `iter_chunks()` (which moves the body into a producer task), this
    /// preserves the body for a subsequent `trailers()` call after terminal
    /// state. Errors map to the stable `RequestBody*` hierarchy. Intended
    /// for use via `asyncio.to_thread` in async handlers so the event loop
    /// stays responsive; direct calls block the caller.
    fn read_chunk<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyBytes>>> {
        // Take-then-put so the std Mutex is never held across `block_on`.
        let mut body = {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            guard.take().ok_or_else(|| {
                crate::RequestBodyConsumedError::new_err("body already consumed")
            })?
        };
        let handle = self.handle.clone();
        let result =
            py.allow_threads(|| handle.block_on(async { body.next_chunk().await }));
        match result {
            Ok(Some(chunk)) => {
                let received = body.bytes_received();
                self.final_bytes_received
                    .store(received, Ordering::Release);
                let bytes = PyBytes::new(py, &chunk);
                // Not terminal: return body for future reads/trailers.
                if let Ok(mut guard) = self.inner.lock() {
                    *guard = Some(body);
                }
                Ok(Some(bytes))
            }
            Ok(None) => {
                let received = body.bytes_received();
                self.final_bytes_received
                    .store(received, Ordering::Release);
                self.final_complete.store(true, Ordering::Release);
                // Terminal: keep body for `trailers()` (which needs `&mut`).
                if let Ok(mut guard) = self.inner.lock() {
                    *guard = Some(body);
                }
                Ok(None)
            }
            Err(e) => {
                let received = body.bytes_received();
                self.final_bytes_received
                    .store(received, Ordering::Release);
                // Preserve the failed body for `trailers()` probing (which
                // will surface `InvalidTrailers`/terminal state); drop on
                // lock failure.
                if let Ok(mut guard) = self.inner.lock() {
                    *guard = Some(body);
                }
                let raw: RawBodyError = e.into();
                Err(raw_body_error_to_pyerr(raw))
            }
        }
    }

    /// Blocking terminal-trailer fetch (GIL released during wait).
    ///
    /// Returns `None` when no trailers were present, or a list of
    /// `(name, value)` text pairs when the peer sent terminal trailers.
    /// Opaque (non-UTF-8) trailer octets are omitted (text-only facade);
    /// the canonical Rust validator remains the authority (denylist +
    /// count/byte limits). Raises `RequestBodyError` on `InvalidTrailers`
    /// and `RequestBodyConsumedError` when called before terminal state
    /// (`TrailersNotReady`). Use after `read_chunk()` returns `None` (or
    /// after `read()`/`iter_chunks()` exhaustion where the body was
    /// preserved). Intended for `asyncio.to_thread` in async handlers.
    fn trailers(&self, py: Python<'_>) -> PyResult<Option<Vec<(String, String)>>> {
        let mut body = {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            guard.take().ok_or_else(|| {
                crate::RequestBodyConsumedError::new_err("body already consumed")
            })?
        };
        let handle = self.handle.clone();
        let result = py.allow_threads(|| handle.block_on(async { body.trailers().await }));
        // Always return the body (trailers() takes `&mut`, body stays owned).
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some(body);
        }
        match result {
            Ok(None) => Ok(None),
            Ok(Some(trailers)) => {
                let mut out = Vec::new();
                for field in trailers.iter() {
                    // Text-only: skip opaque values rather than coercing.
                    if let Ok(text) = field.value.to_str() {
                        out.push((field.name.to_string(), text.to_owned()));
                    }
                }
                Ok(Some(out))
            }
            Err(e) => {
                let raw: RawBodyError = e.into();
                Err(raw_body_error_to_pyerr(raw))
            }
        }
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
    pub(super) receiver: mpsc::Receiver<Result<Vec<u8>, RawBodyError>>,
    pub(super) final_bytes_received: Arc<AtomicU64>,
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

