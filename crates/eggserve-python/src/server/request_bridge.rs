//! Python request bridge (Plan 206 Track B).
//!
//! Owns `PyRequest` (byte-fidelity accessors, interim/tunnel capability
//! getters, lifecycle observers). Body content lives in `body_bridge`;
//! tunnels live in `tunnel_bridge`.

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
use super::body_bridge::{PyBodyChunkIterator, PyRequestBody};
#[allow(unused_imports)]
use super::tunnel_bridge::PyTunnelCapability;

#[pyclass(frozen, name = "Request")]
#[derive(Debug, Clone)]
pub struct PyRequest {
    #[pyo3(get)]
    pub(super) method: String,
    #[pyo3(get)]
    pub(super) path: String,
    #[pyo3(get)]
    pub(super) query: String,
    /// First-wins semantics; for duplicate-sensitive headers use `header_items`.
    #[pyo3(get)]
    pub(super) headers: HashMap<String, String>,
    pub(super) header_items: Vec<(String, String)>,
    #[pyo3(get)]
    pub(super) remote_addr: Option<String>,
    #[pyo3(get)]
    pub(super) remote_address: Option<(String, u16)>,
    #[pyo3(get)]
    pub(super) local_addr: Option<String>,
    #[pyo3(get)]
    pub(super) local_address: Option<(String, u16)>,
    #[pyo3(get)]
    pub(super) scheme: Option<String>,
    #[pyo3(get)]
    pub(super) http_version: String,
    #[pyo3(get)]
    pub(super) body: Option<PyRequestBody>,
    /// Final effective client (trusted PROXY/header override if accepted,
    /// else raw peer). `remote_addr`/`remote_address` never change for
    /// compatibility; read these for downstream decisions. (Plan 202)
    #[pyo3(get)]
    pub(super) effective_addr: Option<String>,
    #[pyo3(get)]
    pub(super) effective_address: Option<(String, u16)>,
    /// Final effective scheme (trusted override if accepted, else `scheme`).
    #[pyo3(get)]
    pub(super) effective_scheme: Option<String>,
    /// Trusted external authority override, if accepted. `None` means use
    /// the canonical Host/target (never silently rewritten).
    #[pyo3(get)]
    pub(super) effective_authority: Option<String>,
    /// PROXY preamble source kind (`proxy_v1`/`proxy_v2`), if accepted.
    #[pyo3(get)]
    pub(super) proxy_provenance: Option<String>,
    /// Header family source kind (`forwarded`/`legacy_forwarded`), if accepted.
    #[pyo3(get)]
    pub(super) forwarded_provenance: Option<String>,
    // Plan 204 async substrate (additive, sync facade behavior unchanged):
    // byte-fidelity views + transport-authenticated metadata + capability
    // handles. Stored at construction from the canonical head/context so
    // async handlers observe the same values without re-parsing.
    pub(super) raw_target_bytes: Vec<u8>,
    pub(super) path_bytes: Vec<u8>,
    pub(super) query_bytes: Option<Vec<u8>>,
    pub(super) header_items_bytes: Vec<(Vec<u8>, Vec<u8>)>,
    pub(super) authority: Option<String>,
    pub(super) tls_protocol_version: Option<String>,
    pub(super) tls_server_name: Option<String>,
    pub(super) tls_alpn: Option<String>,
    pub(super) client_authenticated: bool,
    pub(super) peer_certificates_present: bool,
    pub(super) proxy_source: Option<String>,
    pub(super) proxy_destination: Option<String>,
    // Tokio handle for blocking lifecycle waits (GIL released during wait).
    pub(super) handle: Option<tokio::runtime::Handle>,
    // Cloned lifecycle/interim for disconnect observation + bounded 1xx.
    // `RequestLifecycle`/`InterimSender` are Arc-backed small handles.
    pub(super) lifecycle: Option<eggserve_core::primitives::request_lifecycle::RequestLifecycle>,
    pub(super) interim: Option<eggserve_core::primitives::interim::InterimSender>,
    // One-shot tunnel slot shared with the runtime context (taking via one
    // clone removes for all; second take returns None). `None` when the
    // request is not a validated upgrade/CONNECT/Extended CONNECT.
    pub(super) tunnel_slot:
        Option<Arc<std::sync::Mutex<Option<eggserve_core::primitives::tunnel::TunnelCapability>>>>,
    // Handshake slot populated by `accept_tunnel` (one-shot). After the
    // Python handler returns, the service layer takes this instead of the
    // converted `Response` so the runtime-owned `TunnelAcceptance` (with
    // transport upgrade + duplex handler) survives the Python boundary.
    pub(super) tunnel_handshake:
        Arc<std::sync::Mutex<Option<eggserve_core::primitives::canonical::Response>>>,
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

    // -- Plan 204 byte-fidelity views (additive; text facade above unchanged) --

