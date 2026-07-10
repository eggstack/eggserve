use std::collections::HashMap;
use std::time::Duration;

use pyo3::prelude::*;
use pyo3::types::PyBytes;

use eggserve_core::primitives::client::{
    ClientConfig as RustClientConfig, ClientError, ClientRequestBuilder,
    ClientResponse as RustClientResponse, HttpClient as RustHttpClient, Method as RustMethod,
};

#[pyclass(frozen, name = "ClientError")]
#[derive(Debug)]
pub enum PyClientError {
    InvalidUrl { reason: String },
    UnsupportedScheme { scheme: String },
    MissingHost(),
    InvalidHeader { reason: String },
    BodyTooLarge { limit: u64, actual: u64 },
    Timeout { reason: String },
    DnsError { reason: String },
    ConnectError { reason: String },
    TlsError { reason: String },
    ProtocolError { reason: String },
    ResponseBodyTooLarge { limit: u64 },
    Io { reason: String },
}

impl std::fmt::Display for PyClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUrl { reason } => write!(f, "invalid URL: {reason}"),
            Self::UnsupportedScheme { scheme } => write!(f, "unsupported scheme: {scheme}"),
            Self::MissingHost() => write!(f, "missing host in URL"),
            Self::InvalidHeader { reason } => write!(f, "invalid header: {reason}"),
            Self::BodyTooLarge { limit, actual } => {
                write!(f, "request body too large: {actual} bytes (limit: {limit})")
            }
            Self::Timeout { reason } => write!(f, "timeout: {reason}"),
            Self::DnsError { reason } => write!(f, "DNS error: {reason}"),
            Self::ConnectError { reason } => write!(f, "connection error: {reason}"),
            Self::TlsError { reason } => write!(f, "TLS error: {reason}"),
            Self::ProtocolError { reason } => write!(f, "protocol error: {reason}"),
            Self::ResponseBodyTooLarge { limit } => {
                write!(f, "response body too large: limit={limit}")
            }
            Self::Io { reason } => write!(f, "I/O error: {reason}"),
        }
    }
}

impl std::error::Error for PyClientError {}

impl From<ClientError> for PyClientError {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::InvalidUrl(r) => Self::InvalidUrl { reason: r },
            ClientError::UnsupportedScheme(s) => Self::UnsupportedScheme { scheme: s },
            ClientError::MissingHost => Self::MissingHost(),
            ClientError::InvalidHeader(r) => Self::InvalidHeader { reason: r },
            ClientError::BodyTooLarge { limit, actual } => Self::BodyTooLarge { limit, actual },
            ClientError::Timeout(r) => Self::Timeout { reason: r },
            ClientError::DnsError(r) => Self::DnsError { reason: r },
            ClientError::ConnectError(r) => Self::ConnectError { reason: r },
            ClientError::TlsError(r) => Self::TlsError { reason: r },
            ClientError::ProtocolError(r) => Self::ProtocolError { reason: r },
            ClientError::ResponseBodyTooLarge { limit } => Self::ResponseBodyTooLarge { limit },
            ClientError::Io(err) => Self::Io {
                reason: err.to_string(),
            },
        }
    }
}

impl From<PyClientError> for PyErr {
    fn from(e: PyClientError) -> Self {
        super::EggserveError::new_err(e.to_string())
    }
}

#[pyclass(frozen, name = "Method")]
#[derive(Debug, Clone, Copy)]
pub enum PyMethod {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Patch,
}

impl From<PyMethod> for RustMethod {
    fn from(m: PyMethod) -> Self {
        match m {
            PyMethod::Get => RustMethod::Get,
            PyMethod::Head => RustMethod::Head,
            PyMethod::Post => RustMethod::Post,
            PyMethod::Put => RustMethod::Put,
            PyMethod::Delete => RustMethod::Delete,
            PyMethod::Patch => RustMethod::Patch,
        }
    }
}

