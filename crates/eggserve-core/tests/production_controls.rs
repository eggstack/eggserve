//! Production admission, parser, and lifecycle controls (Plan 164).
//!
//! Deterministic tests for the independent resource budgets closed by
//! Plan 164: explicit parser ceilings, separate in-flight service
//! admission, keep-alive idle timeout, maximum requests per connection,
//! and the response write no-progress timeout. Every wait is bounded so a
//! regression fails fast instead of hanging CI.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use eggserve_core::primitives::canonical::{
    Response, ResponseBody, ResponseStream, ResponseStreamError, StatusCode,
};
use eggserve_core::primitives::connection_info::Scheme;
use eggserve_core::server::connection::{
    serve_http1_connection, ConnectionContext, ConnectionOutcome, ConnectionShutdown,
};
use eggserve_core::server::{service_fn, Request, RuntimeConfig, RuntimeState, Server};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

const BOUND: Duration = Duration::from_secs(5);

fn http_context() -> ConnectionContext {
    ConnectionContext::for_non_socket(Scheme::Http, None)
}

fn bytes_service(body: &'static [u8]) -> impl eggserve_core::server::Service {
    service_fn(move |_req: Request| async move {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(body.to_vec()))
            .unwrap())
    })
}

fn status_code(resp: &[u8]) -> u16 {
    String::from_utf8_lossy(resp)
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse()
        .unwrap_or(0)
}

fn header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn content_length(headers: &[u8]) -> usize {
    let text = String::from_utf8_lossy(headers).to_ascii_lowercase();
    for line in text.lines().skip(1) {
        if let Some(value) = line.strip_prefix("content-length:") {
            return value.trim().parse().unwrap_or(0);
        }
    }
    0
}

/// Read exactly one HTTP/1 response (headers + `Content-Length` body).
/// Returns bytes collected before `bound`, even on timeout/EOF.
async fn read_response<S: AsyncRead + Unpin>(
    stream: &mut S,
    expect_body: bool,
    bound: Duration,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let deadline = tokio::time::Instant::now() + bound;
    loop {
        if let Some(end) = header_end(&buf) {
            let body_len = if expect_body {
                content_length(&buf[..end])
            } else {
                0
            };
            if buf.len() >= end + body_len {
                return buf[..end + body_len].to_vec();
            }
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return buf;
        }
        match tokio::time::timeout(remaining, stream.read(&mut tmp)).await {
            Ok(Ok(0)) => return buf,
            Ok(Ok(n)) => buf.extend_from_slice(&tmp[..n]),
            _ => return buf,
        }
    }
}

/// Read until EOF or `bound`. Used to observe server-initiated closes.
async fn read_until_eof<S: AsyncRead + Unpin>(stream: &mut S, bound: Duration) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let deadline = tokio::time::Instant::now() + bound;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return buf;
        }
        match tokio::time::timeout(remaining, stream.read(&mut tmp)).await {
            Ok(Ok(0)) => return buf,
            Ok(Ok(n)) => buf.extend_from_slice(&tmp[..n]),
            _ => return buf,
        }
    }
}

async fn start_server(
    config: RuntimeConfig,
    service: impl eggserve_core::server::Service,
) -> eggserve_core::server::ServerHandle {
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    handle.ready().await.unwrap();
    handle
}

async fn raw_request(addr: std::net::SocketAddr, request: &[u8]) -> Vec<u8> {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(request).await.unwrap();
    read_until_eof(&mut stream, BOUND).await
}

