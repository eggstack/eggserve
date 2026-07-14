use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple};

use eggserve_core::policy::{
    DirectoryListingPolicy, DotfilePolicy, StaticPolicy as RustStaticPolicy, SymlinkPolicy,
};
use eggserve_core::primitives::body::{BodyKind as RustBodyKind, BodySource as RustBodySource};
use eggserve_core::primitives::header_block::{HeaderBlock as RustHeaderBlock};
use eggserve_core::primitives::http::{self, ReadOnlyMethod};
use eggserve_core::primitives::method::Method as RustMethod;
use eggserve_core::primitives::planner;
use eggserve_core::primitives::response::BodyPlan;
use eggserve_core::primitives::version::HttpVersion as RustHttpVersion;
use eggserve_core::primitives::{
    ConfinedPath, PathDotfilePolicy, PathPolicy, PathRejection,
    ResolvedResource as RustResolvedResource, ResourceDeniedReason, SecureRoot as RustSecureRoot,
};

mod client;
mod server;

// ---------------------------------------------------------------------------
// Exceptions
// ---------------------------------------------------------------------------

pyo3::create_exception!(
    _native,
    EggserveError,
    pyo3::exceptions::PyException,
    "Base exception for eggserve native primitives."
);

pyo3::create_exception!(
    _native,
    PathPolicyError,
    EggserveError,
    "Path validation or confinement error."
);

pyo3::create_exception!(
    _native,
    RequestTargetError,
    EggserveError,
    "Malformed or unsupported request target."
);

pyo3::create_exception!(
    _native,
    SecureRootError,
    EggserveError,
    "Secure root initialization or resolution error."
);

pyo3::create_exception!(
    _native,
    RequestValidationError,
    EggserveError,
    "Request validation error."
);

pyo3::create_exception!(
    _native,
    BodySourceError,
    EggserveError,
    "Body source conversion error."
);

pyo3::create_exception!(
    _native,
    ResponseConstructionError,
    EggserveError,
    "Response construction or validation error."
);

pyo3::create_exception!(
    _native,
    LifecycleError,
    EggserveError,
    "Server lifecycle error (double start, stop before start, etc.)."
);

pyo3::create_exception!(
    _native,
    MethodError,
    EggserveError,
    "Invalid HTTP method."
);

pyo3::create_exception!(
    _native,
    HttpVersionError,
    EggserveError,
    "Unsupported HTTP version."
);

pyo3::create_exception!(
    _native,
    HeaderError,
    EggserveError,
    "Invalid header name or value."
);

pyo3::create_exception!(
    _native,
    DuplicateHeaderError,
    EggserveError,
    "Duplicate header encountered on unique access."
);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn path_rejection_code(rejection: &PathRejection) -> &'static str {
    match rejection {
        PathRejection::Empty => "empty_path",
        PathRejection::TooLong => "path_too_long",
        PathRejection::UnsupportedUriForm => "unsupported_uri_form",
        PathRejection::MalformedPercentEncoding => "malformed_percent_encoding",
        PathRejection::InvalidUtf8 => "invalid_utf8",
        PathRejection::NulByte => "nul_byte",
        PathRejection::AbsolutePath => "absolute_path",
        PathRejection::ParentComponent => "traversal_denied",
        PathRejection::CurrentComponent => "current_component",
        PathRejection::SeparatorAmbiguity => "separator_ambiguity",
        PathRejection::DotfileDenied => "dotfile_denied",
        PathRejection::WindowsPrefixDenied => "windows_prefix_denied",
        PathRejection::WindowsReservedNameDenied => "windows_reserved_name_denied",
        PathRejection::WindowsAlternateStreamDenied => "windows_alternate_stream_denied",
        PathRejection::SymlinkDenied => "symlink_denied",
        PathRejection::RootEscapeDenied => "root_escape_denied",
    }
}

fn path_rejection_to_pyerr(rejection: PathRejection) -> PyErr {
    let code = path_rejection_code(&rejection);
    let msg = rejection.to_string();
    match code {
        "traversal_denied"
        | "dotfile_denied"
        | "separator_ambiguity"
        | "empty_path"
        | "unsupported_uri_form"
        | "nul_byte"
        | "malformed_percent_encoding" => PathPolicyError::new_err((msg, code)),
        _ => RequestTargetError::new_err((msg, code)),
    }
}

fn io_err_to_pyerr(err: std::io::Error) -> PyErr {
    SecureRootError::new_err((err.to_string(), "io_error"))
}

fn parse_method(method: &str) -> Result<ReadOnlyMethod, PyErr> {
    match method {
        "GET" => Ok(ReadOnlyMethod::Get),
        "HEAD" => Ok(ReadOnlyMethod::Head),
        _ => Err(RequestValidationError::new_err((
            format!("unsupported method: {method}"),
            "method_not_allowed",
        ))),
    }
}

fn headers_from_list(headers: Option<&Bound<'_, PyList>>) -> Result<Vec<(String, String)>, PyErr> {
    match headers {
        None => Ok(Vec::new()),
        Some(list) => {
            let mut result = Vec::with_capacity(list.len());
            for item in list.iter() {
                let pair: (String, String) = item.extract().map_err(|_| {
                    RequestValidationError::new_err((
                        "headers must be a list of (name, value) tuples",
                        "invalid_headers",
                    ))
                })?;
                result.push(pair);
            }
            Ok(result)
        }
    }
}

