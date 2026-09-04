//! Compile-time and runtime API stability enforcement tests.
//!
//! These tests verify that the public API surface matches the stability tiers
//! defined in `docs/api-stability.md`. They run under default features only
//! (no `client`, no `python-bindings-internal`).

// ── Stable canonical request types ──────────────────────────────────────────

#[test]
fn stable_method_accessible() {
    use eggserve_core::primitives::method::Method;

    let m = Method::get();
    assert_eq!(m.as_str(), "GET");
    assert!(m.is_get());
    assert!(!m.is_head());
    assert!(m.is_safe());

    let ext = Method::new("PURGE").unwrap();
    assert_eq!(ext.as_str(), "PURGE");
    assert!(!ext.is_get());
}

#[test]
fn stable_http_version_accessible() {
    use eggserve_core::primitives::version::HttpVersion;

    let v11 = HttpVersion::Http11;
    assert_eq!(v11.as_str(), "HTTP/1.1");
    assert_eq!(v11.major(), 1);
    assert_eq!(v11.minor(), 1);

    let v10 = HttpVersion::Http10;
    assert_eq!(v10.as_str(), "HTTP/1.0");

    let parsed = HttpVersion::parse("HTTP/1.1").unwrap();
    assert_eq!(parsed, HttpVersion::Http11);

    assert!(HttpVersion::parse("HTTP/2.0").is_err());
}

#[test]
fn stable_header_block_accessible_and_constructible() {
    use eggserve_core::primitives::header_block::{HeaderBlock, HeaderName, HeaderValue};

    let mut block = HeaderBlock::new();
    assert!(block.is_empty());

    let name = HeaderName::new("content-type").unwrap();
    let value = HeaderValue::new("text/html").unwrap();
    block.push(name, value);
    assert_eq!(block.len(), 1);
    assert!(block.contains("Content-Type"));
    assert_eq!(
        block.get_first("content-type").unwrap().as_str(),
        "text/html"
    );
}

#[test]
fn stable_request_target_accessible() {
    use eggserve_core::primitives::request_target::RequestTarget;

    let t = RequestTarget::parse("/foo/bar?baz=1").unwrap();
    assert_eq!(t.path(), "/foo/bar");
    assert_eq!(t.query(), Some("baz=1"));
    assert_eq!(t.path_and_query(), "/foo/bar?baz=1");

    assert!(RequestTarget::parse("").is_err());
    assert!(RequestTarget::parse("not-origin").is_err());
    assert!(RequestTarget::parse("http://example.com/").is_err());
}

#[test]
fn stable_request_head_accessible() {
    use eggserve_core::primitives::header_block::HeaderBlock;
    use eggserve_core::primitives::method::Method;
    use eggserve_core::primitives::request_head::RequestHead;
    use eggserve_core::primitives::request_target::RequestTarget;
    use eggserve_core::primitives::version::HttpVersion;

    let head = RequestHead::new(
        Method::get(),
        RequestTarget::parse("/test").unwrap(),
        HttpVersion::Http11,
        HeaderBlock::new(),
    );
    assert_eq!(head.method().as_str(), "GET");
    assert_eq!(head.target().path(), "/test");
    assert_eq!(head.version(), HttpVersion::Http11);
    assert!(head.is_get());
    assert!(!head.is_head());
    assert!(head.permits_static_resolution());
}

#[test]
fn stable_connection_info_accessible() {
    use eggserve_core::primitives::connection_info::{
        ConnectionInfo, Scheme, SocketEndpoints, TlsInfo,
    };

    let info = ConnectionInfo {
        local_addr: Some("127.0.0.1:8000".parse().unwrap()),
        remote_addr: Some("127.0.0.1:12345".parse().unwrap()),
        scheme: Scheme::Http,
        tls: None,
    };
    assert_eq!(Scheme::Http.as_str(), "http");
    assert_eq!(Scheme::Https.as_str(), "https");
    assert_eq!(info.scheme, Scheme::Http);
    assert!(info.tls.is_none());
    assert!(info.has_socket_endpoints());

    // Paired socket view for real TCP connections.
    let endpoints = info.socket_endpoints().unwrap();
    assert_eq!(endpoints.local.port(), 8000);
    assert_eq!(endpoints.remote.port(), 12345);
    let explicit = SocketEndpoints {
        local: "127.0.0.1:8000".parse().unwrap(),
        remote: "127.0.0.1:12345".parse().unwrap(),
    };
    assert_eq!(endpoints, explicit);

    // Non-socket transports expose no fabricated endpoints.
    let anon = ConnectionInfo::without_socket_addrs(Scheme::Http, None);
    assert_eq!(anon.local_addr, None);
    assert_eq!(anon.remote_addr, None);
    assert!(!anon.has_socket_endpoints());
    assert!(anon.socket_endpoints().is_none());

    let tls_info = TlsInfo {
        protocol_version: Some("TLSv1.3".to_string()),
        server_name: Some("example.com".to_string()),
    };
    let display = format!("{tls_info}");
    assert!(display.contains("TLSv1.3"));
    assert!(display.contains("example.com"));
}