// ---------------------------------------------------------------------------
// Parser ceilings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn parser_header_count_excess_fails_431() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_headers(8)
        .build()
        .unwrap();
    let handler_called = Arc::new(AtomicBool::new(false));
    let flag = handler_called.clone();
    let handle = start_server(
        config,
        service_fn(move |_req: Request| {
            flag.store(true, Ordering::Relaxed);
            async {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"hello".to_vec()))
                    .unwrap())
            }
        }),
    )
    .await;

    let mut req = b"GET / HTTP/1.1\r\nHost: localhost\r\n".to_vec();
    for i in 0..12u32 {
        req.extend_from_slice(format!("X-Pad-{i}: value\r\n").as_bytes());
    }
    req.extend_from_slice(b"Connection: close\r\n\r\n");
    let resp = raw_request(handle.local_addr(), &req).await;
    assert_eq!(
        status_code(&resp),
        431,
        "got: {}",
        String::from_utf8_lossy(&resp)
    );
    assert!(
        !handler_called.load(Ordering::Relaxed),
        "service must not run after parser rejection"
    );
    handle.shutdown();
}

#[tokio::test]
async fn parser_header_bytes_excess_fails_431() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_header_bytes(1024)
        .build()
        .unwrap();
    let handler_called = Arc::new(AtomicBool::new(false));
    let flag = handler_called.clone();
    let handle = start_server(
        config,
        service_fn(move |_req: Request| {
            flag.store(true, Ordering::Relaxed);
            async {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"hello".to_vec()))
                    .unwrap())
            }
        }),
    )
    .await;

    let big = "v".repeat(2048);
    let req =
        format!("GET / HTTP/1.1\r\nHost: localhost\r\nX-Big: {big}\r\nConnection: close\r\n\r\n");
    let resp = raw_request(handle.local_addr(), req.as_bytes()).await;
    assert_eq!(
        status_code(&resp),
        431,
        "got: {}",
        String::from_utf8_lossy(&resp)
    );
    assert!(
        !handler_called.load(Ordering::Relaxed),
        "service must not run after header-byte rejection"
    );
    handle.shutdown();
}

#[tokio::test]
async fn parser_buf_saturation_never_reaches_service() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_buf_size(8192)
        .build()
        .unwrap();
    let handler_called = Arc::new(AtomicBool::new(false));
    let flag = handler_called.clone();
    let handle = start_server(
        config,
        service_fn(move |_req: Request| {
            flag.store(true, Ordering::Relaxed);
            async {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"hello".to_vec()))
                    .unwrap())
            }
        }),
    )
    .await;

    // A complete 16 KiB header block cannot fit the 8 KiB parser buffer.
    let big = "v".repeat(16 * 1024);
    let req =
        format!("GET / HTTP/1.1\r\nHost: localhost\r\nX-Big: {big}\r\nConnection: close\r\n\r\n");
    let resp = raw_request(handle.local_addr(), req.as_bytes()).await;
    assert_ne!(
        status_code(&resp),
        200,
        "oversized header block must not succeed: {}",
        String::from_utf8_lossy(&resp)
    );
    assert!(
        !handler_called.load(Ordering::Relaxed),
        "service must not run after parser-buffer saturation"
    );
    handle.shutdown();
}

#[tokio::test]
async fn parser_request_target_excess_fails_414() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_target_bytes(512)
        .build()
        .unwrap();
    let handler_called = Arc::new(AtomicBool::new(false));
    let flag = handler_called.clone();
    let handle = start_server(
        config,
        service_fn(move |_req: Request| {
            flag.store(true, Ordering::Relaxed);
            async {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"hello".to_vec()))
                    .unwrap())
            }
        }),
    )
    .await;

    let path = format!("/{}", "a".repeat(1024));
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let resp = raw_request(handle.local_addr(), req.as_bytes()).await;
    assert_eq!(
        status_code(&resp),
        414,
        "got: {}",
        String::from_utf8_lossy(&resp)
    );
    assert!(
        !handler_called.load(Ordering::Relaxed),
        "service must not run after target-length rejection"
    );
    handle.shutdown();
}

// ---------------------------------------------------------------------------
// Separate service admission
// ---------------------------------------------------------------------------

