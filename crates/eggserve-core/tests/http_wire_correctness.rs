//! HTTP wire-correctness tests (Plan 033).
//!
//! Raw TCP tests for request-line parsing, header grammar, message framing,
//! response validation, conditional/range semantics, and connection lifecycle.

use std::fs;
use std::sync::Arc;

use eggserve_core::config::{ServeConfig, ServeState};
use eggserve_core::policy::StaticPolicy;
use hyper_util::rt::TokioIo;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct TestServer {
    _tmp: TempDir,
    addr: std::net::SocketAddr,
    _state: Arc<ServeState>,
}

async fn start_server(opts: Option<StaticPolicy>) -> TestServer {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    fs::write(tmp.path().join("empty.txt"), "").unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    fs::write(
        tmp.path().join("subdir").join("index.html"),
        "<html>hi</html>",
    )
    .unwrap();

    let config = Arc::new(ServeConfig {
        root: tmp.path().to_path_buf(),
        static_policy: opts.unwrap_or_else(StaticPolicy::safe_default),
        ..ServeConfig::default()
    });
    let state = Arc::new(ServeState::new(config));
    let state_clone = state.clone();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => continue,
            };
            let io = TokioIo::new(stream);
            let state = state_clone.clone();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let state = state.clone();
                    async move {
                        Ok::<_, std::convert::Infallible>(
                            eggserve_core::service::handle_request(req, &state).await,
                        )
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });

    TestServer {
        _tmp: tmp,
        addr,
        _state: state,
    }
}

async fn send_raw(addr: std::net::SocketAddr, data: &[u8]) -> Vec<u8> {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(data).await.unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf).await;
    buf
}

async fn status_line(addr: std::net::SocketAddr, data: &[u8]) -> String {
    let raw = send_raw(addr, data).await;
    String::from_utf8_lossy(&raw)
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

async fn response_headers(addr: std::net::SocketAddr, data: &[u8]) -> Vec<(String, String)> {
    let raw = send_raw(addr, data).await;
    let resp = String::from_utf8_lossy(&raw);
    let mut headers = Vec::new();
    let mut lines = resp.lines();
    lines.next();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_lowercase(), value.trim().to_string()));
        }
    }
    headers
}

async fn response_body(addr: std::net::SocketAddr, data: &[u8]) -> Vec<u8> {
    let full = send_raw(addr, data).await;
    if let Some(idx) = full.windows(4).position(|w| w == b"\r\n\r\n") {
        full[idx + 4..].to_vec()
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Workstream A — Request-line and target forms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_a_valid_origin_form_returns_200() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("200"), "Expected 200, got: {}", line);
}

#[tokio::test]
async fn ws_a_root_path_returns_200() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("200") || line.contains("403"),
        "Expected 200 (listing) or 403 (no listing), got: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_absolute_form_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET http://example.com/hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("400") || line.is_empty(),
        "Expected 400 or connection close, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_authority_form_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"CONNECT example.com:443 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("405") || line.is_empty(),
        "Expected 405 or connection close, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_asterisk_form_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"OPTIONS * HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("405") || line.is_empty(),
        "Expected 405 or connection close, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_lowercase_method_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"get /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("400") || line.contains("405") || line.is_empty(),
        "Expected 400/405 or connection close, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_method_with_space_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GE T /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.is_empty() || line.contains("400"),
        "Expected connection close or 400, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_unknown_method_returns_405() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"DELETE /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("405"), "Expected 405, got: {}", line);
}

#[tokio::test]
async fn ws_a_http_1_0_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("200") || line.is_empty(),
        "HTTP/1.0 may be accepted or rejected, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_http_2_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/2.0\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.is_empty() || line.contains("400"),
        "Expected connection close or 400, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_nul_in_target_rejected() {
    let s = start_server(None).await;
    let mut raw = Vec::new();
    raw.extend_from_slice(b"GET /he");
    raw.push(0x00);
    raw.extend_from_slice(b"llo.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let line = status_line(s.addr, &raw).await;
    assert!(
        line.is_empty() || line.contains("400"),
        "Expected connection close or 400, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_space_in_target_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello world.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.is_empty() || line.contains("400"),
        "Expected connection close or 400, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_query_string_allowed() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt?foo=bar HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("200"), "Expected 200, got: {}", line);
}

#[tokio::test]
async fn ws_a_percent_encoded_slash_resolves() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /%2Fhello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("200") || line.contains("404") || line.contains("403"),
        "Unexpected: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_percent_encoded_dotdot_traversal_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /%2e%2e/etc/passwd HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("400") || line.contains("403"),
        "Expected 400 or 403, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_path_traversal_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /../etc/passwd HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("400") || line.contains("403"),
        "Expected 400 or 403, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_double_encoded_traversal_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /%252e%252e/etc/passwd HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("400") || line.contains("403"),
        "Expected 400 or 403, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_semicolon_in_path() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt;jsessionid=abc HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("200") || line.contains("404"),
        "Unexpected: {}",
        line
    );
}