    #[getter]
    fn raw_target_bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.raw_target_bytes)
    }

    #[getter]
    fn path_bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.path_bytes)
    }

    #[getter]
    fn query_bytes<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        self.query_bytes
            .as_ref()
            .map(|q| PyBytes::new(py, q))
    }

    #[getter]
    fn header_items_bytes(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.header_items_bytes.clone()
    }

    #[getter]
    fn authority(&self) -> Option<String> {
        self.authority.clone()
    }

    #[getter]
    fn tls_protocol_version(&self) -> Option<String> {
        self.tls_protocol_version.clone()
    }

    #[getter]
    fn tls_server_name(&self) -> Option<String> {
        self.tls_server_name.clone()
    }

    #[getter]
    fn tls_alpn(&self) -> Option<String> {
        self.tls_alpn.clone()
    }

    #[getter]
    fn client_authenticated(&self) -> bool {
        self.client_authenticated
    }

    #[getter]
    fn peer_certificates_present(&self) -> bool {
        self.peer_certificates_present
    }

    #[getter]
    fn proxy_source(&self) -> Option<String> {
        self.proxy_source.clone()
    }

    #[getter]
    fn proxy_destination(&self) -> Option<String> {
        self.proxy_destination.clone()
    }

    // -- Plan 204 lifecycle observer (transport-neutral, bounded) --

    fn is_disconnected(&self) -> bool {
        self.lifecycle
            .as_ref()
            .is_some_and(|lc| lc.is_cancelled())
    }

    fn cancellation_reason(&self) -> Option<String> {
        self.lifecycle.as_ref().and_then(|lc| {
            lc.cancellation_reason().map(|r| match r {
                eggserve_core::primitives::request_lifecycle::RequestCancellationReason::PeerDisconnected => {
                    "peer_disconnected".to_string()
                }
                eggserve_core::primitives::request_lifecycle::RequestCancellationReason::ServerShutdown => {
                    "server_shutdown".to_string()
                }
                eggserve_core::primitives::request_lifecycle::RequestCancellationReason::ConnectionTimeout => {
                    "connection_timeout".to_string()
                }
                eggserve_core::primitives::request_lifecycle::RequestCancellationReason::TransportFailure => {
                    "transport_failure".to_string()
                }
                // `RequestCancellationReason` is `#[non_exhaustive]`; future
                // variants map to a stable unknown category (no leak).
                _ => "unknown".to_string(),
            })
        })
    }

    #[pyo3(signature = (timeout_secs=None))]
    fn wait_disconnected(&self, py: Python<'_>, timeout_secs: Option<f64>) -> PyResult<bool> {
        let lifecycle = self.lifecycle.clone().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("no lifecycle attached to request")
        })?;
        let handle = self.handle.clone().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("no runtime handle for lifecycle wait")
        })?;
        let timeout = timeout_secs.map(Duration::from_secs_f64);
        Ok(py.allow_threads(|| {
            handle.block_on(async {
                match timeout {
                    Some(d) => tokio::time::timeout(d, lifecycle.cancelled()).await.is_ok(),
                    None => {
                        lifecycle.cancelled().await;
                        true
                    }
                }
            })
        }))
    }

    // -- Plan 204 bounded interim (1xx) sender (native enforcement) --

    #[pyo3(signature = (status, headers=None))]
    fn send_interim(
        &self,
        status: u16,
        headers: Option<Vec<(String, String)>>,
    ) -> PyResult<String> {
        use eggserve_core::primitives::canonical::StatusCode;
        use eggserve_core::primitives::header_block::HeaderBlock;

        let sender = self.interim.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("no interim sender attached to request")
        })?;
        let code = StatusCode::new(status).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("invalid interim status: {e}"))
        })?;
        let mut block = HeaderBlock::new();
        for (name, value) in headers.unwrap_or_default() {
            block.push_str(name, value).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("invalid interim header: {e}"))
            })?;
        }
        // Native enforcement: 1xx-only (no 101), no body/trailers, no
        // post-commit, bounded count/bytes, HTTP/1.0 suppressed, single 100.
        // Python cannot bypass ordering/bounds; failures are sanitized.
        sender
            .send(code, block)
            .map(|d| {
                if d.is_sent() {
                    "sent".to_string()
                } else {
                    "suppressed".to_string()
                }
            })
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
    }

    // -- Plan 204 one-shot tunnel take (presence only; accept is separate) --

    fn has_tunnel(&self) -> bool {
        if let Some(slot) = self.tunnel_slot.as_ref() {
            slot.lock().ok().is_some_and(|g| g.is_some())
        } else {
            false
        }
    }

    fn tunnel_request(&self) -> Option<PyTunnelRequest> {
        let slot = self.tunnel_slot.as_ref()?;
        let guard = slot.lock().ok()?;
        let cap = guard.as_ref()?;
        let req = cap.request();
        Some(PyTunnelRequest {
            kind: req.kind().to_string(),
            protocol: req.protocol().map(|p| p.as_str().to_owned()),
            authority: req.authority().map(|a| a.as_str().to_owned()),
        })
    }

    fn take_tunnel(&self) -> PyResult<Option<PyTunnelCapability>> {
        let slot = self.tunnel_slot.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("no tunnel slot attached to request")
        })?;
        // Clone the Arc so the taken capability + handshake slot stay shared
        // with the service-layer post-return check (which owns a clone via
        // `tunnel_handshake`). Taking here drains the Python-owned slot
        // one-shot; second take returns None (no duplication).
        let taken = slot
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("tunnel lock poisoned"))?
            .take();
        Ok(taken.map(|cap| {
            let request = cap.request().clone();
            PyTunnelCapability {
                inner: Arc::new(std::sync::Mutex::new(Some(cap))),
                request,
                lifecycle: self.lifecycle.clone(),
                handle: self.handle.clone(),
                handshake_slot: Arc::clone(&self.tunnel_handshake),
            }
        }))
    }
}

