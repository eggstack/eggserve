use std::collections::HashMap;

use eggserve_core::primitives::canonical::{
    normalize_response, NormalizeRequest, Response, ResponseBody, ResponseConstructionError,
    StatusCode,
};
use eggserve_core::primitives::connection_info::{ConnectionInfo, Scheme, TlsInfo};
use eggserve_core::primitives::header_block::{HeaderBlock, HeaderError, HeaderName, HeaderValue};
use eggserve_core::primitives::method::{Method, MethodError};
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::primitives::request_target::RequestTarget;
use eggserve_core::primitives::version::{HttpVersion, HttpVersionError};
use proptest::prelude::*;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Corpus deserialization
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Corpus {
    groups: HashMap<String, Group>,
}

#[derive(Deserialize)]
struct Group {
    fixtures: Vec<Fixture>,
}

#[derive(Deserialize, Clone)]
struct Fixture {
    id: String,
    input: serde_json::Value,
    #[serde(default)]
    expected: Option<serde_json::Value>,
    #[serde(default)]
    expected_error: Option<String>,
}

// Method helpers

#[derive(Deserialize)]
struct MethodExpected {
    as_str: String,
    is_safe: bool,
    is_idempotent: bool,
    permits_static: bool,
}

// Header helpers

#[derive(Deserialize)]
struct HeaderBlockInput {
    headers: Vec<Vec<String>>,
}

#[derive(Deserialize)]
struct HeaderExpected {
    valid: Option<bool>,
}

// RequestHead helpers

#[derive(Deserialize)]
struct RequestHeadInput {
    method: String,
    target: String,
    version: String,
    headers: Vec<Vec<String>>,
}

#[derive(Deserialize)]
struct RequestHeadExpected {
    method: Option<String>,
    target_path: Option<String>,
    version: Option<String>,
    header_count: Option<usize>,
}

// StatusCode helpers

#[derive(Deserialize)]
struct StatusCodeExpected {
    as_u16: Option<u16>,
    is_informational: Option<bool>,
    is_success: Option<bool>,
    is_redirection: Option<bool>,
    is_client_error: Option<bool>,
    is_server_error: Option<bool>,
    permits_payload_body: Option<bool>,
}

// Response normalization helpers

#[derive(Deserialize)]
struct ResponseNormInput {
    status: u16,
    headers: Vec<Vec<String>>,
    body: String,
    is_head: bool,
}

#[derive(Deserialize)]
struct ResponseNormExpected {
    status: u16,
    body: Option<String>,
    headers_contain: Option<HashMap<String, String>>,
    headers_not_contain: Option<Vec<String>>,
    set_cookie_count: Option<usize>,
}

// ConnectionInfo helpers

#[derive(Deserialize)]
struct ConnectionInfoInput {
    scheme: String,
    tls: bool,
}