fn plan_to_python(py: Python<'_>, plan: StaticResponsePlan) -> PyResult<PyObject> {
    let status = plan.status_code();

    let headers_list = PyList::empty(py);
    for h in plan.headers.iter() {
        let tup = PyTuple::new(py, [h.name.as_str(), h.value.as_str()])?;
        headers_list.append(tup)?;
    }

    let body_kind = match &plan.body {
        BodyPlan::Empty => "empty",
        BodyPlan::FullBytes(_) => "bytes",
        BodyPlan::FileFull => "file_full",
        BodyPlan::FileRange { .. } => "file_range",
    };

    let range: Option<(u64, u64)> = match &plan.body {
        BodyPlan::FileRange {
            start,
            end_inclusive,
        } => Some((*start, *end_inclusive)),
        _ => None,
    };

    let range_obj: PyObject = match range {
        Some((s, e)) => PyTuple::new(py, [s, e])?.into_any().unbind(),
        None => py.None(),
    };

    let plan_cls = py.import("eggserve")?.getattr("ResponsePlan")?;
    plan_cls
        .call1((status, headers_list, body_kind, range_obj))
        .map(|b| b.unbind())
}

// ---------------------------------------------------------------------------
// BodySource (Rust wrapper for Python body source)
// ---------------------------------------------------------------------------

#[pyclass(name = "BodySource")]
struct PyBodySource {
    inner: RustBodySource,
}

#[pymethods]
impl PyBodySource {
    #[getter]
    fn kind(&self) -> &str {
        match self.inner.kind() {
            RustBodyKind::Empty => "empty",
            RustBodyKind::Bytes => "bytes",
            RustBodyKind::FileFull => "file_full",
            RustBodyKind::FileRange => "file_range",
        }
    }

    #[getter]
    fn length(&self) -> Option<u64> {
        Some(self.inner.len())
    }

    #[getter]
    fn range(&self) -> Option<(u64, u64)> {
        self.inner.range().map(|r| (r.start, r.end_inclusive))
    }

    fn read_all(&mut self) -> PyResult<Vec<u8>> {
        self.inner
            .read_all()
            .map_err(|e| BodySourceError::new_err((e.to_string(), "body_source_error")))
    }

    fn read_range(&mut self, start: u64, end_inclusive: u64) -> PyResult<Vec<u8>> {
        self.inner
            .read_range(start, end_inclusive)
            .map_err(|e| BodySourceError::new_err((e.to_string(), "body_source_error")))
    }

    fn __repr__(&self) -> String {
        match self.inner.range() {
            Some(r) => format!(
                "BodySource(kind={:?}, range=({}..={}))",
                self.inner.kind(),
                r.start,
                r.end_inclusive
            ),
            None => format!(
                "BodySource(kind={:?}, length={:?})",
                self.inner.kind(),
                self.inner.len()
            ),
        }
    }
}

fn confined_from_components(components: &[String]) -> PyResult<ConfinedPath> {
    if components.is_empty() {
        return ConfinedPath::parse("/", &PathPolicy::default()).map_err(path_rejection_to_pyerr);
    }
    let decoded = format!("/{}", components.join("/"));
    ConfinedPath::parse(&decoded, &PathPolicy::default()).map_err(path_rejection_to_pyerr)
}

// ---------------------------------------------------------------------------
// PathPolicy
// ---------------------------------------------------------------------------

#[pyclass(name = "PathPolicy", frozen)]
struct PyPathPolicy {
    inner: PathPolicy,
}

#[pymethods]
impl PyPathPolicy {
    #[new]
    #[pyo3(signature = (allow_dotfiles=false, reject_backslash=true))]
    fn py_new(allow_dotfiles: bool, reject_backslash: bool) -> Self {
        Self {
            inner: PathPolicy {
                dotfiles: if allow_dotfiles {
                    PathDotfilePolicy::Allow
                } else {
                    PathDotfilePolicy::Denied
                },
                reject_backslash,
            },
        }
    }

    #[getter]
    fn allow_dotfiles(&self) -> bool {
        self.inner.dotfiles == PathDotfilePolicy::Allow
    }

    #[getter]
    fn reject_backslash(&self) -> bool {
        self.inner.reject_backslash
    }

    fn __repr__(&self) -> String {
        format!(
            "PathPolicy(allow_dotfiles={}, reject_backslash={})",
            self.allow_dotfiles(),
            self.reject_backslash()
        )
    }
}

// ---------------------------------------------------------------------------
// StaticPolicy
// ---------------------------------------------------------------------------

#[pyclass(name = "StaticPolicy", frozen)]
struct PyStaticPolicy {
    inner: RustStaticPolicy,
}

#[pymethods]
impl PyStaticPolicy {
    #[new]
    #[pyo3(signature = (directory_listing=false, follow_symlinks=false, allow_dotfiles=false))]
    fn py_new(directory_listing: bool, follow_symlinks: bool, allow_dotfiles: bool) -> Self {
        Self {
            inner: RustStaticPolicy {
                directory_listing: if directory_listing {
                    DirectoryListingPolicy::Enabled
                } else {
                    DirectoryListingPolicy::Disabled
                },
                symlinks: if follow_symlinks {
                    SymlinkPolicy::Follow
                } else {
                    SymlinkPolicy::Denied
                },
                dotfiles: if allow_dotfiles {
                    DotfilePolicy::Serve
                } else {
                    DotfilePolicy::Denied
                },
            },
        }
    }

    #[getter]
    fn directory_listing(&self) -> bool {
        self.inner.directory_listing == DirectoryListingPolicy::Enabled
    }

    #[getter]
    fn follow_symlinks(&self) -> bool {
        self.inner.symlinks == SymlinkPolicy::Follow
    }

