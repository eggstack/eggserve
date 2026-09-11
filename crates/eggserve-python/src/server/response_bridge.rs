//! Python response bridge (Plan 206 Track B).
//!
//! Owns `PyResponse` (empty/bytes/text/body_source/stream + trailers) over
//! the bounded 16-chunk bridge. HEAD/body-forbidden never advance iterators.
//! Conversion to canonical responses is shared with `sync_handler` (no
//! duplication); async iterables bridge via the Python-side `AsyncServer`.

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
use super::body_bridge::{PYTHON_STREAM_CHANNEL_BOUND, PythonReceiverStream, spawn_python_stream_producer};
#[allow(unused_imports)]
use super::errors::{RawBodyError, raw_body_error_to_pyerr};
#[allow(unused_imports)]
use super::static_responder::validate_response_status;

#[pyclass(frozen, name = "Response")]
pub struct PyResponse {
    #[pyo3(get)]
    pub(super) status: u16,
    #[pyo3(get)]
    pub(super) headers: HashMap<String, String>,
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
    StreamWithTrailers {
        iterable: Py<PyAny>,
        content_length: Option<u64>,
        trailers: Vec<(String, String)>,
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
            Self::StreamWithTrailers {
                content_length,
                trailers,
                ..
            } => f
                .debug_struct("StreamWithTrailers")
                .field("content_length", content_length)
                .field("trailers", &trailers.len())
                .finish_non_exhaustive(),
            Self::Consumed => write!(f, "Consumed"),
        }
    }
}

#[pymethods]
impl PyResponse {
    #[staticmethod]
    pub(super) fn empty(status: u16) -> PyResult<Self> {
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
    pub(super) fn bytes(
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
            PyResponseBody::Stream { .. } | PyResponseBody::StreamWithTrailers { .. } => {
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

    /// Incrementally produced response with one terminal trailer block.
    ///
    /// Plan 204 async substrate (additive; existing `stream` unchanged):
    /// same bounded 16-chunk bridge as `stream`, plus `trailers` — an
    /// ordered list of `(name, value)` text pairs validated via the single
    /// canonical `Trailers` validator (denylist + count/byte limits, default
    /// limits) before commitment. No data after trailers; `HEAD` and
    /// body-forbidden statuses never advance the iterator and never emit
    /// trailers (suppressed, matching `ResponseStream::with_trailers`
    /// semantics); known length counts data only. Opaque (non-UTF-8)
    /// trailer octets remain Rust-only. `Transfer-Encoding` still rejected.
    /// Async producers remain rejected here (sync iterable only); async
    /// handlers bridge async iterables to this via the `AsyncServer` shim
    /// (bounded 16-queue, backpressure, trailers on close).
    #[staticmethod]
    #[pyo3(signature = (status, iterable, headers=None, content_length=None, trailers=None))]
    fn stream_with_trailers(
        py: Python<'_>,
        status: u16,
        iterable: Py<PyAny>,
        headers: Option<HashMap<String, String>>,
        content_length: Option<u64>,
        trailers: Option<Vec<(String, String)>>,
    ) -> PyResult<Self> {
        validate_response_status(status)?;
        if PyIterator::from_object(iterable.bind(py)).is_err() {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "response iterable must be a synchronous iterable of bytes-like chunks",
            ));
        }
        let trailers = trailers.unwrap_or_default();
        // Validate eagerly via the canonical Trailers validator so
        // misconfiguration fails before commitment (no wire bytes).
        {
            use eggserve_core::primitives::header_block::HeaderBlock;
            use eggserve_core::primitives::trailers::{TrailerLimits, Trailers};
            let mut block = HeaderBlock::new();
            for (name, value) in &trailers {
                block.push_str(name, value).map_err(|e| {
                    pyo3::exceptions::PyValueError::new_err(format!(
                        "invalid response trailer: {e}"
                    ))
                })?;
            }
            Trailers::with_limits(block, &TrailerLimits::default()).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("invalid trailers: {e}"))
            })?;
        }
        Ok(Self {
            status,
            headers: headers.unwrap_or_default(),
            body: std::sync::Mutex::new(PyResponseBody::StreamWithTrailers {
                iterable,
                content_length,
                trailers,
            }),
            extra_headers: Vec::new(),
        })
    }

    fn __repr__(&self) -> String {
        format!("<Response {}>", self.status)
    }
}

