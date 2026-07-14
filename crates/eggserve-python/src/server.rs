use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use tokio::sync::Semaphore;

use eggserve_core::policy;
use eggserve_core::primitives::body::BodySource;
use eggserve_core::primitives::canonical::{
    normalize_response, NormalizeRequest, Response as CanonicalResponse, ResponseBody,
    StatusCode as CanonicalStatusCode,
};
use eggserve_core::primitives::header_block::{HeaderName, HeaderValue};
use eggserve_core::primitives::http::ReadOnlyMethod;
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
        Ok(Self { inner: root, policy: static_policy })
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

// ---------------------------------------------------------------------------
// Python callback service adapter
// ---------------------------------------------------------------------------

struct PythonCallbackService {
    handler: Arc<std::sync::Mutex<Option<Py<PyAny>>>>,
    callback_semaphore: Arc<Semaphore>,
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

            let py_req_obj = py_request
                .into_pyobject(py)
                .map_err(|e| ServiceError::internal(format!("failed to create request: {e}")))?;

            let result = handler_py.bind(py).call1((py_req_obj,)).map_err(|e| {
                eprintln!("Handler error: {e}");
                ServiceError::internal("handler raised an exception")
            })?;

            if result.hasattr("__await__").unwrap_or(false) {
                return Err(ServiceError::internal(
                    "handler returned a coroutine; async handlers are not supported",
                ));
            }

            convert_python_response_to_canonical(py, &result)
        })
    }
}

fn convert_python_response_to_canonical<'py>(
    _py: Python<'py>,
    obj: &Bound<'py, PyAny>,
) -> Result<CanonicalResponse, ServiceError> {
    let status: u16 = obj
        .getattr("status")
        .and_then(|v| v.extract())
        .unwrap_or(500);
    let status = if CanonicalStatusCode::new(status).is_ok() {
        status
    } else {
        500
    };

    let headers: HashMap<String, String> = obj
        .getattr("headers")
        .and_then(|v| v.extract())
        .unwrap_or_default();

    let body = if let Ok(py_resp) = obj.extract::<pyo3::Bound<'_, PyResponse>>() {
        match &py_resp.borrow().body {
            PyResponseBody::Empty => ResponseBody::Empty,
            PyResponseBody::Bytes(data) => ResponseBody::Bytes(data.clone()),
            PyResponseBody::BodySource(BodySource::Bytes(data)) => {
                ResponseBody::Bytes(data.clone())
            }
            PyResponseBody::BodySource(BodySource::Empty) => ResponseBody::Empty,
            _ => ResponseBody::Empty,
        }
    } else {
        match obj.getattr("body") {
            Ok(b) => {
                let kind: String = b
                    .getattr("kind")
                    .and_then(|v| v.extract())
                    .unwrap_or_default();
                match kind.as_str() {
                    "bytes" => {
                        let data: Vec<u8> = b
                            .call_method0("read_all")
                            .and_then(|v| v.extract())
                            .unwrap_or_default();
                        ResponseBody::Bytes(data)
                    }
                    _ => ResponseBody::Empty,
                }
            }
            Err(_) => ResponseBody::Empty,
        }
    };

    let code = CanonicalStatusCode::new(status)
        .map_err(|e| ServiceError::internal(format!("invalid status code: {e}")))?;

    let mut response = CanonicalResponse::builder()
        .status(code)
        .body(body)
        .map_err(|e| ServiceError::internal(format!("failed to build response: {e}")))?;

    for (name, value) in &headers {
        if let (Ok(n), Ok(v)) = (
            HeaderName::new(name.as_str()),
            HeaderValue::new(value.as_str()),
        ) {
            response.head_mut().headers_mut().push(n, v);
        }
    }

    let norm_req = NormalizeRequest::new(false);
    normalize_response(response, &norm_req)
        .map_err(|e| ServiceError::internal(format!("response normalization failed: {e}")))
}

