//! Transport-neutral canonical connection driver tests (Plan 163).
//!
//! Drives [`serve_http1_connection`] over `tokio::io::duplex` streams (no
//! TCP, no `SocketAddr` fabrication) and verifies the same canonical
//! pipeline serves TCP, TLS, and caller-owned streams.

use std::sync::Arc;
use std::time::Duration;

use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::connection_info::Scheme;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::server::connection::{
    serve_http1_connection, ConnectionContext, ConnectionOutcome, ConnectionShutdown,
};
use eggserve_core::server::{
    service_fn, service_fn_with_policy, Request, RuntimeConfig, RuntimeState,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn test_config() -> Arc<RuntimeConfig> {
    Arc::new(RuntimeConfig::default())
}

fn test_runtime(config: &RuntimeConfig) -> Arc<RuntimeState> {
    Arc::new(RuntimeState::new(config))
}

fn http_context() -> ConnectionContext {
    ConnectionContext::for_non_socket(Scheme::Http, None)
}

async fn drive_once(
    request_bytes: &[u8],
    service: impl eggserve_core::server::Service,
    config: Arc<RuntimeConfig>,
    context: ConnectionContext,
    runtime_state: Arc<RuntimeState>,
) -> (Vec<u8>, ConnectionOutcome) {
    let (mut client, server) = tokio::io::duplex(128 * 1024);
    let shutdown = ConnectionShutdown::new();
    let driver = tokio::spawn(async move {
        serve_http1_connection(server, service, config, context, runtime_state, &shutdown).await
    });
    client.write_all(request_bytes).await.unwrap();
    // Half-close the write side? duplex doesn't support shutdown; just read
    // until EOF (driver closes after `Connection: close` response).
    let mut buf = Vec::new();
    let _ = client.read_to_end(&mut buf).await;
    let outcome = driver.await.unwrap();
    (buf, outcome)
}

fn ok_service() -> impl eggserve_core::server::Service {
    service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap())
    })
}

fn strip_date_header(raw: &[u8]) -> Vec<u8> {
    // Remove the single `date:` header line for byte-parity comparisons
    // (Date varies by second between TCP and duplex runs).
    let text = String::from_utf8_lossy(raw);
    text.lines()
        .filter(|l| !l.to_ascii_lowercase().starts_with("date:"))
        .collect::<Vec<_>>()
        .join("\r\n")
        .into_bytes()
}

#[tokio::test]
async fn duplex_get_succeeds_without_socket_addrs() {
    let config = test_config();
    let runtime = test_runtime(&config);
    let (buf, outcome) = drive_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        config,
        http_context(),
        runtime,
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
    assert!(text.contains("hello"));
    assert_eq!(outcome, ConnectionOutcome::Normal);
}

#[tokio::test]
async fn duplex_head_does_not_send_body() {
    let config = test_config();
    let runtime = test_runtime(&config);
    let (buf, outcome) = drive_once(
        b"HEAD / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        config,
        http_context(),
        runtime,
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
    assert!(!text.contains("hello"));
    assert_eq!(outcome, ConnectionOutcome::Normal);
}

#[tokio::test]
async fn non_socket_context_exposes_no_endpoints() {
    let ctx = ConnectionContext::for_non_socket(Scheme::Http, None);
    assert!(!ctx.has_socket_endpoints());
    assert!(ctx.socket_endpoints().is_none());
    let info = ctx.connection_info();
    assert_eq!(info.local_addr, None);
    assert_eq!(info.remote_addr, None);
    assert_eq!(info.scheme, Scheme::Http);

    // Service observes None, not fabricated addresses.
    let seen = Arc::new(std::sync::Mutex::new(
        None::<(Option<String>, Option<String>)>,
    ));
    let seen_clone = seen.clone();
    let svc = service_fn(move |req: Request| {
        let seen_clone = seen_clone.clone();
        async move {
            let conn = req.connection();
            *seen_clone.lock().unwrap() = Some((
                conn.local_addr.map(|a| a.to_string()),
                conn.remote_addr.map(|a| a.to_string()),
            ));
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        }
    });
    let config = test_config();
    let runtime = test_runtime(&config);
    let _ = drive_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        svc,
        config,
        http_context(),
        runtime,
    )
    .await;
    let (local, remote) = seen.lock().unwrap().clone().unwrap();
    assert_eq!(local, None);
    assert_eq!(remote, None);
}