    #[getter]
    fn allow_dotfiles(&self) -> bool {
        self.inner.dotfiles == DotfilePolicy::Serve
    }

    fn __repr__(&self) -> String {
        format!(
            "StaticPolicy(directory_listing={}, follow_symlinks={}, allow_dotfiles={})",
            self.directory_listing(),
            self.follow_symlinks(),
            self.allow_dotfiles()
        )
    }
}

// ---------------------------------------------------------------------------
// RequestTarget
// ---------------------------------------------------------------------------

#[pyclass(name = "RequestTarget", frozen)]
struct PyRequestTarget {
    decoded: String,
    components: Vec<String>,
    confined: ConfinedPath,
}

#[pymethods]
impl PyRequestTarget {
    #[staticmethod]
    #[pyo3(signature = (raw, policy=None))]
    fn parse(raw: &str, policy: Option<&PyPathPolicy>) -> PyResult<Self> {
        let pp = match policy {
            Some(p) => p.inner.clone(),
            None => PathPolicy::default(),
        };
        let confined = ConfinedPath::parse(raw, &pp).map_err(path_rejection_to_pyerr)?;
        Ok(Self {
            decoded: confined.as_str().to_owned(),
            components: confined.components().to_vec(),
            confined,
        })
    }

    #[getter]
    fn decoded_path(&self) -> &str {
        &self.decoded
    }

    #[getter]
    fn components(&self) -> Vec<String> {
        self.components.clone()
    }

    fn __repr__(&self) -> String {
        format!("RequestTarget({:?})", self.decoded)
    }

    fn __str__(&self) -> &str {
        &self.decoded
    }
}

// ---------------------------------------------------------------------------
// SecureRoot
// ---------------------------------------------------------------------------

#[pyclass(name = "SecureRoot")]
struct PySecureRoot {
    root_path: std::path::PathBuf,
    inner: RustSecureRoot,
}

#[pymethods]
impl PySecureRoot {
    #[new]
    #[pyo3(signature = (path, policy=None))]
    fn py_new(path: &Bound<'_, pyo3::PyAny>, policy: Option<&PyStaticPolicy>) -> PyResult<Self> {
        let path_buf: std::path::PathBuf = path.extract()?;
        let static_policy = policy
            .map(|p| p.inner.clone())
            .unwrap_or_else(RustStaticPolicy::safe_default);
        let root = RustSecureRoot::new(&path_buf, static_policy).map_err(io_err_to_pyerr)?;
        let root_path = root.root_path().to_path_buf();
        Ok(Self {
            root_path,
            inner: root,
        })
    }

    #[getter]
    fn policy(&self) -> PyStaticPolicy {
        PyStaticPolicy {
            inner: self.inner.policy().clone(),
        }
    }

    fn resolve(&self, target: &PyRequestTarget) -> PyResult<PyResolvedResource> {
        let result = self.inner.resolve(&target.confined);
        Ok(PyResolvedResource::from_rust(result, self))
    }

    #[pyo3(signature = (raw_path, path_policy=None))]
    fn resolve_path(
        &self,
        raw_path: &str,
        path_policy: Option<&PyPathPolicy>,
    ) -> PyResult<PyResolvedResource> {
        let result = match path_policy {
            Some(pp) => {
                let confined =
                    ConfinedPath::parse(raw_path, &pp.inner).map_err(path_rejection_to_pyerr)?;
                self.inner.resolve(&confined)
            }
            None => self
                .inner
                .resolve_uri(raw_path)
                .map_err(path_rejection_to_pyerr)?,
        };
        Ok(PyResolvedResource::from_rust(result, self))
    }

    fn __repr__(&self) -> String {
        format!("SecureRoot({:?})", self.root_path)
    }
}

// ---------------------------------------------------------------------------
// ResolvedResource
// ---------------------------------------------------------------------------

#[pyclass(name = "ResolvedResource", frozen)]
struct PyResolvedResource {
    kind: String,
    file_data: Option<PyResolvedFileData>,
    dir_components: Option<Vec<String>>,
    root_path: Option<std::path::PathBuf>,
    denied_reason_msg: Option<String>,
    denied_code: Option<String>,
    static_policy: Option<RustStaticPolicy>,
}

struct PyResolvedFileData {
    file: std::sync::Mutex<Option<std::fs::File>>,
    metadata: std::fs::Metadata,
    components: Vec<String>,
    content_type: String,
}

#[pymethods]
impl PyResolvedResource {
    #[getter]
    fn kind(&self) -> &str {
        &self.kind
    }

    #[getter]
    fn is_file(&self) -> bool {
        self.kind == "file"
    }

    #[getter]
    fn is_directory(&self) -> bool {
        self.kind == "directory"
    }

    #[getter]
    fn is_not_found(&self) -> bool {
        self.kind == "not_found"
    }

    #[getter]
    fn is_denied(&self) -> bool {
        self.kind == "denied"
    }

    #[getter]
    fn file(&self) -> PyResult<PyResolvedFile> {
        match &self.file_data {
            Some(fd) => {
                let file_handle = fd
                    .file
                    .lock()
                    .map_err(|_| {
                        EggserveError::new_err(("failed to acquire file lock", "lock_error"))
                    })?
                    .take();
                Ok(PyResolvedFile {
                    file: std::sync::Mutex::new(file_handle),
                    metadata: fd.metadata.clone(),
                    components: fd.components.clone(),
                    content_type: fd.content_type.clone(),
                })
            }
            None => Err(EggserveError::new_err((
                "resource is not a file",
                "not_a_file",
            ))),
        }
    }