#[tokio::test]
async fn service_saturation_returns_503_while_connections_remain() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_connections(64)
        .max_in_flight_requests(1)
        .build()
        .unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let entered_clone = entered.clone();
    let release = Arc::new(tokio::sync::Notify::new());
    let release_clone = release.clone();
    let handle = start_server(
        config,
        service_fn(move |_req: Request| {
            entered_clone.store(true, Ordering::Relaxed);
            let release = release_clone.clone();
            async move {
                release.notified().await;
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"unblocked".to_vec()))
                    .unwrap())
            }
        }),
    )
    .await;
    let addr = handle.local_addr();

    // First connection occupies the single in-flight slot.
    let mut conn1 = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn1
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + BOUND;
    while !entered.load(Ordering::Relaxed) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "first request never entered the service"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let conn1_read = tokio::spawn(async move { read_until_eof(&mut conn1, BOUND).await });

    // Connection budget (64) is nowhere near exhausted, yet the second
    // request must fail deterministically with 503: service admission is
    // independent of connection admission, with no hidden queue.
    let resp2 = raw_request(
        addr,
        b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(
        status_code(&resp2),
        503,
        "saturated service must answer 503, got: {}",
        String::from_utf8_lossy(&resp2)
    );

    // The permit recovers when the first handler completes.
    release.notify_waiters();
    let resp1 = tokio::time::timeout(BOUND, conn1_read)
        .await
        .expect("first connection must complete")
        .unwrap();
    assert_eq!(status_code(&resp1), 200);
    assert!(String::from_utf8_lossy(&resp1).contains("unblocked"));
    handle.shutdown();
}

// ---------------------------------------------------------------------------
// Lifecycle: idle, request count, write progress, hard lifetime
// ---------------------------------------------------------------------------

async fn drive_duplex(
    config: Arc<RuntimeConfig>,
    service: impl eggserve_core::server::Service,
    capacity: usize,
) -> (
    tokio::io::DuplexStream,
    tokio::task::JoinHandle<ConnectionOutcome>,
    Arc<RuntimeState>,
) {
    let runtime_state = Arc::new(RuntimeState::new(&config));
    let returned = runtime_state.clone();
    let (client, server) = tokio::io::duplex(capacity);
    let shutdown = ConnectionShutdown::new();
    let driver = tokio::spawn(async move {
        serve_http1_connection(
            server,
            service,
            config,
            http_context(),
            runtime_state,
            &shutdown,
        )
        .await
    });
    (client, driver, returned)
}

#[tokio::test]
async fn keepalive_idle_timeout_closes_idle_connection() {
    let config = Arc::new(
        RuntimeConfig::builder()
            .keep_alive_idle_timeout(Duration::from_millis(300))
            .connection_total_timeout(Duration::from_secs(30))
            .response_write_timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
    );
    let (mut client, driver, _state) =
        drive_duplex(config, bytes_service(b"hello"), 128 * 1024).await;

    client
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
        .await
        .unwrap();
    let resp = read_response(&mut client, true, BOUND).await;
    assert_eq!(status_code(&resp), 200);

    // No further activity: the idle timer (not the hard lifetime) closes us.
    let rest = read_until_eof(&mut client, BOUND).await;
    assert!(
        rest.is_empty(),
        "idle connection must see EOF, got {} bytes",
        rest.len()
    );
    let outcome = tokio::time::timeout(BOUND, driver)
        .await
        .expect("driver must exit")
        .unwrap();
    assert_eq!(outcome, ConnectionOutcome::IdleTimeout);
}