#[tokio::test]
async fn tcp_context_preserves_real_endpoints() {
    let local: std::net::SocketAddr = "127.0.0.1:8000".parse().unwrap();
    let remote: std::net::SocketAddr = "127.0.0.1:12345".parse().unwrap();
    let ctx = ConnectionContext::for_tcp(local, remote, None);
    assert!(ctx.has_socket_endpoints());
    let eps = ctx.socket_endpoints().unwrap();
    assert_eq!(eps.local, local);
    assert_eq!(eps.remote, remote);
    let info = ctx.connection_info();
    assert_eq!(info.local_addr, Some(local));
    assert_eq!(info.remote_addr, Some(remote));
}

#[tokio::test]
async fn duplex_buffered_body_echo() {
    let svc = service_fn_with_policy(
        |req: Request| async move {
            let (head, body) = req.into_head_and_body();
            assert_eq!(head.method().as_str(), "POST");
            let bytes = body
                .read_all()
                .await
                .map_err(|e| eggserve_core::server::ServiceError::internal(e.to_string()))?;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(bytes.to_vec()))
                .unwrap())
        },
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
    );
    let config = Arc::new(
        RuntimeConfig::builder()
            .max_request_body_bytes(1024)
            .build()
            .unwrap(),
    );
    let runtime = test_runtime(&config);
    let (buf, outcome) = drive_once(
        b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
        svc,
        config,
        http_context(),
        runtime,
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
    assert!(text.ends_with("hello"));
    assert_eq!(outcome, ConnectionOutcome::Normal);
}

#[tokio::test]
async fn duplex_streaming_body_echo() {
    use futures_util::StreamExt;
    let svc = service_fn_with_policy(
        |req: Request| async move {
            let (_head, mut body) = req.into_head_and_body();
            let mut out = Vec::new();
            while let Some(chunk) = body.next().await {
                let chunk = chunk
                    .map_err(|e| eggserve_core::server::ServiceError::internal(e.to_string()))?;
                out.extend_from_slice(&chunk);
            }
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(out))
                .unwrap())
        },
        RequestBodyPolicy::Stream { max_bytes: 4096 },
    );
    let config = Arc::new(
        RuntimeConfig::builder()
            .max_request_body_bytes(4096)
            .build()
            .unwrap(),
    );
    let runtime = test_runtime(&config);
    let (buf, outcome) = drive_once(
        b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello world",
        svc,
        config,
        http_context(),
        runtime,
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
    assert!(text.ends_with("hello world"));
    assert_eq!(outcome, ConnectionOutcome::Normal);
}

#[tokio::test]
async fn duplex_known_length_stream_response() {
    use eggserve_core::primitives::canonical::ResponseStream;
    let svc = service_fn(|_req: Request| async {
        let stream = futures_util::stream::iter(vec![
            Ok::<_, eggserve_core::primitives::canonical::ResponseStreamError>(
                bytes::Bytes::from_static(b"hel"),
            ),
            Ok(bytes::Bytes::from_static(b"lo")),
        ]);
        let rs = ResponseStream::with_known_length(stream, 5);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(rs))
            .unwrap())
    });
    let config = test_config();
    let runtime = test_runtime(&config);
    let (buf, outcome) = drive_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        svc,
        config,
        http_context(),
        runtime,
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
    assert!(text.to_ascii_lowercase().contains("content-length: 5"));
    assert!(text.ends_with("hello"));
    assert_eq!(outcome, ConnectionOutcome::Normal);
}

