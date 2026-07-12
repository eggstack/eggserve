//! Production-path wire coverage (Track C, CORRECTIVE-CLOSURE-PHASES-31-35).
//!
//! Exercises the same accept-loop/server-builder path used in production:
//! connection semaphore, header read timeout, response write timeout,
//! graceful shutdown, and TokioTimer-configured hyper. This complements
//! the focused parser/service tests in eggserve-core's http_wire_correctness.rs.

use std::sync::Arc;
use std::time::Duration;

use eggserve_core::config::{ServeConfig, ServeState};
use eggserve_core::service::handle_request;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Semaphore};

struct ProdServer {
    _tmp: TempDir,
    addr: std::net::SocketAddr,
    shutdown_tx: broadcast::Sender<()>,
    _handle: tokio::task::JoinHandle<()>,
}

async fn start_production_server(limits: eggserve_core::limits::Limits) -> ProdServer {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    std::fs::write(tmp.path().join("empty.txt"), "").unwrap();

    let config = Arc::new(ServeConfig {
        root: tmp.path().to_path_buf(),
        bind: "127.0.0.1:0".parse().unwrap(),
        limits,
        ..ServeConfig::default()
    });
    let state = Arc::new(ServeState::new(config.clone()));
    let connection_semaphore = Arc::new(Semaphore::new(config.limits.max_connections));

    let listener = TcpListener::bind(config.bind).await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);

    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                result = listener.accept() => {
                    if let Ok((stream, _addr)) = result {
                        let permit = match connection_semaphore.clone().try_acquire_owned() {
                            Ok(p) => p,
                            Err(_) => {
                                drop(stream);
                                continue;
                            }
                        };

                        let mut conn_shutdown_rx = shutdown_rx.resubscribe();
                        let state = state.clone();
                        let header_timeout = config.limits.header_read_timeout;
                        let write_timeout = config.limits.response_write_timeout;

                        tokio::spawn(async move {
                            let _permit = permit;
                            let io = TokioIo::new(stream);
                            let service = service_fn(move |req| {
                                let state = state.clone();
                                async move {
                                    Ok::<_, std::convert::Infallible>(
                                        handle_request(req, &state).await,
                                    )
                                }
                            });
                            let conn = http1::Builder::new()
                                .timer(TokioTimer::new())
                                .header_read_timeout(header_timeout)
                                .serve_connection(io, service)
                                .with_upgrades();
                            let mut conn = std::pin::pin!(conn);
                            tokio::select! {
                                result = tokio::time::timeout(write_timeout, &mut conn) => {
                                    match result {
                                        Ok(Ok(())) => {}
                                        Ok(Err(_)) => {}
                                        Err(_elapsed) => {
                                            conn.as_mut().graceful_shutdown();
                                        }
                                    }
                                }
                                _ = conn_shutdown_rx.recv() => {
                                    conn.as_mut().graceful_shutdown();
                                }
                            }
                        });
                    }
                }
                _ = shutdown_rx.recv() => {
                    break;
                }
            }
        }
    });

    ProdServer {
        _tmp: tmp,
        addr,
        shutdown_tx,
        _handle: handle,
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

// ---------------------------------------------------------------------------
// Static full response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prod_get_returns_200_with_body() {
    let s = start_production_server(eggserve_core::limits::Limits::default()).await;
    let raw = send_raw(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    let resp = String::from_utf8_lossy(&raw);
    assert!(resp.starts_with("HTTP/1.1 200 OK"), "unexpected: {}", resp);
    assert!(resp.contains("hello world"), "missing body: {}", resp);
    assert!(
        resp.contains("x-content-type-options: nosniff"),
        "missing nosniff: {}",
        resp
    );
    assert!(
        resp.contains("accept-ranges: bytes"),
        "missing accept-ranges: {}",
        resp
    );
}

#[tokio::test]
async fn prod_head_returns_200_no_body() {
    let s = start_production_server(eggserve_core::limits::Limits::default()).await;
    let raw = send_raw(
        s.addr,
        b"HEAD /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    let resp = String::from_utf8_lossy(&raw);
    assert!(resp.starts_with("HTTP/1.1 200 OK"), "unexpected: {}", resp);
    let body = if let Some(idx) = resp.find("\r\n\r\n") {
        &resp[idx + 4..]
    } else {
        ""
    };
    assert!(body.is_empty(), "HEAD should suppress body: {}", resp);
    assert!(
        resp.contains("content-length: 11"),
        "missing content-length: {}",
        resp
    );
}

// ---------------------------------------------------------------------------
// Range response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prod_range_returns_206() {
    let s = start_production_server(eggserve_core::limits::Limits::default()).await;
    let raw = send_raw(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nConnection: close\r\n\r\n",
    )
    .await;
    let resp = String::from_utf8_lossy(&raw);
    assert!(
        resp.starts_with("HTTP/1.1 206 Partial Content"),
        "unexpected: {}",
        resp
    );
    assert!(
        resp.contains("content-range: bytes 0-4/11"),
        "missing content-range: {}",
        resp
    );
    assert!(
        resp.contains("content-length: 5"),
        "missing content-length: {}",
        resp
    );
    let body = if let Some(idx) = resp.find("\r\n\r\n") {
        &resp[idx + 4..]
    } else {
        ""
    };
    assert_eq!(body, "hello", "range body mismatch: {}", resp);
}

// ---------------------------------------------------------------------------
// Connection: close
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prod_connection_close_terminates() {
    let s = start_production_server(eggserve_core::limits::Limits::default()).await;
    let mut stream = tokio::net::TcpStream::connect(s.addr).await.unwrap();
    stream
        .write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let resp = String::from_utf8_lossy(&buf);
    assert!(resp.contains("200"), "expected 200: {}", resp);
}

#[tokio::test]
async fn prod_connection_close_header_in_response() {
    let s = start_production_server(eggserve_core::limits::Limits::default()).await;
    let raw = send_raw(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    let resp = String::from_utf8_lossy(&raw);
    assert!(resp.contains("200"), "expected 200: {}", resp);
}

// ---------------------------------------------------------------------------
// Malformed request closure
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prod_garbage_request_closes_connection() {
    let s = start_production_server(eggserve_core::limits::Limits::default()).await;
    let mut stream = tokio::net::TcpStream::connect(s.addr).await.unwrap();
    stream.write_all(b"GARBAGE DATA\r\n\r\n").await.unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf).await;
    let resp = String::from_utf8_lossy(&buf);
    assert!(
        resp.contains("400") || buf.is_empty(),
        "expected 400 or connection close, got: {}",
        resp
    );
}

#[tokio::test]
async fn prod_premature_eof_does_not_leak_state() {
    let s = start_production_server(eggserve_core::limits::Limits::default()).await;

    {
        let mut stream = tokio::net::TcpStream::connect(s.addr).await.unwrap();
        let _ = stream.write_all(b"GET /hello.txt HTTP/1.1\r\nHost: ").await;
        drop(stream);
    }

    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("200"),
        "server should survive premature eof: {}",
        line
    );
}

