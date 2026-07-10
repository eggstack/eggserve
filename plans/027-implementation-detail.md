# Plan 027: Detailed Implementation Steps

## Overview

This document provides the step-by-step implementation plan for Plan 027: Rust-Owned Python Server Primitives. It specifies exact file changes, function signatures, code structure, and validation steps.

## Architecture

### Module layout

```
crates/eggserve-python/
├── Cargo.toml              # add tokio, hyper, hyper-util, http-body-util, bytes, futures-util
├── src/
│   ├── lib.rs              # existing (994 lines) — add new module + re-exports
│   └── server.rs           # NEW: ~600-800 lines — PyRequest, PyResponse, StaticResponder, PyServer, ServerConfig
└── python/eggserve/
    ├── __init__.py         # add new exports
    ├── server.py           # extend with ServerConfig and serve_directory() update
    ├── test_server_primitives.py  # NEW: ~400 lines — tests for in-process server
    └── ...
```

### Data flow

```
Python handler
     ↑ (GIL + spawn_blocking)
     |
PyServer.serve_forever()
     ↓
tokio runtime
     ↓
TcpListener::bind → accept loop → connection semaphore
     ↓
http1::Builder → serve_connection → service_fn
     ↓
handle_request (adapted) → construct PyRequest → call Python handler → get PyResponse
     ↓
PyResponse → hyper Response<BoxBodyInner> → serialize to socket
```

## Step 1: Update `crates/eggserve-python/Cargo.toml`

Add dependencies needed for the server loop. These mirror what `eggserve-bin` uses:

```toml
[dependencies]
eggserve-core = { path = "../eggserve-core" }
pyo3 = { version = "0.24", features = ["extension-module"] }
tokio = { version = "1", features = ["rt", "net", "time", "signal", "sync"] }
hyper = { version = "1", features = ["http1", "server"] }
hyper-util = { version = "0.1", features = ["tokio", "http1"] }
http-body-util = { version = "0.1" }
bytes = { version = "1" }
futures-util = { version = "0.3" }
```

**Rationale**: The server needs tokio for the async runtime and TcpListener, hyper for HTTP/1.1 parsing, hyper-util for TokioIo/TokioTimer adapters, http-body-util for body types, bytes for buffer management, and futures-util for streaming body unfold.

## Step 2: Create `crates/eggserve-python/src/server.rs`

This is the core new file. It contains all PyO3 types for the server primitives.

### 2.1 Imports and module structure

```rust
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::BodyExt;
use http_body_util::{Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, OwnedSemaphorePermit, Semaphore};

use eggserve_core::config::{ServeConfig as CoreServeConfig, ServeState};
use eggserve_core::policy::StaticPolicy as RustStaticPolicy;
use eggserve_core::primitives::body::BodySource;
use eggserve_core::primitives::http::ReadOnlyMethod;
use eggserve_core::primitives::planner::plan_file_response;
use eggserve_core::primitives::response::{HeaderMapPlan, StaticResponsePlan};
use eggserve_core::primitives::{ConfinedPath, PathPolicy, SecureRoot as RustSecureRoot};
use eggserve_core::response::BoxBodyInner;

use crate::{PyStaticPolicy, PyBodySource};
```

### 2.2 `ServerConfig` (frozen pyclass)

Configuration with safe defaults matching CLI. Python-facing name: `ServerConfig`.

```rust
#[pyclass(name = "ServerConfig", frozen)]
#[allow(dead_code)]
pub struct PyServerConfig {
    bind: String,
    port: u16,
    public: bool,
    policy: PyStaticPolicy,
    max_connections: usize,
    max_file_streams: usize,
}

#[pymethods]
impl PyServerConfig {
    #[new]
    #[pyo3(signature = (*, bind="127.0.0.1", port=8000, public=false, policy=None, max_connections=64, max_file_streams=32))]
    fn py_new(
        bind: &str,
        port: u16,
        public: bool,
        policy: Option<PyStaticPolicy>,
        max_connections: usize,
        max_file_streams: usize,
    ) -> PyResult<Self> {
        // Validate port range
        if port == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "port must be between 1 and 65535",
            ));
        }
        // Validate public bind
        let is_loopback = bind == "127.0.0.1" || bind == "::1" || bind == "localhost";
        if !public && !is_loopback {
            return Err(pyo3::exceptions::PyValueError::new_err(
                format!("binding to {bind} requires public=True to acknowledge public exposure intent"),
            ));
        }
        Ok(Self {
            bind: bind.to_string(),
            port,
            public,
            policy: policy.unwrap_or_else(|| PyStaticPolicy {
                inner: RustStaticPolicy::safe_default(),
            }),
            max_connections,
            max_file_streams,
        })
    }

    #[getter]
    fn bind(&self) -> &str { &self.bind }

    #[getter]
    fn port(&self) -> u16 { self.port }

    #[getter]
    fn public(&self) -> bool { self.public }

    #[getter]
    fn policy(&self) -> PyStaticPolicy { self.policy.clone() }

    #[getter]
    fn max_connections(&self) -> usize { self.max_connections }

    #[getter]
    fn max_file_streams(&self) -> usize { self.max_file_streams }
}
```

### 2.3 `Request` (frozen pyclass)

Immutable request metadata exposed to Python.