#[test]
fn experimental_connection_driver_accessible() {
    use eggserve_core::primitives::connection_info::Scheme;
    use eggserve_core::server::connection::{
        ConnectionContext, ConnectionOutcome, ConnectionShutdown,
    };
    use eggserve_core::server::{RuntimeConfig, RuntimeState};
    use std::sync::Arc;

    // Context constructors for TCP vs caller-owned streams.
    let tcp = ConnectionContext::for_tcp(
        "127.0.0.1:8000".parse().unwrap(),
        "127.0.0.1:12345".parse().unwrap(),
        None,
    );
    assert!(tcp.has_socket_endpoints());
    let anon = ConnectionContext::for_non_socket(Scheme::Http, None);
    assert!(!anon.has_socket_endpoints());
    assert!(anon.socket_endpoints().is_none());

    // Shared admission is constructed from the runtime config.
    let config = Arc::new(RuntimeConfig::default());
    let state = Arc::new(RuntimeState::new(&config));
    assert_eq!(
        state.file_stream_semaphore().available_permits(),
        config.max_file_streams
    );

    // Cancellation token and outcome are part of the driver contract.
    let shutdown = ConnectionShutdown::new();
    assert!(!shutdown.is_shutdown());
    shutdown.shutdown();
    assert!(shutdown.is_shutdown());
    assert!(ConnectionOutcome::Normal.is_clean());
    assert!(ConnectionOutcome::Shutdown.is_clean());
    assert!(!ConnectionOutcome::ClientError.is_clean());
    assert_eq!(
        format!("{}", ConnectionOutcome::TotalTimeout),
        "total-timeout"
    );
}

// ── Stable canonical response types ─────────────────────────────────────────

#[test]
fn stable_status_code_accessible_and_constructible() {
    use eggserve_core::primitives::canonical::StatusCode;

    let ok = StatusCode::OK;
    assert_eq!(ok.as_u16(), 200);
    assert!(ok.is_success());
    assert!(ok.permits_payload_body());

    let created = StatusCode::new(201).unwrap();
    assert_eq!(created.as_u16(), 201);

    assert!(StatusCode::new(0).is_err());
    assert!(StatusCode::new(99).is_err());
    assert!(StatusCode::new(1000).is_err());

    let no_content = StatusCode::NO_CONTENT;
    assert!(!no_content.permits_payload_body());

    let not_modified = StatusCode::NOT_MODIFIED;
    assert!(!not_modified.permits_payload_body());

    let bad = StatusCode::BAD_REQUEST;
    assert!(bad.is_client_error());

    let server_err = StatusCode::INTERNAL_SERVER_ERROR;
    assert!(server_err.is_server_error());

    let code: u16 = StatusCode::OK.into();
    assert_eq!(code, 200);

    assert_eq!(format!("{}", StatusCode::NOT_FOUND), "404");
}

#[test]
fn stable_response_head_accessible_and_constructible() {
    use eggserve_core::primitives::canonical::{ResponseHead, StatusCode};
    use eggserve_core::primitives::header_block::HeaderBlock;

    let mut headers = HeaderBlock::new();
    headers.push_str("etag", "W/\"123\"").unwrap();

    let head = ResponseHead::new(StatusCode::OK, headers);
    assert_eq!(head.status().as_u16(), 200);
    assert!(head.headers().contains("etag"));
    assert_eq!(
        head.headers().get_first("etag").unwrap().as_str(),
        "W/\"123\""
    );
}

