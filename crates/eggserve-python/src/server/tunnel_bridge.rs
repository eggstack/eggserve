//! Python tunnel bridge (Plan 206 Track B).
//!
//! Owns `PyTunnelRequest`/`PyTunnelCapability`/`PyTunnel` (one-shot duplex).
//! Single owner for tunnel channel state; request/response bridges refer
//! here without duplicating conversion.

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
#[allow(unused_imports)]
use super::response_bridge::PyResponseBody;

#[pyclass(frozen, name = "TunnelRequest")]
#[derive(Debug, Clone)]
pub struct PyTunnelRequest {
    #[pyo3(get)]
    pub(super) kind: String,
    #[pyo3(get)]
    pub(super) protocol: Option<String>,
    #[pyo3(get)]
    pub(super) authority: Option<String>,
}

#[pymethods]
impl PyTunnelRequest {
    fn __repr__(&self) -> String {
        format!(
            "<TunnelRequest kind={} protocol={:?} authority={:?}>",
            self.kind, self.protocol, self.authority
        )
    }
}

/// Bound for each tunnel direction (Python <-> runtime shuttle).
///
/// Matches the response-stream bridge philosophy (16 chunks): the runtime
/// handler task blocks on a full channel (GIL released), so a slow Python
/// consumer applies backpressure without unbounded buffering. Chunk payloads
/// are bounded by the caller (tunnel `send` rejects oversized frames at
/// 64 KiB); 16 × 64 KiB = 1 MiB max per direction in flight.
pub(crate) const TUNNEL_CHANNEL_BOUND: usize = 16;
/// Maximum single tunnel frame accepted from Python (prevents one huge
/// `send` from exhausting memory; runtime `TunnelIo` duplex is 32 KiB, so
/// larger Python frames are chunked by the shuttle, not buffered whole).
pub(crate) const TUNNEL_MAX_FRAME_BYTES: usize = 64 * 1024;

#[pyclass(frozen, name = "TunnelCapability")]
pub struct PyTunnelCapability {
    pub(super) inner: Arc<std::sync::Mutex<Option<eggserve_core::primitives::tunnel::TunnelCapability>>>,
    pub(super) request: eggserve_core::primitives::tunnel::TunnelRequest,
    pub(super) lifecycle: Option<eggserve_core::primitives::request_lifecycle::RequestLifecycle>,
    pub(super) handle: Option<tokio::runtime::Handle>,
    pub(super) handshake_slot: Arc<std::sync::Mutex<Option<CanonicalResponse>>>,
}

impl std::fmt::Debug for PyTunnelCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PyTunnelCapability")
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}

#[pymethods]
impl PyTunnelCapability {
    #[getter]
    fn kind(&self) -> String {
        self.request.kind().to_string()
    }

    #[getter]
    fn protocol(&self) -> Option<String> {
        self.request.protocol().map(|p| p.as_str().to_owned())
    }

    #[getter]
    fn authority(&self) -> Option<String> {
        self.request.authority().map(|a| a.as_str().to_owned())
    }