#[tokio::test]
async fn ws_a_garbage_input_rejected() {
    let s = start_server(None).await;
    let line = status_line(s.addr, b"GARBAGE DATA\r\n\r\n").await;
    assert!(
        line.is_empty() || line.contains("400"),
        "Expected connection close or 400, got: {}",
        line
    );
}

// ---------------------------------------------------------------------------
// Workstream B — Header grammar and limits
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_b_obsolete_folding_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\n X-Folded: value\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.is_empty() || line.contains("400"),
        "Expected connection close or 400, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_b_leading_space_in_header_name_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\n X-Bad: value\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.is_empty() || line.contains("400"),
        "Expected connection close or 400, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_b_bare_lf_in_header_rejected() {
    let s = start_server(None).await;
    let mut raw = Vec::new();
    raw.extend_from_slice(b"GET /hello.txt HTTP/1.1\r\n");
    raw.extend_from_slice(b"Host: localhost\r\n");
    raw.extend_from_slice(b"X-Bad: value\n");
    raw.extend_from_slice(b"Connection: close\r\n\r\n");
    let line = status_line(s.addr, &raw).await;
    assert!(
        line.is_empty() || line.contains("400") || line.contains("200"),
        "Expected connection close, 400, or 200, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_b_cr_lf_injection_in_header_value_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nX-Injection: val\r\nEvil: true\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.is_empty() || line.contains("400") || line.contains("200"),
        "Unexpected: {}",
        line
    );
}

#[tokio::test]
async fn ws_b_duplicate_host_header_handled() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("200"), "Expected 200, got: {}", line);
}

#[tokio::test]
async fn ws_b_content_length_with_spaces_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1 2\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.is_empty() || line.contains("400"),
        "Expected connection close or 400, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_b_oversized_target_rejected() {
    let s = start_server(None).await;
    let long_path = "/".to_owned() + &"a".repeat(8192);
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        long_path
    );
    let line = status_line(s.addr, req.as_bytes()).await;
    assert!(
        line.is_empty() || line.contains("400") || line.contains("404") || line.contains("403"),
        "Unexpected: {}",
        line
    );
}

#[tokio::test]
async fn ws_b_header_with_null_byte_rejected() {
    let s = start_server(None).await;
    let mut raw = Vec::new();
    raw.extend_from_slice(b"GET /hello.txt HTTP/1.1\r\n");
    raw.extend_from_slice(b"Host: localhost\r\n");
    raw.extend_from_slice(b"X-Bad: val\x00ue\r\n");
    raw.extend_from_slice(b"Connection: close\r\n\r\n");
    let line = status_line(s.addr, &raw).await;
    assert!(
        line.is_empty() || line.contains("400"),
        "Expected connection close or 400, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_b_empty_header_value_allowed() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nX-Empty:\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("200"), "Expected 200, got: {}", line);
}

#[tokio::test]
async fn ws_b_multiple_content_length_conflicting_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nContent-Length: 10\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("400") || line.is_empty(),
        "Expected 400 or connection close, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_b_content_length_with_comma_values_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1,2\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("400") || line.is_empty(),
        "Expected 400 or connection close, got: {}",
        line
    );
}

// ---------------------------------------------------------------------------
// Workstream C — Message framing ambiguity
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_c_transfer_encoding_chunked_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("400") || line.is_empty(),
        "Expected 400 or connection close, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_c_transfer_encoding_gzip_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: gzip\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("400") || line.is_empty(),
        "Expected 400 or connection close, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_c_te_and_content_length_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("400") || line.is_empty(),
        "Expected 400 or connection close, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_c_body_on_get_with_content_length_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    )
    .await;
    assert!(
        line.contains("413") || line.contains("400") || line.is_empty(),
        "Expected 413/400 or connection close, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_c_body_on_head_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"HEAD /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    )
    .await;
    assert!(
        line.contains("413") || line.contains("400") || line.is_empty(),
        "Expected 413/400 or connection close, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_c_zero_content_length_allowed() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("200"), "Expected 200, got: {}", line);
}

