//! Octet-preserving canonical HTTP metadata (Plan 173).
//!
//! Wire-level fidelity for header field-value octets and request-target byte
//! truthfulness. Inbound tests exercise the actual connection parser path via
//! `serve_http1_connection` over duplex streams (no fabricated
//! `HeaderBlock::from_bytes`-only coverage).

use std::sync::Arc;

use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::connection_info::Scheme;
use eggserve_core::primitives::header_block::{HeaderBlock, HeaderError, HeaderValue};
use eggserve_core::primitives::request_target::RequestTarget;
use eggserve_core::server::connection::{
    serve_http1_connection, ConnectionContext, ConnectionShutdown,
};
use eggserve_core::server::{service_fn, Request, RuntimeConfig, RuntimeState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ── Track A: validation contract ────────────────────────────────────────────

#[test]
fn rejects_cr_lf_nul_del_and_controls() {
    for bad in [
        b"a\rb".as_slice(),
        b"a\nb",
        b"a\x00b",
        b"a\x7fb",
        b"a\x01b",
        b"a\x1fb",
    ] {
        assert_eq!(
            HeaderValue::from_bytes(bad).unwrap_err(),
            HeaderError::InvalidValue,
            "must reject {bad:?}"
        );
    }
}

#[test]
fn preserves_visible_and_opaque_octets() {
    // Visible ASCII + obs-text without UTF-8 interpretation.
    let v = HeaderValue::from_bytes(b"hello\xfa\xfb").unwrap();
    assert_eq!(v.as_bytes(), b"hello\xfa\xfb");
    assert!(v.to_str().is_err());

    // Plain text still interprets.
    let v = HeaderValue::from_bytes(b"text/html; charset=utf-8").unwrap();
    assert_eq!(v.to_str().unwrap(), "text/html; charset=utf-8");

    // Empty is valid for the generic primitive.
    assert!(HeaderValue::from_bytes(b"").unwrap().is_empty());
}

#[test]
fn ows_trimming_is_deliberate_canonical_invariant() {
    assert_eq!(
        HeaderValue::from_bytes(b" \tvalue\t ").unwrap().as_bytes(),
        b"value"
    );
    assert_eq!(
        HeaderValue::from_str(" \tvalue\t ").unwrap().as_bytes(),
        b"value"
    );
    // Interior OWS is preserved.
    assert_eq!(
        HeaderValue::from_bytes(b"a  b\tc").unwrap().as_bytes(),
        b"a  b\tc"
    );
}

#[test]
fn header_limits_count_bytes_not_scalars() {
    // `é` is 2 bytes in UTF-8; limits must see 2, not 1 scalar.
    let v = HeaderValue::from_str("é").unwrap();
    assert_eq!(v.as_bytes().len(), 2);
    assert_eq!(v.to_str().unwrap(), "é");
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn test_config() -> Arc<RuntimeConfig> {
    Arc::new(RuntimeConfig::default())
}

fn http_context() -> ConnectionContext {
    ConnectionContext::for_non_socket(Scheme::Http, None)
}

async fn drive_once(request_bytes: &[u8], service: impl eggserve_core::server::Service) -> Vec<u8> {
    let config = test_config();
    let runtime = Arc::new(RuntimeState::new(&config));
    let (mut client, server) = tokio::io::duplex(128 * 1024);
    let shutdown = ConnectionShutdown::new();
    let driver = tokio::spawn(async move {
        serve_http1_connection(server, service, config, http_context(), runtime, &shutdown).await
    });
    client.write_all(request_bytes).await.unwrap();
    let mut buf = Vec::new();
    let _ = client.read_to_end(&mut buf).await;
    let _ = driver.await;
    buf
}

fn body_from_raw(raw: &[u8]) -> Vec<u8> {
    if let Some(idx) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
        raw[idx + 4..].to_vec()
    } else {
        Vec::new()
    }
}

fn raw_header_value(raw: &[u8], name: &str) -> Option<Vec<u8>> {
    let want = name.as_bytes();
    let mut pos = 0;
    while let Some(start) = raw[pos..].windows(2).position(|w| w == b"\r\n") {
        let line_start = pos + start + 2;
        if raw[line_start..].starts_with(b"\r\n") {
            break;
        }
        let line_end = match raw[line_start..].windows(2).position(|w| w == b"\r\n") {
            Some(e) => line_start + e,
            None => break,
        };
        let line = &raw[line_start..line_end];
        if let Some(colon) = line.iter().position(|b| *b == b':') {
            let (n, v) = line.split_at(colon);
            if n.eq_ignore_ascii_case(want) {
                // Skip single SP after colon per field-line emission.
                let mut val = &v[1..];
                if val.starts_with(b" ") {
                    val = &val[1..];
                }
                return Some(val.to_vec());
            }
        }
        pos = line_end;
    }
    None
}

// ── Track C1: inbound opaque bytes reach the service ────────────────────────

#[tokio::test]
async fn inbound_opaque_bytes_reach_service_unchanged() {
    let svc = service_fn(|req: Request| async move {
        let bytes = req
            .head()
            .headers()
            .get_first("x-opaque")
            .map(|v| v.as_bytes().to_vec())
            .unwrap_or_default();
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(bytes))
            .unwrap())
    });
    let mut req = b"GET / HTTP/1.1\r\nHost: x\r\nX-Opaque: hello".to_vec();
    req.extend_from_slice(b"\xff\xfe");
    req.extend_from_slice(b"\r\nConnection: close\r\n\r\n");
    let raw = drive_once(&req, svc).await;
    let body = body_from_raw(&raw);
    assert_eq!(
        body, b"hello\xff\xfe",
        "opaque bytes must round-trip, got {body:?}"
    );
}