```rust
#[pyclass(name = "Request", frozen)]
#[allow(dead_code)]
pub struct PyRequest {
    method: String,
    target: String,
    path: String,
    query: Option<String>,
    headers: Vec<(String, String)>,
    remote_addr: Option<String>,
    http_version: String,
    has_body: bool,
}

#[pymethods]
impl PyRequest {
    #[getter]
    fn method(&self) -> &str { &self.method }

    #[getter]
    fn target(&self) -> &str { &self.target }

    #[getter]
    fn path(&self) -> &str { &self.path }

    #[getter]
    fn query(&self) -> Option<&str> { self.query.as_deref() }

    #[getter]
    fn headers(&self, py: Python<'_>) -> PyResult<PyObject> {
        let list = PyList::empty(py);
        for (name, value) in &self.headers {
            let tup = PyTuple::new(py, [name.as_str(), value.as_str()])?;
            list.append(tup)?;
        }
        Ok(list.into_any().unbind())
    }

    #[getter]
    fn remote_addr(&self) -> Option<&str> { self.remote_addr.as_deref() }

    #[getter]
    fn http_version(&self) -> &str { &self.http_version }

    #[getter]
    fn has_body(&self) -> bool { self.has_body }

    fn __repr__(&self) -> String {
        format!("Request({} {})", self.method, self.path)
    }

    fn __str__(&self) -> String {
        format!("{} {} HTTP/{}", self.method, self.target, self.http_version)
    }
}

impl PyRequest {
    fn from_hyper<B>(req: &Request<B>, remote_addr: Option<SocketAddr>) -> Self {
        let method = req.method().as_str().to_string();
        let target = req.uri().path_and_query()
            .map(|pq| pq.as_str().to_string())
            .unwrap_or_else(|| req.uri().path().to_string());
        let path = req.uri().path().to_string();
        let query = req.uri().query().map(|q| q.to_string());

        let mut headers = Vec::new();
        for (name, value) in req.headers().iter() {
            if let Ok(v) = value.to_str() {
                headers.push((name.as_str().to_string(), v.to_string()));
            }
        }

        let has_body = req.headers().get(hyper::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map_or(false, |len| len > 0);

        let version = match req.version() {
            hyper::Version::HTTP_10 => "1.0",
            hyper::Version::HTTP_11 => "1.1",
            _ => "1.1",
        };

        let remote = remote_addr.map(|a| a.to_string());

        PyRequest {
            method,
            target,
            path,
            query,
            headers,
            remote_addr: remote,
            http_version: version.to_string(),
            has_body,
        }
    }
}
```

### 2.4 `Response` (frozen pyclass)

Immutable response object with factory methods.

```rust
#[pyclass(name = "Response", frozen)]
#[allow(dead_code)]
pub struct PyResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: PyResponseBody,
}

#[derive(Clone)]
enum PyResponseBody {
    Empty,
    Bytes(Vec<u8>),
    BodySource(PyBodySource),
}

#[pymethods]
impl PyResponse {
    #[staticmethod]
    #[pyo3(signature = (*, status=204, headers=None))]
    fn empty(status: u16, headers: Option<Vec<(String, String)>>) -> PyResult<Self> {
        validate_status(status)?;
        let hdrs = validate_headers(headers.unwrap_or_default())?;
        Ok(Self { status, headers: hdrs, body: PyResponseBody::Empty })
    }

    #[staticmethod]
    #[pyo3(signature = (data, *, status=200, headers=None, content_type=None))]
    fn bytes(
        data: Vec<u8>,
        status: u16,
        headers: Option<Vec<(String, String)>>,
        content_type: Option<&str>,
    ) -> PyResult<Self> {
        validate_status(status)?;
        let mut hdrs = validate_headers(headers.unwrap_or_default())?;
        if let Some(ct) = content_type {
            // Content-Type from argument takes precedence
            hdrs.retain(|(n, _)| !n.eq_ignore_ascii_case("content-type"));
            hdrs.push(("content-type".to_string(), ct.to_string()));
        }
        Ok(Self { status, headers: hdrs, body: PyResponseBody::Bytes(data) })
    }

    #[staticmethod]
    #[pyo3(signature = (text, *, status=200, headers=None, content_type="text/plain; charset=utf-8"))]
    fn text(
        text: String,
        status: u16,
        headers: Option<Vec<(String, String)>>,
        content_type: &str,
    ) -> PyResult<Self> {
        Self::bytes(text.into_bytes(), status, headers, Some(content_type))
    }

    #[staticmethod]
    #[pyo3(signature = (body, *, status=200, headers=None))]
    fn body_source(
        body: PyBodySource,
        status: u16,
        headers: Option<Vec<(String, String)>>,
    ) -> PyResult<Self> {
        validate_status(status)?;
        let hdrs = validate_headers(headers.unwrap_or_default())?;
        Ok(Self { status, headers: hdrs, body: PyResponseBody::BodySource(body) })
    }

    #[getter]
    fn status(&self) -> u16 { self.status }

    #[getter]
    fn headers(&self) -> Vec<(String, String)> { self.headers.clone() }

    #[getter]
    fn body_kind(&self) -> &str {
        match &self.body {
            PyResponseBody::Empty => "empty",
            PyResponseBody::Bytes(_) => "bytes",
            PyResponseBody::BodySource(_) => "body_source",
        }
    }

    fn __repr__(&self) -> String {
        format!("Response(status={}, body_kind={:?})", self.status, self.body_kind())
    }
}
```

Validation helpers:

```rust
fn validate_status(status: u16) -> PyResult<()> {
    if status < 100 || status > 599 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("invalid status code: {status}"),
        ));
    }
    Ok(())
}

fn validate_headers(headers: Vec<(String, String)>) -> PyResult<Vec<(String, String)>> {
    for (name, value) in &headers {
        if name.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "header name must not be empty",
            ));
        }
        // Reject control characters in header values
        if value.bytes().any(|b| b < 0x20 || b == 0x7f) {
            return Err(pyo3::exceptions::PyValueError::new_err(
                format!("header value for {name} contains invalid control characters"),
            ));
        }
    }
    Ok(headers)
}
```

### 2.5 `StaticResponder` (pyclass)

Wraps SecureRoot + response planning. The first pass supports file-only static responses.

```rust
#[pyclass(name = "StaticResponder")]
#[allow(dead_code)]
pub struct PyStaticResponder {
    root: RustSecureRoot,
}

#[pymethods]
impl PyStaticResponder {
    #[new]
    #[pyo3(signature = (directory, policy=None))]
    fn py_new(directory: &str, policy: Option<PyStaticPolicy>) -> PyResult<Self> {
        let static_policy = policy
            .map(|p| p.inner)
            .unwrap_or_else(RustStaticPolicy::safe_default);
        let root = RustSecureRoot::new(directory, static_policy)
            .map_err(|e| crate::SecureRootError::new_err((e.to_string(), "io_error")))?;
        Ok(Self { root })
    }

    fn respond(&self, py: Python<'_>, request: &PyRequest) -> PyResult<PyResponse> {
        // Only GET and HEAD
        let method = match request.method.as_str() {
            "GET" => ReadOnlyMethod::Get,
            "HEAD" => ReadOnlyMethod::Head,
            _ => {
                return Ok(PyResponse {
                    status: 405,
                    headers: vec![("allow".to_string(), "GET, HEAD".to_string())],
                    body: PyResponseBody::Empty,
                });
            }
        };

        // Reject bodies on GET/HEAD (matches CLI behavior)
        // ...

        // Resolve path
        let path_policy = PathPolicy {
            dotfiles: match self.root.policy().dotfiles {
                eggserve_core::policy::DotfilePolicy::Denied => PathPolicy::default().dotfiles,
                eggserve_core::policy::DotfilePolicy::Serve => {
                    eggserve_core::path::DotfilePolicy::Allow
                }
            },
            reject_backslash: true,
        };

        let confined = match ConfinedPath::parse(&request.path, &path_policy) {
            Ok(p) => p,
            Err(rejection) => {
                return Ok(map_path_rejection_to_response(rejection));
            }
        };

        let resolved = self.root.resolve(&confined);

        match resolved {
            eggserve_core::primitives::ResolvedResource::File(file) => {
                // Plan response (conditional, range, etc.)
                let if_none_match = request.header_value("if-none-match");
                let if_modified_since = request.header_value("if-modified-since");
                let range = request.header_value("range");
                let if_range = request.header_value("if-range");

                let plan = plan_file_response(
                    method,
                    &file.metadata(),
                    &file.content_type(),
                    if_none_match,
                    if_modified_since,
                    range,
                    if_range,
                );

                let mut headers: Vec<(String, String)> = plan.headers.iter()
                    .map(|h| (h.name.clone(), h.value.clone()))
                    .collect();

                let body_source = file.into_body(&plan)
                    .map_err(|e| {
                        pyo3::exceptions::PyRuntimeError::new_err(
                            format!("body source error: {e}")
                        )
                    })?;

                let py_body = PyBodySource { inner: body_source };

                Ok(PyResponse {
                    status: plan.status_code(),
                    headers,
                    body: PyResponseBody::BodySource(py_body),
                })
            }
            eggserve_core::primitives::ResolvedResource::Directory(_dir) => {
                // For first pass: check for index.html, else 403
                // (full directory listing support can be added later)
                Ok(PyResponse {
                    status: 403,
                    headers: vec![],
                    body: PyResponseBody::Empty,
                })
            }
            eggserve_core::primitives::ResolvedResource::NotFound => {
                Ok(PyResponse {
                    status: 404,
                    headers: vec![],
                    body: PyResponseBody::Empty,
                })
            }
            eggserve_core::primitives::ResolvedResource::Denied(_) => {
                Ok(PyResponse {
                    status: 403,
                    headers: vec![],
                    body: PyResponseBody::Empty,
                })
            }
        }
    }
}
```

### 2.6 `Server` (pyclass) — the main server lifecycle

```rust
#[pyclass(name = "Server")]
#[allow(dead_code)]
pub struct PyServer {
    config: PyServerConfig,
    handler: PyObject,
    shutdown_tx: std::sync::Mutex<Option<broadcast::Sender<()>>>,
    bound_addr: std::sync::Mutex<Option<SocketAddr>>,
}
```

Methods:

```rust
#[pymethods]
impl PyServer {
    #[new]
    fn py_new(config: PyServerConfig, handler: PyObject) -> Self {
        Self {
            config,
            handler,
            shutdown_tx: std::sync::Mutex::new(None),
            bound_addr: std::sync::Mutex::new(None),
        }
    }

    #[getter]
    fn addr(&self) -> PyResult<Option<String>> {
        let guard = self.bound_addr.lock().map_err(|_| {
            pyo3::exceptions::PyRuntimeError::new_err("lock poisoned")
        })?;
        Ok(guard.map(|a| a.to_string()))
    }

    fn serve_forever(&self, py: Python<'_>) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(
                format!("failed to create tokio runtime: {e}")
            ))?;

        rt.block_on(self._run_server(py))
    }

    fn start(&self, py: Python<'_>) -> PyResult<()> {
        // Start server in a background thread
        let config = self.config.clone();
        let handler = self.handler.clone_ref(py);
        let (shutdown_tx, _) = broadcast::channel::<()>(1);

        *self.shutdown_tx.lock().unwrap() = Some(shutdown_tx.clone());

        let bound_addr = self.bound_addr.clone();
        let shutdown_rx = shutdown_tx.subscribe();

        // We need to handle the GIL properly in the background thread
        // Use py.allow_threads or spawn_blocking
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                // ... accept loop ...
            });
        });

        Ok(())
    }

    fn stop(&self) -> PyResult<()> {
        let guard = self.shutdown_tx.lock().map_err(|_| {
            pyo3::exceptions::PyRuntimeError::new_err("lock poisoned")
        })?;
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(());
        }
        Ok(())
    }

    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    fn __exit__(
        &self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        self.stop()?;
        Ok(false)
    }
}
```

### 2.7 Internal server loop

The core accept loop, adapted from `eggserve-bin/src/lib.rs`:

```rust
impl PyServer {
    async fn _run_server(&self, py: Python<'_>) -> PyResult<()> {
        let addr: SocketAddr = format!("{}:{}", self.config.bind, self.config.port)
            .parse()
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(
                format!("invalid bind address: {e}")
            ))?;

        let listener = TcpListener::bind(addr).await
            .map_err(|e| pyo3::exceptions::PyOSError::new_err(
                format!("failed to bind to {addr}: {e}")
            ))?;

        let actual_addr = listener.local_addr()
            .map_err(|e| pyo3::exceptions::PyOSError::new_err(
                format!("failed to get local address: {e}")
            ))?;

        *self.bound_addr.lock().unwrap() = Some(actual_addr);

        let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);

        let core_config = Arc::new(CoreServeConfig {
            bind: actual_addr,
            root: std::path::PathBuf::from("."),  // not used for Python handler
            limits: eggserve_core::limits::Limits {
                max_connections: self.config.max_connections,
                max_file_streams: self.config.max_file_streams,
                ..Default::default()
            },
            static_policy: self.config.policy.inner.clone(),
        });

        let connection_semaphore = Arc::new(Semaphore::new(self.config.max_connections));
        let handler = self.handler.clone();

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, peer_addr)) => {
                            let permit = match connection_semaphore.clone().try_acquire_owned() {
                                Ok(p) => p,
                                Err(_) => {
                                    drop(stream);
                                    continue;
                                }
                            };

                            let mut shutdown_rx = shutdown_rx.resubscribe();
                            let handler = handler.clone();
                            let core_config = core_config.clone();

                            tokio::spawn(async move {
                                let _permit = permit;
                                let io = TokioIo::new(stream);
                                serve_python_connection(
                                    io, handler, core_config, peer_addr,
                                    &mut shutdown_rx,
                                ).await;
                            });
                        }
                        Err(e) => {
                            eprintln!("accept error: {e}");
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    break;
                }
            }
        }

        Ok(())
    }
}
```

Connection handler:

```rust
async fn serve_python_connection<I>(
    io: TokioIo<I>,
    handler: PyObject,
    _config: Arc<CoreServeConfig>,
    peer_addr: SocketAddr,
    shutdown_rx: &mut broadcast::Receiver<()>,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |req: Request<Incoming>| {
        let handler = handler.clone();
        async move {
            let response = handle_python_request(req, &handler, peer_addr).await;
            Ok::<_, std::convert::Infallible>(response)
        }
    });

    let conn = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(std::time::Duration::from_secs(10))
        .serve_connection(io, service)
        .with_upgrades();

    let mut conn = std::pin::pin!(conn);
    tokio::select! {
        result = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            &mut conn,
        ) => {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {}
                Err(_) => {
                    conn.as_mut().graceful_shutdown();
                }
            }
        }
        _ = shutdown_rx.recv() => {
            conn.as_mut().graceful_shutdown();
        }
    }
}
```

Per-request handler with GIL bridge:

```rust
async fn handle_python_request(
    req: Request<Incoming>,
    handler: &PyObject,
    peer_addr: SocketAddr,
) -> Response<BoxBodyInner> {
    let is_head = *req.method() == Method::HEAD;

    // Reject non-GET/HEAD methods early
    if *req.method() != Method::GET && *req.method() != Method::HEAD {
        return method_not_allowed_response();
    }

    // Validate no request body
    if let Err(rejection) = validate_no_request_body(&req, 0) {
        return match rejection {
            BodyRejection::BodyTooLarge => payload_too_large_response(),
            _ => bad_request_response(),
        };
    }

    // Construct PyRequest from hyper request
    let py_request = PyRequest::from_hyper(&req, Some(peer_addr));

    // Release the GIL during Rust processing, then re-acquire for callback
    let handler = handler.clone();
    let py_response: PyResponse = match tokio::task::spawn_blocking(move || {
        Python::with_gil(|py| -> PyResult<PyResponse> {
            let result = handler.call1(py, (&py_request,))?;
            result.extract::<PyResponse>(py)
        })
    }).await {
        Ok(Ok(resp)) => resp,
        Ok(Err(_err)) => {
            // Python exception → 500 without leaking traceback
            return internal_error_response();
        }
        Err(_join_err) => {
            return internal_error_response();
        }
    };

    // Convert PyResponse to hyper Response
    response_to_hyper(py_response, is_head).await
}
```

Response conversion:

```rust
async fn response_to_hyper(resp: PyResponse, is_head: bool) -> Response<BoxBodyInner> {
    let status = StatusCode::from_u16(resp.status)
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    let mut builder = Response::builder().status(status);
    for (name, value) in &resp.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    builder = builder.header("x-content-type-options", "nosniff");

    match resp.body {
        PyResponseBody::Empty => {
            builder.body(Full::new(Bytes::new())
                .map_err(|never| match never {})
                .boxed()).unwrap()
        }
        PyResponseBody::Bytes(data) => {
            let len = data.len();
            if is_head {
                // HEAD responses have no body
                builder = builder.header("content-length", len.to_string());
                builder.body(Full::new(Bytes::new())
                    .map_err(|never| match never {})
                    .boxed()).unwrap()
            } else {
                builder = builder.header("content-length", len.to_string());
                builder.body(Full::new(Bytes::from(data))
                    .map_err(|never| match never {})
                    .boxed()).unwrap()
            }
        }
        PyResponseBody::BodySource(py_body) => {
            // Convert BodySource to streaming response
            body_source_to_hyper_response(py_body.inner, status, is_head).await
        }
    }
}
```

Body source streaming (reuses patterns from `service.rs`):

```rust
async fn body_source_to_hyper_response(
    source: BodySource,
    status: StatusCode,
    is_head: bool,
) -> Response<BoxBodyInner> {
    use crate::response_helpers::{file_response, file_response_range, planned_response};
    // Delegate to response helpers that handle file streaming
    // (similar to service::body_source_to_response but without ServeState)
    match source {
        BodySource::Empty => {
            planned_response(status, &HeaderMapPlan::new())
        }
        BodySource::Bytes(b) => {
            if is_head {
                planned_response(status, &HeaderMapPlan::new())
            } else {
                let body = Full::new(Bytes::from(b))
                    .map_err(|never| match never {})
                    .boxed();
                Response::builder().status(status).body(body).unwrap()
            }
        }
        BodySource::FileFull { file, len, mime } => {
            if is_head {
                let mut builder = Response::builder().status(status);
                builder = builder.header("content-length", len.to_string());
                builder = builder.header("content-type", mime);
                builder.body(Full::new(Bytes::new())
                    .map_err(|never| match never {})
                    .boxed()).unwrap()
            } else {
                // Stream file using tokio
                let tokio_file = tokio::fs::File::from_std(file);
                crate::response_helpers::file_response_stream(tokio_file, len, mime, status)
                    .await
            }
        }
        BodySource::FileRange { file, range, .. } => {
            if is_head {
                let mut builder = Response::builder().status(status);
                builder = builder.header("content-length", range.len().to_string());
                builder.body(Full::new(Bytes::new())
                    .map_err(|never| match never {})
                    .boxed()).unwrap()
            } else {
                let tokio_file = tokio::fs::File::from_std(file);
                crate::response_helpers::file_range_stream(
                    tokio_file, range.start, range.end_inclusive, status,
                ).await
            }
        }
    }
}
```

### 2.8 Module registration

Add to `lib.rs` module declarations and `_native` module registration:

```rust
// At top of lib.rs, add:
pub mod server;

// In the _native module function, add:
m.add_class::<server::PyServerConfig>()?;
m.add_class::<server::PyRequest>()?;
m.add_class::<server::PyResponse>()?;
m.add_class::<server::PyStaticResponder>()?;
m.add_class::<server::PyServer>()?;
```

## Step 3: Add response helper functions

The body source streaming code needs helper functions for file streaming. Add a small `response_helpers` module or put them in `server.rs`. Since `eggserve-core`'s `response.rs` uses `ServeState` for the semaphore, and we don't need that for the Python server (connection semaphore handles backpressure), create simplified versions:

In `server.rs`, add:

```rust
fn planned_response(status: StatusCode, headers: &HeaderMapPlan) -> Response<BoxBodyInner> {
    let mut builder = Response::builder().status(status);
    for header in headers.iter() {
        builder = builder.header(&header.name, &header.value);
    }
    builder.body(Full::new(Bytes::new())
        .map_err(|never| match never {})
        .boxed()).unwrap()
}

fn method_not_allowed_response() -> Response<BoxBodyInner> {
    let mut builder = Response::builder().status(StatusCode::METHOD_NOT_ALLOWED);
    builder = builder.header("allow", "GET, HEAD");
    builder.body(Full::new(Bytes::from("405 Method Not Allowed\n"))
        .map_err(|never| match never {})
        .boxed()).unwrap()
}

fn internal_error_response() -> Response<BoxBodyInner> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Full::new(Bytes::from("500 Internal Server Error\n"))
            .map_err(|never| match never {})
            .boxed()).unwrap()
}

fn bad_request_response() -> Response<BoxBodyInner> {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(Full::new(Bytes::from("400 Bad Request\n"))
            .map_err(|never| match never {})
            .boxed()).unwrap()
}

fn payload_too_large_response() -> Response<BoxBodyInner> {
    Response::builder()
        .status(StatusCode::PAYLOAD_TOO_LARGE)
        .body(Full::new(Bytes::from("413 Payload Too Large\n"))
            .map_err(|never| match never {})
            .boxed()).unwrap()
}

fn not_found_response() -> Response<BoxBodyInner> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Full::new(Bytes::from("404 Not Found\n"))
            .map_err(|never| match never {})
            .boxed()).unwrap()
}

async fn file_response_stream(
    file: tokio::fs::File,
    len: u64,
    mime: &str,
    status: StatusCode,
) -> Response<BoxBodyInner> {
    let mut builder = Response::builder().status(status);
    builder = builder.header("content-length", len.to_string());
    builder = builder.header("content-type", mime);

    let stream = futures_util::stream::unfold(file, |mut file| async move {
        let mut buf = vec![0u8; 8192];
        match tokio::io::AsyncReadExt::read(&mut file, &mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(buf))), file))
            }
            Err(_) => None,
        }
    });

    let body = StreamBody::new(stream);
    let body: BoxBodyInner = BodyExt::boxed(body);
    builder.body(body).unwrap()
}

async fn file_range_stream(
    mut file: tokio::fs::File,
    start: u64,
    end_inclusive: u64,
    status: StatusCode,
) -> Response<BoxBodyInner> {
    use std::io::SeekFrom;
    use tokio::io::AsyncSeekExt;

    let len = end_inclusive - start + 1;
    let _ = file.seek(SeekFrom::Start(start)).await;

    let stream = futures_util::stream::unfold(
        (file, len),
        |(mut file, remaining)| async move {
            if remaining == 0 {
                return None;
            }
            let mut buf = vec![0u8; (remaining as usize).min(8192)];
            match tokio::io::AsyncReadExt::read(&mut file, &mut buf).await {
                Ok(0) => None,
                Ok(n) => {
                    let n = (n as u64).min(remaining) as usize;
                    buf.truncate(n);
                    let remaining = remaining.saturating_sub(n as u64);
                    Some((Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(buf))), (file, remaining)))
                }
                Err(_) => None,
            }
        },
    );

    let body = StreamBody::new(stream);
    let body: BoxBodyInner = BodyExt::boxed(body);

    let mut builder = Response::builder().status(status);
    builder.body(body).unwrap()
}
```