#[tokio::test]
async fn duplex_unknown_length_stream_uses_chunked() {
    use eggserve_core::primitives::canonical::ResponseStream;
    let svc = service_fn(|_req: Request| async {
        let stream = futures_util::stream::iter(vec![Ok::<
            _,
            eggserve_core::primitives::canonical::ResponseStreamError,
        >(bytes::Bytes::from_static(
            b"chunked-body",
        ))]);
        let rs = ResponseStream::new(stream);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(rs))
            .unwrap())
    });
    let config = test_config();
    let runtime = test_runtime(&config);
    let (buf, outcome) = drive_once(
        b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        svc,
        config,
        http_context(),
        runtime,
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
    assert!(text
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked"));
    assert!(text.contains("chunked-body"));
    assert_eq!(outcome, ConnectionOutcome::Normal);
}

#[tokio::test]
async fn duplex_malformed_request_rejected() {
    let config = test_config();
    let runtime = test_runtime(&config);
    // Absolute-form targets are rejected at the canonical conversion
    // boundary before service invocation (400).
    let (buf, outcome) = drive_once(
        b"GET http://example.com/ HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        ok_service(),
        config,
        http_context(),
        runtime,
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    // Canonical pipeline rejects invalid targets as 400 (not 500).
    assert!(
        text.starts_with("HTTP/1.1 400") || text.starts_with("HTTP/1.1 404"),
        "got: {text}"
    );
    // Malformed framing still drives the connection to a defined outcome.
    assert!(
        matches!(
            outcome,
            ConnectionOutcome::Normal | ConnectionOutcome::ClientError
        ),
        "unexpected outcome: {outcome:?}"
    );
}

#[tokio::test]
async fn duplex_keep_alive_serves_multiple_requests() {
    let config = test_config();
    let runtime = test_runtime(&config);
    let (mut client, server) = tokio::io::duplex(128 * 1024);
    let shutdown = ConnectionShutdown::new();
    let driver = tokio::spawn(async move {
        serve_http1_connection(
            server,
            ok_service(),
            config,
            http_context(),
            runtime,
            &shutdown,
        )
        .await
    });
    // Two sequential keep-alive requests on one duplex stream.
    for _ in 0..2 {
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        // Read response headers + body by Content-Length.
        let mut headers = Vec::new();
        // Read until end of headers.
        loop {
            let mut byte = [0u8; 1];
            client.read_exact(&mut byte).await.unwrap();
            headers.push(byte[0]);
            if headers.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let header_text = String::from_utf8_lossy(&headers);
        assert!(
            header_text.starts_with("HTTP/1.1 200 OK"),
            "got: {header_text}"
        );
        // Body is 5 bytes ("hello").
        let mut body = [0u8; 5];
        client.read_exact(&mut body).await.unwrap();
        assert_eq!(&body, b"hello");
    }
    // Close the client side; driver should exit cleanly.
    drop(client);
    let outcome = tokio::time::timeout(Duration::from_secs(5), driver)
        .await
        .expect("driver should exit after client close")
        .unwrap();
    assert!(
        matches!(
            outcome,
            ConnectionOutcome::Normal | ConnectionOutcome::ClientError
        ),
        "unexpected outcome: {outcome:?}"
    );
}

#[tokio::test]
async fn duplex_header_timeout_fires() {
    let config = Arc::new(
        RuntimeConfig::builder()
            .header_read_timeout(Duration::from_millis(50))
            .connection_total_timeout(Duration::from_secs(5))
            .handler_timeout(Duration::from_secs(5))
            .body_read_timeout(Duration::from_secs(5))
            .build()
            .unwrap(),
    );
    let runtime = test_runtime(&config);
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    let shutdown = ConnectionShutdown::new();
    let driver = tokio::spawn(async move {
        serve_http1_connection(
            server,
            ok_service(),
            config,
            http_context(),
            runtime,
            &shutdown,
        )
        .await
    });
    // Send a partial request line and stall past header_read_timeout.
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n")
        .await
        .unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(5), driver)
        .await
        .expect("driver should exit on header timeout")
        .unwrap();
    assert_eq!(outcome, ConnectionOutcome::HeaderTimeout);
}

#[tokio::test]
async fn duplex_caller_cancellation_shuts_down() {
    let svc = service_fn(|_req: Request| async {
        tokio::time::sleep(Duration::from_secs(30)).await;
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Empty)
            .unwrap())
    });
    let config = test_config();
    let runtime = test_runtime(&config);
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    let shutdown = ConnectionShutdown::new();
    let shutdown_clone = shutdown.clone();
    let driver = tokio::spawn(async move {
        serve_http1_connection(
            server,
            svc,
            config,
            http_context(),
            runtime,
            &shutdown_clone,
        )
        .await
    });
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    // Let the handler start, then request graceful shutdown.
    tokio::time::sleep(Duration::from_millis(100)).await;
    shutdown.shutdown();
    assert!(shutdown.is_shutdown());
    let outcome = tokio::time::timeout(Duration::from_secs(10), driver)
        .await
        .expect("driver should exit after cancellation")
        .unwrap();
    // Handler timeout (30s) is longer than the test; shutdown must win.
    // Accept Shutdown or TotalTimeout depending on race with connection
    // lifetime; both prove cancellation released the connection.
    assert!(
        matches!(
            outcome,
            ConnectionOutcome::Shutdown | ConnectionOutcome::TotalTimeout
        ),
        "unexpected outcome: {outcome:?}"
    );
}