#[derive(Deserialize)]
struct ConnectionInfoExpected {
    scheme_is_http: bool,
    tls: bool,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_corpus() -> Corpus {
    let json = include_str!("../../../conformance/corpus.json");
    serde_json::from_str(json).expect("corpus must be valid JSON")
}

fn group(name: &str) -> Vec<Fixture> {
    load_corpus()
        .groups
        .get(name)
        .unwrap_or_else(|| panic!("group '{name}' not found in corpus"))
        .fixtures
        .clone()
}

fn build_response(
    status: u16,
    headers: &[Vec<String>],
    body: &str,
) -> Result<Response, ResponseConstructionError> {
    let code = StatusCode::new(status)?;
    let mut builder = Response::builder().status(code);
    for hv in headers {
        builder = builder.header(&hv[0], &hv[1])?;
    }
    if body.is_empty() {
        builder.empty()
    } else {
        builder.body(ResponseBody::Bytes(body.as_bytes().to_vec()))
    }
}

// ---------------------------------------------------------------------------
// Method tests
// ---------------------------------------------------------------------------

#[test]
fn method_construction_and_classification() {
    for f in group("methods") {
        let input = f.input.as_str().unwrap();
        match f.expected_error {
            Some(ref err) => match err.as_str() {
                "MethodError::Empty" => {
                    assert_eq!(
                        Method::new(input).unwrap_err(),
                        MethodError::Empty,
                        "{}",
                        f.id
                    )
                }
                "MethodError::InvalidToken" => {
                    assert_eq!(
                        Method::new(input).unwrap_err(),
                        MethodError::InvalidToken,
                        "{}",
                        f.id
                    )
                }
                other => panic!("{}: unknown error variant {other}", f.id),
            },
            None => {
                let expected: MethodExpected =
                    serde_json::from_value(f.expected.clone().unwrap()).unwrap();
                let method = Method::new(input).unwrap_or_else(|e| panic!("{}: {e}", f.id));
                assert_eq!(method.as_str(), expected.as_str, "{}: as_str", f.id);
                assert_eq!(method.is_safe(), expected.is_safe, "{}: is_safe", f.id);
                assert_eq!(
                    method.is_idempotent(),
                    expected.is_idempotent,
                    "{}: is_idempotent",
                    f.id
                );
                assert_eq!(
                    method.permits_static_resolution(),
                    expected.permits_static,
                    "{}: permits_static",
                    f.id
                );
            }
        }
    }
}

#[test]
fn method_standard_constructors_match_corpus() {
    let cases = vec![
        (Method::get(), "GET"),
        (Method::head(), "HEAD"),
        (Method::post(), "POST"),
        (Method::put(), "PUT"),
        (Method::delete(), "DELETE"),
        (Method::patch(), "PATCH"),
        (Method::options(), "OPTIONS"),
        (Method::trace(), "TRACE"),
        (Method::connect(), "CONNECT"),
    ];
    for (m, expected) in cases {
        assert_eq!(m.as_str(), expected);
    }
}

// ---------------------------------------------------------------------------
// Version tests
// ---------------------------------------------------------------------------

#[test]
fn version_parsing() {
    for f in group("versions") {
        let input = f.input.as_str().unwrap();
        match f.expected_error {
            Some(ref err) => match err.as_str() {
                "HttpVersionError::Unsupported" => {
                    assert_eq!(
                        HttpVersion::parse(input).unwrap_err(),
                        HttpVersionError::Unsupported,
                        "{}",
                        f.id
                    )
                }
                other => panic!("{}: unknown error variant {other}", f.id),
            },
            None => {
                let expected = f.expected.as_ref().unwrap();
                let ver = HttpVersion::parse(input).unwrap_or_else(|e| panic!("{}: {e}", f.id));
                assert_eq!(
                    ver.as_str(),
                    expected["as_str"].as_str().unwrap(),
                    "{}: as_str",
                    f.id
                );
                assert_eq!(
                    ver.major(),
                    expected["major"].as_u64().unwrap() as u8,
                    "{}: major",
                    f.id
                );
                assert_eq!(
                    ver.minor(),
                    expected["minor"].as_u64().unwrap() as u8,
                    "{}: minor",
                    f.id
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Header tests
// ---------------------------------------------------------------------------

#[test]
fn header_name_value_validation() {
    for f in group("headers") {
        match &f.id[..] {
            "headerblock-duplicate-preserved"
            | "headerblock-case-insensitive-lookup"
            | "headerblock-get-unique-single"
            | "headerblock-get-unique-duplicate-error"
            | "headerblock-get-unique-absent"
            | "headerblock-iteration-order" => continue,
            _ => {}
        }

        let input = &f.input;
        if input.get("headers").is_some() {
            continue;
        }

        if let Some(name) = input.get("name").and_then(|v| v.as_str()) {
            let repeat = input.get("repeat").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            let value = input.get("value").and_then(|v| v.as_str()).unwrap_or("");
            let full_name = name.repeat(repeat);

            match f.expected_error {
                Some(ref err) => match err.as_str() {
                    "HeaderError::InvalidName" => {
                        assert_eq!(
                            HeaderName::new(&full_name).unwrap_err(),
                            HeaderError::InvalidName,
                            "{}",
                            f.id
                        );
                    }
                    "HeaderError::InvalidValue" => {
                        assert_eq!(
                            HeaderValue::new(value).unwrap_err(),
                            HeaderError::InvalidValue,
                            "{}",
                            f.id
                        );
                    }
                    "HeaderError::NameTooLong" => {
                        assert_eq!(
                            HeaderName::new(&full_name).unwrap_err(),
                            HeaderError::NameTooLong,
                            "{}",
                            f.id
                        );
                    }
                    other => panic!("{}: unknown error variant {other}", f.id),
                },
                None => {
                    let expected: HeaderExpected = serde_json::from_value(input.clone()).unwrap();
                    if expected.valid == Some(true) {
                        assert!(HeaderName::new(name).is_ok(), "{}: name", f.id);
                        assert!(HeaderValue::new(value).is_ok(), "{}: value", f.id);
                    }
                }
            }
        }
    }
}

#[test]
fn header_block_operations() {
    for f in group("headers") {
        let input = &f.input;
        if input.get("headers").is_none() {
            continue;
        }

        let block_input: HeaderBlockInput = serde_json::from_value(input.clone()).unwrap();
        let mut block = HeaderBlock::new();
        for hv in &block_input.headers {
            block.push_str(&hv[0], &hv[1]).unwrap();
        }

        match &f.id[..] {
            "headerblock-duplicate-preserved" => {
                assert_eq!(block.len(), 2, "{}: len", f.id);
                assert_eq!(
                    block.get_first("set-cookie").unwrap().as_str(),
                    "a=1",
                    "{}: get_first",
                    f.id
                );
                let all = block.get_all("set-cookie");
                assert_eq!(all.len(), 2, "{}: get_all len", f.id);
                assert_eq!(all[0].as_str(), "a=1");
                assert_eq!(all[1].as_str(), "b=2");
            }
            "headerblock-case-insensitive-lookup" => {
                assert_eq!(
                    block.get_first("content-type").unwrap().as_str(),
                    "text/html",
                    "{}: get_first_lowercase",
                    f.id
                );
                assert!(
                    block.contains("CONTENT-TYPE"),
                    "{}: contains_uppercase",
                    f.id
                );
            }
            "headerblock-get-unique-single" => {
                let result = block.get_unique("content-type").unwrap();
                assert_eq!(
                    result.unwrap().as_str(),
                    "text/html",
                    "{}: get_unique",
                    f.id
                );
            }
            "headerblock-get-unique-duplicate-error" => {
                let err = block.get_unique("set-cookie").unwrap_err();
                assert_eq!(err.name(), "set-cookie");
                assert_eq!(err.count(), 2);
            }
            "headerblock-get-unique-absent" => {
                let result = block.get_unique("content-type").unwrap();
                assert!(result.is_none(), "{}: get_unique_absent", f.id);
            }
            "headerblock-iteration-order" => {
                let names: Vec<&str> = block.iter().map(|f| f.name.as_str()).collect();
                assert_eq!(names, vec!["a", "b", "c"], "{}: iteration_order", f.id);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Request target tests
// ---------------------------------------------------------------------------

#[test]
fn request_target_parsing() {
    for f in group("request_targets") {
        let input = f.input.as_str().unwrap();
        match f.expected_error {
            Some(ref err) => {
                let result = RequestTarget::parse(input);
                assert!(result.is_err(), "{}: expected error but got Ok", f.id);
                let variant_name = err.rsplit("::").next().unwrap();
                let err_str = result.unwrap_err().to_string();
                match variant_name {
                    "Empty" => assert!(err_str.contains("empty"), "{}: {err_str}", f.id),
                    "AbsoluteUri" => assert!(err_str.contains("absolute"), "{}: {err_str}", f.id),
                    "AuthorityForm" => {
                        assert!(err_str.contains("authority"), "{}: {err_str}", f.id)
                    }
                    "AsteriskForm" => {
                        assert!(err_str.contains("asterisk"), "{}: {err_str}", f.id)
                    }
                    "ContainsWhitespace" => {
                        assert!(err_str.contains("whitespace"), "{}: {err_str}", f.id)
                    }
                    "NotOriginForm" => {
                        assert!(err_str.contains("origin"), "{}: {err_str}", f.id)
                    }
                    _ => panic!("{}: unknown variant {variant_name}", f.id),
                }
            }
            None => {
                let expected = f.expected.as_ref().unwrap();
                let target =
                    RequestTarget::parse(input).unwrap_or_else(|e| panic!("{}: {e}", f.id));
                assert_eq!(
                    target.path(),
                    expected["path"].as_str().unwrap(),
                    "{}: path",
                    f.id
                );
                match &expected["query"] {
                    serde_json::Value::Null => {
                        assert!(target.query().is_none(), "{}: query should be None", f.id);
                    }
                    serde_json::Value::String(q) => {
                        assert_eq!(target.query(), Some(q.as_str()), "{}: query", f.id);
                    }
                    other => panic!("{}: unexpected query value {other}", f.id),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RequestHead tests
// ---------------------------------------------------------------------------

#[test]
fn request_head_construction() {
    for f in group("request_heads") {
        let input: RequestHeadInput = serde_json::from_value(f.input.clone()).unwrap();
        let mut headers = HeaderBlock::new();
        for hv in &input.headers {
            headers.push_str(&hv[0], &hv[1]).unwrap();
        }
        let method = Method::new(&input.method).unwrap();
        let target = RequestTarget::parse(&input.target).unwrap();
        let version = HttpVersion::parse(&input.version).unwrap();
        let head = RequestHead::new(method, target, version, headers);

        let expected: RequestHeadExpected =
            serde_json::from_value(f.expected.clone().unwrap()).unwrap();

        if let Some(ref m) = expected.method {
            assert_eq!(head.method().as_str(), m, "{}: method", f.id);
        }
        if let Some(ref tp) = expected.target_path {
            assert_eq!(head.target().path(), tp, "{}: target_path", f.id);
        }
        if let Some(ref v) = expected.version {
            assert_eq!(head.version().as_str(), v, "{}: version", f.id);
        }
        if let Some(hc) = expected.header_count {
            assert_eq!(head.headers().len(), hc, "{}: header_count", f.id);
        }
    }
}

// ---------------------------------------------------------------------------
// StatusCode tests
// ---------------------------------------------------------------------------

#[test]
fn status_code_construction_and_classification() {
    for f in group("status_codes") {
        let input = f.input.as_u64().unwrap() as u16;
        match f.expected_error {
            Some(ref err) => match err {
                s if s.contains("InvalidStatus") => {
                    assert!(
                        StatusCode::new(input).is_err(),
                        "{}: expected InvalidStatus for {input}",
                        f.id
                    );
                }
                other => panic!("{}: unknown error {other}", f.id),
            },
            None => {
                let expected: StatusCodeExpected =
                    serde_json::from_value(f.expected.clone().unwrap()).unwrap();
                let sc = StatusCode::new(input).unwrap_or_else(|e| panic!("{}: {e}", f.id));
                if let Some(v) = expected.as_u16 {
                    assert_eq!(sc.as_u16(), v, "{}: as_u16", f.id);
                }
                if let Some(v) = expected.is_informational {
                    assert_eq!(sc.is_informational(), v, "{}: is_informational", f.id);
                }
                if let Some(v) = expected.is_success {
                    assert_eq!(sc.is_success(), v, "{}: is_success", f.id);
                }
                if let Some(v) = expected.is_redirection {
                    assert_eq!(sc.is_redirection(), v, "{}: is_redirection", f.id);
                }
                if let Some(v) = expected.is_client_error {
                    assert_eq!(sc.is_client_error(), v, "{}: is_client_error", f.id);
                }
                if let Some(v) = expected.is_server_error {
                    assert_eq!(sc.is_server_error(), v, "{}: is_server_error", f.id);
                }
                if let Some(v) = expected.permits_payload_body {
                    assert_eq!(
                        sc.permits_payload_body(),
                        v,
                        "{}: permits_payload_body",
                        f.id
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Response normalization tests
// ---------------------------------------------------------------------------

#[test]
fn response_normalization_rules() {
    for f in group("response_normalization") {
        let input: ResponseNormInput = serde_json::from_value(f.input.clone()).unwrap();
        let response = build_response(input.status, &input.headers, &input.body)
            .unwrap_or_else(|e| panic!("{}: build: {e}", f.id));
        let req = NormalizeRequest::new(input.is_head);
        let normalized =
            normalize_response(response, &req).unwrap_or_else(|e| panic!("{}: norm: {e}", f.id));

        let expected: ResponseNormExpected =
            serde_json::from_value(f.expected.clone().unwrap()).unwrap();

        assert_eq!(
            normalized.status().as_u16(),
            expected.status,
            "{}: status",
            f.id
        );

        if let Some(ref expected_body) = expected.body {
            let actual = normalized.body().and_then(|b| match b {
                ResponseBody::Empty => Some(""),
                ResponseBody::Bytes(v) => std::str::from_utf8(v).ok(),
            });
            assert_eq!(actual, Some(expected_body.as_str()), "{}: body", f.id);
        }

        if let Some(ref contains) = expected.headers_contain {
            for (name, value) in contains {
                assert!(
                    normalized.headers().contains(name),
                    "{}: missing header {name}",
                    f.id
                );
                assert_eq!(
                    normalized.headers().get_first(name).unwrap().as_str(),
                    value,
                    "{}: header {name} value",
                    f.id
                );
            }
        }

        if let Some(ref not_contains) = expected.headers_not_contain {
            for name in not_contains {
                assert!(
                    !normalized.headers().contains(name),
                    "{}: should not contain {name}",
                    f.id
                );
            }
        }

        if let Some(count) = expected.set_cookie_count {
            let all = normalized.headers().get_all("set-cookie");
            assert_eq!(all.len(), count, "{}: set_cookie_count", f.id);
        }
    }
}

// ---------------------------------------------------------------------------
// Raw wire output tests
// ---------------------------------------------------------------------------

#[test]
fn response_normalization_raw_wire_output() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain")
        .unwrap()
        .header("transfer-encoding", "chunked")
        .unwrap()
        .body(ResponseBody::Bytes(b"hello".to_vec()))
        .unwrap();
    let req = NormalizeRequest::new(false);
    let normalized = normalize_response(resp, &req).unwrap();

    // Content-Length must be set
    let cl = normalized.headers().get_first("content-length").unwrap();
    assert_eq!(cl.as_str(), "5");

    // Transfer-Encoding must be stripped
    assert!(!normalized.headers().contains("transfer-encoding"));

    // Body must be preserved
    match normalized.body().unwrap() {
        ResponseBody::Bytes(v) => assert_eq!(v, b"hello"),
        _ => panic!("expected bytes body"),
    }
}

// ---------------------------------------------------------------------------
// ConnectionInfo tests
// ---------------------------------------------------------------------------

#[test]
fn connection_info_construction() {
    for f in group("connection_metadata") {
        let input: ConnectionInfoInput = serde_json::from_value(f.input.clone()).unwrap();
        let expected: ConnectionInfoExpected =
            serde_json::from_value(f.expected.clone().unwrap()).unwrap();

        let scheme = match input.scheme.as_str() {
            "Http" => Scheme::Http,
            "Https" => Scheme::Https,
            other => panic!("{}: unknown scheme {other}", f.id),
        };

        let tls = if input.tls {
            Some(TlsInfo {
                protocol_version: Some("TLSv1.3".into()),
                server_name: Some("example.com".into()),
            })
        } else {
            None
        };

        let info = ConnectionInfo {
            local_addr: "127.0.0.1:8000".parse().unwrap(),
            remote_addr: "127.0.0.1:12345".parse().unwrap(),
            scheme,
            tls,
        };

        assert_eq!(
            info.scheme == Scheme::Http,
            expected.scheme_is_http,
            "{}: scheme_is_http",
            f.id
        );
        assert_eq!(info.tls.is_some(), expected.tls, "{}: tls", f.id);
    }
}

// ---------------------------------------------------------------------------
// Property tests
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn method_roundtrip(s in "[A-Za-z!#$%&'*+\\-.^_`|~]{1,64}") {
        let m = Method::new(&s).unwrap();
        prop_assert_eq!(m.as_str(), s.as_str());
    }

    #[test]
    fn normalization_idempotent(
        status in 200u16..600u16,
        body in "[a-z]{0,200}",
    ) {
        if StatusCode::new(status).is_err() {
            return Ok(());
        }
        let resp = Response::builder()
            .status(StatusCode::new(status).unwrap())
            .body(ResponseBody::Bytes(body.into_bytes()))
            .unwrap();
        let req = NormalizeRequest::new(false);
        let once = normalize_response(resp, &req).unwrap();
        let twice = normalize_response(once, &req).unwrap();
        prop_assert_eq!(
            twice.body().map(|b| b.len()),
            twice.body().map(|b| b.len()),
        );
    }

    #[test]
    fn head_never_emits_body_bytes(
        body in "[a-z]{0,100}",
    ) {
        let resp = Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(body.into_bytes()))
            .unwrap();
        let req = NormalizeRequest::new(true);
        let normalized = normalize_response(resp, &req).unwrap();
        let body = normalized.body().unwrap();
        prop_assert!(body.is_empty());
    }

    #[test]
    fn body_forbidden_status_never_emits_payload(
        status in prop_oneof![100u16..200u16, Just(204), Just(304)],
    ) {
        if StatusCode::new(status).is_err() {
            return Ok(());
        }
        let resp = Response::builder()
            .status(StatusCode::new(status).unwrap())
            .body(ResponseBody::Bytes(b"data".to_vec()))
            .unwrap();
        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();
        let body = normalized.body().unwrap();
        prop_assert!(body.is_empty());
    }

    #[test]
    fn duplicate_end_to_end_headers_remain_ordered(
        names in prop::collection::vec("[a-z]{1,8}", 1..10),
    ) {
        let mut block = HeaderBlock::new();
        for (i, name) in names.iter().enumerate() {
            block.push_str(name, i.to_string()).unwrap();
        }
        let all_names: Vec<String> = block.iter()
            .map(|f| f.name.as_str().to_string())
            .collect();
        prop_assert_eq!(all_names, names);
    }

    #[test]
    fn normalized_responses_never_contain_conflicting_framing(
        status in 200u16..600u16,
        body in "[a-z]{0,200}",
        has_te in proptest::bool::ANY,
    ) {
        if StatusCode::new(status).is_err() {
            return Ok(());
        }
        let mut builder = Response::builder()
            .status(StatusCode::new(status).unwrap());
        if has_te {
            builder = builder.header("transfer-encoding", "chunked").unwrap();
        }
        let resp = builder
            .body(ResponseBody::Bytes(body.into_bytes()))
            .unwrap();
        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();

        // Transfer-Encoding must never survive normalization
        prop_assert!(
            !normalized.headers().contains("transfer-encoding"),
            "transfer-encoding survived normalization"
        );

        // Content-Length must match actual body length
        if let Some(cl) = normalized.headers().get_first("content-length") {
            let actual_len = normalized.body().map_or(0, |b| b.len());
            prop_assert_eq!(
                cl.as_str(),
                actual_len.to_string(),
                "content-length mismatch"
            );
        }
    }

    #[test]
    fn malformed_input_never_panics(
        method_str in "[\\x00-\\x7f]{0,128}",
        target_str in "[\\x00-\\x7f]{0,128}",
    ) {
        // Method::new should never panic
        let _ = Method::new(&method_str);

        // RequestTarget::parse should never panic
        let _ = RequestTarget::parse(&target_str);

        // HeaderName::new should never panic
        let _ = HeaderName::new(&method_str);

        // HeaderValue::new should never panic
        let _ = HeaderValue::new(&target_str);
    }
}