#[pyclass(frozen, name = "ClientConfig")]
#[derive(Debug, Clone)]
pub struct PyClientConfig {
    #[pyo3(get)]
    pub connect_timeout: f64,
    #[pyo3(get)]
    pub request_timeout: f64,
    #[pyo3(get)]
    pub max_response_body_bytes: Option<u64>,
    #[pyo3(get)]
    pub verify_tls: bool,
}

#[pymethods]
impl PyClientConfig {
    #[new]
    #[pyo3(signature = (connect_timeout=10.0, request_timeout=30.0, max_response_body_bytes=Some(10_485_760), verify_tls=true))]
    fn new(
        connect_timeout: f64,
        request_timeout: f64,
        max_response_body_bytes: Option<u64>,
        verify_tls: bool,
    ) -> PyResult<Self> {
        if connect_timeout.is_nan() || connect_timeout.is_infinite() || connect_timeout < 0.0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "connect_timeout must be a non-negative finite number",
            ));
        }
        if request_timeout.is_nan() || request_timeout.is_infinite() || request_timeout < 0.0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "request_timeout must be a non-negative finite number",
            ));
        }
        if let Some(limit) = max_response_body_bytes {
            if limit == 0 {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "max_response_body_bytes must be greater than zero",
                ));
            }
        }
        Ok(Self {
            connect_timeout,
            request_timeout,
            max_response_body_bytes,
            verify_tls,
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "ClientConfig(connect_timeout={}, request_timeout={}, max_response_body_bytes={:?}, verify_tls={})",
            self.connect_timeout, self.request_timeout, self.max_response_body_bytes, self.verify_tls
        )
    }
}

impl From<&PyClientConfig> for RustClientConfig {
    fn from(c: &PyClientConfig) -> Self {
        Self {
            connect_timeout: Duration::from_secs_f64(c.connect_timeout),
            request_timeout: Duration::from_secs_f64(c.request_timeout),
            max_response_body_bytes: c.max_response_body_bytes,
            verify_tls: c.verify_tls,
        }
    }
}

#[pyclass(frozen, name = "ClientRequest")]
#[derive(Debug, Clone)]
pub struct PyClientRequest {
    #[pyo3(get)]
    pub method: PyMethod,
    #[pyo3(get)]
    pub url: String,
    #[pyo3(get)]
    pub headers: HashMap<String, String>,
    #[pyo3(get)]
    pub body: Option<Vec<u8>>,
}

#[pymethods]
impl PyClientRequest {
    fn __repr__(&self) -> String {
        format!("ClientRequest(method={:?}, url={})", self.method, self.url)
    }
}

#[pyclass(frozen, name = "ClientResponse")]
#[derive(Debug, Clone)]
pub struct PyClientResponse {
    #[pyo3(get)]
    pub status: u16,
    #[pyo3(get)]
    pub headers: HashMap<String, String>,
    #[pyo3(get)]
    pub body: Vec<u8>,
}

#[pymethods]
impl PyClientResponse {
    fn text(&self) -> PyResult<String> {
        String::from_utf8(self.body.clone())
            .map_err(|e| pyo3::exceptions::PyUnicodeDecodeError::new_err(e.to_string()))
    }

    fn bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(py, &self.body))
    }

    fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    fn content_length(&self) -> Option<u64> {
        self.headers
            .get("content-length")
            .and_then(|v| v.parse().ok())
    }

    fn content_type(&self) -> Option<&str> {
        self.headers.get("content-type").map(|s| s.as_str())
    }

    fn __repr__(&self) -> String {
        format!(
            "ClientResponse(status={}, body_len={})",
            self.status,
            self.body.len()
        )
    }
}

impl From<RustClientResponse> for PyClientResponse {
    fn from(r: RustClientResponse) -> Self {
        Self {
            status: r.status,
            headers: r.headers,
            body: r.body,
        }
    }
}

#[pyclass(frozen, name = "HttpClient")]
pub struct PyHttpClient {
    inner: RustHttpClient,
}

