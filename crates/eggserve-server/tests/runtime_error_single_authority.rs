//! Milestone 306 — the runtime error representation has a single authority.
//!
//! `eggserve_primitives::canonical::runtime_error_with_policy` is the sole
//! runtime-error representation table for H1, H2, and H3. This suite proves,
//! from outside the crate, that
//!
//! 1. the public authority path (`runtime_error_with_policy` ->
//!    `normalize_response` -> `eggserve_server::adapters`) renders the
//!    documented status, header set, and body for the whole
//!    status x policy x HEAD x body-forbidden matrix, and
//! 2. a runtime error actually reaches the client with exactly those bytes.
//!
//! The crate-internal equivalence test (which covers the crate-private
//! helpers this milestone changed) lives in `crates/eggserve-server/src/response.rs`.
//! It carries the frozen baseline capture of the pre-consolidation wire bytes.

use std::time::Duration;

use eggserve_primitives::canonical::{
    normalize_response, runtime_error_with_policy, NormalizeRequest, StatusCode,
};
use eggserve_primitives::policy::ErrorRepresentationPolicy;
use eggserve_server::{service_fn, RuntimeConfig, Server};
use http_body_util::BodyExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// The canonical plain-text body the authority renders for a status.
fn canonical_body(status: u16) -> &'static str {
    match status {
        100 => "100 Continue\n",
        204 => "204 No Content\n",
        205 => "205 Reset Content\n",
        304 => "304 Not Modified\n",
        400 => "400 Bad Request\n",
        404 => "404 Not Found\n",
        405 => "405 Method Not Allowed\n",
        413 => "413 Payload Too Large\n",
        417 => "417 Expectation Failed\n",
        500 => "500 Internal Server Error\n",
        503 => "503 Service Unavailable\n",
        _ => "",
    }
}

/// The status codes the runtime error path can render.
const MATRIX_STATUSES: &[u16] = &[100, 204, 205, 304, 400, 404, 405, 413, 417, 500, 503];

/// The rendered representation of one runtime error.
struct Representation {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// Render a runtime error through the public authority path.
async fn authority_representation(
    status: u16,
    is_head: bool,
    policy: ErrorRepresentationPolicy,
) -> Representation {
    let canonical_status = StatusCode::new(status).expect("canonical status");
    let canonical = runtime_error_with_policy(canonical_status, is_head, policy);
    let normalized =
        normalize_response(canonical, &NormalizeRequest::new(is_head)).expect("normalizes");
    let response = eggserve_server::adapters::to_hyper_response_without_origin_date(normalized)
        .expect("converts to the Hyper boundary");
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().expect("header value is text").to_owned(),
            )
        })
        .collect();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body collects")
        .to_bytes()
        .to_vec();
    Representation {
        status,
        headers,
        body,
    }
}

/// The header set the representation must carry, derived from the documented
/// rules: `Minimal` adds `Content-Type`, `405` adds `Allow`, payload-permitting
/// statuses carry `Content-Length`, and body-forbidden statuses carry none.
fn expected_headers(
    status: u16,
    policy: ErrorRepresentationPolicy,
    is_head: bool,
) -> Vec<(String, String)> {
    let mut expected: Vec<(String, String)> = Vec::new();
    if policy == ErrorRepresentationPolicy::Minimal {
        expected.push((
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        ));
    }
    if status == 405 {
        expected.push(("allow".to_string(), "GET, HEAD".to_string()));
    }
    let body_forbidden = status < 200 || status == 204 || status == 205 || status == 304;
    if !body_forbidden {
        let carries_body = policy == ErrorRepresentationPolicy::Minimal && !is_head;
        let length = if carries_body {
            canonical_body(status).len()
        } else {
            0
        };
        expected.push(("content-length".to_string(), length.to_string()));
    }
    expected
}