#[tokio::test]
async fn keepalive_gap_bounded_by_header_timeout_when_shorter() {
    // Hyper starts its header-read timeout while a connection sits idle
    // between requests. When the header timeout is shorter than the
    // EggServe idle timeout, the Hyper timeout wins and the close is
    // reported as a header timeout. Operators wanting long-lived
    // keep-alive must raise the header timeout too.
    let config = Arc::new(
        RuntimeConfig::builder()
            .header_read_timeout(Duration::from_millis(300))
            .connection_total_timeout(Duration::from_secs(30))
            .keep_alive_idle_timeout(Duration::from_secs(30))
            .response_write_timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
    );
    let (mut client, driver, _state) =
        drive_duplex(config, bytes_service(b"hello"), 128 * 1024).await;

    client
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
        .await
        .unwrap();
    let resp = read_response(&mut client, true, BOUND).await;
    assert_eq!(status_code(&resp), 200);

    let rest = read_until_eof(&mut client, BOUND).await;
    assert!(
        rest.is_empty(),
        "idle connection must see EOF, got {} bytes",
        rest.len()
    );
    let outcome = tokio::time::timeout(BOUND, driver)
        .await
        .expect("driver must exit")
        .unwrap();
    assert_eq!(outcome, ConnectionOutcome::HeaderTimeout);
}

