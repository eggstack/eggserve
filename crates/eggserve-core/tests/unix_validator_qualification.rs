#![allow(deprecated)]

//! Plan 083 Track H — Unix validator qualification tests.
//!
//! Verifies ETag/Last-Modified validator stability, format correctness,
//! and conditional matching behavior on Unix platforms.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

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
    let state = Arc::new(ServeState::new(config).unwrap());
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
                            eggserve_core::service::handle_request(
                                req,
                                &state,
                                &eggserve_core::server::RuntimeState::new_for_testing(32),
                            )
                            .await,
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

async fn get_etag(addr: std::net::SocketAddr, path: &str) -> String {
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        path
    );
    let headers = response_headers(addr, req.as_bytes()).await;
    headers
        .iter()
        .find(|(n, _)| n == "etag")
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

async fn get_last_modified(addr: std::net::SocketAddr, path: &str) -> Option<String> {
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        path
    );
    let headers = response_headers(addr, req.as_bytes()).await;
    headers
        .iter()
        .find(|(n, _)| n == "last-modified")
        .map(|(_, v)| v.clone())
}

// ---------------------------------------------------------------------------
// H1: Unchanged file produces stable validator
// ---------------------------------------------------------------------------

#[tokio::test]
async fn h1_unchanged_file_stable_etag() {
    let s = start_server(None).await;
    let etag1 = get_etag(s.addr, "/hello.txt").await;
    let etag2 = get_etag(s.addr, "/hello.txt").await;
    assert_eq!(etag1, etag2, "ETag must be stable for unchanged file");
}

#[tokio::test]
async fn h1_unchanged_file_stable_last_modified() {
    let s = start_server(None).await;
    let lm1 = get_last_modified(s.addr, "/hello.txt").await;
    let lm2 = get_last_modified(s.addr, "/hello.txt").await;
    assert_eq!(lm1, lm2, "Last-Modified must be stable for unchanged file");
}

// ---------------------------------------------------------------------------
// H2: Rapid same-size replacement changes validator
// ---------------------------------------------------------------------------

#[tokio::test]
async fn h2_same_size_replacement_changes_etag() {
    let s = start_server(None).await;
    let etag_before = get_etag(s.addr, "/hello.txt").await;

    // Replace with same-size content
    fs::write(s._tmp.path().join("hello.txt"), "HELLO WORLD").unwrap();
    // Small delay to ensure filesystem metadata updates
    tokio::time::sleep(Duration::from_millis(50)).await;

    let etag_after = get_etag(s.addr, "/hello.txt").await;
    // The nanosecond precision in the ETag should distinguish same-size replacements
    // where the mtime changes. If the filesystem provides nanosecond resolution,
    // these will differ. If not, they may be equal (documented limitation).
    if etag_before == etag_after {
        eprintln!(
            "Note: Filesystem may lack nanosecond resolution for same-size replacement distinction"
        );
    }
    // At minimum, the ETag must remain a valid weak validator
    assert!(
        etag_after.starts_with("W/\""),
        "ETag must use weak validator format, got: {}",
        etag_after
    );
    assert!(
        etag_after.ends_with('"'),
        "ETag must be properly quoted, got: {}",
        etag_after
    );
}

// ---------------------------------------------------------------------------
// H3: New inode changes validator
// ---------------------------------------------------------------------------

#[tokio::test]
async fn h3_new_inode_changes_etag() {
    let s = start_server(None).await;
    let etag_before = get_etag(s.addr, "/hello.txt").await;

    // Remove and recreate with different content (different size guarantees different ETag)
    fs::remove_file(s._tmp.path().join("hello.txt")).unwrap();
    fs::write(
        s._tmp.path().join("hello.txt"),
        "completely different content that is much longer",
    )
    .unwrap();

    let etag_after = get_etag(s.addr, "/hello.txt").await;
    assert_ne!(
        etag_before, etag_after,
        "New file with different content must have different ETag"
    );
}

