//! Compile/import fixture for plan 049 Track E.
//!
//! External-consumer tests that verify the public API can be used without
//! importing Hyper. cargo-semver-checks compatibility: this file serves as a
//! compile-time API snapshot. All public items from `eggserve_core::primitives`
//! are exercised here. If a public API is removed or changed, this file will
//! fail to compile, providing an immediate signal of semver-incompatible changes.
//!
//! Every import goes through `eggserve_core::primitives`
//! (the public facade). This file is an integration test outside the crate
//! module tree, simulating a downstream consumer.

use eggserve_core::primitives::canonical::{
    normalize_response, NormalizeRequest, Response, ResponseBody, ResponseConstructionError,
    StatusCode,
};
use eggserve_core::primitives::connection_info::{ConnectionInfo, Scheme, TlsInfo};
use eggserve_core::primitives::header_block::{
    DuplicateHeaderError, HeaderBlock, HeaderError, HeaderField, HeaderName, HeaderValue,
};
use eggserve_core::primitives::http::{
    validate_method, validate_request_body, validate_request_target, ReadOnlyMethod,
    RequestValidationError,
};
use eggserve_core::primitives::method::{Method, MethodError};
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::primitives::request_target::{RequestTarget, RequestTargetError};
use eggserve_core::primitives::version::{HttpVersion, HttpVersionError};
use std::net::SocketAddr;

// ── Method construction and inspection ──────────────────────────────────────

#[test]
fn construct_standard_methods() {
    assert_eq!(Method::get().as_str(), "GET");
    assert_eq!(Method::head().as_str(), "HEAD");
    assert_eq!(Method::post().as_str(), "POST");
    assert_eq!(Method::put().as_str(), "PUT");
    assert_eq!(Method::delete().as_str(), "DELETE");
    assert_eq!(Method::patch().as_str(), "PATCH");
    assert_eq!(Method::options().as_str(), "OPTIONS");
    assert_eq!(Method::trace().as_str(), "TRACE");
    assert_eq!(Method::connect().as_str(), "CONNECT");
}

#[test]
fn construct_extension_method() {
    let m = Method::new("PURGE").unwrap();
    assert_eq!(m.as_str(), "PURGE");
    assert!(!m.is_safe());
    assert!(!m.is_idempotent());
}

#[test]
fn method_validation_errors() {
    assert_eq!(Method::new("").unwrap_err(), MethodError::Empty);
    assert_eq!(
        Method::new("GET POST").unwrap_err(),
        MethodError::InvalidToken
    );
}

#[test]
fn method_classification() {
    assert!(Method::get().is_safe());
    assert!(Method::get().is_idempotent());
    assert!(Method::get().permits_static_resolution());
    assert!(!Method::post().is_safe());
    assert!(!Method::post().permits_static_resolution());
}

#[test]
fn method_display() {
    assert_eq!(format!("{}", Method::get()), "GET");
    assert_eq!(format!("{}", Method::new("PURGE").unwrap()), "PURGE");
}

#[test]
fn method_equality() {
    assert_eq!(Method::get(), "GET");
    assert_ne!(Method::get(), "POST");
}

#[test]
fn method_error_is_std_error() {
    let err: &dyn std::error::Error = &MethodError::Empty;
    assert!(!err.to_string().is_empty());
}

// ── HttpVersion construction and inspection ─────────────────────────────────

#[test]
fn http_version_construct_and_inspect() {
    let v10 = HttpVersion::Http10;
    assert_eq!(v10.as_str(), "HTTP/1.0");
    assert_eq!(v10.major(), 1);
    assert_eq!(v10.minor(), 0);

    let v11 = HttpVersion::Http11;
    assert_eq!(v11.as_str(), "HTTP/1.1");
    assert_eq!(v11.major(), 1);
    assert_eq!(v11.minor(), 1);
}

#[test]
fn http_version_error_is_std_error() {
    let err: &dyn std::error::Error = &HttpVersionError::Unsupported;
    assert!(!err.to_string().is_empty());
}

// ── HeaderBlock construction and inspection ─────────────────────────────────

#[test]
fn header_block_construct_and_inspect() {
    let mut hb = HeaderBlock::new();
    hb.push(
        HeaderName::new("X-Custom").unwrap(),
        HeaderValue::new("value").unwrap(),
    );
    assert!(hb.contains("x-custom"));
    assert_eq!(hb.get_first("X-Custom").unwrap().as_str(), "value");
    assert_eq!(hb.len(), 1);
    assert!(!hb.is_empty());
}