#[tokio::test]
async fn repeated_keepalive_requests_reset_idle_deadline() {
    let config = Arc::new(
        RuntimeConfig::builder()
            .keep_alive_idle_timeout(Duration::from_millis(600))
            .connection_total_timeout(Duration::from_secs(30))
            .response_write_timeout(Duration::from_secs(30))
            .header_read_timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
    );
    let (mut client, driver, _state) =
        drive_duplex(config, bytes_service(b"hello"), 128 * 1024).await;

    // Three sequential requests, 200 ms apart: activity keeps resetting the
    // 600 ms idle deadline, so none of them may be cut off.
    for _ in 0..3 {
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
            .await
            .unwrap();
        let resp = read_response(&mut client, true, BOUND).await;
        assert_eq!(status_code(&resp), 200);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Then go quiet: the idle timer must still fire afterwards.
    let rest = read_until_eof(&mut client, BOUND).await;
    assert!(rest.is_empty());
    let outcome = tokio::time::timeout(BOUND, driver)
        .await
        .expect("driver must exit")
        .unwrap();
    assert_eq!(outcome, ConnectionOutcome::IdleTimeout);
}

#[tokio::test]
async fn max_requests_per_connection_closes_cleanly_after_limit() {
    let config = Arc::new(
        RuntimeConfig::builder()
            .max_requests_per_connection(Some(2))
            .keep_alive_idle_timeout(Duration::from_secs(30))
            .connection_total_timeout(Duration::from_secs(30))
            .header_read_timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
    );
    let (mut client, driver, _state) =
        drive_duplex(config, bytes_service(b"hello"), 128 * 1024).await;

    client
        .write_all(b"GET /one HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
        .await
        .unwrap();
    let first = read_response(&mut client, true, BOUND).await;
    assert_eq!(status_code(&first), 200);
    assert!(
        !String::from_utf8_lossy(&first)
            .to_ascii_lowercase()
            .contains("connection: close"),
        "first response must keep the connection reusable"
    );

    client
        .write_all(b"GET /two HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
        .await
        .unwrap();
    let second = read_response(&mut client, true, BOUND).await;
    assert_eq!(
        status_code(&second),
        200,
        "limit response must complete correctly"
    );
    assert!(
        String::from_utf8_lossy(&second)
            .to_ascii_lowercase()
            .contains("connection: close"),
        "limit response must signal close: {}",
        String::from_utf8_lossy(&second)
    );

    let rest = read_until_eof(&mut client, BOUND).await;
    assert!(rest.is_empty());
    let outcome = tokio::time::timeout(BOUND, driver)
        .await
        .expect("driver must exit")
        .unwrap();
    assert_eq!(outcome, ConnectionOutcome::Normal);
}

#[tokio::test]
async fn max_requests_counts_head_responses() {
    let config = Arc::new(
        RuntimeConfig::builder()
            .max_requests_per_connection(Some(1))
            .keep_alive_idle_timeout(Duration::from_secs(30))
            .connection_total_timeout(Duration::from_secs(30))
            .header_read_timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
    );
    let (mut client, driver, _state) =
        drive_duplex(config, bytes_service(b"hello"), 128 * 1024).await;

    client
        .write_all(b"HEAD / HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
        .await
        .unwrap();
    let resp = read_response(&mut client, false, BOUND).await;
    assert_eq!(status_code(&resp), 200);
    assert!(
        String::from_utf8_lossy(&resp)
            .to_ascii_lowercase()
            .contains("connection: close"),
        "even the first response counts toward the limit"
    );
    let rest = read_until_eof(&mut client, BOUND).await;
    assert!(rest.is_empty());
    let outcome = tokio::time::timeout(BOUND, driver)
        .await
        .expect("driver must exit")
        .unwrap();
    assert_eq!(outcome, ConnectionOutcome::Normal);
}

fn big_stream_service(
    chunk_count: usize,
    chunk_delay: Duration,
) -> impl eggserve_core::server::Service {
    service_fn(move |_req: Request| async move {
        let chunks: Vec<Result<Bytes, ResponseStreamError>> = (0..chunk_count)
            .map(|_| Ok(Bytes::from(vec![0xAB; 65_536])))
            .collect();
        let total = (chunk_count * 65_536) as u64;
        let paced =
            futures_util::stream::unfold((chunks, chunk_delay), |(mut chunks, delay)| async move {
                match chunks.pop() {
                    Some(chunk) => {
                        tokio::time::sleep(delay).await;
                        Some((chunk, (chunks, delay)))
                    }
                    None => None,
                }
            });
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(ResponseStream::with_known_length(
                paced, total,
            )))
            .unwrap())
    })
}

#[tokio::test]
async fn write_stall_timeout_fires_on_stalled_reader() {
    let config = Arc::new(
        RuntimeConfig::builder()
            .response_write_timeout(Duration::from_millis(300))
            .keep_alive_idle_timeout(Duration::from_secs(30))
            .connection_total_timeout(Duration::from_secs(30))
            .header_read_timeout(Duration::from_secs(30))
            // Short drain so the stalled connection is reaped promptly
            // instead of lingering in post-shutdown drain.
            .graceful_shutdown_timeout(Duration::from_millis(500))
            .build()
            .unwrap(),
    );
    // 2 MiB through a 64 KiB pipe the client stops draining: the writer
    // must stall and the no-progress timer must fire.
    let (mut client, driver, _state) =
        drive_duplex(config, big_stream_service(32, Duration::ZERO), 64 * 1024).await;

    client
        .write_all(b"GET /big HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    // Read response headers only, then stop: TCP backpressure stalls us.
    let mut head = Vec::new();
    let mut tmp = [0u8; 1024];
    let deadline = tokio::time::Instant::now() + BOUND;
    while header_end(&head).is_none() && tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, client.read(&mut tmp)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => head.extend_from_slice(&tmp[..n]),
            _ => break,
        }
    }
    assert_eq!(status_code(&head), 200);

    // Stop reading entirely: without socket progress the server must give
    // up on the stalled reader and close promptly. (Reading here would
    // drain the pipe — including during the bounded post-shutdown drain —
    // and relieve the backpressure under test.)
    tokio::time::sleep(Duration::from_secs(2)).await;
    let rest = read_until_eof(&mut client, BOUND).await;
    assert!(
        (rest.len() as u64) < 2 * 1024 * 1024,
        "stalled reader must not receive the full 2 MiB (got {} bytes)",
        rest.len()
    );
    let outcome = tokio::time::timeout(BOUND, driver)
        .await
        .expect("driver must exit")
        .unwrap();
    assert_eq!(outcome, ConnectionOutcome::WriteTimeout);
}

#[tokio::test]
async fn healthy_progressive_download_never_trips_write_timeout() {
    let config = Arc::new(
        RuntimeConfig::builder()
            .response_write_timeout(Duration::from_millis(500))
            .keep_alive_idle_timeout(Duration::from_secs(30))
            .connection_total_timeout(Duration::from_secs(30))
            .header_read_timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
    );
    // 512 KiB in small paced chunks with continuous progress: slow in
    // total, but never stalled, so the no-progress timer must not fire.
    let (mut client, driver, _state) = drive_duplex(
        config,
        big_stream_service(8, Duration::from_millis(5)),
        64 * 1024,
    )
    .await;

    client
        .write_all(b"GET /big HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let resp = read_until_eof(&mut client, Duration::from_secs(15)).await;
    assert_eq!(status_code(&resp), 200);
    let end = header_end(&resp).expect("response must have headers");
    assert_eq!(
        resp.len() - end,
        8 * 65_536,
        "healthy download must complete fully"
    );
    let outcome = tokio::time::timeout(BOUND, driver)
        .await
        .expect("driver must exit")
        .unwrap();
    assert_eq!(outcome, ConnectionOutcome::Normal);
}

#[tokio::test]
async fn total_lifetime_remains_the_hard_ceiling() {
    // total=500ms with every sub-budget at/below it (builder cross-checks).
    // Requests every 100ms keep resetting Hyper's idle header timer
    // (400ms), so only the absolute total lifetime can fire.
    let config = Arc::new(
        RuntimeConfig::builder()
            .connection_total_timeout(Duration::from_millis(500))
            .keep_alive_idle_timeout(Duration::from_secs(30))
            .response_write_timeout(Duration::from_secs(30))
            .header_read_timeout(Duration::from_millis(400))
            .handler_timeout(Duration::from_millis(400))
            .body_read_timeout(Duration::from_millis(400))
            .build()
            .unwrap(),
    );
    let (mut client, driver, _state) =
        drive_duplex(config, bytes_service(b"hello"), 128 * 1024).await;

    client
        .write_all(b"GET /first HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
        .await
        .unwrap();
    let resp = read_response(&mut client, true, BOUND).await;
    assert_eq!(status_code(&resp), 200);

    // Keep the connection busy past the 500ms lifetime.
    for _ in 0..6 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = client
            .write_all(b"GET /again HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
            .await;
        let _ = read_response(&mut client, true, Duration::from_millis(300)).await;
    }

    let rest = read_until_eof(&mut client, BOUND).await;
    assert!(rest.is_empty());
    let outcome = tokio::time::timeout(BOUND, driver)
        .await
        .expect("driver must exit")
        .unwrap();
    assert_eq!(outcome, ConnectionOutcome::TotalTimeout);
}

#[tokio::test]
async fn slowloris_header_delivery_times_out() {
    let config = Arc::new(
        RuntimeConfig::builder()
            .header_read_timeout(Duration::from_millis(400))
            .keep_alive_idle_timeout(Duration::from_secs(30))
            .connection_total_timeout(Duration::from_secs(30))
            .response_write_timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
    );
    let (mut client, driver, _state) =
        drive_duplex(config, bytes_service(b"hello"), 128 * 1024).await;

    // Trickle an incomplete header block, then go silent.
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let rest = read_until_eof(&mut client, BOUND).await;
    assert!(rest.is_empty(), "slowloris connection must be closed");
    let outcome = tokio::time::timeout(BOUND, driver)
        .await
        .expect("driver must exit")
        .unwrap();
    assert_eq!(outcome, ConnectionOutcome::HeaderTimeout);
}

#[tokio::test]
async fn shutdown_during_blocked_handler_releases_connection() {
    let config = Arc::new(
        RuntimeConfig::builder()
            .graceful_shutdown_timeout(Duration::from_millis(500))
            .build()
            .unwrap(),
    );
    let runtime_state = Arc::new(RuntimeState::new(&config));
    let entered = Arc::new(AtomicBool::new(false));
    let entered_clone = entered.clone();
    let (client, server) = tokio::io::duplex(128 * 1024);
    let mut client = client;
    let shutdown = ConnectionShutdown::new();
    let shutdown_clone = shutdown.clone();
    let service = service_fn(move |_req: Request| {
        entered_clone.store(true, Ordering::Relaxed);
        async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"late".to_vec()))
                .unwrap())
        }
    });
    let driver = tokio::spawn(async move {
        serve_http1_connection(
            server,
            service,
            config,
            http_context(),
            runtime_state,
            &shutdown_clone,
        )
        .await
    });

    client
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + BOUND;
    while !entered.load(Ordering::Relaxed) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "handler never entered"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    shutdown.shutdown();
    let outcome = tokio::time::timeout(BOUND, driver)
        .await
        .expect("driver must exit")
        .unwrap();
    assert_eq!(outcome, ConnectionOutcome::Shutdown);
    let _ = client;
}