/// The public authority path renders the documented representation for the
/// whole matrix.
#[tokio::test]
async fn the_public_authority_renders_the_documented_representation() {
    for status in MATRIX_STATUSES {
        for policy in [
            ErrorRepresentationPolicy::Minimal,
            ErrorRepresentationPolicy::Empty,
        ] {
            for is_head in [false, true] {
                let rendered = authority_representation(*status, is_head, policy).await;
                assert_eq!(
                    rendered.status, *status,
                    "status for {status} {policy:?} head={is_head}"
                );
                let body_forbidden =
                    *status < 200 || *status == 204 || *status == 205 || *status == 304;
                let expected_body =
                    if policy == ErrorRepresentationPolicy::Empty || is_head || body_forbidden {
                        Vec::new()
                    } else {
                        canonical_body(*status).as_bytes().to_vec()
                    };
                assert_eq!(
                    rendered.headers,
                    expected_headers(*status, policy, is_head),
                    "headers for {status} {policy:?} head={is_head}"
                );
                assert_eq!(
                    rendered.body, expected_body,
                    "body for {status} {policy:?} head={is_head}"
                );
            }
        }
    }
}

/// Start a runtime that answers every request with a fixed response.
async fn start_server() -> (std::net::SocketAddr, eggserve_server::ServerControl) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = Server::builder()
        .runtime(RuntimeConfig::builder().bind(addr).build().unwrap())
        .from_listener(listener)
        .build()
        .unwrap();
    let service = service_fn(|_req: eggserve_server::Request| async {
        Ok(eggserve_primitives::Response::builder()
            .status(eggserve_primitives::StatusCode::OK)
            .body(eggserve_primitives::ResponseBody::Bytes(b"ok".to_vec()))
            .unwrap())
    });
    let handle = server.start_with_service(service).await.unwrap();
    let (control, completion) = handle.into_parts();
    tokio::spawn(async move {
        let mut completion = completion;
        let _ = completion.wait().await;
    });
    (addr, control)
}

/// Send a raw request and read the whole response.
async fn raw_request(addr: std::net::SocketAddr, request: &str) -> Vec<u8> {
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut wire = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), socket.read_to_end(&mut wire))
        .await
        .expect("response completes")
        .unwrap();
    wire
}

/// Split a raw response into its status line, header lines, and body bytes.
fn split_wire(wire: &[u8]) -> (String, Vec<(String, String)>, Vec<u8>) {
    let split = wire
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("head terminator");
    let head = String::from_utf8_lossy(&wire[..split]).to_string();
    let body = wire[split + 4..].to_vec();
    let mut lines = head.split("\r\n");
    let status = lines.next().expect("status line").to_string();
    let headers = lines
        .map(|line| {
            let (name, value) = line.split_once(": ").expect("header field");
            (name.to_ascii_lowercase(), value.to_string())
        })
        .collect();
    (status, headers, body)
}

/// A runtime error reaches the client with exactly the documented bytes.
#[tokio::test]
async fn a_runtime_error_reaches_the_client_with_the_documented_bytes() {
    let (addr, control) = start_server().await;

    // A `TRACE` request that carries a body is rejected with 400 by the
    // runtime, through the delegated runtime-error path.
    let wire = raw_request(
        addr,
        "TRACE / HTTP/1.1\r\nHost: t\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    )
    .await;
    let (status_line, headers, body) = split_wire(&wire);
    assert_eq!(status_line, "HTTP/1.1 400 Bad Request");
    let owned: Vec<(String, String)> = headers
        .into_iter()
        // Runtime-owned, time-varying metadata is not part of the error
        // representation.
        .filter(|(name, _)| !matches!(name.as_str(), "date" | "server" | "connection"))
        .collect();
    assert_eq!(
        owned,
        vec![
            (
                "content-type".to_string(),
                "text/plain; charset=utf-8".to_string()
            ),
            ("content-length".to_string(), "16".to_string()),
        ]
    );
    assert_eq!(body, b"400 Bad Request\n");

    // An unknown `Expect` value is rejected with 417, through the same
    // delegated runtime-error path.
    let wire = raw_request(
        addr,
        "GET / HTTP/1.1\r\nHost: t\r\nExpect: not-a-known-expectation\r\nConnection: close\r\n\r\n",
    )
    .await;
    let (status_line, headers, body) = split_wire(&wire);
    assert_eq!(status_line, "HTTP/1.1 417 Expectation Failed");
    let owned: Vec<(String, String)> = headers
        .into_iter()
        .filter(|(name, _)| !matches!(name.as_str(), "date" | "server" | "connection"))
        .collect();
    assert_eq!(
        owned,
        vec![
            (
                "content-type".to_string(),
                "text/plain; charset=utf-8".to_string()
            ),
            ("content-length".to_string(), "23".to_string()),
        ]
    );
    assert_eq!(body, b"417 Expectation Failed\n");

    control.shutdown();
}