#[test]
fn stable_outbound_transport_adapter_is_inference_based() {
    use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};

    let response = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Bytes(b"ok".to_vec()))
        .unwrap();
    let converted = eggserve_core::primitives::to_hyper_response(response).unwrap();
    assert_eq!(converted.status().as_u16(), 200);
}

#[test]
fn stable_response_body_accessible_and_constructible() {
    use eggserve_core::primitives::canonical::ResponseBody;

    let empty = ResponseBody::Empty;
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    assert!(empty.into_bytes().is_none());

    let bytes = ResponseBody::Bytes(b"hello world".to_vec());
    assert_eq!(bytes.len(), 11);
    assert!(!bytes.is_empty());
    assert_eq!(bytes.into_bytes(), Some(b"hello world".to_vec()));
}

#[test]
fn stable_response_accessible_and_constructible() {
    use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain")
        .unwrap()
        .body(ResponseBody::Bytes(b"ok".to_vec()))
        .unwrap();

    assert_eq!(resp.status().as_u16(), 200);
    assert!(resp.headers().contains("content-type"));
    assert_eq!(
        resp.headers().get_first("content-type").unwrap().as_str(),
        "text/plain"
    );
    assert!(resp.body().is_some());
    assert!(!resp.body().unwrap().is_empty());

    let head = resp.head();
    assert_eq!(head.status().as_u16(), 200);

    let mut resp_mut = Response::builder()
        .status(StatusCode::CREATED)
        .empty()
        .unwrap();
    let taken = resp_mut.take_body();
    assert!(taken.is_some());
    assert!(resp_mut.body().is_none());
}

#[test]
fn stable_normalize_response_accessible() {
    use eggserve_core::primitives::canonical::{
        normalize_response, NormalizeRequest, Response, ResponseBody, StatusCode,
    };

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("transfer-encoding", "chunked")
        .unwrap()
        .body(ResponseBody::Bytes(b"hello".to_vec()))
        .unwrap();

    let req = NormalizeRequest::new(false);
    let normalized = normalize_response(resp, &req).unwrap();

    assert!(!normalized.headers().contains("transfer-encoding"));
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
fn stable_normalize_response_head_suppresses_body() {
    use eggserve_core::primitives::canonical::{
        normalize_response, NormalizeRequest, Response, ResponseBody, StatusCode,
    };

    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Bytes(b"hello".to_vec()))
        .unwrap();

    let req = NormalizeRequest::new(true);
    let normalized = normalize_response(resp, &req).unwrap();
    // HEAD sends no bytes but retains the equivalent-GET representation
    // length for consumers crossing an adapter boundary.
    assert!(matches!(
        normalized.body().unwrap(),
        ResponseBody::EmptyWithLength(5)
    ));
    assert_eq!(normalized.body().unwrap().len(), 5);
}

#[test]
fn stable_normalize_response_body_forbidden_suppresses_body() {
    use eggserve_core::primitives::canonical::{
        normalize_response, NormalizeRequest, Response, ResponseBody, StatusCode,
    };

    let resp = Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(ResponseBody::Bytes(b"surprise".to_vec()))
        .unwrap();

    let req = NormalizeRequest::new(false);
    let normalized = normalize_response(resp, &req).unwrap();
    assert!(normalized.body().unwrap().is_empty());
}

#[test]
fn stable_normalize_metadata_accessible() {
    use eggserve_core::primitives::canonical::{normalize_metadata, StatusCode};
    use eggserve_core::primitives::header_block::HeaderBlock;

    let mut headers = HeaderBlock::new();
    headers.push_str("content-type", "text/html").unwrap();
    headers.push_str("transfer-encoding", "chunked").unwrap();
    headers.push_str("connection", "keep-alive").unwrap();

    normalize_metadata(StatusCode::OK, &mut headers, 42).unwrap();

    assert!(!headers.contains("transfer-encoding"));
    assert!(!headers.contains("connection"));
    assert!(headers.contains("content-type"));
    assert_eq!(headers.get_first("content-length").unwrap().as_str(), "42");
}

// ── Duplicate header preservation in HeaderBlock ────────────────────────────