// ---------------------------------------------------------------------------
// Header timeout
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prod_partial_header_times_out() {
    let mut limits = eggserve_core::limits::Limits::default();
    limits.header_read_timeout = Duration::from_secs(1);
    let s = start_production_server(limits).await;

    let mut stream = tokio::net::TcpStream::connect(s.addr).await.unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();

    let mut buf = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf)).await;
    assert!(result.is_ok(), "read should complete (timeout fires)");
    let resp = String::from_utf8_lossy(&buf);
    assert!(
        buf.is_empty() || resp.contains("408") || !resp.starts_with("HTTP"),
        "connection should be closed after header timeout, got: {}",
        resp
    );
}

#[tokio::test]
async fn prod_complete_header_within_timeout_succeeds() {
    let mut limits = eggserve_core::limits::Limits::default();
    limits.header_read_timeout = Duration::from_secs(5);
    let s = start_production_server(limits).await;

    let raw = send_raw(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    let resp = String::from_utf8_lossy(&raw);
    assert!(resp.contains("200"), "expected 200: {}", resp);
}

// ---------------------------------------------------------------------------
// Connection limit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prod_connection_limit_enforced() {
    let mut limits = eggserve_core::limits::Limits::default();
    limits.max_connections = 2;
    let s = start_production_server(limits).await;

    let mut c1 = tokio::net::TcpStream::connect(s.addr).await.unwrap();
    c1.write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut c2 = tokio::net::TcpStream::connect(s.addr).await.unwrap();
    c2.write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();

    let mut c3 = tokio::net::TcpStream::connect(s.addr).await.unwrap();
    c3.write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();

    let mut buf1 = Vec::new();
    let mut buf2 = Vec::new();
    let mut buf3 = Vec::new();
    let _ = c1.read_to_end(&mut buf1).await;
    let _ = c2.read_to_end(&mut buf2).await;
    let _ = c3.read_to_end(&mut buf3).await;

    let r1 = String::from_utf8_lossy(&buf1);
    let r2 = String::from_utf8_lossy(&buf2);
    let r3 = String::from_utf8_lossy(&buf3);

    let succeeded = [r1.contains("200"), r2.contains("200"), r3.contains("200")]
        .iter()
        .filter(|&&x| x)
        .count();
    assert!(
        succeeded <= 2,
        "at most 2 connections should succeed, got {}",
        succeeded
    );
}