#[tokio::test]
async fn inbound_duplicate_ordering_preserved_with_opaque() {
    let svc = service_fn(|req: Request| async move {
        let all: Vec<Vec<u8>> = req
            .head()
            .headers()
            .get_all("x-dup")
            .into_iter()
            .map(|v| v.as_bytes().to_vec())
            .collect();
        // Echo ordering as joined body with 0x00 separators (0x00 never
        // appears in legal values, so the split is unambiguous).
        let mut body = Vec::new();
        for (i, v) in all.iter().enumerate() {
            if i > 0 {
                body.push(0x1f);
            }
            body.extend_from_slice(v);
        }
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(body))
            .unwrap())
    });
    let mut req = b"GET / HTTP/1.1\r\nHost: x\r\n".to_vec();
    req.extend_from_slice(b"X-Dup: a\xff\r\n");
    req.extend_from_slice(b"X-Dup: b\r\n");
    req.extend_from_slice(b"x-dup: c\xfe\r\n");
    req.extend_from_slice(b"Connection: close\r\n\r\n");
    let raw = drive_once(&req, svc).await;
    let body = body_from_raw(&raw);
    assert_eq!(
        body, b"a\xff\x1fb\x1fc\xfe",
        "duplicate order + opaque bytes must be preserved, got {body:?}"
    );
}

// ── Track C2: outbound opaque bytes reach the wire ──────────────────────────

#[tokio::test]
async fn outbound_opaque_bytes_reach_wire_unchanged() {
    let svc = service_fn(|_req: Request| async move {
        let mut headers = HeaderBlock::new();
        headers.push_bytes("x-opaque", b"hello\xff\xfe").unwrap();
        let mut resp = Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"ok".to_vec()))
            .unwrap();
        for f in headers.iter() {
            resp.head_mut()
                .headers_mut()
                .push(f.name.clone(), f.value.clone());
        }
        Ok(resp)
    });
    let raw = drive_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        svc,
    )
    .await;
    let value = raw_header_value(&raw, "x-opaque").expect("x-opaque must be present");
    assert_eq!(
        value, b"hello\xff\xfe",
        "wire must carry exact octets, got {value:?}"
    );
}