#[test]
fn header_block_push_str() {
    let mut hb = HeaderBlock::new();
    hb.push_str("content-type", "text/html").unwrap();
    assert_eq!(hb.get_first("Content-Type").unwrap().as_str(), "text/html");
}

#[test]
fn header_block_duplicate_preservation() {
    let mut hb = HeaderBlock::new();
    hb.push_str("set-cookie", "a=1").unwrap();
    hb.push_str("set-cookie", "b=2").unwrap();
    let all = hb.get_all("set-cookie");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].as_str(), "a=1");
    assert_eq!(all[1].as_str(), "b=2");
}

#[test]
fn header_block_get_unique_single() {
    let mut hb = HeaderBlock::new();
    hb.push_str("content-type", "text/html").unwrap();
    let val = hb.get_unique("content-type").unwrap();
    assert_eq!(val.unwrap().as_str(), "text/html");
}

#[test]
fn header_block_get_unique_duplicate_error() {
    let mut hb = HeaderBlock::new();
    hb.push_str("set-cookie", "a=1").unwrap();
    hb.push_str("set-cookie", "b=2").unwrap();
    let err = hb.get_unique("set-cookie").unwrap_err();
    assert_eq!(err.name(), "set-cookie");
    assert_eq!(err.count(), 2);
}

#[test]
fn header_block_iteration_order() {
    let mut hb = HeaderBlock::new();
    hb.push_str("a", "1").unwrap();
    hb.push_str("b", "2").unwrap();
    hb.push_str("c", "3").unwrap();
    let names: Vec<&str> = hb.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c"]);
}

#[test]
fn header_name_validation_errors() {
    assert_eq!(HeaderName::new("").unwrap_err(), HeaderError::InvalidName);
    assert_eq!(
        HeaderName::new("foo bar").unwrap_err(),
        HeaderError::InvalidName
    );
    let long = "x".repeat(257);
    assert_eq!(HeaderName::new(long).unwrap_err(), HeaderError::NameTooLong);
}

#[test]
fn header_value_validation_errors() {
    assert_eq!(
        HeaderValue::new("foo\rbar").unwrap_err(),
        HeaderError::InvalidValue
    );
    assert_eq!(
        HeaderValue::new("foo\nbar").unwrap_err(),
        HeaderError::InvalidValue
    );
    assert_eq!(
        HeaderValue::new("foo\0bar").unwrap_err(),
        HeaderError::InvalidValue
    );
}

#[test]
fn duplicate_header_error_is_std_error() {
    let mut hb = HeaderBlock::new();
    hb.push_str("set-cookie", "a=1").unwrap();
    hb.push_str("set-cookie", "b=2").unwrap();
    let err = hb.get_unique("set-cookie").unwrap_err();
    let err_ref: &dyn std::error::Error = &err;
    assert!(!err_ref.to_string().is_empty());
    assert_eq!(err.name(), "set-cookie");
    assert_eq!(err.count(), 2);
}

// ── RequestTarget construction and inspection ───────────────────────────────

#[test]
fn request_target_construct_and_inspect() {
    let rt = RequestTarget::parse("/path?key=val").unwrap();
    assert_eq!(rt.path(), "/path");
    assert_eq!(rt.query(), Some("key=val"));
}

#[test]
fn request_target_without_query() {
    let rt = RequestTarget::parse("/path").unwrap();
    assert_eq!(rt.path(), "/path");
    assert_eq!(rt.query(), None);
}

#[test]
fn request_target_error_is_std_error() {
    let err: &dyn std::error::Error = &RequestTargetError::NotOriginForm;
    assert!(!err.to_string().is_empty());
}

// ── RequestHead construction and inspection ─────────────────────────────────

#[test]
fn request_head_construct_without_hyper() {
    let head = RequestHead::new(
        Method::get(),
        RequestTarget::parse("/").unwrap(),
        HttpVersion::Http11,
        HeaderBlock::new(),
    );
    assert!(head.is_get());
    assert!(!head.is_head());
    assert!(head.permits_static_resolution());
    assert_eq!(head.version(), HttpVersion::Http11);
}

#[test]
fn request_head_with_headers() {
    let mut headers = HeaderBlock::new();
    headers.push_str("content-type", "text/html").unwrap();
    headers.push_str("accept", "application/json").unwrap();
    let head = RequestHead::new(
        Method::get(),
        RequestTarget::parse("/api").unwrap(),
        HttpVersion::Http11,
        headers,
    );
    assert!(head.headers().contains("content-type"));
    assert_eq!(head.headers().len(), 2);
}