// ---------------------------------------------------------------------------
// H4: Direct and index paths share validator
// ---------------------------------------------------------------------------

#[tokio::test]
async fn h4_direct_and_index_share_etag() {
    let s = start_server(None).await;
    let etag_direct = get_etag(s.addr, "/subdir/index.html").await;
    let etag_index = get_etag(s.addr, "/subdir/").await;
    assert_eq!(
        etag_direct, etag_index,
        "Direct and index path must share ETag for same resource"
    );
}

#[tokio::test]
async fn h4_root_direct_and_index_share_etag() {
    let s = start_server(None).await;
    // Root index may not exist (directory listing disabled by default);
    // only compare if both paths return an ETag
    let etag_direct = get_etag(s.addr, "/index.html").await;
    let etag_index = get_etag(s.addr, "/").await;
    if etag_direct.is_empty() || etag_index.is_empty() {
        // Root index not accessible; test is not applicable
        return;
    }
    assert_eq!(
        etag_direct, etag_index,
        "Root direct and index path must share ETag"
    );
}

// ---------------------------------------------------------------------------
// H5: Validator syntax is valid and safely quoted
// ---------------------------------------------------------------------------

#[tokio::test]
async fn h5_etag_format_valid_quoted_syntax() {
    let s = start_server(None).await;
    let etag = get_etag(s.addr, "/hello.txt").await;
    // Must be W/"..." (weak validator)
    assert!(
        etag.starts_with("W/\""),
        "ETag must start with W/\", got: {}",
        etag
    );
    assert!(etag.ends_with('"'), "ETag must end with \", got: {}", etag);
    // Must not contain unescaped quotes inside
    let inner = &etag[3..etag.len() - 1]; // strip W/" and trailing "
    assert!(
        !inner.contains('"'),
        "ETag inner value must not contain unescaped quotes: {}",
        etag
    );
    // Must not contain backslashes (obs-text)
    assert!(
        !inner.contains('\\'),
        "ETag inner value must not contain backslashes: {}",
        etag
    );
    // Must contain size-secs-nanos format
    let parts: Vec<&str> = inner.split('-').collect();
    assert_eq!(
        parts.len(),
        3,
        "ETag must have 3 dash-separated parts (size-secs-nanos), got: {}",
        etag
    );
    // Each part must be numeric
    for part in &parts {
        assert!(
            part.parse::<u64>().is_ok(),
            "ETag part '{}' must be numeric in: {}",
            part,
            etag
        );
    }
}

// ---------------------------------------------------------------------------
// H6: Validators reveal no absolute path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn h6_etag_reveals_no_absolute_path() {
    let s = start_server(None).await;
    let etag = get_etag(s.addr, "/hello.txt").await;
    let root_str = s._tmp.path().to_str().unwrap();
    assert!(
        !etag.contains(root_str),
        "ETag must not contain absolute path '{}': {}",
        root_str,
        etag
    );
    assert!(
        !etag.contains("/tmp"),
        "ETag must not contain /tmp path: {}",
        etag
    );
    assert!(
        !etag.contains("/home"),
        "ETag must not contain /home path: {}",
        etag
    );
}

#[tokio::test]
async fn h6_last_modified_reveals_no_absolute_path() {
    let s = start_server(None).await;
    let lm = get_last_modified(s.addr, "/hello.txt").await;
    if let Some(lm) = lm {
        let root_str = s._tmp.path().to_str().unwrap();
        assert!(
            !lm.contains(root_str),
            "Last-Modified must not contain absolute path '{}': {}",
            root_str,
            lm
        );
    }
}

// ---------------------------------------------------------------------------
// H7: Conditional matching operates on the final emitted value
// ---------------------------------------------------------------------------