impl Service for PythonCallbackService {
    fn call(
        &self,
        head: RequestHead,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<CanonicalResponse, ServiceError>> + Send + '_>,
    > {
        let handler = self.handler.clone();
        let callback_semaphore = self.callback_semaphore.clone();

        Box::pin(async move {
            let _callback_permit = callback_semaphore
                .acquire_owned()
                .await
                .map_err(|_| ServiceError::internal("callback semaphore closed"))?;

            let method_str = head.method().as_str().to_string();
            let target = head.target().path().to_string();
            let query = head.target().query().unwrap_or("").to_string();
            let has_body = false;

            let py_request = PyRequest {
                method: method_str,
                path: target,
                query,
                headers: head
                    .headers()
                    .iter()
                    .map(|f| (f.name.to_string(), f.value.to_string()))
                    .collect(),
                remote_addr: None,
                http_version: format!("{:?}", head.version()),
                has_body,
            };

            tokio::task::spawn_blocking(move || Self::call_python_callback(&handler, py_request))
                .await
                .map_err(|e| ServiceError::internal(format!("callback task failed: {e}")))?
        })
    }
}

// ---------------------------------------------------------------------------
// Python Server — delegates to Rust runtime
// ---------------------------------------------------------------------------

#[pyclass(name = "Server")]
#[allow(dead_code)]
pub struct PyServer {
    bind: String,
    port: u16,
    public: bool,
    addr: std::sync::Mutex<Option<String>>,
    responder: PyStaticResponder,
    handler: Option<std::sync::Mutex<Option<Py<PyAny>>>>,
    handle: std::sync::Mutex<Option<ServerHandle>>,
    runtime: std::sync::Mutex<Option<tokio::runtime::Runtime>>,
    has_been_started: std::sync::atomic::AtomicBool,
    max_connections: usize,
    max_file_streams: usize,
    max_python_callbacks: usize,
    header_timeout: Duration,
    write_timeout: Duration,
    handler_timeout: Duration,
    graceful_shutdown_timeout: Duration,
    lifecycle: Arc<eggserve_core::server::lifecycle::Lifecycle>,
}

#[pymethods]
impl PyServer {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (root, bind="127.0.0.1", port=8000, policy=None, handler=None, public=false, max_connections=100, max_file_streams=64, max_python_callbacks=8, header_timeout_secs=10, write_timeout_secs=30, handler_timeout_secs=30, graceful_shutdown_timeout_secs=10))]
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
        handler_timeout_secs: u64,
        graceful_shutdown_timeout_secs: u64,
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

        let static_policy = policy
            .map(|p| p.inner)
            .unwrap_or_else(StaticPolicy::safe_default);
        let secure_root = SecureRoot::new(root, static_policy.clone()).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("failed to create secure root: {e}"))
        })?;
        let responder = PyStaticResponder { root: secure_root, policy: static_policy };

        Ok(Self {
            bind: bind.to_string(),
            port,
            public,
            addr: std::sync::Mutex::new(None),
            responder,
            handler: handler.map(|h| std::sync::Mutex::new(Some(h))),
            handle: std::sync::Mutex::new(None),
            runtime: std::sync::Mutex::new(None),
            has_been_started: std::sync::atomic::AtomicBool::new(false),
            max_connections,
            max_file_streams,
            max_python_callbacks,
            header_timeout: Duration::from_secs(header_timeout_secs),
            write_timeout: Duration::from_secs(write_timeout_secs),
            handler_timeout: Duration::from_secs(handler_timeout_secs),
            graceful_shutdown_timeout: Duration::from_secs(graceful_shutdown_timeout_secs),
            lifecycle: Arc::new(eggserve_core::server::lifecycle::Lifecycle::new()),
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
        } else if self.has_been_started.load(std::sync::atomic::Ordering::Acquire) {
            Ok("stopped".to_string())
        } else {
            Ok(self.lifecycle.state().to_string())
        }
    }

    fn start(&self, py: Python<'_>) -> PyResult<()> {
        let mut handle_guard = self
            .handle
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        if handle_guard.is_some() {
            return Err(crate::LifecycleError::new_err("Server already started"));
        }

        let mut runtime_guard = self
            .runtime
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        if runtime_guard.is_some() {
            return Err(crate::LifecycleError::new_err("Server already started"));
        }

        let bind_addr: SocketAddr = format!("{}:{}", self.bind, self.port)
            .parse()
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("invalid bind address"))?;

        let runtime_config = RuntimeConfig::builder()
            .bind(bind_addr)
            .max_connections(self.max_connections)
            .max_file_streams(self.max_file_streams)
            .header_read_timeout(self.header_timeout)
            .response_write_timeout(self.write_timeout)
            .handler_timeout(self.handler_timeout)
            .graceful_shutdown_timeout(self.graceful_shutdown_timeout)
            .build();

        let serve_config = Arc::new(eggserve_core::config::ServeConfig {
            root: self.responder.root.root_path().to_path_buf(),
            static_policy: self.responder.policy.clone(),
            ..eggserve_core::config::ServeConfig::default()
        });

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        let server_handle = py.allow_threads(|| -> PyResult<ServerHandle> {
            rt.block_on(async {
                if let Some(handler_arc) = &self.handler {
                    let cloned_handler = handler_arc
                        .lock()
                        .map_err(|_| {
                            pyo3::exceptions::PyRuntimeError::new_err("handler lock poisoned")
                        })?
                        .as_ref()
                        .map(|h| Python::with_gil(|py| h.clone_ref(py)))
                        .ok_or_else(|| {
                            pyo3::exceptions::PyRuntimeError::new_err("handler already consumed")
                        })?;

                    let shared_handler = Arc::new(std::sync::Mutex::new(Some(cloned_handler)));
                    let service = PythonCallbackService {
                        handler: shared_handler,
                        callback_semaphore: Arc::new(Semaphore::new(self.max_python_callbacks)),
                    };

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

                    let handle = server.start_with_service(service).await.map_err(|e| {
                        pyo3::exceptions::PyRuntimeError::new_err(format!(
                            "failed to start server: {e}"
                        ))
                    })?;
                    handle.ready().await.map_err(|e| {
                        pyo3::exceptions::PyRuntimeError::new_err(format!(
                            "server failed during startup: {e}"
                        ))
                    })?;
                    Ok(handle)
                } else {
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
                    handle.ready().await.map_err(|e| {
                        pyo3::exceptions::PyRuntimeError::new_err(format!(
                            "server failed during startup: {e}"
                        ))
                    })?;
                    Ok(handle)
                }
            })
        })?;

        let local_addr = server_handle.local_addr();

        *self
            .addr
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))? =
            Some(local_addr.to_string());

        *runtime_guard = Some(rt);
        drop(runtime_guard);

        *handle_guard = Some(server_handle);
        self.has_been_started
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    fn stop(&self, py: Python<'_>) -> PyResult<()> {
        let mut handle_guard = self
            .handle
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        if let Some(handle) = handle_guard.take() {
            handle.shutdown();
            let runtime_guard = self
                .runtime
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            if let Some(rt) = runtime_guard.as_ref() {
                py.allow_threads(|| {
                    rt.block_on(async {
                        let _ = tokio::time::timeout(
                            self.graceful_shutdown_timeout + Duration::from_secs(2),
                            handle.wait(),
                        )
                        .await;
                    });
                });
            }
            drop(runtime_guard);
        }
        drop(handle_guard);

        let mut runtime_guard = self
            .runtime
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        *runtime_guard = None;
        drop(runtime_guard);

        *self
            .addr
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))? = None;
        Ok(())
    }

    fn wait_ready(&self, py: Python<'_>) -> PyResult<()> {
        let handle_guard = self
            .handle
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        let handle = handle_guard
            .as_ref()
            .ok_or_else(|| crate::LifecycleError::new_err("server not started"))?;

        let state = handle.state();
        match state {
            LifecycleState::Running => Ok(()),
            LifecycleState::Created => Err(crate::LifecycleError::new_err("server not started")),
            LifecycleState::Starting => {
                let runtime_guard = self
                    .runtime
                    .lock()
                    .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
                if let Some(rt) = runtime_guard.as_ref() {
                    py.allow_threads(|| {
                        rt.block_on(async {
                            let _ = handle.ready().await;
                        });
                        Ok::<(), PyErr>(())
                    })?;
                } else {
                    return Err(crate::LifecycleError::new_err("server not started"));
                }
                drop(runtime_guard);
                drop(handle_guard);
                let state = self.lifecycle.state();
                if state == LifecycleState::Running {
                    Ok(())
                } else if state == LifecycleState::Failed {
                    Err(crate::LifecycleError::new_err(
                        "server failed during startup",
                    ))
                } else {
                    Ok(())
                }
            }
            LifecycleState::Stopped | LifecycleState::Failed | LifecycleState::Draining => {
                Err(crate::LifecycleError::new_err("server is not running"))
            }
        }
    }

    fn shutdown(&self) -> PyResult<()> {
        self.lifecycle.drain().ok();
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

        let mut handle_guard = self
            .handle
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        if let Some(handle) = handle_guard.take() {
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

            let deadline = std::time::Instant::now() + timeout;
            while !self.lifecycle.state().is_terminal() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }

            match result {
                Some(ShutdownResult::Clean) => Ok("clean".to_string()),
                _ => Ok("timeout".to_string()),
            }
        } else {
            Ok("clean".to_string())
        }
    }

    fn wait(&self, py: Python<'_>) -> PyResult<String> {
        let mut handle_guard = self
            .handle
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
        if let Some(handle) = handle_guard.take() {
            let runtime_guard = self
                .runtime
                .lock()
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("lock poisoned"))?;
            if let Some(rt) = runtime_guard.as_ref() {
                py.allow_threads(|| {
                    rt.block_on(async {
                        let _ = handle.wait().await;
                    });
                    Ok::<(), PyErr>(())
                })?;
            }
            drop(runtime_guard);
            Ok("stopped".to_string())
        } else {
            Ok("stopped".to_string())
        }
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