#[tokio::test]
async fn outbound_response_policy_still_applies_to_opaque() {
    // Hop-by-hop stripping is not bypassed by byte preservation: a
    // service-provided `Transfer-Encoding` is still stripped by normalization.
    let svc = service_fn(|_req: Request| async move {
        let mut resp = Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"ok".to_vec()))
            .unwrap();
        // Bypass the builder's text path via byte construction, then let
        // normalization strip it as runtime-owned framing.
        use eggserve_core::primitives::header_block::{HeaderName, HeaderValue};
        resp.head_mut().headers_mut().push(
            HeaderName::new("transfer-encoding").unwrap(),
            HeaderValue::from_bytes(b"chunked\xff")
                .unwrap_or_else(|_| HeaderValue::from_bytes(b"chunked").unwrap()),
        );
        Ok(resp)
    });
    let raw = drive_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        svc,
    )
    .await;
    assert!(
        raw_header_value(&raw, "transfer-encoding").is_none(),
        "runtime-owned framing must still be stripped"
    );
}

// ── Track E: request-target fidelity ────────────────────────────────────────

#[test]
fn request_target_byte_accessors_expose_accepted_wire_bytes() {
    let cases = [
        "/",
        "/foo",
        "/foo%20bar",
        "/foo%2Fbar",
        "/a?b=1&c=2",
        "/path?key=hello%20world",
        "/foo%FF%FE",
        "/%41%42%43",
        "/a//b///c",
        "/foo?bar",
        "/path?a=1?b=2",
        "/foo#bar",
        "/a?b#frag",
    ];
    for raw in cases {
        let t = RequestTarget::parse(raw).unwrap();
        assert_eq!(t.raw_bytes(), raw.as_bytes(), "raw_bytes for {raw}");
        assert_eq!(t.path_bytes(), t.path().as_bytes());
        match t.query() {
            Some(q) => assert_eq!(t.query_bytes().unwrap(), q.as_bytes()),
            None => assert!(t.query_bytes().is_none()),
        }
        // String round-trip is lossless for accepted origin-form.
        assert_eq!(t.raw(), raw);
    }
}

#[test]
fn request_target_empty_query_canonicalizes() {
    // `/path` and `/path?` deliberately canonicalize identically: empty query
    // maps to `None`. Documented, not fabricated.
    let bare = RequestTarget::parse("/path").unwrap();
    let empty = RequestTarget::parse("/path?").unwrap();
    assert!(bare.query().is_none());
    assert!(empty.query().is_none());
    assert!(bare.query_bytes().is_none());
    assert!(empty.query_bytes().is_none());
    assert_eq!(bare.path(), empty.path());
}

#[test]
fn request_target_malformed_remains_rejected() {
    for bad in [
        "",
        "*",
        "foo",
        "http://example.com/",
        "//example.com/file",
        "/foo bar",
        "/foo\tbar",
        "/foo\x1fbar",
        "example.com:443",
    ] {
        assert!(
            RequestTarget::parse(bad).is_err(),
            "{bad:?} must be rejected"
        );
    }
}

#[tokio::test]
async fn request_target_wire_corpus_round_trips() {
    // Accepted origin-form targets survive Hyper parsing into the identical
    // canonical string/bytes. The service echoes path+query; the test compares
    // against the original wire bytes.
    let targets = [
        "/",
        "/hello.txt",
        "/foo%20bar",
        "/foo%2Fbar",
        "/a?b=1&c=2",
        "/path?key=hello%20world",
        "/a//b",
        "/path?a=1?b=2",
    ];
    for target in targets {
        let svc = service_fn(|req: Request| async move {
            let t = req.head().target().path_and_query().to_owned();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(t.into_bytes()))
                .unwrap())
        });
        let req = format!("GET {target} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
        let raw = drive_once(req.as_bytes(), svc).await;
        let body = body_from_raw(&raw);
        assert_eq!(
            body,
            target.as_bytes(),
            "target {target} must round-trip through Hyper + canonical"
        );
    }
}
