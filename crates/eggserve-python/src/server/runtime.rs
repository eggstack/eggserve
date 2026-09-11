//! Python server runtime (Plan 206 Track B).
//!
//! Owns `PyServer` (same native runtime as the facade, no second accept
//! loop) and lifecycle (`STARTUP_TIMEOUT`, `wait_until_running`,
//! start/stop/wait/shutdown). Static composition lives in
//! `static_responder`; callback conversion lives in `sync_handler`.
//! GIL acquisition/release sites are contained here for review.

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
use super::static_responder::{PyStaticPolicyWrapper, ServerBodySource, ServerSecureRoot};
#[allow(unused_imports)]
use super::sync_handler::PythonCallbackService;

/// Maximum time to wait for the server to reach Running state during startup.
pub(super) const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// Wait for a [`ServerHandle`] to reach `LifecycleState::Running`.
///
/// Returns `Ok(())` only after observing the `Running` state. All other
/// outcomes — timeout, `Failed`, or any non-running terminal state —
/// produce an error with a descriptive message.
///
/// The `timeout` parameter makes this testable without sleeping for the
/// production 30-second `STARTUP_TIMEOUT`.
pub(super) async fn wait_until_running(
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


/// Dropping a started `Server` without calling `stop()` (or leaving the
/// context manager) drops the native tokio runtime synchronously when the
/// last Python reference disappears; that teardown waits for in-flight
/// tasks, so interpreter shutdown or GC can stall while connections drain.
/// Always stop a running server explicitly.
#[pyclass(frozen, name = "Server")]
#[allow(dead_code)]
pub struct PyServer {
    pub(super) bind: String,
    pub(super) port: u16,
    pub(super) bind_address: SocketAddr,
    pub(super) public: bool,
    pub(super) addr: std::sync::Mutex<Option<String>>,
    pub(super) static_root: Option<std::path::PathBuf>,
    pub(super) static_policy: StaticPolicy,
    pub(super) handler: Option<std::sync::Mutex<Option<Py<PyAny>>>>,
    pub(super) handle: std::sync::Mutex<Option<ServerHandle>>,
    pub(super) runtime: std::sync::Mutex<Option<tokio::runtime::Runtime>>,
    pub(super) has_been_started: std::sync::atomic::AtomicBool,
    pub(super) starting: std::sync::atomic::AtomicBool,
    pub(super) max_connections: usize,
    pub(super) max_file_streams: usize,
    pub(super) max_python_callbacks: usize,
    pub(super) header_timeout: Duration,
    pub(super) connection_total_timeout: Duration,
    pub(super) handler_timeout: Duration,
    pub(super) graceful_shutdown_timeout: Duration,
    pub(super) body_policy: RequestBodyPolicy,
    pub(super) max_request_body_bytes: u64,
    pub(super) body_read_timeout: Duration,
    pub(super) tls_config: Option<std::sync::Arc<rustls::ServerConfig>>,
    pub(super) default_content_type: String,
    pub(super) extra_response_headers: Vec<(String, String)>,
    // Plan 164 production controls (operator-meaningful subset).
    pub(super) max_in_flight_requests: usize,
    pub(super) max_buf_size: usize,
    pub(super) max_headers: usize,
    pub(super) max_header_bytes: usize,
    pub(super) max_request_target_bytes: usize,
    pub(super) keep_alive_idle_timeout: Duration,
    pub(super) max_requests_per_connection: Option<u64>,
    pub(super) response_write_timeout: Duration,
    // Plan 165 response privacy subset (safe for Python embedding).
    pub(super) server_header: Option<String>,
    pub(super) date_suppressed: bool,
    pub(super) stripped_response_headers: Vec<String>,
    pub(super) error_empty: bool,
    // Plan 202 trusted-proxy policy (safe defaults: nothing trusted).
    pub(super) trusted_proxies: Vec<String>,
    pub(super) trust_unix_local: bool,
    pub(super) proxy_protocol: bool,
    pub(super) forwarded_standard: bool,
    pub(super) forwarded_legacy: bool,
}

#[pymethods]
impl PyServer {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (root=None, bind="127.0.0.1", port=8000, policy=None, handler=None, public=false, max_connections=64, max_file_streams=32, max_python_callbacks=8, header_timeout_secs=10, connection_total_timeout_secs=60, handler_timeout_secs=30, graceful_shutdown_timeout_secs=10, request_body_mode="reject", max_request_body_bytes=0, body_timeout_secs=30, tls_certfile=None, tls_keyfile=None, default_content_type="application/octet-stream", extra_response_headers=None, max_in_flight_requests=64, max_buf_size=65536, max_headers=100, max_header_bytes=32768, max_request_target_bytes=8192, keep_alive_idle_timeout_secs=60, max_requests_per_connection=None, response_write_timeout_secs=30, server_header=None, date_policy="system", stripped_response_headers=None, error_policy="minimal", trusted_proxies=None, trust_unix_local=false, proxy_protocol=false, forwarded_standard=false, forwarded_legacy=false))]
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
        trusted_proxies: Option<Vec<String>>,
        trust_unix_local: bool,
        proxy_protocol: bool,
        forwarded_standard: bool,
        forwarded_legacy: bool,
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

        // Plan 202 trusted-proxy policy: validate CIDR/IP literals eagerly;
        // DNS names and out-of-range prefixes fail before listener startup.
        // Loopback is not implicitly trusted; list it explicitly when needed.
        let trusted_proxies = trusted_proxies.unwrap_or_default();
        for entry in &trusted_proxies {
            eggserve_core::primitives::proxy::IpPrefix::parse(entry).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("invalid trusted_proxies entry: {e}"))
            })?;
        }

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
            trusted_proxies,
            trust_unix_local,
            proxy_protocol,
            forwarded_standard,
            forwarded_legacy,
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
            trusted_proxies,
            trust_unix_local,
            proxy_protocol,
            forwarded_standard,
            forwarded_legacy,
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
                this.trusted_proxies.clone(),
                this.trust_unix_local,
                this.proxy_protocol,
                this.forwarded_standard,
                this.forwarded_legacy,
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
        // Plan 202 trusted-proxy policy: explicit peers/CIDRs (no DNS),
        // Unix local-trust flag, PROXY preamble mode, and header-derived
        // forwarding switches. Defaults trust nothing; `remote_addr` never
        // changes for compatibility, effective values are separate getters.
        {
            use eggserve_core::primitives::proxy::TrustedProxyConfig;
            let mut proxy_config = TrustedProxyConfig::default();
            for entry in &trusted_proxies {
                let prefix =
                    eggserve_core::primitives::proxy::IpPrefix::parse(entry).map_err(|e| {
                        pyo3::exceptions::PyValueError::new_err(format!(
                            "invalid trusted_proxies entry: {e}"
                        ))
                    })?;
                proxy_config.peers.push(prefix);
            }
            proxy_config.trust_unix = trust_unix_local;
            proxy_config.proxy_protocol.enabled = proxy_protocol;
            proxy_config.forwarded.standard_enabled = forwarded_standard;
            proxy_config.forwarded.legacy_enabled = forwarded_legacy;
            runtime_builder = runtime_builder.trusted_proxy(proxy_config);
        }
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