    /// Accept the tunnel with validated handshake headers.
    ///
    /// One-shot: second call fails (already accepted/taken). Validates via
    /// the canonical `TunnelCapability::accept` (framing rejected,
    /// hop-by-hop stripped, bounded 32 fields / 8 KiB; H1 `101` vs
    /// `CONNECT`/`Extended` `200` selected by the runtime; runtime owns
    /// transition/framing bytes, no raw socket). Returns `(handshake,
    /// tunnel)`: `handshake` is the `Response` the handler must return as
    /// its final service result (denial stays ordinary HTTP by returning a
    /// normal `Response` without calling `accept`); `tunnel` is the bounded
    /// single-owner duplex for the downstream codec (no WebSocket framing
    /// in EggServe). Python never receives Hyper/h2/h3/Quinn objects.
    #[pyo3(signature = (headers=None))]
    fn accept(
        &self,
        py: Python<'_>,
        headers: Option<Vec<(String, String)>>,
    ) -> PyResult<(PyResponse, PyTunnel)> {
        use eggserve_core::primitives::header_block::HeaderBlock;

        let mut slot = self
            .inner
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("tunnel lock poisoned"))?;
        let capability = slot.take().ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err("tunnel already accepted or taken")
        })?;
        drop(slot);

        let mut block = HeaderBlock::new();
        for (name, value) in headers.unwrap_or_default() {
            block.push_str(name, value).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("invalid tunnel header: {e}"))
            })?;
        }

        let lifecycle = self.lifecycle.clone().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("no lifecycle for tunnel")
        })?;
        let shuttle_lifecycle = lifecycle.clone();
        // Bounded duplex shuttle: runtime `TunnelIo` <-> Python channels.
        // `to_python`: runtime reads from transport, Python `recv`s.
        // `to_runtime`: Python `send`s, runtime writes to transport.
        let (to_python_tx, to_python_rx) =
            mpsc::channel::<Result<Vec<u8>, String>>(TUNNEL_CHANNEL_BOUND);
        let (to_runtime_tx, mut to_runtime_rx) =
            mpsc::channel::<Vec<u8>>(TUNNEL_CHANNEL_BOUND);
        let to_python_rx = Arc::new(std::sync::Mutex::new(Some(to_python_rx)));
        let to_runtime_tx = Arc::new(std::sync::Mutex::new(Some(to_runtime_tx)));

        let tunnel = PyTunnel {
            to_python_rx: Arc::clone(&to_python_rx),
            to_runtime_tx: Arc::clone(&to_runtime_tx),
            lifecycle: Some(lifecycle.clone()),
            handle: self.handle.clone(),
            closed: Arc::new(AtomicBool::new(false)),
        };

        // Downstream handler owns `TunnelIo` + lifecycle, no raw transport.
        // Shuttle loop: transport -> Python channel, Python channel ->
        // transport, with lifecycle cancellation waking idle waits. No
        // payload bytes logged; failures are sanitized (truncation/close).
        let handler = move |mut io: eggserve_core::primitives::tunnel::TunnelIo,
                            lc: eggserve_core::primitives::request_lifecycle::RequestLifecycle| async move {
            let mut buf = vec![0u8; 32 * 1024];
            loop {
                tokio::select! {
                    biased;
                    _ = lc.cancelled() => break,
                    _ = shuttle_lifecycle.cancelled() => break,
                    read = tokio::io::AsyncReadExt::read(&mut io, &mut buf) => {
                        match read {
                            Ok(0) => break,
                            Ok(n) => {
                                let chunk = buf[..n].to_vec();
                                if to_python_tx.send(Ok(chunk)).await.is_err() {
                                    break;
                                }
                            }
                            Err(_) => {
                                let _ = to_python_tx.send(Err("transport read failed".to_string())).await;
                                break;
                            }
                        }
                    }
                    chunk = to_runtime_rx.recv() => {
                        match chunk {
                            Some(data) => {
                                if tokio::io::AsyncWriteExt::write_all(&mut io, &data).await.is_err() {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
            // Best-effort shutdown; Python `recv` observes EOF via channel close.
            let _ = tokio::io::AsyncWriteExt::shutdown(&mut io).await;
        };

        let handshake = capability.accept(block, handler).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("tunnel accept failed: {e}"))
        })?;
        // Publish the runtime-owned handshake (with `TunnelAcceptance`) for
        // the service-layer post-return check. The Python-visible handshake
        // copy carries the same status/headers for the handler to return;
        // the stored canonical response (with acceptance) wins after return.
        let status = handshake.status().as_u16();
        let mut headers_map = HashMap::new();
        let mut extra: Vec<(String, String)> = Vec::new();
        for field in handshake.head().headers().iter() {
            // `HeaderValue::to_str` is fallible (opaque); handshake headers
            // are app-supplied text validated above, so lossy fallback never
            // triggers for accepted handshakes (defensive: skip opaque).
            if let Ok(text) = field.value.to_str() {
                extra.push((field.name.to_string(), text.to_owned()));
            }
        }
        // `headers` dict is first-wins for compatibility; ordered extras
        // preserve duplicates for the canonical conversion below.
        for (n, v) in &extra {
            headers_map.entry(n.to_ascii_lowercase()).or_insert_with(|| v.clone());
        }
        {
            let mut slot = self.handshake_slot.lock().map_err(|_| {
                pyo3::exceptions::PyRuntimeError::new_err("handshake lock poisoned")
            })?;
            *slot = Some(handshake);
        }
        let py_handshake = PyResponse {
            status,
            headers: headers_map,
            body: std::sync::Mutex::new(PyResponseBody::Empty),
            extra_headers: extra,
        };
        let _ = py;
        Ok((py_handshake, tunnel))
    }

    fn __repr__(&self) -> String {
        format!("<TunnelCapability kind={}>", self.request.kind())
    }
}

#[pyclass(frozen, name = "Tunnel")]
pub struct PyTunnel {
    to_python_rx:
        Arc<std::sync::Mutex<Option<mpsc::Receiver<Result<Vec<u8>, String>>>>>,
    pub(super) to_runtime_tx: Arc<std::sync::Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    pub(super) lifecycle: Option<eggserve_core::primitives::request_lifecycle::RequestLifecycle>,
    pub(super) handle: Option<tokio::runtime::Handle>,
    pub(super) closed: Arc<AtomicBool>,
}

impl std::fmt::Debug for PyTunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PyTunnel").finish_non_exhaustive()
    }
}