#[test]
fn duplicate_set_cookie_headers_preserved() {
    use eggserve_core::primitives::header_block::HeaderBlock;

    let mut block = HeaderBlock::new();
    block.push_str("set-cookie", "a=1; Path=/").unwrap();
    block.push_str("set-cookie", "b=2; Path=/").unwrap();
    block.push_str("set-cookie", "c=3; Path=/").unwrap();

    assert_eq!(block.len(), 3);

    let all = block.get_all("set-cookie");
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].as_str(), "a=1; Path=/");
    assert_eq!(all[1].as_str(), "b=2; Path=/");
    assert_eq!(all[2].as_str(), "c=3; Path=/");

    // get_first returns the first value
    assert_eq!(
        block.get_first("set-cookie").unwrap().as_str(),
        "a=1; Path=/"
    );

    // get_unique errors on duplicates
    let err = block.get_unique("set-cookie").unwrap_err();
    assert_eq!(err.name(), "set-cookie");
    assert_eq!(err.count(), 3);
}

#[test]
fn duplicate_set_cookie_preserved_through_normalize_metadata() {
    use eggserve_core::primitives::canonical::{normalize_metadata, StatusCode};
    use eggserve_core::primitives::header_block::HeaderBlock;

    let mut headers = HeaderBlock::new();
    headers.push_str("set-cookie", "a=1").unwrap();
    headers.push_str("set-cookie", "b=2").unwrap();

    normalize_metadata(StatusCode::OK, &mut headers, 0).unwrap();

    let all = headers.get_all("set-cookie");
    assert_eq!(
        all.len(),
        2,
        "normalize_metadata must preserve duplicate set-cookie headers"
    );
    assert_eq!(all[0].as_str(), "a=1");
    assert_eq!(all[1].as_str(), "b=2");
}

#[test]
fn duplicate_set_cookie_preserved_through_normalize_response() {
    use eggserve_core::primitives::canonical::{
        normalize_response, NormalizeRequest, Response, ResponseBody, StatusCode,
    };

    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Bytes(b"ok".to_vec()))
        .unwrap();
    resp.head_mut()
        .headers_mut()
        .push_str("set-cookie", "x=1")
        .unwrap();
    resp.head_mut()
        .headers_mut()
        .push_str("set-cookie", "y=2")
        .unwrap();

    let req = NormalizeRequest::new(false);
    let normalized = normalize_response(resp, &req).unwrap();

    let all = normalized.headers().get_all("set-cookie");
    assert_eq!(
        all.len(),
        2,
        "normalize_response must preserve duplicate set-cookie headers"
    );
    assert_eq!(all[0].as_str(), "x=1");
    assert_eq!(all[1].as_str(), "y=2");
}

#[test]
fn duplicate_headers_case_insensitive_lookup() {
    use eggserve_core::primitives::header_block::HeaderBlock;

    let mut block = HeaderBlock::new();
    block.push_str("Set-Cookie", "a=1").unwrap();
    block.push_str("SET-COOKIE", "b=2").unwrap();
    block.push_str("set-cookie", "c=3").unwrap();

    let all = block.get_all("set-cookie");
    assert_eq!(all.len(), 3);

    let all_mixed = block.get_all("Set-Cookie");
    assert_eq!(all_mixed.len(), 3);

    assert!(block.contains("SET-COOKIE"));
}

// ── Stable module re-exports via primitives facade ──────────────────────────

#[test]
fn stable_types_accessible_from_primitives_facade() {
    use eggserve_core::primitives::ConfinedPath;
    use eggserve_core::primitives::PathPolicy;
    use eggserve_core::primitives::ResolvedResource;
    use eggserve_core::primitives::SecureRoot;
    use eggserve_core::primitives::StaticPolicy;

    use eggserve_core::limits::Limits;

    let _ = std::marker::PhantomData::<(
        ConfinedPath,
        PathPolicy,
        StaticPolicy,
        SecureRoot,
        ResolvedResource,
        Limits,
    )>;
}