#[pymethods]
impl PyHttpClient {
    #[new]
    #[pyo3(signature = (config=None))]
    fn new(config: Option<PyClientConfig>) -> Self {
        let rust_config = config
            .as_ref()
            .map(RustClientConfig::from)
            .unwrap_or_default();
        Self {
            inner: RustHttpClient::new(rust_config),
        }
    }

    fn send(&self, request: &PyClientRequest) -> PyResult<PyClientResponse> {
        let method: RustMethod = request.method.into();

        let mut builder = ClientRequestBuilder::new(method)
            .url(&request.url)
            .map_err(PyClientError::from)?;

        for (name, value) in &request.headers {
            builder = builder
                .header(name.as_str(), value.as_str())
                .map_err(PyClientError::from)?;
        }

        if let Some(body) = &request.body {
            builder = builder.body(body.clone());
        }

        let req = builder.build().map_err(PyClientError::from)?;
        let resp = self.inner.send(&req).map_err(PyClientError::from)?;
        Ok(PyClientResponse::from(resp))
    }

    fn get(&self, url: &str) -> PyResult<PyClientResponse> {
        let req = ClientRequestBuilder::new(RustMethod::Get)
            .url(url)
            .map_err(PyClientError::from)?
            .build()
            .map_err(PyClientError::from)?;
        let resp = self.inner.send(&req).map_err(PyClientError::from)?;
        Ok(PyClientResponse::from(resp))
    }

    fn head(&self, url: &str) -> PyResult<PyClientResponse> {
        let req = ClientRequestBuilder::new(RustMethod::Head)
            .url(url)
            .map_err(PyClientError::from)?
            .build()
            .map_err(PyClientError::from)?;
        let resp = self.inner.send(&req).map_err(PyClientError::from)?;
        Ok(PyClientResponse::from(resp))
    }

    fn post(&self, url: &str, body: Option<Vec<u8>>) -> PyResult<PyClientResponse> {
        let mut builder = ClientRequestBuilder::new(RustMethod::Post)
            .url(url)
            .map_err(PyClientError::from)?;
        if let Some(b) = body {
            builder = builder.body(b);
        }
        let req = builder.build().map_err(PyClientError::from)?;
        let resp = self.inner.send(&req).map_err(PyClientError::from)?;
        Ok(PyClientResponse::from(resp))
    }

    fn put(&self, url: &str, body: Option<Vec<u8>>) -> PyResult<PyClientResponse> {
        let mut builder = ClientRequestBuilder::new(RustMethod::Put)
            .url(url)
            .map_err(PyClientError::from)?;
        if let Some(b) = body {
            builder = builder.body(b);
        }
        let req = builder.build().map_err(PyClientError::from)?;
        let resp = self.inner.send(&req).map_err(PyClientError::from)?;
        Ok(PyClientResponse::from(resp))
    }

    fn delete(&self, url: &str) -> PyResult<PyClientResponse> {
        let req = ClientRequestBuilder::new(RustMethod::Delete)
            .url(url)
            .map_err(PyClientError::from)?
            .build()
            .map_err(PyClientError::from)?;
        let resp = self.inner.send(&req).map_err(PyClientError::from)?;
        Ok(PyClientResponse::from(resp))
    }

    fn patch(&self, url: &str, body: Option<Vec<u8>>) -> PyResult<PyClientResponse> {
        let mut builder = ClientRequestBuilder::new(RustMethod::Patch)
            .url(url)
            .map_err(PyClientError::from)?;
        if let Some(b) = body {
            builder = builder.body(b);
        }
        let req = builder.build().map_err(PyClientError::from)?;
        let resp = self.inner.send(&req).map_err(PyClientError::from)?;
        Ok(PyClientResponse::from(resp))
    }

    fn __repr__(&self) -> String {
        "HttpClient()".to_string()
    }
}

#[pymethods]
impl PyClientError {
    fn __repr__(&self) -> String {
        format!("ClientError({self})")
    }
}