#[tokio::test]
async fn permits_recover_after_idle_expiry() {
    // A small connection budget plus idle expiry: after the first
    // connection is reaped, a new one must still be admitted and served.
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_connections(1)
        .max_in_flight_requests(1)
        .keep_alive_idle_timeout(Duration::from_millis(300))
        .connection_total_timeout(Duration::from_secs(30))
        .header_read_timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let handle = start_server(config, bytes_service(b"hello")).await;
    let addr = handle.local_addr();

    let mut conn1 = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn1
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n")
        .await
        .unwrap();
    let resp1 = read_response(&mut conn1, true, BOUND).await;
    assert_eq!(status_code(&resp1), 200);
    // Idle expiry reaps the connection and its permit.
    let rest = read_until_eof(&mut conn1, BOUND).await;
    assert!(rest.is_empty());

    let resp2 = raw_request(
        addr,
        b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(
        status_code(&resp2),
        200,
        "permits must recover after expiry"
    );
    handle.shutdown();
}

// ---------------------------------------------------------------------------
// TLS parity (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "tls")]
mod tls_parity {
    use super::*;

    fn init_tls() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
    }

    struct TlsContext {
        server_config: Arc<rustls::ServerConfig>,
        client_config: Arc<rustls::ClientConfig>,
    }

    fn make_tls_context() -> TlsContext {
        use rustls::pki_types::PrivatePkcs8KeyDer;
        init_tls();
        let key_pair = rcgen::KeyPair::generate().expect("generate key pair");
        let params =
            rcgen::CertificateParams::new(vec!["localhost".to_string()]).expect("create params");
        let cert = params.self_signed(&key_pair).expect("self-sign cert");
        let cert_der: rustls::pki_types::CertificateDer<'static> = cert.into();
        let key_der = PrivatePkcs8KeyDer::from(key_pair.serialize_der());
        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der.clone()], key_der.into())
            .expect("server TLS config");
        let mut root_store = rustls::RootCertStore::empty();
        root_store.add(cert_der).unwrap();
        let client_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        TlsContext {
            server_config: Arc::new(server_config),
            client_config: Arc::new(client_config),
        }
    }

    #[tokio::test]
    async fn tls_idle_timeout_matches_plaintext() {
        let ctx = make_tls_context();
        let config = RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .tls_config(ctx.server_config.clone())
            .keep_alive_idle_timeout(Duration::from_millis(300))
            .connection_total_timeout(Duration::from_secs(30))
            .header_read_timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        let handle = start_server(config, bytes_service(b"hello")).await;
        let addr = handle.local_addr();

        let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
        let connector = tokio_rustls::TlsConnector::from(ctx.client_config.clone());
        let domain = "localhost".try_into().unwrap();
        let tls_stream = connector.connect(domain, tcp).await.unwrap();
        let (mut reader, mut writer) = tokio::io::split(tls_stream);
        writer
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n")
            .await
            .unwrap();
        let resp = read_response(&mut reader, true, BOUND).await;
        let status = status_code(&resp);
        assert_eq!(status, 200, "got: {}", String::from_utf8_lossy(&resp));

        // Idle expiry must close TLS connections exactly like plaintext.
        let rest = read_until_eof(&mut reader, BOUND).await;
        assert!(rest.is_empty(), "TLS idle connection must see EOF");
        handle.shutdown();
    }
}