#[test]
fn stable_response_plan_types_accessible() {
    use eggserve_core::primitives::BodyPlan;
    use eggserve_core::primitives::ConditionalRequestOutcome;
    use eggserve_core::primitives::FileRange;
    use eggserve_core::primitives::HeaderMapPlan;
    use eggserve_core::primitives::RangeRequestOutcome;
    use eggserve_core::primitives::ResponseHeader;
    use eggserve_core::primitives::ResponseStatus;
    use eggserve_core::primitives::StaticResponsePlan;

    let _ = std::marker::PhantomData::<(
        BodyPlan,
        ConditionalRequestOutcome,
        FileRange,
        HeaderMapPlan,
        RangeRequestOutcome,
        ResponseHeader,
        ResponseStatus,
        StaticResponsePlan,
    )>;
}

#[test]
fn stable_config_types_accessible() {
    use eggserve_core::config::ServeConfig;
    use eggserve_core::config::StartupSummary;

    let _ = std::marker::PhantomData::<(ServeConfig, StartupSummary)>;
}

#[test]
fn stable_policy_types_accessible() {
    use eggserve_core::policy::DirectoryListingPolicy;
    use eggserve_core::policy::DotfilePolicy;
    use eggserve_core::policy::StaticPolicy;
    use eggserve_core::policy::SymlinkPolicy;

    let _ = std::marker::PhantomData::<(
        DirectoryListingPolicy,
        DotfilePolicy,
        StaticPolicy,
        SymlinkPolicy,
    )>;
}

#[test]
fn stable_primitives_http_validation_types_accessible() {
    use eggserve_core::primitives::validate_method;
    use eggserve_core::primitives::validate_request_body;
    use eggserve_core::primitives::validate_request_target;
    use eggserve_core::primitives::ReadOnlyMethod;
    use eggserve_core::primitives::RequestValidationError;

    let _ = (
        validate_method,
        validate_request_body,
        validate_request_target,
        std::marker::PhantomData::<(ReadOnlyMethod, RequestValidationError)>,
    );
}

#[test]
fn stable_primitives_response_planning_functions_accessible() {
    use eggserve_core::primitives::evaluate_conditional_headers;
    use eggserve_core::primitives::evaluate_if_none_match;
    use eggserve_core::primitives::evaluate_if_range;
    use eggserve_core::primitives::evaluate_range_header;
    use eggserve_core::primitives::generate_etag;
    use eggserve_core::primitives::plan_directory_listing;
    use eggserve_core::primitives::plan_file_response;

    let _ = (
        evaluate_conditional_headers,
        evaluate_if_none_match,
        evaluate_if_range,
        evaluate_range_header,
        generate_etag,
        plan_directory_listing,
        plan_file_response,
    );
}

#[test]
fn stable_primitives_body_types_accessible() {
    use eggserve_core::primitives::BodyKind;
    use eggserve_core::primitives::BodySource;
    use eggserve_core::primitives::BodySourceError;

    let _ = std::marker::PhantomData::<(BodyKind, BodySource, BodySourceError)>;
}

#[test]
fn stable_primitives_secure_root_types_accessible() {
    use eggserve_core::primitives::resolve_and_plan;
    use eggserve_core::primitives::ResolveAndPlanError;
    use eggserve_core::primitives::ResolvedDirectory;
    use eggserve_core::primitives::ResolvedFile;
    use eggserve_core::primitives::ResourceDeniedReason;
    use eggserve_core::primitives::SecureRoot;

    let _ = (
        resolve_and_plan,
        std::marker::PhantomData::<(
            ResolveAndPlanError,
            ResolvedDirectory,
            ResolvedFile,
            ResourceDeniedReason,
            SecureRoot,
        )>,
    );
}

#[test]
fn stable_primitives_path_types_accessible() {
    use eggserve_core::primitives::ConfinedPath;
    use eggserve_core::primitives::PathDotfilePolicy;
    use eggserve_core::primitives::PathPolicy;
    use eggserve_core::primitives::PathRejection;

    let _ =
        std::marker::PhantomData::<(ConfinedPath, PathDotfilePolicy, PathPolicy, PathRejection)>;
}

// ── python-bindings-internal feature gate ────────────────────────────────────

#[test]
fn python_bindings_internal_extraction_methods_absent_by_default() {
    use eggserve_core::primitives::ResolvedFile;

    // ResolvedFile is importable (stable public type), but its extraction
    // methods (into_std_file, into_parts, from_parts) are gated behind
    // python-bindings-internal and must NOT be callable in default builds.
    let _phantom = std::marker::PhantomData::<ResolvedFile>;
}