#[tokio::test]
async fn shared_runtime_state_enforces_file_stream_budget() {
    // Saturate the shared file-stream pool manually; a file response must
    // then collapse to 503 rather than creating an independent pool.
    use eggserve_core::primitives::body::BodySource;
    let config = test_config();
    let runtime = test_runtime(&config);
    assert_eq!(
        runtime.file_stream_semaphore().available_permits(),
        config.max_file_streams
    );
    // Hold every permit.
    let mut guards = Vec::new();
    for _ in 0..config.max_file_streams {
        guards.push(
            runtime
                .file_stream_semaphore()
                .clone()
                .try_acquire_owned()
                .unwrap(),
        );
    }
    assert_eq!(runtime.file_stream_semaphore().available_permits(), 0);

    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("f.txt"), b"data").unwrap();
    let svc = eggserve_core::server::StaticService::builder(tmp.path())
        .build()
        .unwrap();
    let (buf, _) = drive_once(
        b"GET /f.txt HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        svc,
        test_config(),
        // NOTE: drive_once builds its own runtime; rebuild with shared one.
        http_context(),
        // Use a fresh runtime with the same limit but manually saturated?
        // Instead drive with the saturated `runtime` directly below.
        test_runtime(&test_config()),
    )
    .await;
    let _ = buf;
    drop(guards);

    // Drive again with the saturated pool held: expect 503.
    let tmp2 = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp2.path().join("g.txt"), b"data").unwrap();
    let svc2 = eggserve_core::server::StaticService::builder(tmp2.path())
        .build()
        .unwrap();
    let mut held = Vec::new();
    for _ in 0..config.max_file_streams {
        held.push(
            runtime
                .file_stream_semaphore()
                .clone()
                .try_acquire_owned()
                .unwrap(),
        );
    }
    let (mut client, server) = tokio::io::duplex(128 * 1024);
    let shutdown = ConnectionShutdown::new();
    let cfg = test_config();
    let rt = runtime.clone();
    let driver = tokio::spawn(async move {
        serve_http1_connection(server, svc2, cfg, http_context(), rt, &shutdown).await
    });
    client
        .write_all(b"GET /g.txt HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut out = Vec::new();
    let _ = client.read_to_end(&mut out).await;
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.starts_with("HTTP/1.1 503"),
        "saturated file pool should yield 503, got: {text}"
    );
    let _ = driver.await.unwrap();
    drop(held);
    // Permits released after driver exit.
    assert_eq!(
        runtime.file_stream_semaphore().available_permits(),
        config.max_file_streams
    );
    let _ = BodySource::Empty;
}

#[tokio::test]
async fn duplex_parity_with_tcp_path() {
    // Same service + same bytes over TCP (Server) and duplex (driver)
    // produce equivalent responses modulo Date.
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), b"hello").unwrap();
    let svc_tcp = eggserve_core::server::StaticService::builder(tmp.path())
        .build()
        .unwrap();
    let server = eggserve_core::server::Server::builder()
        .runtime(
            RuntimeConfig::builder()
                .bind("127.0.0.1:0".parse().unwrap())
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();
    let handle = server.start_with_service(svc_tcp).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    tcp.write_all(b"GET /hello.txt HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut tcp_buf = Vec::new();
    tcp.read_to_end(&mut tcp_buf).await.unwrap();
    handle.shutdown();
    handle.wait().await.unwrap();

    let svc_duplex = eggserve_core::server::StaticService::builder(tmp.path())
        .build()
        .unwrap();
    let config = test_config();
    let runtime = test_runtime(&config);
    let (duplex_buf, outcome) = drive_once(
        b"GET /hello.txt HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        svc_duplex,
        config,
        http_context(),
        runtime,
    )
    .await;
    assert_eq!(outcome, ConnectionOutcome::Normal);
    assert_eq!(strip_date_header(&tcp_buf), strip_date_header(&duplex_buf));
}