#[pymethods]
impl PyTunnel {
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
            || self
                .lifecycle
                .as_ref()
                .is_some_and(|lc| lc.is_cancelled())
    }

    /// Blocking receive (GIL released during wait).
    ///
    /// Returns `bytes` on data, `None` on orderly EOF/close/cancel. Transport
    /// failures raise `ConnectionError` with sanitized text (no payload).
    /// Intended for use via `asyncio.to_thread` in async handlers so the
    /// event loop stays responsive; direct calls block the caller.
    fn recv<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyBytes>>> {
        let handle = self.handle.clone().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("no runtime handle for tunnel recv")
        })?;
        // Take the receiver out for one blocking wait, then put back unless
        // terminal. `std` Mutex guard cannot be held across `block_on`.
        let mut rx = {
            let mut guard = self
                .to_python_rx
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("tunnel lock poisoned"))?;
            guard.take().ok_or_else(|| {
                pyo3::exceptions::PyRuntimeError::new_err("tunnel already closed")
            })?
        };
        let result = py.allow_threads(|| handle.block_on(rx.recv()));
        match result {
            Some(Ok(chunk)) => {
                // Not terminal: return receiver for future recvs.
                if let Ok(mut guard) = self.to_python_rx.lock() {
                    *guard = Some(rx);
                }
                Ok(Some(PyBytes::new(py, &chunk)))
            }
            Some(Err(msg)) => {
                self.closed.store(true, Ordering::Release);
                Err(pyo3::exceptions::PyConnectionError::new_err(msg))
            }
            None => {
                self.closed.store(true, Ordering::Release);
                Ok(None)
            }
        }
    }

    /// Blocking send (GIL released during backpressure wait).
    ///
    /// Bounded: blocks when 16 chunks are in flight (GIL released), so slow
    /// transport applies backpressure without unbounded buffering.
    /// Disconnect/cancel raises `ConnectionError`. Use via
    /// `asyncio.to_thread` in async handlers.
    fn send(&self, py: Python<'_>, data: Vec<u8>) -> PyResult<()> {
        if data.len() > TUNNEL_MAX_FRAME_BYTES {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "tunnel frame {} exceeds {} bytes",
                data.len(),
                TUNNEL_MAX_FRAME_BYTES
            )));
        }
        if self.is_closed() {
            return Err(pyo3::exceptions::PyConnectionError::new_err(
                "tunnel is closed",
            ));
        }
        let tx = {
            let guard = self
                .to_runtime_tx
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("tunnel lock poisoned"))?;
            guard.clone().ok_or_else(|| {
                pyo3::exceptions::PyRuntimeError::new_err("tunnel already closed")
            })?
        };
        py.allow_threads(|| tx.blocking_send(data)).map_err(|_| {
            self.closed.store(true, Ordering::Release);
            pyo3::exceptions::PyConnectionError::new_err("tunnel closed during send")
        })
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        // Dropping the sender signals EOF to the shuttle (which then shuts
        // down the transport side). Receiver drop signals the shuttle to
        // stop forwarding transport bytes.
        if let Ok(mut guard) = self.to_runtime_tx.lock() {
            guard.take();
        }
        if let Ok(mut guard) = self.to_python_rx.lock() {
            guard.take();
        }
    }

    fn __repr__(&self) -> String {
        format!("<Tunnel closed={}>", self.is_closed())
    }
}