#[test]
fn request_head_clone_preserves_values() {
    let head = RequestHead::new(
        Method::get(),
        RequestTarget::parse("/foo?bar").unwrap(),
        HttpVersion::Http11,
        HeaderBlock::new(),
    );
    let cloned = head.clone();
    assert_eq!(cloned.method().as_str(), "GET");
    assert_eq!(cloned.target().path(), "/foo");
    assert_eq!(cloned.target().query(), Some("bar"));
}

// ── ConnectionInfo access ───────────────────────────────────────────────────

#[test]
fn connection_info_construction() {
    let info = ConnectionInfo {
        local_addr: "127.0.0.1:8000".parse::<SocketAddr>().unwrap(),
        remote_addr: "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        scheme: Scheme::Http,
        tls: None,
    };
    assert_eq!(info.local_addr.port(), 8000);
    assert_eq!(info.remote_addr.port(), 12345);
    assert_eq!(info.scheme, Scheme::Http);
    assert!(info.tls.is_none());
}

#[test]
fn connection_info_with_tls() {
    let info = ConnectionInfo {
        local_addr: "0.0.0.0:443".parse::<SocketAddr>().unwrap(),
        remote_addr: "10.0.0.1:54321".parse::<SocketAddr>().unwrap(),
        scheme: Scheme::Https,
        tls: Some(TlsInfo {
            protocol_version: Some("TLSv1.3".to_string()),
            server_name: Some("example.com".to_string()),
        }),
    };
    assert_eq!(info.scheme, Scheme::Https);
    let tls = info.tls.as_ref().unwrap();
    assert_eq!(tls.protocol_version.as_deref(), Some("TLSv1.3"));
    assert_eq!(tls.server_name.as_deref(), Some("example.com"));
}

#[test]
fn scheme_as_str() {
    assert_eq!(Scheme::Http.as_str(), "http");
    assert_eq!(Scheme::Https.as_str(), "https");
}

#[test]
fn scheme_display() {
    assert_eq!(format!("{}", Scheme::Http), "http");
    assert_eq!(format!("{}", Scheme::Https), "https");
}

#[test]
fn connection_info_equality() {
    let a = ConnectionInfo {
        local_addr: "127.0.0.1:8000".parse().unwrap(),
        remote_addr: "127.0.0.1:12345".parse().unwrap(),
        scheme: Scheme::Http,
        tls: None,
    };
    let b = a.clone();
    assert_eq!(a, b);
}

// ── Response construction (byte and empty bodies) ──────────────────────────

#[test]
fn build_byte_response() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain")
        .unwrap()
        .body(ResponseBody::Bytes(b"hello".to_vec()))
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers().get_first("content-type").unwrap().as_str(),
        "text/plain"
    );
    assert_eq!(resp.body().unwrap().len(), 5);
}

#[test]
fn build_empty_response() {
    let resp = Response::builder()
        .status(StatusCode::NO_CONTENT)
        .empty()
        .unwrap();
    assert_eq!(resp.status().as_u16(), 204);
    assert!(resp.body().unwrap().is_empty());
}

#[test]
fn build_response_with_validated_headers() {
    let name = HeaderName::new("x-request-id").unwrap();
    let value = HeaderValue::new("abc-123").unwrap();
    let resp = Response::builder()
        .status(StatusCode::OK)
        .push_header(name, value)
        .body(ResponseBody::Bytes(b"ok".to_vec()))
        .unwrap();
    assert_eq!(
        resp.headers().get_first("x-request-id").unwrap().as_str(),
        "abc-123"
    );
}

#[test]
fn response_builder_no_status_returns_error() {
    let result = Response::builder()
        .header("content-type", "text/plain")
        .unwrap()
        .empty();
    assert!(result.is_err());
}

#[test]
fn response_builder_invalid_header_name_rejected() {
    let result = Response::builder()
        .status(StatusCode::OK)
        .header("", "value");
    assert!(result.is_err());
}

#[test]
fn response_builder_invalid_header_value_rejected() {
    let result = Response::builder()
        .status(StatusCode::OK)
        .header("x-test", "val\r\ninjection");
    assert!(result.is_err());
}

// ── StatusCode validation and classification ────────────────────────────────

#[test]
fn status_code_valid_range() {
    assert!(StatusCode::new(1).is_ok());
    assert!(StatusCode::new(200).is_ok());
    assert!(StatusCode::new(999).is_ok());
}

#[test]
fn status_code_zero_rejected() {
    assert!(StatusCode::new(0).is_err());
}

#[test]
fn status_code_over_999_rejected() {
    assert!(StatusCode::new(1000).is_err());
}