## Step 4: Update `crates/eggserve-python/src/lib.rs`

Add module declaration and re-exports:

```rust
// Add at top:
pub mod server;

// Add to _native module registration:
m.add_class::<server::PyServerConfig>()?;
m.add_class::<server::PyRequest>()?;
m.add_class::<server::PyResponse>()?;
m.add_class::<server::PyStaticResponder>()?;
m.add_class::<server::PyServer>()?;
```

Also need to make `PyStaticPolicy`'s `inner` field accessible (it's currently private). Either:
- Make it `pub(crate)` (preferred)
- Or add a method to convert to `RustStaticPolicy`

Since `PyStaticPolicy` already has `inner: RustStaticPolicy` and `server.rs` needs to access it, mark it `pub(crate)`:

```rust
// In lib.rs, change PyStaticPolicy:
#[pyclass(name = "StaticPolicy", frozen)]
pub(crate) struct PyStaticPolicy {
    pub(crate) inner: RustStaticPolicy,
}
```

## Step 5: Update `python/eggserve/__init__.py`

Add new exports:

```python
from eggserve.server import (
    ServeConfig,
    ServerProcess,
    serve_directory,
)

# Add:
try:
    from eggserve._native import (
        # ... existing ...
        ServerConfig,
        Request,
        Response,
        StaticResponder,
        Server,
    )
except ImportError:
    pass
```

## Step 6: Create `python/eggserve/test_server_primitives.py`

Test file for the in-process server. Key test cases:

```python
"""Tests for the eggserve in-process server primitives.

Uses stdlib unittest to validate Server, ServerConfig, Request,
Response, and StaticResponder.
"""

import threading
import time
import unittest
import urllib.request

from eggserve._native import (
    Request,
    Response,
    Server,
    ServerConfig,
    StaticResponder,
)


class TestServerConfig(unittest.TestCase):
    def test_defaults(self):
        config = ServerConfig()
        self.assertEqual(config.bind, "127.0.0.1")
        self.assertEqual(config.port, 8000)
        self.assertFalse(config.public)

    def test_frozen(self):
        config = ServerConfig()
        with self.assertRaises(AttributeError):
            config.port = 9000  # type: ignore[misc]

    def test_public_bind_requires_public_flag(self):
        with self.assertRaises(ValueError) as ctx:
            ServerConfig(bind="0.0.0.0", public=False)
        self.assertIn("public=True", str(ctx.exception))

    def test_port_zero_rejected(self):
        with self.assertRaises(ValueError):
            ServerConfig(port=0)


class TestRequest(unittest.TestCase):
    def test_repr(self):
        # Request is constructed internally, but we can test properties
        pass  # Will be tested via integration tests


class TestResponse(unittest.TestCase):
    def test_empty_default(self):
        resp = Response.empty()
        self.assertEqual(resp.status, 204)
        self.assertEqual(resp.body_kind, "empty")

    def test_bytes_default(self):
        resp = Response.bytes(b"hello")
        self.assertEqual(resp.status, 200)
        self.assertEqual(resp.body_kind, "bytes")

    def test_text_default(self):
        resp = Response.text("hello")
        self.assertEqual(resp.status, 200)
        self.assertIn(("content-type", "text/plain; charset=utf-8"), resp.headers)

    def test_custom_status(self):
        resp = Response.empty(status=200)
        self.assertEqual(resp.status, 200)

    def test_invalid_status_raises(self):
        with self.assertRaises(ValueError):
            Response.empty(status=99)
        with self.assertRaises(ValueError):
            Response.empty(status=600)

    def test_text_with_custom_content_type(self):
        resp = Response.text("ok", content_type="text/html")
        self.assertIn(("content-type", "text/html"), resp.headers)


class TestStaticResponder(unittest.TestCase):
    def test_respond_get_file(self):
        import tempfile
        import os
        tmp = tempfile.mkdtemp()
        try:
            with open(os.path.join(tmp, "hello.txt"), "w") as f:
                f.write("hello")
            static = StaticResponder(tmp)
            req = Request  # Can't construct directly; test via integration
        finally:
            import shutil
            shutil.rmtree(tmp)


class TestServerIntegration(unittest.TestCase):
    def test_serve_forever_and_stop(self):
        import tempfile
        import os
        import urllib.request

        tmp = tempfile.mkdtemp()
        try:
            with open(os.path.join(tmp, "hello.txt"), "w") as f:
                f.write("hello")

            static = StaticResponder(tmp)

            def handler(request):
                return static.respond(request)

            config = ServerConfig(bind="127.0.0.1", port=0)
            server = Server(config, handler)

            # Start in background thread
            def run():
                server.serve_forever()

            t = threading.Thread(target=run, daemon=True)
            t.start()

            # Wait for server to start
            time.sleep(0.5)

            addr = server.addr
            self.assertIsNotNone(addr)
            self.assertIn("127.0.0.1", addr)

            # Make request
            resp = urllib.request.urlopen(f"http://{addr}/hello.txt")
            self.assertEqual(resp.status, 200)
            self.assertEqual(resp.read(), b"hello")

            # Stop
            server.stop()
        finally:
            import shutil
            shutil.rmtree(tmp)


if __name__ == "__main__":
    unittest.main()
```

