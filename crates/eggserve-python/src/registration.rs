use pyo3::prelude::*;
use pyo3::types::PyModule;

use super::{
    generate_etag_fn, parse_http_version_fn, parse_method_fn, run_cli_fn, validate_interim_fn,
    validate_trailers_fn,
    validate_method_fn, validate_request_body_fn, validate_request_target_fn,
    EggserveError, PathPolicyError, RequestTargetError, SecureRootError,
    RequestValidationError, BodySourceError, ResponseConstructionError, LifecycleError,
    RequestBodyError, RequestBodyRejectedError, RequestBodyTooLargeError,
    RequestBodyTimeoutError, RequestBodyDisconnectedError, RequestBodyIncompleteError,
    RequestBodyConsumedError, RequestBodyCancelledError, MethodError, HttpVersionError,
    HeaderError, DuplicateHeaderError, PyPathPolicy, PyStaticPolicy, PyRequestTarget,
    PySecureRoot, PyResolvedResource, PyResolvedFile, PyResolvedDirectory, PyResponsePlan,
    PyBodySource, PyMethod, PyHttpVersion, PyHeaderBlock, PyConnectionInfo,
    PyCanonicalRequest,
};

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("EggserveError", m.py().get_type::<EggserveError>())?;
    m.add("PathPolicyError", m.py().get_type::<PathPolicyError>())?;
    m.add("RequestTargetError", m.py().get_type::<RequestTargetError>())?;
    m.add("SecureRootError", m.py().get_type::<SecureRootError>())?;
    m.add("RequestValidationError", m.py().get_type::<RequestValidationError>())?;
    m.add("BodySourceError", m.py().get_type::<BodySourceError>())?;
    m.add("ResponseConstructionError", m.py().get_type::<ResponseConstructionError>())?;
    m.add("LifecycleError", m.py().get_type::<LifecycleError>())?;
    m.add("RequestBodyError", m.py().get_type::<RequestBodyError>())?;
    m.add("RequestBodyRejectedError", m.py().get_type::<RequestBodyRejectedError>())?;
    m.add("RequestBodyTooLargeError", m.py().get_type::<RequestBodyTooLargeError>())?;
    m.add("RequestBodyTimeoutError", m.py().get_type::<RequestBodyTimeoutError>())?;
    m.add("RequestBodyDisconnectedError", m.py().get_type::<RequestBodyDisconnectedError>())?;
    m.add("RequestBodyIncompleteError", m.py().get_type::<RequestBodyIncompleteError>())?;
    m.add("RequestBodyConsumedError", m.py().get_type::<RequestBodyConsumedError>())?;
    m.add("RequestBodyCancelledError", m.py().get_type::<RequestBodyCancelledError>())?;
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
    m.add_function(wrap_pyfunction!(validate_trailers_fn, m)?)?;
    m.add_function(wrap_pyfunction!(validate_interim_fn, m)?)?;
    m.add_function(wrap_pyfunction!(run_cli_fn, m)?)?;

    m.add_class::<super::server::PyRequestBody>()?;
    m.add_class::<super::server::PyBodyChunkIterator>()?;
    m.add_class::<super::server::PyRequest>()?;
    m.add_class::<super::server::PyTunnelRequest>()?;
    m.add_class::<super::server::PyTunnelCapability>()?;
    m.add_class::<super::server::PyTunnel>()?;
    m.add_class::<super::server::PyResponse>()?;
    m.add_class::<super::server::PyStaticResponder>()?;
    m.add_class::<super::server::PyStaticPolicyWrapper>()?;
    m.add_class::<super::server::ServerSecureRoot>()?;
    m.add_class::<super::server::ServerBodySource>()?;
    m.add_class::<super::server::ServerRequestError>()?;
    m.add_class::<super::server::PyServer>()?;
    Ok(())
}