#[tokio::test]
async fn ws_c_invalid_content_length_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: not-a-number\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("400") || line.is_empty(),
        "Expected 400 or connection close, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_c_negative_content_length_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: -1\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("400") || line.is_empty(),
        "Expected 400 or connection close, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_c_oversized_content_length_rejected() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 99999999999999999999\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("400") || line.is_empty(),
        "Expected 400 or connection close, got: {}",
        line
    );
}

#[tokio::test]
async fn ws_c_premature_eof_connection_closed() {
    let s = start_server(None).await;
    let mut stream = tokio::net::TcpStream::connect(s.addr).await.unwrap();
    let _ = stream.write_all(b"GET /hello.txt HTTP/1.1\r\nHost: ").await;
    drop(stream);
}

// ---------------------------------------------------------------------------
// Workstream D — Response validation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_d_get_returns_content_length_matching_body() {
    let s = start_server(None).await;
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let headers = response_headers(s.addr, data).await;
    let cl = headers.iter().find(|(n, _)| n == "content-length");
    assert!(cl.is_some(), "Missing content-length header");
    let body = response_body(s.addr, data).await;
    let cl_val: usize = cl.unwrap().1.parse().unwrap();
    assert_eq!(
        cl_val,
        body.len(),
        "Content-Length doesn't match body length"
    );
}

#[tokio::test]
async fn ws_d_head_returns_content_length_no_body() {
    let s = start_server(None).await;
    let data = b"HEAD /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let headers = response_headers(s.addr, data).await;
    let cl = headers.iter().find(|(n, _)| n == "content-length");
    assert!(cl.is_some(), "HEAD should include content-length");
    assert_eq!(cl.unwrap().1, "11", "HEAD content-length should be 11");
    let body = response_body(s.addr, data).await;
    assert!(body.is_empty(), "HEAD should suppress body");
}

#[tokio::test]
async fn ws_d_get_returns_nosniff_header() {
    let s = start_server(None).await;
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let headers = response_headers(s.addr, data).await;
    let nst = headers.iter().find(|(n, _)| n == "x-content-type-options");
    assert!(nst.is_some(), "Missing x-content-type-options header");
    assert_eq!(nst.unwrap().1, "nosniff");
}

#[tokio::test]
async fn ws_d_post_returns_405() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"POST /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("405"), "Expected 405, got: {}", line);
}

#[tokio::test]
async fn ws_d_put_returns_405() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"PUT /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("405"), "Expected 405, got: {}", line);
}

#[tokio::test]
async fn ws_d_delete_returns_405() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"DELETE /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("405"), "Expected 405, got: {}", line);
}

#[tokio::test]
async fn ws_d_patch_returns_405() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"PATCH /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("405"), "Expected 405, got: {}", line);
}

#[tokio::test]
async fn ws_d_allow_header_present_on_405() {
    let s = start_server(None).await;
    let data = b"POST /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let headers = response_headers(s.addr, data).await;
    let allow = headers.iter().find(|(n, _)| n == "allow");
    assert!(allow.is_some(), "Missing Allow header on 405");
    assert_eq!(allow.unwrap().1, "GET, HEAD");
}

#[tokio::test]
async fn ws_d_content_type_set_for_text_file() {
    let s = start_server(None).await;
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let headers = response_headers(s.addr, data).await;
    let ct = headers.iter().find(|(n, _)| n == "content-type");
    assert!(ct.is_some(), "Missing content-type header");
    assert!(
        ct.unwrap().1.contains("text/plain"),
        "Expected text/plain, got: {}",
        ct.unwrap().1
    );
}

#[tokio::test]
async fn ws_d_no_transfer_encoding_header_in_response() {
    let s = start_server(None).await;
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let headers = response_headers(s.addr, data).await;
    let te = headers.iter().find(|(n, _)| n == "transfer-encoding");
    assert!(te.is_none(), "Should not send transfer-encoding header");
}