## Step 7: Update documentation

### 7.1 `docs/python-api.md`

Add new section after "Native primitives" section:

```markdown
## In-process server

The in-process server provides a Rust-owned server loop with Python request dispatch. Rust owns socket I/O, HTTP parsing, response serialization, file streaming, backpressure, concurrency limits, and timeout enforcement. Python provides handler callbacks that return explicit response objects.

**This is NOT an ASGI/WSGI server.** It is a primitive for building servers.

### `ServerConfig`

Configuration for the in-process server.

```python
from eggserve import ServerConfig

config = ServerConfig(
    bind="127.0.0.1",
    port=8000,
    public=False,
    policy=StaticPolicy(),
    max_connections=64,
    max_file_streams=32,
)
```

### `Request`

Immutable request metadata exposed to Python handlers.

Properties: `method`, `target`, `path`, `query`, `headers`, `remote_addr`, `http_version`, `has_body`.

### `Response`

Immutable response object with factory methods.

```python
from eggserve import Response

# Empty response
resp = Response.empty(status=204)

# Bytes response
resp = Response.bytes(b"hello", content_type="text/plain")

# Text response (default content-type: text/plain; charset=utf-8)
resp = Response.text("hello")

# From body source (for static files)
resp = Response.body_source(body_source, status=200)
```

### `StaticResponder`

Convenience wrapper around `SecureRoot` for static file responses.

```python
from eggserve import StaticResponder

static = StaticResponder("public", policy=StaticPolicy())
response = static.respond(request)
```

### `Server`

The main server lifecycle object.

```python
from eggserve import Server, ServerConfig, StaticResponder

static = StaticResponder("public")

def handler(request):
    if request.path == "/health":
        return Response.text("ok")
    return static.respond(request)

server = Server(ServerConfig(port=8000), handler)

# Blocking
server.serve_forever()

# Or lifecycle control
server.start()
print(server.addr)  # "127.0.0.1:8000"
server.stop()

# Context manager
with Server(ServerConfig(port=8000), handler) as server:
    # server starts on enter, stops on exit
    pass
```
```

### 7.2 `docs/extension-contract.md`

Add to "Allowed integration patterns" section:

```markdown
### In-process server with Python dispatch

Python code may use the in-process server to handle requests while Rust owns I/O:

```python
from eggserve import Server, ServerConfig, StaticResponder, Response

static = StaticResponder("public")

def handler(request):
    if request.path == "/health":
        return Response.text("ok")
    return static.respond(request)

server = Server(ServerConfig(port=8000), handler)
server.serve_forever()
```

The Python handler receives immutable request metadata and returns explicit response objects. Rust handles socket I/O, HTTP parsing, response serialization, and file streaming.
```

### 7.3 `architecture/eggserve-python.md`

Add section on the server primitives architecture:

```markdown
## In-Process Server (`server.rs`)

The server module provides a Rust-owned server loop with Python request dispatch.

### Architecture

- **Rust owns**: socket binding, accept loop, HTTP/1.1 parsing, connection semaphore, header read timeout, write timeout, graceful shutdown, response serialization, file streaming
- **Python provides**: handler callback, request routing decisions, response objects

### GIL Management

Python callbacks are invoked via `tokio::task::spawn_blocking` + `Python::with_gil`. This ensures:
- The GIL is not held during Rust file streaming or I/O
- The tokio runtime is not blocked by Python execution
- Concurrent requests can be handled while one Python callback runs

### Body Source Streaming

File-backed responses from `StaticResponder` produce `BodySource` objects that are converted to Hyper streaming bodies without reading into Python memory. The file handle was opened during path resolution and is carried forward — no path reopening.

### Safety Properties

- Handler exceptions map to 500 responses without leaking Python tracebacks
- Public bind requires explicit `public=True` flag
- Safe defaults match CLI (no symlinks, no dotfiles, no directory listing)
- Path resolution through `SecureRoot` maintains confinement guarantees
```

### 7.4 `docs/threat-model.md`

Add to relevant sections:

```markdown
### Python callback resources

When using the in-process server primitive:

- Python callbacks execute in `spawn_blocking` threads, not the tokio runtime thread
- GIL contention is bounded by the connection semaphore (max_connections)
- Python exceptions are caught and mapped to 500 responses without leaking tracebacks
- Python callbacks receive immutable request metadata, not raw sockets
- File streaming for static responses bypasses Python entirely (Rust streams directly)

### Python callback exception behavior

- Handler exceptions produce 500 Internal Server Error
- Python tracebacks are never included in HTTP response bodies
- Exceptions are logged to stderr by default
- The server continues accepting connections after handler exceptions
```

### 7.5 `README.md`

Add brief mention in the features section:

```markdown
### In-process server (Python)

```python
from eggserve import Server, ServerConfig, StaticResponder, Response

static = StaticResponder("public")

def handler(request):
    if request.path == "/health":
        return Response.text("ok")
    return static.respond(request)

server = Server(ServerConfig(port=8000), handler)
server.serve_forever()
```
```

### 7.6 `AGENTS.md`

Add to "Important quirks" section:

```markdown
- **In-process server**: The `Server` class provides a Rust-owned server loop with Python dispatch. Rust owns I/O; Python provides handler callbacks. This is NOT ASGI/WSGI.
- **GIL management**: Python callbacks run in `spawn_blocking` threads, not the tokio runtime. The GIL is acquired per-callback.
- **BodySource in server context**: When `StaticResponder` returns a response with a `BodySource`, Rust streams the file directly without Python involvement.
```

## Step 8: Add example

Create `examples/python_dynamic_static.py`:

```python
"""Example: dynamic endpoint + static files using the in-process server."""

from eggserve import Server, ServerConfig, StaticResponder, Response

static = StaticResponder("public")

def handler(request):
    if request.path == "/health":
        return Response.text("ok")
    if request.path == "/time":
        import datetime
        now = datetime.datetime.now().isoformat()
        return Response.text(now, content_type="text/plain")
    return static.respond(request)

if __name__ == "__main__":
    config = ServerConfig(port=8000)
    server = Server(config, handler)
    print(f"Serving on {server.addr or '127.0.0.1:8000'}")
    server.serve_forever()
```

## Step 9: Validation

### Rust validation

```sh
# From workspace root
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Python crate (separate build)
cd crates/eggserve-python
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

### Python validation

```sh
cd crates/eggserve-python

# Build wheel
maturin build --release -o dist

# Install
python -m pip install --force-reinstall dist/*.whl

# Run tests
PYTHONPATH=python python -m unittest eggserve.test_primitives -v
PYTHONPATH=python python -m unittest eggserve.test_server -v
PYTHONPATH=python python -m unittest eggserve.test_server_primitives -v

# Smoke test
python -m eggserve --help
```

### Full validation sequence

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
cargo audit
cargo deny check

cd crates/eggserve-python
maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
PYTHONPATH=python python -m unittest eggserve.test_primitives -v
PYTHONPATH=python python -m unittest eggserve.test_server -v
PYTHONPATH=python python -m unittest eggserve.test_server_primitives -v
python -m eggserve --help
```

## Step 10: Commit

```sh
git add crates/eggserve-python/
git add docs/python-api.md docs/extension-contract.md docs/threat-model.md
git add architecture/eggserve-python.md
git add plans/027-implementation-detail.md
git add examples/python_dynamic_static.py
git add README.md AGENTS.md

git commit -m "Plan 027: Rust-Owned Python Server Primitives

Add in-process server with Python request dispatch:
- ServerConfig: configuration with safe defaults
- Request: immutable request metadata
- Response: explicit response objects with factory methods
- StaticResponder: convenience wrapper for static file responses
- Server: Rust-owned accept loop with Python callback bridge

Rust owns socket I/O, HTTP parsing, response serialization, file
streaming, backpressure, concurrency limits, and timeout enforcement.
Python provides handler callbacks that return explicit response objects.

This is NOT ASGI/WSGI. It is a primitive for building servers."
```

## Implementation notes

### Dependency management

The Python crate adds `tokio`, `hyper`, `hyper-util`, `http-body-util`, `bytes`, and `futures-util`. These are already used by `eggserve-bin` and are well-established crates. The Python crate's independent build (separate `Cargo.lock`) means these don't affect the workspace.

### GIL safety

The key GIL management pattern:

```
tokio spawn
  → spawn_blocking (runs on blocking thread pool)
    → Python::with_gil (acquires GIL)
      → call handler
      → extract response
    → drop GIL
  → return to tokio
```

This ensures:
- Tokio runtime is never blocked by Python
- File streaming happens without GIL
- Concurrent requests are handled by tokio while Python callbacks execute

### Response validation

Status codes are validated at construction time. Header values are checked for control characters. Content-Length is generated by Rust, not Python. Python cannot lie about content length for Rust-owned body sources.

### Error handling

- Python exceptions → 500 Internal Server Error (no traceback in body)
- Invalid request methods → 405 Method Not Allowed
- Request bodies on GET/HEAD → rejected per policy
- Path resolution failures → mapped to appropriate HTTP status
- Handler not callable → error at Server construction time

### Directory listing (deferred)

The first pass of `StaticResponder` returns 403 for directories. Full directory listing parity with the CLI can be added in a follow-up plan. This keeps the first pass focused and testable.

### TLS (deferred)

TLS support is not included in the first pass. It can be added later using the existing `rustls`/`tokio-rustls` infrastructure from `eggserve-bin`.
