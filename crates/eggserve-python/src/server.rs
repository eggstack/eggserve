use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::Frame;
use hyper::service::Service;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, OwnedSemaphorePermit, Semaphore};
use tokio::task;

use eggserve_core::policy;
use eggserve_core::primitives::body::BodySource;
use eggserve_core::primitives::http::{
    validate_request_body, ReadOnlyMethod, RequestValidationError,
};
use eggserve_core::primitives::{
    resolve_and_plan, ConfinedPath, PathDotfilePolicy, PathPolicy, PathRejection,
    ResolveAndPlanError, SecureRoot, StaticPolicy,
};

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type FileStream = StreamBody<
    Pin<
        Box<
            dyn futures_util::Stream<Item = Result<Frame<Bytes>, std::io::Error>>
                + Send
                + Sync,
        >,
    >,
>;

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

#[pyclass(frozen, name = "Request")]
#[derive(Debug, Clone)]
pub struct PyRequest {
    #[pyo3(get)]
    method: String,
    #[pyo3(get)]
    path: String,
    #[pyo3(get)]
    query: String,
    #[pyo3(get)]
    headers: HashMap<String, String>,
    #[pyo3(get)]
    remote_addr: Option<String>,
    #[pyo3(get)]
    http_version: String,
    #[pyo3(get)]
    has_body: bool,
}

#[pymethods]
impl PyRequest {
    fn __repr__(&self) -> String {
        format!("<Request {} {}>", self.method, self.path)
    }
}

#[pyclass(frozen, name = "Response")]
#[derive(Debug)]
pub struct PyResponse {
    #[pyo3(get)]
    status: u16,
    #[pyo3(get)]
    headers: HashMap<String, String>,
    pub(crate) body: PyResponseBody,
}

#[derive(Debug)]
pub(crate) enum PyResponseBody {
    Empty,
    Bytes(Vec<u8>),
    BodySource(BodySource),
}

#[pymethods]
impl PyResponse {
    #[staticmethod]
    fn empty(status: u16) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            body: PyResponseBody::Empty,
        }
    }

    #[staticmethod]
    #[pyo3(signature = (status, data, headers=None))]
    fn bytes(status: u16, data: Vec<u8>, headers: Option<HashMap<String, String>>) -> Self {
        Self {
            status,
            headers: headers.unwrap_or_default(),
            body: PyResponseBody::Bytes(data),
        }
    }

    #[staticmethod]
    #[pyo3(signature = (status, text, headers=None))]
    fn text(status: u16, text: String, headers: Option<HashMap<String, String>>) -> Self {
        let mut h = headers.unwrap_or_default();
        h.entry("content-type".to_string())
            .or_insert_with(|| "text/plain; charset=utf-8".to_string());
        Self {
            status,
            headers: h,
            body: PyResponseBody::Bytes(text.into_bytes()),
        }
    }

    #[staticmethod]
    fn body_source(
        status: u16,
        body: &ServerBodySource,
        headers: Option<HashMap<String, String>>,
    ) -> PyResult<Self> {
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
            body: PyResponseBody::BodySource(source),
        })
    }

    #[getter]
    fn body(&self) -> ServerBodySource {
        let source = match &self.body {
            PyResponseBody::Empty => BodySource::Empty,
            PyResponseBody::Bytes(data) => BodySource::Bytes(data.clone()),
            PyResponseBody::BodySource(source) => match source {
                BodySource::Empty => BodySource::Empty,
                BodySource::Bytes(data) => BodySource::Bytes(data.clone()),
                BodySource::FileFull { file, len, mime } => match file.try_clone() {
                    Ok(cloned) => BodySource::FileFull {
                        file: cloned,
                        len: *len,
                        mime,
                    },
                    Err(_) => BodySource::Empty,
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
                    Err(_) => BodySource::Empty,
                },
            },
        };
        ServerBodySource {
            inner: std::sync::Mutex::new(Some(source)),
        }
    }

    fn __repr__(&self) -> String {
        format!("<Response {}>", self.status)
    }
}