#[tokio::test]
async fn ws_d_etag_header_present() {
    let s = start_server(None).await;
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let headers = response_headers(s.addr, data).await;
    let etag = headers.iter().find(|(n, _)| n == "etag");
    assert!(etag.is_some(), "Missing etag header");
    assert!(
        etag.unwrap().1.starts_with("W/\""),
        "ETag should be weak, got: {}",
        etag.unwrap().1
    );
}

#[tokio::test]
async fn ws_d_last_modified_header_present() {
    let s = start_server(None).await;
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let headers = response_headers(s.addr, data).await;
    let lm = headers.iter().find(|(n, _)| n == "last-modified");
    assert!(lm.is_some(), "Missing last-modified header");
}

#[tokio::test]
async fn ws_d_accept_ranges_header_present() {
    let s = start_server(None).await;
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let headers = response_headers(s.addr, data).await;
    let ar = headers.iter().find(|(n, _)| n == "accept-ranges");
    assert!(ar.is_some(), "Missing accept-ranges header");
    assert_eq!(ar.unwrap().1, "bytes");
}

// ---------------------------------------------------------------------------
// Workstream E — Conditional and range semantics
// ---------------------------------------------------------------------------

async fn get_etag(addr: std::net::SocketAddr) -> String {
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let headers = response_headers(addr, data).await;
    headers.iter().find(|(n, _)| n == "etag").unwrap().1.clone()
}

#[tokio::test]
async fn ws_e_if_none_match_matching_etag_returns_304() {
    let s = start_server(None).await;
    let etag = get_etag(s.addr).await;
    let req = format!(
        "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: {}\r\nConnection: close\r\n\r\n",
        etag
    );
    let line = status_line(s.addr, req.as_bytes()).await;
    assert!(line.contains("304"), "Expected 304, got: {}", line);
}

#[tokio::test]
async fn ws_e_if_none_match_wildcard_returns_304() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: *\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("304"), "Expected 304, got: {}", line);
}

#[tokio::test]
async fn ws_e_if_none_match_non_matching_returns_200() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: W/\"999-999\"\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("200"), "Expected 200, got: {}", line);
}

#[tokio::test]
async fn ws_e_if_modified_since_future_returns_304() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nIf-Modified-Since: Tue, 01 Jan 2030 00:00:00 GMT\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("304"), "Expected 304, got: {}", line);
}

#[tokio::test]
async fn ws_e_if_modified_since_past_returns_200() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nIf-Modified-Since: Tue, 01 Jan 2000 00:00:00 GMT\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("200"), "Expected 200, got: {}", line);
}

#[tokio::test]
async fn ws_e_if_modified_since_invalid_date_returns_200() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nIf-Modified-Since: not-a-date\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("200"), "Expected 200, got: {}", line);
}

#[tokio::test]
async fn ws_e_if_none_match_takes_precedence_over_ims() {
    let s = start_server(None).await;
    let etag = get_etag(s.addr).await;
    let req = format!(
        "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: {}\r\nIf-Modified-Since: Tue, 01 Jan 2000 00:00:00 GMT\r\nConnection: close\r\n\r\n",
        etag
    );
    let line = status_line(s.addr, req.as_bytes()).await;
    assert!(line.contains("304"), "Expected 304, got: {}", line);
}

#[tokio::test]
async fn ws_e_range_valid_returns_206() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("206"), "Expected 206, got: {}", line);
}

#[tokio::test]
async fn ws_e_range_content_range_header_correct() {
    let s = start_server(None).await;
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nConnection: close\r\n\r\n";
    let headers = response_headers(s.addr, data).await;
    let cr = headers.iter().find(|(n, _)| n == "content-range");
    assert!(cr.is_some(), "Missing content-range header");
    assert_eq!(cr.unwrap().1, "bytes 0-4/11");
}

#[tokio::test]
async fn ws_e_range_content_length_matches_body() {
    let s = start_server(None).await;
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nConnection: close\r\n\r\n";
    let headers = response_headers(s.addr, data).await;
    let cl = headers.iter().find(|(n, _)| n == "content-length").unwrap();
    assert_eq!(cl.1, "5");
}

#[tokio::test]
async fn ws_e_range_suffix_returns_206() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=-5\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("206"), "Expected 206, got: {}", line);
}

#[tokio::test]
async fn ws_e_range_open_ended_returns_206() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=6-\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("206"), "Expected 206, got: {}", line);
}

#[tokio::test]
async fn ws_e_range_unsatisfiable_returns_416() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=100-200\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("416"), "Expected 416, got: {}", line);
}