#[test]
fn status_code_classification() {
    assert!(StatusCode::CONTINUE.is_informational());
    assert!(StatusCode::OK.is_success());
    assert!(StatusCode::NOT_MODIFIED.is_redirection());
    assert!(StatusCode::BAD_REQUEST.is_client_error());
    assert!(StatusCode::INTERNAL_SERVER_ERROR.is_server_error());
}

#[test]
fn status_code_permits_payload() {
    assert!(!StatusCode::CONTINUE.permits_payload_body());
    assert!(!StatusCode::NO_CONTENT.permits_payload_body());
    assert!(!StatusCode::NOT_MODIFIED.permits_payload_body());
    assert!(StatusCode::OK.permits_payload_body());
}

#[test]
fn status_code_display() {
    assert_eq!(format!("{}", StatusCode::OK), "200");
    assert_eq!(format!("{}", StatusCode::NOT_FOUND), "404");
}

#[test]
fn status_code_into_u16() {
    let code: u16 = StatusCode::OK.into();
    assert_eq!(code, 200);
}

// ── Response normalization ──────────────────────────────────────────────────

#[test]
fn normalize_head_suppresses_body() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Bytes(b"hello".to_vec()))
        .unwrap();
    let req = NormalizeRequest::new(true);
    let normalized = normalize_response(resp, &req).unwrap();
    assert!(normalized.body().unwrap().is_empty());
}

#[test]
fn normalize_sets_content_length() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Bytes(b"hello".to_vec()))
        .unwrap();
    let req = NormalizeRequest::new(false);
    let normalized = normalize_response(resp, &req).unwrap();
    assert_eq!(
        normalized
            .headers()
            .get_first("content-length")
            .unwrap()
            .as_str(),
        "5"
    );
}

#[test]
fn normalize_strips_transfer_encoding() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("transfer-encoding", "chunked")
        .unwrap()
        .body(ResponseBody::Bytes(b"hello".to_vec()))
        .unwrap();
    let req = NormalizeRequest::new(false);
    let normalized = normalize_response(resp, &req).unwrap();
    assert!(!normalized.headers().contains("transfer-encoding"));
}

// ── ResponseConstructionError ───────────────────────────────────────────────

#[test]
fn response_construction_error_display() {
    let err = ResponseConstructionError::InvalidStatus(0);
    assert!(err.to_string().contains("0"));

    let err = ResponseConstructionError::ForbiddenFramingHeader("transfer-encoding".into());
    assert!(err.to_string().contains("transfer-encoding"));

    let err = ResponseConstructionError::BodyAlreadyConsumed;
    assert!(!err.to_string().is_empty());

    let err = ResponseConstructionError::ContentLengthMismatch {
        declared: 100,
        actual: 50,
    };
    assert!(err.to_string().contains("100"));
}

#[test]
fn response_construction_error_is_std_error() {
    let err: &dyn std::error::Error = &ResponseConstructionError::InvalidStatus(0);
    assert!(!err.to_string().is_empty());
}

// ── Request validation (read-only) ─────────────────────────────────────────

#[test]
fn validate_read_only_method() {
    let result = validate_method("GET");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ReadOnlyMethod::Get);

    let result = validate_method("HEAD");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ReadOnlyMethod::Head);

    let result = validate_method("POST");
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        RequestValidationError::MethodNotAllowed
    );
}

#[test]
fn validate_request_target_accepts_origin_form() {
    let result = validate_request_target("/path");
    assert!(result.is_ok());
}

#[test]
fn validate_request_target_rejects_absolute_form() {
    let result = validate_request_target("http://example.com/path");
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        RequestValidationError::InvalidRequestTarget
    );
}

#[test]
fn validate_request_body_empty() {
    let result = validate_request_body(None, None, 1024);
    assert!(result.is_ok());
}

#[test]
fn validate_request_body_too_large() {
    let result = validate_request_body(Some("2048"), None, 1024);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), RequestValidationError::BodyTooLarge);
}

#[test]
fn validate_request_body_conflicting_headers() {
    let result = validate_request_body(Some("100"), Some("chunked"), 1024);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        RequestValidationError::ConflictingBodyHeaders
    );
}

#[test]
fn request_validation_error_is_std_error() {
    let err: &dyn std::error::Error = &RequestValidationError::MethodNotAllowed;
    assert!(!err.to_string().is_empty());
}

// ── ReadOnlyMethod ──────────────────────────────────────────────────────────

#[test]
fn read_only_method_display() {
    assert_eq!(format!("{}", ReadOnlyMethod::Get), "GET");
    assert_eq!(format!("{}", ReadOnlyMethod::Head), "HEAD");
}

