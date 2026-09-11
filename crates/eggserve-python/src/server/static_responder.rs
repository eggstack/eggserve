//! Python static responder composition (Plan 206 Track B).
//!
//! Owns `PyStaticResponder` + policy/root/body-source wrappers and static
//! helpers. Caller-owned composition only (no routing); never mixed with
//! event-loop scheduling (see `runtime`).

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
use super::request_bridge::PyRequest;
#[allow(unused_imports)]
use super::response_bridge::{PyResponse, PyResponseBody};

#[pyclass(frozen, name = "StaticResponder")]
#[derive(Debug, Clone)]
pub struct PyStaticResponder {
    pub(super) root: SecureRoot,
    pub(super) policy: StaticPolicy,
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

pub(super) fn file_suffix(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .rsplit_once('.')
        .map(|(_, suffix)| format!(".{suffix}"))
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub(super) fn build_response(
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

pub(super) fn validate_extra_response_headers(
    default_content_type: &str,
    headers: &[(String, String)],
) -> PyResult<()> {
    eggserve_core::config::validate_static_metadata(
        default_content_type,
        headers,
    )
    .map_err(pyo3::exceptions::PyValueError::new_err)
}

pub(super) fn apply_static_metadata(
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
            .map(|v| v.to_str().unwrap_or("").to_owned())
            .unwrap_or_else(|_| value.clone());
            response.extra_headers.push((name.clone(), canonical));
        }
    }
    Ok(())
}

pub(super) fn validate_response_status(status: u16) -> PyResult<()> {
    if (100..600).contains(&status) {
        Ok(())
    } else {
        Err(pyo3::exceptions::PyValueError::new_err(format!(
            "status code {status} is outside 100-599"
        )))
    }
}

pub(super) fn build_error_response(status: u16, reason: &str) -> PyResult<PyResponse> {
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

pub(super) fn directory_listing_bytes(entries: &[(String, bool)]) -> Vec<u8> {
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
    pub(super) inner: StaticPolicy,
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
    pub(super) policy: StaticPolicy,
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