#[pyclass(frozen, name = "StaticResponder")]
#[derive(Debug, Clone)]
pub struct PyStaticResponder {
    root: SecureRoot,
}

#[pymethods]
impl PyStaticResponder {
    #[new]
    fn new(root: &ServerSecureRoot) -> Self {
        Self {
            root: root.inner.clone(),
        }
    }

    #[pyo3(signature = (method, target, headers=None, has_body=false, remote_addr=None, http_version=None))]
    fn respond(
        &self,
        method: &str,
        target: &str,
        headers: Option<HashMap<String, String>>,
        has_body: bool,
        remote_addr: Option<String>,
        http_version: Option<String>,
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
        let path = match ConfinedPath::parse(target, &path_policy) {
            Ok(p) => p,
            Err(e) => {
                let is_malformed = matches!(
                    e,
                    PathRejection::MalformedPercentEncoding
                        | PathRejection::InvalidUtf8
                        | PathRejection::NulByte
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
        let if_none_match = hdrs.get("if-none-match").map(|s| s.as_str());
        let if_modified_since = hdrs.get("if-modified-since").map(|s| s.as_str());
        let range = hdrs.get("range").map(|s| s.as_str());
        let if_range = hdrs.get("if-range").map(|s| s.as_str());

        match resolve_and_plan(
            &self.root,
            &path,
            ro_method,
            if_none_match,
            if_modified_since,
            range,
            if_range,
        ) {
            Ok((plan, body_source)) => build_response(plan, body_source),
            Err(ResolveAndPlanError::NotFound) => build_error_response(404, "Not Found"),
            Err(ResolveAndPlanError::IsDirectory) => build_error_response(403, "Forbidden"),
            Err(ResolveAndPlanError::Denied(_)) => build_error_response(403, "Forbidden"),
            Err(ResolveAndPlanError::Body(e)) => Err(pyo3::exceptions::PyRuntimeError::new_err(
                format!("body error: {e}"),
            )),
        }
    }
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
        body: PyResponseBody::BodySource(body_source),
    })
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
        body: PyResponseBody::Bytes(reason.as_bytes().to_vec()),
    })
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
}

#[pymethods]
impl ServerSecureRoot {
    #[new]
    #[pyo3(signature = (path, policy=None))]
    fn new(path: String, policy: Option<PyStaticPolicyWrapper>) -> PyResult<Self> {
        let static_policy = policy
            .map(|p| p.inner)
            .unwrap_or_else(StaticPolicy::safe_default);
        let root = SecureRoot::new(path, static_policy).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("failed to create secure root: {e}"))
        })?;
        Ok(Self { inner: root })
    }

    #[getter]
    fn root_path(&self) -> String {
        self.inner.root_path().to_string_lossy().to_string()
    }
}

#[pyclass(name = "ServerBodySource")]
pub struct ServerBodySource {
    pub(crate) inner: std::sync::Mutex<Option<BodySource>>,
}

#[pymethods]
impl ServerBodySource {
    #[pyo3(signature = (status=200))]
    fn to_response(&self, status: u16) -> PyResult<PyResponse> {
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
            body: PyResponseBody::BodySource(source),
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
            .map(|r| (r.start, r.end_inclusive)))
    }

    fn read_all<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        let mut source = inner.take().ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err("BodySource already consumed")
        })?;
        let data = source
            .read_all()
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
        let data = source
            .read_range(start, end_inclusive)
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

#[pyclass(frozen, name = "Server")]
#[allow(dead_code)]
pub struct PyServer {
    bind: String,
    port: u16,
    public: bool,
    addr: std::sync::Mutex<Option<String>>,
    responder: PyStaticResponder,
    handler: Option<std::sync::Mutex<Option<Py<PyAny>>>>,
    shutdown_tx: std::sync::Mutex<Option<broadcast::Sender<()>>>,
    handle: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    max_connections: usize,
    max_file_streams: usize,
    max_python_callbacks: usize,
    header_timeout: Duration,
    write_timeout: Duration,
}

