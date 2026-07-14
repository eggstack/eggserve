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

fn parse_status_code(raw: &[u8]) -> u16 {
    let resp = String::from_utf8_lossy(raw);
    let first_line = resp.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() >= 2 {
        parts[1].parse().unwrap_or(0)
    } else {
        0
    }
}

fn parse_headers(raw: &[u8]) -> Vec<(String, String)> {
    let resp = String::from_utf8_lossy(raw);
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

fn get_header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.clone())
}

fn get_all_headers(headers: &[(String, String)], name: &str) -> Vec<String> {
    headers
        .iter()
        .filter(|(n, _)| n == name)
        .map(|(_, v)| v.clone())
        .collect()
}

fn body_from_raw(raw: &[u8]) -> Vec<u8> {
    if let Some(idx) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
        raw[idx + 4..].to_vec()
    } else {
        Vec::new()
    }
}

async fn get_etag(addr: std::net::SocketAddr) -> String {
    let data = b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let raw = send_raw(addr, data).await;
    let headers = parse_headers(&raw);
    get_header(&headers, "etag").unwrap()
}

#[tokio::test]
async fn test_normalize_response_wire_output() {
    let s = start_server(None).await;
    let raw = send_raw(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;

    let status = parse_status_code(&raw);
    assert_eq!(status, 200);

    let headers = parse_headers(&raw);
    let cl = get_header(&headers, "content-length");
    assert!(cl.is_some(), "Missing content-length");

    let cl_val: usize = cl.unwrap().parse().unwrap();
    let body = body_from_raw(&raw);
    assert_eq!(cl_val, body.len(), "Content-Length must match body length");

    let te = get_header(&headers, "transfer-encoding");
    assert!(te.is_none(), "Response must not contain Transfer-Encoding");
}

#[tokio::test]
async fn test_duplicate_response_headers_wire() {
    let s = start_server(None).await;
    let raw = send_raw(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;

    let headers = parse_headers(&raw);
    let accept_ranges = get_all_headers(&headers, "accept-ranges");
    assert!(
        !accept_ranges.is_empty(),
        "Response must contain accept-ranges header"
    );

    let content_type = get_all_headers(&headers, "content-type");
    assert_eq!(content_type.len(), 1, "Content-Type must appear once");

    let etag = get_all_headers(&headers, "etag");
    assert_eq!(etag.len(), 1, "ETag must appear once");

    let last_modified = get_all_headers(&headers, "last-modified");
    assert_eq!(last_modified.len(), 1, "Last-Modified must appear once");
}

#[tokio::test]
async fn test_head_response_wire_parity() {
    let s = start_server(None).await;

    let get_raw = send_raw(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    let head_raw = send_raw(
        s.addr,
        b"HEAD /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;

    let get_status = parse_status_code(&get_raw);
    let head_status = parse_status_code(&head_raw);
    assert_eq!(
        get_status, head_status,
        "GET and HEAD must return same status"
    );

    let get_headers = parse_headers(&get_raw);
    let head_headers = parse_headers(&head_raw);

    let get_cl = get_header(&get_headers, "content-length");
    let head_cl = get_header(&head_headers, "content-length");
    assert_eq!(
        get_cl, head_cl,
        "GET and HEAD must have same content-length"
    );

    let get_ct = get_header(&get_headers, "content-type");
    let head_ct = get_header(&head_headers, "content-type");
    assert_eq!(get_ct, head_ct, "GET and HEAD must have same content-type");

    let get_etag = get_header(&get_headers, "etag");
    let head_etag = get_header(&head_headers, "etag");
    assert_eq!(get_etag, head_etag, "GET and HEAD must have same ETag");

    let head_body = body_from_raw(&head_raw);
    assert!(head_body.is_empty(), "HEAD response must not contain body");

    let get_body = body_from_raw(&get_raw);
    assert!(!get_body.is_empty(), "GET response must contain body");
}

#[tokio::test]
async fn test_conditional_304_wire() {
    let s = start_server(None).await;
    let etag = get_etag(s.addr).await;

    let req = format!(
        "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: {}\r\nConnection: close\r\n\r\n",
        etag
    );
    let raw = send_raw(s.addr, req.as_bytes()).await;

    let status = parse_status_code(&raw);
    assert_eq!(status, 304, "Matching ETag must return 304");

    let headers = parse_headers(&raw);
    let resp_etag = get_header(&headers, "etag");
    assert_eq!(resp_etag, Some(etag), "304 must preserve the ETag header");

    let body = body_from_raw(&raw);
    assert!(body.is_empty(), "304 response must not contain body");

    let cl = get_header(&headers, "content-length");
    assert!(cl.is_none(), "304 response must not contain content-length");
}

#[tokio::test]
async fn test_range_206_wire() {
    let s = start_server(None).await;
    let raw = send_raw(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nConnection: close\r\n\r\n",
    )
    .await;

    let status = parse_status_code(&raw);
    assert_eq!(status, 206, "Valid range request must return 206");

    let headers = parse_headers(&raw);
    let cr = get_header(&headers, "content-range");
    assert!(cr.is_some(), "206 must include Content-Range header");
    assert_eq!(
        cr.unwrap(),
        "bytes 0-4/11",
        "Content-Range must match requested range"
    );

    let cl = get_header(&headers, "content-length");
    assert!(cl.is_some(), "206 must include content-length");
    assert_eq!(
        cl.unwrap(),
        "5",
        "content-length must match range body length"
    );

    let body = body_from_raw(&raw);
    assert_eq!(body, b"hello", "Range body must be the first 5 bytes");
}

#[tokio::test]
async fn test_http10_keepalive_wire() {
    let s = start_server(None).await;
    let mut stream = tokio::net::TcpStream::connect(s.addr).await.unwrap();

    stream
        .write_all(b"GET /hello.txt HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();

    let mut first_response = Vec::new();
    stream.read_to_end(&mut first_response).await.unwrap();

    let first_status = parse_status_code(&first_response);
    assert_eq!(first_status, 200, "HTTP/1.0 request must return 200");

    let second_result = tokio::net::TcpStream::connect(s.addr).await;
    assert!(
        second_result.is_ok(),
        "Server must accept new connections after HTTP/1.0"
    );
}

#[tokio::test]
async fn test_malformed_request_rejection_wire() {
    let s = start_server(None).await;

    let garbage = send_raw(s.addr, b"GARBAGE DATA\r\n\r\n").await;
    let garbage_status = parse_status_code(&garbage);
    assert_eq!(garbage_status, 400, "Garbage input must return 400");

    let bad_method = send_raw(
        s.addr,
        b"DELETE /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    let bad_method_status = parse_status_code(&bad_method);
    assert_eq!(bad_method_status, 405, "Unsupported method must return 405");

    let bad_headers = send_raw(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1 2\r\nConnection: close\r\n\r\n",
    )
    .await;
    let bad_headers_status = parse_status_code(&bad_headers);
    assert_eq!(
        bad_headers_status, 400,
        "Malformed Content-Length must return 400"
    );

    let valid = send_raw(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    let valid_status = parse_status_code(&valid);
    assert_eq!(
        valid_status, 200,
        "Valid request after malformed must still succeed"
    );
}