    #[getter]
    fn directory(&self) -> PyResult<PyResolvedDirectory> {
        match (&self.dir_components, &self.root_path) {
            (Some(comps), Some(rp)) => Ok(PyResolvedDirectory {
                components: comps.clone(),
                root_path: rp.clone(),
                static_policy: self
                    .static_policy
                    .clone()
                    .unwrap_or_else(RustStaticPolicy::safe_default),
            }),
            _ => Err(EggserveError::new_err((
                "resource is not a directory",
                "not_a_directory",
            ))),
        }
    }

    #[getter]
    fn denied_reason(&self) -> PyResult<(&str, &str)> {
        match (&self.denied_reason_msg, &self.denied_code) {
            (Some(reason), Some(code)) => Ok((reason.as_str(), code.as_str())),
            _ => Err(EggserveError::new_err((
                "resource is not denied",
                "not_denied",
            ))),
        }
    }

    fn __repr__(&self) -> String {
        match self.kind.as_str() {
            "file" => "ResolvedResource(kind='file')".to_string(),
            "directory" => "ResolvedResource(kind='directory')".to_string(),
            "not_found" => "ResolvedResource(kind='not_found')".to_string(),
            "denied" => format!(
                "ResolvedResource(kind='denied', reason={:?})",
                self.denied_reason_msg.as_deref().unwrap_or("")
            ),
            _ => "ResolvedResource(kind='?')".to_string(),
        }
    }
}