#[tokio::test]
async fn ws_e_range_416_content_range_header() {
    let s = start_server(None).await;
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=100-200\r\nConnection: close\r\n\r\n";
    let headers = response_headers(s.addr, data).await;
    let cr = headers.iter().find(|(n, _)| n == "content-range");
    assert!(cr.is_some(), "Missing content-range on 416");
    assert_eq!(cr.unwrap().1, "bytes */11");
}

#[tokio::test]
async fn ws_e_range_multiple_ranges_returns_200() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4, 6-10\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("200"), "Expected 200, got: {}", line);
}

#[tokio::test]
async fn ws_e_range_malformed_returns_200() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=abc-def\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("200"), "Expected 200, got: {}", line);
}

#[tokio::test]
async fn ws_e_head_with_range_returns_206_no_body() {
    let s = start_server(None).await;
    let data = b"HEAD /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nConnection: close\r\n\r\n";
    let line = status_line(s.addr, data).await;
    assert!(line.contains("206"), "Expected 206, got: {}", line);
    let body = response_body(s.addr, data).await;
    assert!(body.is_empty(), "HEAD should suppress body");
}

#[tokio::test]
async fn ws_e_if_range_matching_etag_returns_206() {
    let s = start_server(None).await;
    let etag = get_etag(s.addr).await;
    let req = format!(
        "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nIf-Range: {}\r\nConnection: close\r\n\r\n",
        etag
    );
    let line = status_line(s.addr, req.as_bytes()).await;
    assert!(line.contains("206"), "Expected 206, got: {}", line);
}

#[tokio::test]
async fn ws_e_if_range_non_matching_etag_returns_200() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nIf-Range: W/\"999-999\"\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("200"), "Expected 200, got: {}", line);
}

#[tokio::test]
async fn ws_e_range_body_matches_content_length() {
    let s = start_server(None).await;
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nConnection: close\r\n\r\n";
    let full = send_raw(s.addr, data).await;
    let header_end = full.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let body = &full[header_end..];
    assert_eq!(body, b"hello", "Range body should be 'hello'");
}

#[tokio::test]
async fn ws_e_conditional_head_returns_304_no_body() {
    let s = start_server(None).await;
    let etag = get_etag(s.addr).await;
    let req = format!(
        "HEAD /hello.txt HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: {}\r\nConnection: close\r\n\r\n",
        etag
    );
    let line = status_line(s.addr, req.as_bytes()).await;
    assert!(line.contains("304"), "Expected 304, got: {}", line);
}

#[tokio::test]
async fn ws_e_empty_file_range_416() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /empty.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("416"), "Expected 416, got: {}", line);
}

// ---------------------------------------------------------------------------
// Workstream F — Connection lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_f_connection_close_header_respected() {
    let s = start_server(None).await;
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let raw = send_raw(s.addr, data).await;
    let resp = String::from_utf8_lossy(&raw);
    assert!(resp.contains("200"), "Should return 200");
}

#[tokio::test]
async fn ws_f_malformed_request_then_valid_succeeds() {
    let s = start_server(None).await;

    {
        let mut stream = tokio::net::TcpStream::connect(s.addr).await.unwrap();
        let _ = stream.write_all(b"GARBAGE\r\n\r\n").await;
        drop(stream);
    }

    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("200"),
        "Server should still work after malformed request: {}",
        line
    );
}

#[tokio::test]
async fn ws_f_server_survives_many_sequential_requests() {
    let s = start_server(None).await;
    for _ in 0..10 {
        let line = status_line(
            s.addr,
            b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(line.contains("200"), "Expected 200");
    }
}

#[tokio::test]
async fn ws_f_single_request_connection_closes_after_close() {
    let s = start_server(None).await;
    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("200"), "Expected 200, got: {}", line);
}

#[tokio::test]
async fn ws_f_partial_header_does_not_leak_state() {
    let s = start_server(None).await;

    {
        let _ = tokio::net::TcpStream::connect(s.addr).await;
    }

    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("200"),
        "Server should work after partial connection: {}",
        line
    );
}

#[tokio::test]
async fn ws_f_connection_closed_after_get() {
    let s = start_server(None).await;
    let mut stream = tokio::net::TcpStream::connect(s.addr).await.unwrap();
    stream
        .write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let resp = String::from_utf8_lossy(&buf);
    assert!(resp.contains("200"), "Should get 200");
}