// ── File and stream response building ────────────────────────────────────────

#[test]
fn build_response_with_file_body_plan() {
    use eggserve_core::primitives::response::{BodyPlan, ResponseStatus, StaticResponsePlan};
    let plan = StaticResponsePlan {
        status: ResponseStatus::OK,
        headers: Default::default(),
        body: BodyPlan::FileFull,
    };
    assert_eq!(plan.status.0, 200);
    assert!(matches!(plan.body, BodyPlan::FileFull));
}

#[test]
fn build_response_with_range_body_plan() {
    use eggserve_core::primitives::response::{BodyPlan, ResponseStatus, StaticResponsePlan};
    let plan = StaticResponsePlan {
        status: ResponseStatus::PARTIAL_CONTENT,
        headers: Default::default(),
        body: BodyPlan::FileRange {
            start: 0,
            end_inclusive: 99,
        },
    };
    assert_eq!(plan.status.0, 206);
    match &plan.body {
        BodyPlan::FileRange {
            start,
            end_inclusive,
        } => {
            assert_eq!(*start, 0);
            assert_eq!(*end_inclusive, 99);
        }
        _ => panic!("expected FileRange"),
    }
}

#[test]
fn build_response_with_empty_body_plan() {
    use eggserve_core::primitives::response::{BodyPlan, ResponseStatus, StaticResponsePlan};
    let plan = StaticResponsePlan {
        status: ResponseStatus(204),
        headers: Default::default(),
        body: BodyPlan::Empty,
    };
    assert_eq!(plan.status.0, 204);
    assert!(matches!(plan.body, BodyPlan::Empty));
}

#[test]
fn build_response_with_bytes_body_plan() {
    use eggserve_core::primitives::response::{BodyPlan, ResponseStatus, StaticResponsePlan};
    let plan = StaticResponsePlan {
        status: ResponseStatus::OK,
        headers: Default::default(),
        body: BodyPlan::FullBytes(b"hello".to_vec()),
    };
    match &plan.body {
        BodyPlan::FullBytes(v) => assert_eq!(v, b"hello"),
        _ => panic!("expected FullBytes"),
    }
}

// ── PhantomData compile check for all canonical types ───────────────────────

#[test]
fn all_canonical_types_importable_without_hyper() {
    let _ = std::marker::PhantomData::<(
        Method,
        MethodError,
        HttpVersion,
        HttpVersionError,
        HeaderBlock,
        HeaderName,
        HeaderValue,
        HeaderField,
        HeaderError,
        DuplicateHeaderError,
        RequestTarget,
        RequestTargetError,
        RequestHead,
        ConnectionInfo,
        Scheme,
        TlsInfo,
        StatusCode,
        ResponseBody,
        ResponseConstructionError,
        ReadOnlyMethod,
        RequestValidationError,
    )>;
}

fn _assert_send<T: Send>() {}
fn _assert_sync<T: Send + Sync>() {}

#[test]
fn canonical_types_are_send_and_sync() {
    _assert_send::<Method>();
    _assert_send::<MethodError>();
    _assert_send::<HttpVersion>();
    _assert_send::<HttpVersionError>();
    _assert_send::<HeaderBlock>();
    _assert_send::<HeaderName>();
    _assert_send::<HeaderValue>();
    _assert_send::<HeaderField>();
    _assert_send::<HeaderError>();
    _assert_send::<DuplicateHeaderError>();
    _assert_send::<RequestTarget>();
    _assert_send::<RequestTargetError>();
    _assert_send::<RequestHead>();
    _assert_send::<ConnectionInfo>();
    _assert_send::<Scheme>();
    _assert_send::<TlsInfo>();
    _assert_send::<StatusCode>();
    _assert_send::<ResponseBody>();
    _assert_send::<ResponseConstructionError>();
    _assert_send::<ReadOnlyMethod>();
    _assert_send::<RequestValidationError>();

    _assert_sync::<Method>();
    _assert_sync::<HttpVersion>();
    _assert_sync::<HeaderBlock>();
    _assert_sync::<HeaderName>();
    _assert_sync::<HeaderValue>();
    _assert_sync::<HeaderField>();
    _assert_sync::<RequestTarget>();
    _assert_sync::<RequestHead>();
    _assert_sync::<ConnectionInfo>();
    _assert_sync::<Scheme>();
    _assert_sync::<TlsInfo>();
    _assert_sync::<StatusCode>();
    _assert_sync::<ReadOnlyMethod>();
}