impl PyResolvedResource {
    fn from_rust(resource: RustResolvedResource, root: &PySecureRoot) -> Self {
        let static_policy = root.inner.policy().clone();
        match resource {
            RustResolvedResource::File(f) => {
                let ct = f.content_type().to_string();
                let comps = f.safe_relative_components().to_vec();
                let (file_handle, metadata) = f.into_parts();
                Self {
                    kind: "file".to_string(),
                    file_data: Some(PyResolvedFileData {
                        file: std::sync::Mutex::new(Some(file_handle)),
                        metadata,
                        components: comps,
                        content_type: ct,
                    }),
                    dir_components: None,
                    root_path: None,
                    denied_reason_msg: None,
                    denied_code: None,
                    static_policy: Some(static_policy),
                }
            }
            RustResolvedResource::Directory(d) => Self {
                kind: "directory".to_string(),
                file_data: None,
                dir_components: Some(d.components().to_vec()),
                root_path: Some(root.root_path.clone()),
                denied_reason_msg: None,
                denied_code: None,
                static_policy: Some(static_policy),
            },
            RustResolvedResource::NotFound => Self {
                kind: "not_found".to_string(),
                file_data: None,
                dir_components: None,
                root_path: None,
                denied_reason_msg: None,
                denied_code: None,
                static_policy: None,
            },
            RustResolvedResource::Denied(reason) => {
                let (msg, code) = match &reason {
                    ResourceDeniedReason::SymlinkDenied => {
                        ("symlink denied".to_string(), "symlink_denied".to_string())
                    }
                    ResourceDeniedReason::DotfileDenied => {
                        ("dotfile denied".to_string(), "dotfile_denied".to_string())
                    }
                    ResourceDeniedReason::RootEscapeDenied => (
                        "root escape denied".to_string(),
                        "root_escape_denied".to_string(),
                    ),
                    ResourceDeniedReason::PolicyDenied(inner) => {
                        (inner.to_string(), path_rejection_code(inner).to_string())
                    }
                };
                Self {
                    kind: "denied".to_string(),
                    file_data: None,
                    dir_components: None,
                    root_path: None,
                    denied_reason_msg: Some(msg),
                    denied_code: Some(code),
                    static_policy: None,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ResolvedFile
// ---------------------------------------------------------------------------

#[pyclass(name = "ResolvedFile", frozen)]
struct PyResolvedFile {
    file: std::sync::Mutex<Option<std::fs::File>>,
    metadata: std::fs::Metadata,
    components: Vec<String>,
    content_type: String,
}

#[pymethods]
impl PyResolvedFile {
    #[getter]
    fn length(&self) -> u64 {
        self.metadata.len()
    }

    #[getter]
    fn modified(&self) -> Option<f64> {
        self.metadata.modified().ok().and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs_f64())
        })
    }

    #[getter]
    fn content_type(&self) -> &str {
        &self.content_type
    }

    #[getter]
    fn safe_relative_components(&self) -> Vec<String> {
        self.components.clone()
    }

    #[pyo3(signature = (method="GET", headers=None))]
    fn plan_response(
        &self,
        _py: Python<'_>,
        method: &str,
        headers: Option<&Bound<'_, PyList>>,
    ) -> PyResult<PyResponsePlan> {
        let _ = headers_from_list(headers)?;
        let ro = parse_method(method)?;
        let plan = planner::plan_file_response(
            ro,
            &self.metadata,
            &self.content_type,
            None,
            None,
            None,
            None,
        );
        Ok(PyResponsePlan {
            inner: plan,
            py_obj: std::sync::OnceLock::new(),
        })
    }

    #[pyo3(signature = (method="GET", headers=None))]
    fn plan_conditional_response(
        &self,
        _py: Python<'_>,
        method: &str,
        headers: Option<&Bound<'_, PyList>>,
    ) -> PyResult<PyResponsePlan> {
        let hdrs = headers_from_list(headers)?;
        let ro = parse_method(method)?;

        let mut if_none_match: Option<String> = None;
        let mut if_modified_since: Option<String> = None;
        let mut range_header: Option<String> = None;
        let mut if_range: Option<String> = None;

        for (name, value) in &hdrs {
            match name.to_lowercase().as_str() {
                "if-none-match" => if_none_match = Some(value.clone()),
                "if-modified-since" => if_modified_since = Some(value.clone()),
                "range" => range_header = Some(value.clone()),
                "if-range" => if_range = Some(value.clone()),
                _ => {}
            }
        }

        let plan = planner::plan_file_response(
            ro,
            &self.metadata,
            &self.content_type,
            if_none_match.as_deref(),
            if_modified_since.as_deref(),
            range_header.as_deref(),
            if_range.as_deref(),
        );
        Ok(PyResponsePlan {
            inner: plan,
            py_obj: std::sync::OnceLock::new(),
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "ResolvedFile(length={}, content_type={:?})",
            self.metadata.len(),
            self.content_type
        )
    }

    #[pyo3(signature = (plan,))]
    fn body_for_plan(&self, _py: Python<'_>, plan: &PyResponsePlan) -> PyResult<PyBodySource> {
        let mut file_guard = self
            .file
            .lock()
            .map_err(|_| EggserveError::new_err(("failed to acquire file lock", "lock_error")))?;
        let file = file_guard.take().ok_or_else(|| {
            BodySourceError::new_err(("resolved file already consumed", "already_consumed"))
        })?;
        drop(file_guard);

        let resolved_file = eggserve_core::primitives::ResolvedFile::from_parts(
            file,
            self.metadata.clone(),
            self.components.clone(),
        );
        let body_source = resolved_file
            .into_body(&plan.inner)
            .map_err(|e| BodySourceError::new_err((e.to_string(), "body_source_error")))?;
        Ok(PyBodySource { inner: body_source })
    }
}

// ---------------------------------------------------------------------------
// ResolvedDirectory
// ---------------------------------------------------------------------------

#[pyclass(name = "ResolvedDirectory", frozen)]
struct PyResolvedDirectory {
    components: Vec<String>,
    root_path: std::path::PathBuf,
    static_policy: RustStaticPolicy,
}

#[pymethods]
impl PyResolvedDirectory {
    #[getter]
    fn safe_relative_components(&self) -> Vec<String> {
        self.components.clone()
    }

    fn list(&self) -> PyResult<PyObject> {
        Python::with_gil(|py| {
            let root = RustSecureRoot::new(&self.root_path, self.static_policy.clone())
                .map_err(io_err_to_pyerr)?;
            let confined = confined_from_components(&self.components)?;
            let result = root.resolve(&confined);
            match result {
                RustResolvedResource::Directory(dir) => {
                    let entries = dir.list(&root).map_err(io_err_to_pyerr)?;
                    let py_list = PyList::empty(py);
                    for entry in &entries {
                        let name_obj: PyObject =
                            entry.0.as_str().into_pyobject(py)?.into_any().unbind();
                        let flag_obj: PyObject =
                            entry.1.into_pyobject(py)?.to_owned().into_any().unbind();
                        let tup = PyTuple::new(py, [name_obj, flag_obj])?;
                        py_list.append(tup)?;
                    }
                    Ok(py_list.into_any().unbind())
                }
                _ => Err(SecureRootError::new_err((
                    "path does not resolve to a directory",
                    "not_a_directory",
                ))),
            }
        })
    }

    fn resolve_child(&self, child: &str) -> PyResult<PyResolvedResource> {
        Python::with_gil(|_py| {
            let root = RustSecureRoot::new(&self.root_path, self.static_policy.clone())
                .map_err(io_err_to_pyerr)?;
            let confined = confined_from_components(&self.components)?;
            let result = root.resolve(&confined);
            match result {
                RustResolvedResource::Directory(dir) => {
                    let child_result = dir.resolve_child(child, &root);
                    Ok(PyResolvedResource::from_rust(
                        child_result,
                        &PySecureRoot {
                            root_path: self.root_path.clone(),
                            inner: root,
                        },
                    ))
                }
                _ => Err(SecureRootError::new_err((
                    "path does not resolve to a directory",
                    "not_a_directory",
                ))),
            }
        })
    }

    fn __repr__(&self) -> String {
        format!("ResolvedDirectory(components={:?})", self.components)
    }
}

// ---------------------------------------------------------------------------
// Standalone functions
// ---------------------------------------------------------------------------

#[pyfunction]
#[pyo3(name = "validate_method")]
#[pyo3(signature = (method,))]
fn validate_method_fn(method: &str) -> PyResult<String> {
    let ro = parse_method(method)?;
    Ok(ro.as_str().to_string())
}

#[pyfunction]
#[pyo3(name = "validate_request_body")]
#[pyo3(signature = (content_length=None, transfer_encoding=None, max_body_bytes=0))]
fn validate_request_body_fn(
    content_length: Option<&str>,
    transfer_encoding: Option<&str>,
    max_body_bytes: u64,
) -> PyResult<()> {
    http::validate_request_body(content_length, transfer_encoding, max_body_bytes)
        .map_err(|e| RequestValidationError::new_err((e.to_string(), "body_validation_error")))
}

#[pyfunction]
#[pyo3(name = "validate_request_target")]
#[pyo3(signature = (target,))]
fn validate_request_target_fn(target: &str) -> PyResult<()> {
    http::validate_request_target(target)
        .map_err(|e| RequestValidationError::new_err((e.to_string(), "invalid_request_target")))
}

#[pyfunction]
#[pyo3(name = "generate_etag")]
fn generate_etag_fn(py: Python<'_>, file: &PyResolvedFile) -> PyResult<PyObject> {
    match planner::generate_etag(&file.metadata) {
        Some(etag) => Ok(etag.into_pyobject(py)?.into_any().unbind()),
        None => Ok(py.None()),
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

use eggserve_core::primitives::response::StaticResponsePlan;

// ---------------------------------------------------------------------------
// ResponsePlan (Rust wrapper for Python ResponsePlan)
// ---------------------------------------------------------------------------

#[pyclass(name = "ResponsePlan", frozen)]
#[allow(dead_code)]
struct PyResponsePlan {
    inner: StaticResponsePlan,
    py_obj: std::sync::OnceLock<PyObject>,
}

#[pymethods]
#[allow(dead_code)]
impl PyResponsePlan {
    #[getter]
    fn status(&self) -> u16 {
        self.inner.status_code()
    }

    #[getter]
    fn headers(&self, py: Python<'_>) -> PyObject {
        let list = pyo3::types::PyList::empty(py);
        for h in self.inner.headers.iter() {
            let tup = pyo3::types::PyTuple::new(py, [h.name.as_str(), h.value.as_str()]).unwrap();
            list.append(tup).unwrap();
        }
        list.into_any().unbind()
    }

    #[getter]
    fn body_kind(&self) -> &str {
        match &self.inner.body {
            BodyPlan::Empty => "empty",
            BodyPlan::FullBytes(_) => "bytes",
            BodyPlan::FileFull => "file_full",
            BodyPlan::FileRange { .. } => "file_range",
        }
    }

    #[getter]
    fn range(&self) -> Option<(u64, u64)> {
        match &self.inner.body {
            BodyPlan::FileRange {
                start,
                end_inclusive,
            } => Some((*start, *end_inclusive)),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "ResponsePlan(status={}, body_kind={:?})",
            self.inner.status_code(),
            self.body_kind()
        )
    }
}

#[allow(dead_code)]
impl PyResponsePlan {
    fn to_py(&self, py: Python<'_>) -> PyResult<PyObject> {
        if let Some(obj) = self.py_obj.get() {
            return Ok(obj.clone_ref(py));
        }
        plan_to_python(py, self.inner.clone())
    }
}

// ---------------------------------------------------------------------------
// Method (canonical HTTP method)
// ---------------------------------------------------------------------------

#[pyclass(name = "Method", frozen)]
#[derive(Debug, Clone)]
struct PyMethod {
    inner: RustMethod,
}

#[pymethods]
impl PyMethod {
    #[new]
    fn py_new(value: &str) -> PyResult<Self> {
        let inner = RustMethod::new(value)
            .map_err(|e| MethodError::new_err((e.to_string(), "invalid_method")))?;
        Ok(Self { inner })
    }

    #[staticmethod]
    fn get() -> Self {
        Self { inner: RustMethod::get() }
    }

    #[staticmethod]
    fn head() -> Self {
        Self { inner: RustMethod::head() }
    }

    #[staticmethod]
    fn post() -> Self {
        Self { inner: RustMethod::post() }
    }

    #[staticmethod]
    fn put() -> Self {
        Self { inner: RustMethod::put() }
    }

    #[staticmethod]
    fn delete() -> Self {
        Self { inner: RustMethod::delete() }
    }

    #[staticmethod]
    fn patch() -> Self {
        Self { inner: RustMethod::patch() }
    }

    #[getter]
    fn as_str(&self) -> &str {
        self.inner.as_str()
    }

    #[getter]
    fn is_safe(&self) -> bool {
        self.inner.is_safe()
    }

    #[getter]
    fn is_idempotent(&self) -> bool {
        self.inner.is_idempotent()
    }

    #[getter]
    fn permits_static_resolution(&self) -> bool {
        self.inner.permits_static_resolution()
    }

    fn __str__(&self) -> &str {
        self.inner.as_str()
    }

    fn __repr__(&self) -> String {
        format!("Method({:?})", self.inner.as_str())
    }

    fn __eq__(&self, other: &PyMethod) -> bool {
        self.inner == other.inner
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }
}

// ---------------------------------------------------------------------------
// HttpVersion (canonical HTTP version)
// ---------------------------------------------------------------------------

#[pyclass(name = "HttpVersion", frozen)]
#[derive(Debug, Clone, Copy)]
struct PyHttpVersion {
    inner: RustHttpVersion,
}

#[pymethods]
impl PyHttpVersion {
    #[new]
    fn py_new(value: &str) -> PyResult<Self> {
        let inner = RustHttpVersion::parse(value)
            .map_err(|e| HttpVersionError::new_err((e.to_string(), "unsupported_version")))?;
        Ok(Self { inner })
    }

    #[staticmethod]
    fn http10() -> Self {
        Self { inner: RustHttpVersion::Http10 }
    }

    #[staticmethod]
    fn http11() -> Self {
        Self { inner: RustHttpVersion::Http11 }
    }

    #[getter]
    fn major(&self) -> u8 {
        self.inner.major()
    }

    #[getter]
    fn minor(&self) -> u8 {
        self.inner.minor()
    }

    fn __str__(&self) -> &str {
        self.inner.as_str()
    }

    fn __repr__(&self) -> String {
        format!("HttpVersion({:?})", self.inner.as_str())
    }

    fn __eq__(&self, other: &PyHttpVersion) -> bool {
        self.inner == other.inner
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }
}

// ---------------------------------------------------------------------------
// HeaderBlock (duplicate-preserving headers)
// ---------------------------------------------------------------------------

#[pyclass(name = "HeaderBlock", frozen)]
#[derive(Debug, Clone)]
struct PyHeaderBlock {
    inner: RustHeaderBlock,
}

#[pymethods]
impl PyHeaderBlock {
    #[new]
    #[pyo3(signature = (fields=None))]
    fn py_new(fields: Option<Vec<(String, String)>>) -> PyResult<Self> {
        let mut inner = RustHeaderBlock::new();
        if let Some(fields) = fields {
            for (name, value) in fields {
                inner.push_str(name, value)
                    .map_err(|e| HeaderError::new_err((e.to_string(), "invalid_header")))?;
            }
        }
        Ok(Self { inner })
    }

    #[getter]
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[getter]
    fn len(&self) -> usize {
        self.inner.len()
    }

    fn get_first(&self, name: &str) -> Option<String> {
        self.inner.get_first(name).map(|v| v.as_str().to_string())
    }

    fn get_all(&self, name: &str) -> Vec<String> {
        self.inner.get_all(name).into_iter().map(|v| v.as_str().to_string()).collect()
    }

    fn get_unique(&self, name: &str) -> PyResult<Option<String>> {
        match self.inner.get_unique(name) {
            Ok(opt) => Ok(opt.map(|v| v.as_str().to_string())),
            Err(e) => Err(DuplicateHeaderError::new_err((e.to_string(), e.name().to_string(), e.count()))),
        }
    }

    fn contains(&self, name: &str) -> bool {
        self.inner.contains(name)
    }

    fn iter(&self, py: Python<'_>) -> PyResult<PyObject> {
        let list = pyo3::types::PyList::empty(py);
        for field in self.inner.iter() {
            let tup = pyo3::types::PyTuple::new(py, [field.name.as_str(), field.value.as_str()])?;
            list.append(tup)?;
        }
        Ok(list.into_any().unbind())
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    fn __iter__<'py>(slf: Py<Self>, py: Python<'py>) -> PyResult<PyObject> {
        let list = pyo3::types::PyList::empty(py);
        let borrowed = slf.borrow(py);
        for field in borrowed.inner.iter() {
            let tup = pyo3::types::PyTuple::new(py, [field.name.as_str(), field.value.as_str()])?;
            list.append(tup)?;
        }
        Ok(list.into_any().unbind())
    }

    fn __repr__(&self) -> String {
        format!("HeaderBlock(len={})", self.inner.len())
    }
}

// ---------------------------------------------------------------------------
// ConnectionInfo (connection metadata)
// ---------------------------------------------------------------------------

#[pyclass(name = "ConnectionInfo", frozen)]
#[derive(Debug, Clone)]
struct PyConnectionInfo {
    local_addr: String,
    remote_addr: String,
    scheme: String,
    tls_protocol_version: Option<String>,
    tls_server_name: Option<String>,
}

#[pymethods]
impl PyConnectionInfo {
    #[new]
    #[pyo3(signature = (local_addr, remote_addr, scheme="http", tls_protocol_version=None, tls_server_name=None))]
    fn py_new(
        local_addr: &str,
        remote_addr: &str,
        scheme: &str,
        tls_protocol_version: Option<String>,
        tls_server_name: Option<String>,
    ) -> PyResult<Self> {
        if scheme != "http" && scheme != "https" {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "scheme must be 'http' or 'https'"
            ));
        }
        Ok(Self {
            local_addr: local_addr.to_string(),
            remote_addr: remote_addr.to_string(),
            scheme: scheme.to_string(),
            tls_protocol_version,
            tls_server_name,
        })
    }

    #[getter]
    fn local_addr(&self) -> &str {
        &self.local_addr
    }

    #[getter]
    fn remote_addr(&self) -> &str {
        &self.remote_addr
    }

    #[getter]
    fn scheme(&self) -> &str {
        &self.scheme
    }

    #[getter]
    fn is_tls(&self) -> bool {
        self.tls_protocol_version.is_some() || self.tls_server_name.is_some()
    }

    #[getter]
    fn tls_protocol_version(&self) -> Option<&str> {
        self.tls_protocol_version.as_deref()
    }

    #[getter]
    fn tls_server_name(&self) -> Option<&str> {
        self.tls_server_name.as_deref()
    }

    fn __repr__(&self) -> String {
        format!(
            "ConnectionInfo(local_addr={:?}, remote_addr={:?}, scheme={:?}, is_tls={})",
            self.local_addr, self.remote_addr, self.scheme, self.is_tls()
        )
    }
}

// ---------------------------------------------------------------------------
// CanonicalRequest (canonical HTTP request head for Python)
// ---------------------------------------------------------------------------

#[pyclass(name = "CanonicalRequest", frozen)]
#[derive(Debug, Clone)]
struct PyCanonicalRequest {
    method: String,
    path: String,
    query: Option<String>,
    version: String,
    headers: Vec<(String, String)>,
    remote_addr: Option<String>,
    local_addr: Option<String>,
    scheme: String,
}

#[pymethods]
impl PyCanonicalRequest {
    #[new]
    #[pyo3(signature = (method, path, version="HTTP/1.1", headers=None, query=None, remote_addr=None, local_addr=None, scheme="http"))]
    fn py_new(
        method: &str,
        path: &str,
        version: &str,
        headers: Option<Vec<(String, String)>>,
        query: Option<String>,
        remote_addr: Option<String>,
        local_addr: Option<String>,
        scheme: &str,
    ) -> PyResult<Self> {
        // Validate method
        let _ = RustMethod::new(method)
            .map_err(|e| MethodError::new_err((e.to_string(), "invalid_method")))?;
        // Validate version
        let _ = RustHttpVersion::parse(version)
            .map_err(|e| HttpVersionError::new_err((e.to_string(), "unsupported_version")))?;
        // Validate path starts with /
        if !path.starts_with('/') {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "path must start with '/'"
            ));
        }

        Ok(Self {
            method: method.to_string(),
            path: path.to_string(),
            query,
            version: version.to_string(),
            headers: headers.unwrap_or_default(),
            remote_addr,
            local_addr,
            scheme: scheme.to_string(),
        })
    }

    #[getter]
    fn method(&self) -> &str {
        &self.method
    }

    #[getter]
    fn path(&self) -> &str {
        &self.path
    }

    #[getter]
    fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    #[getter]
    fn version(&self) -> &str {
        &self.version
    }

    #[getter]
    fn headers(&self) -> Vec<(String, String)> {
        self.headers.clone()
    }

    #[getter]
    fn remote_addr(&self) -> Option<&str> {
        self.remote_addr.as_deref()
    }

    #[getter]
    fn local_addr(&self) -> Option<&str> {
        self.local_addr.as_deref()
    }

    #[getter]
    fn scheme(&self) -> &str {
        &self.scheme
    }

    #[getter]
    fn is_head(&self) -> bool {
        self.method == "HEAD"
    }

    #[getter]
    fn is_get(&self) -> bool {
        self.method == "GET"
    }

    fn header_block(&self) -> PyResult<PyHeaderBlock> {
        let mut inner = RustHeaderBlock::new();
        for (name, value) in &self.headers {
            inner.push_str(name.as_str(), value.as_str())
                .map_err(|e| HeaderError::new_err((e.to_string(), "invalid_header")))?;
        }
        Ok(PyHeaderBlock { inner })
    }

    fn __repr__(&self) -> String {
        format!(
            "CanonicalRequest(method={:?}, path={:?}, version={:?})",
            self.method, self.path, self.version
        )
    }
}

// ---------------------------------------------------------------------------
// Standalone functions (continued)
// ---------------------------------------------------------------------------

#[pyfunction]
#[pyo3(name = "parse_method")]
fn parse_method_fn(value: &str) -> PyResult<PyMethod> {
    PyMethod::py_new(value)
}

#[pyfunction]
#[pyo3(name = "parse_http_version")]
fn parse_http_version_fn(value: &str) -> PyResult<PyHttpVersion> {
    PyHttpVersion::py_new(value)
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("EggserveError", m.py().get_type::<EggserveError>())?;
    m.add("PathPolicyError", m.py().get_type::<PathPolicyError>())?;
    m.add(
        "RequestTargetError",
        m.py().get_type::<RequestTargetError>(),
    )?;
    m.add("SecureRootError", m.py().get_type::<SecureRootError>())?;
    m.add(
        "RequestValidationError",
        m.py().get_type::<RequestValidationError>(),
    )?;
    m.add(
        "BodySourceError",
        m.py().get_type::<BodySourceError>(),
    )?;
    m.add(
        "ResponseConstructionError",
        m.py().get_type::<ResponseConstructionError>(),
    )?;
    m.add("LifecycleError", m.py().get_type::<LifecycleError>())?;
    m.add("MethodError", m.py().get_type::<MethodError>())?;
    m.add("HttpVersionError", m.py().get_type::<HttpVersionError>())?;
    m.add("HeaderError", m.py().get_type::<HeaderError>())?;
    m.add("DuplicateHeaderError", m.py().get_type::<DuplicateHeaderError>())?;

    m.add_class::<PyPathPolicy>()?;
    m.add_class::<PyStaticPolicy>()?;
    m.add_class::<PyRequestTarget>()?;
    m.add_class::<PySecureRoot>()?;
    m.add_class::<PyResolvedResource>()?;
    m.add_class::<PyResolvedFile>()?;
    m.add_class::<PyResolvedDirectory>()?;
    m.add_class::<PyResponsePlan>()?;
    m.add_class::<PyBodySource>()?;

    m.add_class::<PyMethod>()?;
    m.add_class::<PyHttpVersion>()?;
    m.add_class::<PyHeaderBlock>()?;
    m.add_class::<PyConnectionInfo>()?;
    m.add_class::<PyCanonicalRequest>()?;

    m.add_function(wrap_pyfunction!(validate_method_fn, m)?)?;
    m.add_function(wrap_pyfunction!(validate_request_body_fn, m)?)?;
    m.add_function(wrap_pyfunction!(validate_request_target_fn, m)?)?;
    m.add_function(wrap_pyfunction!(generate_etag_fn, m)?)?;
    m.add_function(wrap_pyfunction!(parse_method_fn, m)?)?;
    m.add_function(wrap_pyfunction!(parse_http_version_fn, m)?)?;

    m.add_class::<server::PyRequest>()?;
    m.add_class::<server::PyResponse>()?;
    m.add_class::<server::PyStaticResponder>()?;
    m.add_class::<server::PyStaticPolicyWrapper>()?;
    m.add_class::<server::ServerSecureRoot>()?;
    m.add_class::<server::ServerBodySource>()?;
    m.add_class::<server::ServerRequestError>()?;
    m.add_class::<server::PyServer>()?;

    m.add_class::<client::PyClientError>()?;
    m.add_class::<client::PyMethod>()?;
    m.add_class::<client::PyClientConfig>()?;
    m.add_class::<client::PyClientRequest>()?;
    m.add_class::<client::PyClientResponse>()?;
    m.add_class::<client::PyHttpClient>()?;

    Ok(())
}