#[pymethods]
impl PyServer {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (root, bind="127.0.0.1", port=8000, policy=None, handler=None, public=false, max_connections=100, max_file_streams=64, max_python_callbacks=8, header_timeout_secs=10, write_timeout_secs=30))]
    fn new(
        root: String,
        bind: &str,
        port: u16,
        policy: Option<PyStaticPolicyWrapper>,
        handler: Option<Py<PyAny>>,
        public: bool,
        max_connections: usize,
        max_file_streams: usize,
        max_python_callbacks: usize,
        header_timeout_secs: u64,
        write_timeout_secs: u64,
    ) -> PyResult<Self> {
        let bind_addr: SocketAddr = format!("{bind}:{port}")
            .parse()
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("invalid bind address"))?;
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
        if write_timeout_secs == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "write_timeout_secs must be greater than zero",
            ));
        }

        let static_policy = policy
            .map(|p| p.inner)
            .unwrap_or_else(StaticPolicy::safe_default);
        let secure_root = SecureRoot::new(root, static_policy).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("failed to create secure root: {e}"))
        })?;
        let responder = PyStaticResponder { root: secure_root };

        Ok(Self {
            bind: bind.to_string(),
            port,
            public,
            addr: std::sync::Mutex::new(None),
            responder,
            handler: handler.map(|h| std::sync::Mutex::new(Some(h))),
            shutdown_tx: std::sync::Mutex::new(None),
            handle: std::sync::Mutex::new(None),
            max_connections,
            max_file_streams,
            max_python_callbacks,
            header_timeout: Duration::from_secs(header_timeout_secs),
            write_timeout: Duration::from_secs(write_timeout_secs),
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

    fn start(&self, py: Python<'_>) -> PyResult<()> {
        let mut tx_guard = self
            .shutdown_tx
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        if tx_guard.is_some() {
            return Err(crate::LifecycleError::new_err(
                "Server already started",
            ));
        }

        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        *tx_guard = Some(shutdown_tx.clone());

        let responder = self.responder.clone();
        let handler: Option<Arc<std::sync::Mutex<Option<Py<PyAny>>>>> =
            self.handler.as_ref().map(|m| {
                Arc::new(std::sync::Mutex::new(m.lock().ok().and_then(|guard| {
                    guard
                        .as_ref()
                        .map(|py_any| Python::with_gil(|py| py_any.clone_ref(py)))
                })))
            });
        let max_file_streams = self.max_file_streams;
        let max_python_callbacks = self.max_python_callbacks;
        let header_timeout = self.header_timeout;
        let write_timeout = self.write_timeout;

        let bind_str = format!("{}:{}", self.bind, self.port);
        let bind_addr: SocketAddr = bind_str
            .parse()
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("invalid bind address"))?;

        let rt_handle = py.allow_threads(|| {
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
            let listener = rt.block_on(async {
                TcpListener::bind(&bind_addr)
                    .await
                    .map_err(|e| pyo3::exceptions::PyOSError::new_err(e.to_string()))
            })?;
            let local_addr = listener
                .local_addr()
                .map_err(|e| pyo3::exceptions::PyOSError::new_err(e.to_string()))?;
            Ok::<_, PyErr>((rt, listener, local_addr))
        })?;

        let (rt, listener, local_addr) = rt_handle;

        *self
            .addr
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))? =
            Some(local_addr.to_string());

        let conn_semaphore = Arc::new(Semaphore::new(self.max_connections));
        let file_stream_semaphore = Arc::new(Semaphore::new(max_file_streams));
        let callback_semaphore = Arc::new(Semaphore::new(max_python_callbacks));

        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            rt.block_on(async move {
                let mut shutdown_rx = shutdown_tx.subscribe();
                let _ = ready_tx.send(());
                loop {
                    tokio::select! {
                        accept = listener.accept() => {
                            match accept {
                                Ok((stream, _remote_addr)) => {
                                    let permit = match conn_semaphore.clone().acquire_owned().await {
                                        Ok(p) => p,
                                        Err(_) => break,
                                    };
                                    let responder = responder.clone();
                                    let handler = handler.clone();
                                    let file_stream_semaphore = file_stream_semaphore.clone();
                                    let callback_semaphore = callback_semaphore.clone();
                                    let mut conn_shutdown_rx = shutdown_tx.subscribe();
                                    task::spawn(async move {
                                        let _permit = permit;
                                        let io = TokioIo::new(stream);
                                        let service = ServerService {
                                            responder,
                                            handler,
                                            file_stream_semaphore,
                                            callback_semaphore,
                                        };
                                        let conn = hyper::server::conn::http1::Builder::new()
                                            .timer(TokioTimer::new())
                                            .header_read_timeout(header_timeout)
                                            .serve_connection(io, service);
                                        let mut conn = std::pin::pin!(conn);
                                        tokio::select! {
                                            result = tokio::time::timeout(write_timeout, &mut conn) => {
                                                match result {
                                                    Ok(Ok(())) => {}
                                                    Ok(Err(_e)) => {}
                                                    Err(_elapsed) => {
                                                        conn.as_mut().graceful_shutdown();
                                                    }
                                                }
                                            }
                                            _ = conn_shutdown_rx.recv() => {
                                                conn.as_mut().graceful_shutdown();
                                            }
                                        }
                                    });
                                }
                                Err(_e) => break,
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            break;
                        }
                    }
                }
            });
        });

        ready_rx.recv().map_err(|_| {
            pyo3::exceptions::PyRuntimeError::new_err("server thread failed to start")
        })?;

        *self
            .handle
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))? =
            Some(handle);
        Ok(())
    }

    fn stop(&self, py: Python<'_>) -> PyResult<()> {
        let mut tx_guard = self
            .shutdown_tx
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        if let Some(tx) = tx_guard.take() {
            let _ = tx.send(());
        }
        drop(tx_guard);

        let mut handle_guard = self
            .handle
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        if let Some(handle) = handle_guard.take() {
            py.allow_threads(|| {
                let _ = handle.join();
            });
        }
        drop(handle_guard);

        *self
            .addr
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))? = None;
        Ok(())
    }

    fn __enter__(slf: Py<Self>) -> PyResult<Py<Self>> {
        Python::with_gil(|py| {
            slf.borrow(py).start(py)?;
            Ok(slf)
        })
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

#[derive(Clone)]
#[allow(dead_code)]
struct ServerService {
    responder: PyStaticResponder,
    handler: Option<Arc<std::sync::Mutex<Option<Py<PyAny>>>>>,
    file_stream_semaphore: Arc<Semaphore>,
    callback_semaphore: Arc<Semaphore>,
}

impl Service<Request<hyper::body::Incoming>> for ServerService {
    type Response = Response<BoxBody<Bytes, BoxError>>;
    type Error = BoxError;
    type Future =
        Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<hyper::body::Incoming>) -> Self::Future {
        let responder = self.responder.clone();
        let handler = self.handler.clone();
        let callback_semaphore = self.callback_semaphore.clone();
        let file_stream_semaphore = self.file_stream_semaphore.clone();

        Box::pin(async move {
            let method = req.method().clone();
            let uri = req.uri().clone();
            if uri.authority().is_some() {
                let body = Full::new(Bytes::from("Bad Request"))
                    .map_err(|e| -> BoxError { Box::new(e) })
                    .boxed();
                let resp = Response::builder()
                    .status(400u16)
                    .header("content-type", "text/plain")
                    .body(body)
                    .map_err(|e| -> BoxError { Box::new(e) })?;
                return Ok(resp);
            }
            let is_head = method == hyper::Method::HEAD;
            let is_read_only = method == hyper::Method::GET || is_head;
            if is_read_only {
                if let Err(error) = validate_read_only_body(req.headers()) {
                    let response = body_validation_response(error);
                    return convert_to_hyper_response(
                        response,
                        Some(file_stream_semaphore),
                        is_head,
                    )
                    .await;
                }
            }
            let headers: HashMap<String, String> = req
                .headers()
                .iter()
                .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.to_string(), v.to_string())))
                .collect();
            let has_body = request_has_body(req.headers());
            let target = uri.path().to_string();
            let query = uri.query().unwrap_or("").to_string();
            let http_version = format!("{:?}", req.version());

            let method_str = method.as_str().to_string();
            let full_target = if query.is_empty() {
                target.clone()
            } else {
                format!("{target}?{query}")
            };

            let response = if let Some(handler_arc) = handler {
                let _callback_permit = callback_semaphore
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| -> BoxError { "callback semaphore closed".into() })?;
                let py_request = PyRequest {
                    method: method_str,
                    path: target,
                    query,
                    headers,
                    remote_addr: None,
                    http_version,
                    has_body,
                };

                let response = Python::with_gil(|py| {
                    let handler_gil = handler_arc
                        .lock()
                        .map_err(|_| -> BoxError { "handler lock poisoned".into() })?;
                    let handler_py = handler_gil
                        .as_ref()
                        .ok_or_else(|| -> BoxError { "handler already consumed".into() })?
                        .clone_ref(py);
                    drop(handler_gil);
                    let py_req_obj = py_request.into_pyobject(py)?;
                    match handler_py.bind(py).call1((py_req_obj,)) {
                        Ok(obj) => {
                            let status: u16 = obj
                                .getattr("status")
                                .and_then(|v| v.extract())
                                .unwrap_or(500);
                            let status = if valid_http_status(status) {
                                status
                            } else {
                                500
                            };
                            let headers: HashMap<String, String> = obj
                                .getattr("headers")
                                .and_then(|v| v.extract())
                                .unwrap_or_default();
                            let body = if let Ok(py_resp) =
                                obj.extract::<pyo3::Bound<'_, PyResponse>>()
                            {
                                match &py_resp.borrow().body {
                                    PyResponseBody::Empty => PyResponseBody::Empty,
                                    PyResponseBody::Bytes(data) => {
                                        PyResponseBody::Bytes(data.clone())
                                    }
                                    PyResponseBody::BodySource(BodySource::FileFull {
                                        file,
                                        len,
                                        mime,
                                    }) => PyResponseBody::BodySource(BodySource::FileFull {
                                        file: file
                                            .try_clone()
                                            .map_err(|e| -> BoxError { Box::new(e) })?,
                                        len: *len,
                                        mime,
                                    }),
                                    PyResponseBody::BodySource(BodySource::FileRange {
                                        file,
                                        range,
                                        total_len,
                                        mime,
                                    }) => PyResponseBody::BodySource(BodySource::FileRange {
                                        file: file
                                            .try_clone()
                                            .map_err(|e| -> BoxError { Box::new(e) })?,
                                        range: *range,
                                        total_len: *total_len,
                                        mime,
                                    }),
                                    other => {
                                        let _ = other;
                                        PyResponseBody::Empty
                                    }
                                }
                            } else {
                                let body = match obj.getattr("body") {
                                    Ok(b) => {
                                        let kind: String = b
                                            .getattr("kind")
                                            .and_then(|v| v.extract())
                                            .unwrap_or_default();
                                        match kind.as_str() {
                                            "empty" => PyResponseBody::Empty,
                                            "bytes" => {
                                                let data: Vec<u8> = b
                                                    .call_method0("read_all")
                                                    .and_then(|v| v.extract())
                                                    .unwrap_or_default();
                                                PyResponseBody::Bytes(data)
                                            }
                                            _ => PyResponseBody::Empty,
                                        }
                                    }
                                    Err(_) => PyResponseBody::Empty,
                                };
                                body
                            };
                            Ok::<_, BoxError>(PyResponse {
                                status,
                                headers,
                                body,
                            })
                        }
                        Err(e) => {
                            eprintln!("Handler error: {e}");
                            Err("handler raised an exception".into())
                        }
                    }
                });

                response
                    .unwrap_or_else(|_| build_error_response(500, "Internal Server Error").unwrap())
            } else {
                task::spawn_blocking(move || {
                    if method_str != "GET" && method_str != "HEAD" {
                        return build_error_response(405, "Method Not Allowed").unwrap();
                    }
                    if has_body {
                        return build_error_response(400, "Bad Request").unwrap();
                    }

                    responder
                        .respond(
                            &method_str,
                            &full_target,
                            Some(headers),
                            false,
                            None,
                            Some(http_version),
                        )
                        .unwrap_or_else(|e| {
                            let py_str = e.to_string();
                            let msg = py_str.strip_prefix("ValueError: ").unwrap_or(&py_str);
                            if msg.starts_with("Path rejected") {
                                build_error_response(403, "Forbidden").unwrap()
                            } else if msg.starts_with("Invalid request target") {
                                build_error_response(400, "Bad Request").unwrap()
                            } else {
                                build_error_response(500, "Internal Server Error").unwrap()
                            }
                        })
                })
                .await
                .map_err(|e| -> BoxError { Box::new(e) })?
            };

            convert_to_hyper_response(response, Some(file_stream_semaphore), is_head).await
        })
    }
}