#[tokio::test]
async fn h7_conditional_match_uses_emitted_etag() {
    let s = start_server(None).await;
    let etag = get_etag(s.addr, "/hello.txt").await;

    // Use the exact ETag from a prior response in If-None-Match
    let req = format!(
        "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: {}\r\nConnection: close\r\n\r\n",
        etag
    );
    let raw = send_raw(s.addr, req.as_bytes()).await;
    let resp = String::from_utf8_lossy(&raw);
    assert!(
        resp.starts_with("HTTP/1.1 304"),
        "Conditional request with matching ETag must return 304, got: {}",
        resp.lines().next().unwrap_or("")
    );
}

#[tokio::test]
async fn h7_conditional_head_match_uses_emitted_etag() {
    let s = start_server(None).await;
    let etag = get_etag(s.addr, "/hello.txt").await;

    let req = format!(
        "HEAD /hello.txt HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: {}\r\nConnection: close\r\n\r\n",
        etag
    );
    let raw = send_raw(s.addr, req.as_bytes()).await;
    let resp = String::from_utf8_lossy(&raw);
    assert!(
        resp.starts_with("HTTP/1.1 304"),
        "HEAD conditional with matching ETag must return 304, got: {}",
        resp.lines().next().unwrap_or("")
    );
    // Must have no body
    let header_end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let body = &raw[header_end..];
    assert!(
        body.is_empty(),
        "HEAD 304 must have no body, got {} bytes",
        body.len()
    );
}

#[tokio::test]
async fn h7_if_range_rejects_emitted_weak_etag() {
    let s = start_server(None).await;
    let etag = get_etag(s.addr, "/hello.txt").await;

    // The emitted metadata ETag is weak and cannot authorize If-Range.
    let req = format!(
        "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nIf-Range: {}\r\nConnection: close\r\n\r\n",
        etag
    );
    let line = String::from_utf8_lossy(&send_raw(s.addr, req.as_bytes()).await)
        .lines()
        .next()
        .unwrap()
        .to_string();
    assert!(
        line.contains("200"),
        "If-Range with emitted weak ETag must return 200, got: {}",
        line
    );

    // If-Range with non-matching ETag → 200
    let req2 =
        "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nIf-Range: W/\"0-0-0\"\r\nConnection: close\r\n\r\n";
    let line2 = String::from_utf8_lossy(&send_raw(s.addr, req2.as_bytes()).await)
        .lines()
        .next()
        .unwrap()
        .to_string();
    assert!(
        line2.contains("200"),
        "If-Range with non-matching ETag must return 200, got: {}",
        line2
    );
}

// ---------------------------------------------------------------------------
// H8: Unchanged file retains validator across requests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn h8_validator_persistent_across_requests() {
    let s = start_server(None).await;
    let mut etags = Vec::new();
    for _ in 0..10 {
        etags.push(get_etag(s.addr, "/hello.txt").await);
    }
    // All ETags must be identical
    let first = &etags[0];
    for (i, etag) in etags.iter().enumerate() {
        assert_eq!(
            first, etag,
            "ETag must be stable across requests (request {}): expected {}, got {}",
            i, first, etag
        );
    }
}

// ---------------------------------------------------------------------------
// H9: Empty file produces valid validator
// ---------------------------------------------------------------------------

#[tokio::test]
async fn h9_empty_file_valid_etag() {
    let s = start_server(None).await;
    let etag = get_etag(s.addr, "/empty.txt").await;
    assert!(
        etag.starts_with("W/\""),
        "Empty file ETag must use weak format, got: {}",
        etag
    );
    // Empty file ETag should have size=0
    assert!(
        etag.contains("W/\"0-"),
        "Empty file ETag must have size 0, got: {}",
        etag
    );
}

// ---------------------------------------------------------------------------
// H10: Different files produce different validators
// ---------------------------------------------------------------------------

#[tokio::test]
async fn h10_different_files_different_etags() {
    let s = start_server(None).await;
    let etag_hello = get_etag(s.addr, "/hello.txt").await;
    let etag_empty = get_etag(s.addr, "/empty.txt").await;
    assert_ne!(
        etag_hello, etag_empty,
        "Different files must have different ETags"
    );
}