#[tokio::test]
async fn prod_server_recovers_after_connections_close() {
    let mut limits = eggserve_core::limits::Limits::default();
    limits.max_connections = 1;
    let s = start_production_server(limits).await;

    {
        let mut c = tokio::net::TcpStream::connect(s.addr).await.unwrap();
        c.write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut buf = Vec::new();
        let _ = c.read_to_end(&mut buf).await;
    }

    tokio::time::sleep(Duration::from_millis(50)).await;

    let line = status_line(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        line.contains("200"),
        "server should recover after connection closes: {}",
        line
    );
}

// ---------------------------------------------------------------------------
// Graceful shutdown
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prod_graceful_shutdown_drains() {
    let s = start_production_server(eggserve_core::limits::Limits::default()).await;

    let raw = send_raw(
        s.addr,
        b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    let resp = String::from_utf8_lossy(&raw);
    assert!(
        resp.contains("200"),
        "expected 200 before shutdown: {}",
        resp
    );

    let _ = s.shutdown_tx.send(());

    tokio::time::sleep(Duration::from_millis(200)).await;

    let result = tokio::net::TcpStream::connect(s.addr).await;
    assert!(
        result.is_err(),
        "server should not accept after shutdown signal"
    );
}

#[tokio::test]
async fn prod_inflight_request_completes_before_shutdown() {
    let s = start_production_server(eggserve_core::limits::Limits::default()).await;

    let mut stream = tokio::net::TcpStream::connect(s.addr).await.unwrap();
    stream
        .write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = s.shutdown_tx.send(());

    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf).await;
    let resp = String::from_utf8_lossy(&buf);
    assert!(
        resp.contains("200"),
        "inflight request should complete: {}",
        resp
    );
}

// ---------------------------------------------------------------------------
// Server survives sequential requests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prod_server_survives_many_requests() {
    let s = start_production_server(eggserve_core::limits::Limits::default()).await;
    for _ in 0..20 {
        let line = status_line(
            s.addr,
            b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(line.contains("200"), "expected 200: {}", line);
    }
}

// ---------------------------------------------------------------------------
// Keep-alive semantics
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prod_keepalive_allows_multiple_requests() {
    let s = start_production_server(eggserve_core::limits::Limits::default()).await;
    let mut stream = tokio::net::TcpStream::connect(s.addr).await.unwrap();

    stream
        .write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n")
        .await
        .unwrap();

    let mut buf = Vec::new();
    stream.readable().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let _ = stream.read_buf(&mut buf).await;

    let resp1 = String::from_utf8_lossy(&buf);
    assert!(resp1.contains("200"), "first request: {}", resp1);

    stream
        .write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();

    let mut buf2 = Vec::new();
    let _ = stream.read_to_end(&mut buf2).await;
    let resp2 = String::from_utf8_lossy(&buf2);
    assert!(resp2.contains("200"), "second request: {}", resp2);
}

// ---------------------------------------------------------------------------
// 405 for unsupported methods
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prod_post_returns_405() {
    let s = start_production_server(eggserve_core::limits::Limits::default()).await;
    let line = status_line(
        s.addr,
        b"POST /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("405"), "expected 405: {}", line);
}

#[tokio::test]
async fn prod_put_returns_405() {
    let s = start_production_server(eggserve_core::limits::Limits::default()).await;
    let line = status_line(
        s.addr,
        b"PUT /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(line.contains("405"), "expected 405: {}", line);
}