fn validate_read_only_body(
    headers: &hyper::HeaderMap,
) -> Result<(), RequestValidationError> {
    let content_length = headers
        .get(hyper::header::CONTENT_LENGTH)
        .map(|value| {
            value
                .to_str()
                .map_err(|_| RequestValidationError::InvalidContentLength)
        })
        .transpose()?;
    let transfer_encoding = headers
        .get(hyper::header::TRANSFER_ENCODING)
        .map(|value| {
            value
                .to_str()
                .map_err(|_| RequestValidationError::UnsupportedTransferEncoding)
        })
        .transpose()?;
    validate_request_body(content_length, transfer_encoding, 0)
}

fn request_has_body(headers: &hyper::HeaderMap) -> bool {
    let content_length = headers
        .get(hyper::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .is_some_and(|length| length > 0);
    let transfer_encoding = headers
        .get(hyper::header::TRANSFER_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| !value.trim().is_empty());
    content_length || transfer_encoding
}

fn body_validation_response(error: RequestValidationError) -> PyResponse {
    let (status, reason) = match error {
        RequestValidationError::BodyTooLarge => (413, "Payload Too Large"),
        RequestValidationError::InvalidContentLength
        | RequestValidationError::UnsupportedTransferEncoding
        | RequestValidationError::ConflictingBodyHeaders => (400, "Bad Request"),
        RequestValidationError::MethodNotAllowed | RequestValidationError::InvalidRequestTarget => {
            (400, "Bad Request")
        }
    };
    build_error_response(status, reason).expect("static body validation response is valid")
}

fn stream_file(
    file: tokio::fs::File,
    limit: Option<u64>,
    permit: Option<OwnedSemaphorePermit>,
) -> FileStream {
    let initial_remaining = limit.unwrap_or(u64::MAX);
    let stream = futures_util::stream::unfold(
        (file, initial_remaining, permit),
        |(mut file, remaining, permit)| async move {
            if remaining == 0 {
                return None;
            }
            let mut buf = vec![0u8; 65536];
            let chunk_size = std::cmp::min(remaining, buf.len() as u64) as usize;
            buf.resize(chunk_size, 0);
            match file.read(&mut buf).await {
                Ok(0) => None,
                Ok(n) => {
                    buf.truncate(n);
                    Some((
                        Ok::<_, std::io::Error>(Frame::data(Bytes::from(buf))),
                        (file, remaining - n as u64, permit),
                    ))
                }
                Err(e) => {
                    eprintln!("warn: file stream I/O error: {e}");
                    Some((Err(e), (file, 0, permit)))
                }
            }
        },
    );
    StreamBody::new(Box::pin(stream)
        as Pin<
            Box<dyn futures_util::Stream<Item = Result<Frame<Bytes>, std::io::Error>> + Send + Sync>,
        >)
}

fn valid_http_status(status: u16) -> bool {
    eggserve_core::primitives::canonical::StatusCode::new(status).is_ok()
}

fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn validate_handler_response(resp: &PyResponse) -> bool {
    if !valid_http_status(resp.status) {
        return false;
    }
    if resp.status < 200 {
        return false;
    }
    for (name, value) in &resp.headers {
        if name.is_empty() || hyper::header::HeaderName::from_bytes(name.as_bytes()).is_err() {
            return false;
        }
        for &b in value.as_bytes() {
            if b == 0 || b == b'\r' || b == b'\n' {
                return false;
            }
        }
        if is_hop_by_hop_header(name) {
            return false;
        }
    }
    if matches!(resp.status, 204 | 304)
        && !matches!(resp.body, PyResponseBody::Empty)
    {
        return false;
    }
    true
}

fn fallback_500() -> Result<Response<BoxBody<Bytes, BoxError>>, BoxError> {
    let body = Full::new(Bytes::from("Internal Server Error"))
        .map_err(|e| -> BoxError { Box::new(e) })
        .boxed();
    Response::builder()
        .status(500u16)
        .header("content-type", "text/plain")
        .body(body)
        .map_err(|e| -> BoxError { Box::new(e) })
}

async fn convert_to_hyper_response(
    resp: PyResponse,
    file_stream_semaphore: Option<Arc<Semaphore>>,
    suppress_body: bool,
) -> Result<Response<BoxBody<Bytes, BoxError>>, BoxError> {
    if !validate_handler_response(&resp) {
        return fallback_500();
    }

    // For non-file bodies, use the canonical normalization path.
    let is_file_body = matches!(
        &resp.body,
        PyResponseBody::BodySource(BodySource::FileFull { .. } | BodySource::FileRange { .. })
    );

    if !is_file_body {
        let body_bytes = match &resp.body {
            PyResponseBody::Empty => Vec::new(),
            PyResponseBody::Bytes(data) => data.clone(),
            PyResponseBody::BodySource(BodySource::Empty) => Vec::new(),
            PyResponseBody::BodySource(BodySource::Bytes(data)) => data.clone(),
            _ => unreachable!(),
        };

        // Build canonical response with headers from the handler.
        let code = eggserve_core::primitives::canonical::StatusCode::new(resp.status)
            .map_err(|e| -> BoxError { e.into() })?;
        let mut canonical = eggserve_core::primitives::canonical::Response::builder()
            .status(code)
            .body(eggserve_core::primitives::canonical::ResponseBody::Bytes(body_bytes))
            .map_err(|e| -> BoxError { e.into() })?;

        // Copy handler headers into canonical response.
        for (name, value) in &resp.headers {
            if let (Ok(n), Ok(v)) = (
                eggserve_core::primitives::header_block::HeaderName::new(name.as_str()),
                eggserve_core::primitives::header_block::HeaderValue::new(value.as_str()),
            ) {
                canonical.head_mut().headers_mut().push(n, v);
            }
        }

        // Normalize: HEAD suppression, body-forbidden, hop-by-hop, content-length.
        let norm_req = eggserve_core::primitives::canonical::NormalizeRequest::new(suppress_body);
        let normalized =
            eggserve_core::primitives::canonical::normalize_response(canonical, &norm_req)
                .map_err(|e| -> BoxError { e.into() })?;

        // Convert to Hyper response.
        let hyper_status = hyper::StatusCode::from_u16(normalized.status().as_u16())
            .map_err(|e| -> BoxError { e.into() })?;
        let mut builder = Response::builder().status(hyper_status);
        for field in normalized.headers().iter() {
            builder = builder.header(field.name.as_str(), field.value.as_str());
        }
        let body = match normalized.body() {
            Some(eggserve_core::primitives::canonical::ResponseBody::Bytes(b)) => {
                Full::new(Bytes::from(b.clone()))
                    .map_err(|e| -> BoxError { Box::new(e) })
                    .boxed()
            }
            _ => Full::new(Bytes::new())
                .map_err(|e| -> BoxError { Box::new(e) })
                .boxed(),
        };
        return Ok(builder.body(body)?);
    }

    // File-backed body path (preserves streaming + semaphore).
    let file_body = match &resp.body {
        PyResponseBody::BodySource(BodySource::FileFull { len, mime, .. }) => {
            Some((false, *len, *mime, None))
        }
        PyResponseBody::BodySource(BodySource::FileRange {
            range,
            total_len,
            mime,
            ..
        }) => {
            let range_len = match range
                .end_inclusive
                .checked_sub(range.start)
                .and_then(|length| length.checked_add(1))
            {
                Some(length) => length,
                None => return fallback_500(),
            };
            Some((true, range_len, *mime, Some((*range, *total_len))))
        }
        _ => None,
    };
    let mut builder = Response::builder().status(resp.status);
    for (name, value) in &resp.headers {
        if let Some((is_range, _, _, _)) = file_body {
            let is_body_header = name.eq_ignore_ascii_case("content-length")
                || name.eq_ignore_ascii_case("content-type")
                || (is_range && name.eq_ignore_ascii_case("content-range"));
            if is_body_header {
                continue;
            }
        }
        builder = builder.header(name.as_str(), value.as_str());
    }
    if let Some((_, file_len, mime, range_info)) = file_body {
        builder = builder
            .header("content-type", mime)
            .header("content-length", file_len.to_string());
        if let Some((range, total_len)) = range_info {
            builder = builder
                .status(206u16)
                .header(
                    "content-range",
                    format!(
                        "bytes {}-{}/{}",
                        range.start, range.end_inclusive, total_len
                    ),
                );
        }
    }

    let strip_body = suppress_body || matches!(resp.status, 204 | 304);

    if strip_body {
        let body = Full::new(Bytes::new())
            .map_err(|e| -> BoxError { Box::new(e) })
            .boxed();
        return Ok(builder.body(body)?);
    }

    match resp.body {
        PyResponseBody::Empty => {
            let body = Full::new(Bytes::new())
                .map_err(|e| -> BoxError { Box::new(e) })
                .boxed();
            Ok(builder.body(body)?)
        }
        PyResponseBody::Bytes(data) => {
            let body = Full::new(Bytes::from(data))
                .map_err(|e| -> BoxError { Box::new(e) })
                .boxed();
            Ok(builder.body(body)?)
        }
        PyResponseBody::BodySource(body_source) => match body_source {
            BodySource::Empty => {
                let body = Full::new(Bytes::new())
                    .map_err(|e| -> BoxError { Box::new(e) })
                    .boxed();
                Ok(builder.body(body)?)
            }
            BodySource::Bytes(data) => {
                let body = Full::new(Bytes::from(data))
                    .map_err(|e| -> BoxError { Box::new(e) })
                    .boxed();
                Ok(builder.body(body)?)
            }
            BodySource::FileFull { file, .. } => {
                let permit = if let Some(sem) = &file_stream_semaphore {
                    Some(
                        sem.clone()
                            .acquire_owned()
                            .await
                            .map_err(|_| -> BoxError { "file stream semaphore closed".into() })?,
                    )
                } else {
                    None
                };
                let tokio_file = tokio::fs::File::from_std(file);
                let body = stream_file(tokio_file, None, permit)
                    .map_err(|e| -> BoxError { Box::new(e) })
                    .boxed();
                Ok(builder.body(body)?)
            }
            BodySource::FileRange {
                file,
                range,
                ..
            } => {
                let permit = if let Some(sem) = &file_stream_semaphore {
                    Some(
                        sem.clone()
                            .acquire_owned()
                            .await
                            .map_err(|_| -> BoxError { "file stream semaphore closed".into() })?,
                    )
                } else {
                    None
                };
                let start = range.start;
                let range_len = file_body
                    .map(|(_, length, _, _)| length)
                    .ok_or_else(|| -> BoxError { "missing file range metadata".into() })?;

                let mut tokio_file = tokio::fs::File::from_std(file);
                tokio_file.seek(std::io::SeekFrom::Start(start)).await?;
                let body = stream_file(tokio_file, Some(range_len), permit)
                    .map_err(|e| -> BoxError { Box::new(e) })
                    .boxed();
                Ok(builder.body(body)?)
            }
        },
    }
}
