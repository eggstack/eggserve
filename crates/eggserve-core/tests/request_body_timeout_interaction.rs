use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use eggserve_core::config::ServeConfig;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::request::Request;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::server::config::RuntimeConfig;
use eggserve_core::server::{service_fn_with_policy, Server};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn start_server(
    config: RuntimeConfig,
    policy: RequestBodyPolicy,
    handler: impl Fn(
            Request,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<Response, eggserve_core::server::ServiceError>,
                    > + Send
                    + 'static,
            >,
        > + Send
        + Sync
        + 'static,
) -> (eggserve_core::server::handle::ServerHandle, TempDir) {
    let tmp = TempDir::new().unwrap();
    let serve_config = Arc::new(ServeConfig {
        root: tmp.path().to_path_buf(),
        ..ServeConfig::default()
    });
    let server = Server::builder()
        .runtime(config)
        .serve_config(serve_config)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn_with_policy(handler, policy))
        .await
        .unwrap();
    handle.ready().await.unwrap();
    (handle, tmp)
}

#[tokio::test]
async fn handler_timeout_before_body_timeout() {
    // Handler timeout (50ms) expires before body timeout (5s)
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .handler_timeout(Duration::from_millis(50))
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let handler_called = Arc::new(AtomicBool::new(false));
    let handler_called_clone = handler_called.clone();

    let (handle, _tmp) = start_server(
        config,
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
        move |req: Request| {
            let called = handler_called_clone.clone();
            Box::pin(async move {
                called.store(true, Ordering::Relaxed);
                // Slow handler - sleeps longer than handler timeout
                tokio::time::sleep(Duration::from_secs(10)).await;
                let (_head, body) = req.into_head_and_body();
                let _ = body.read_all().await;
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"ok".to_vec()))
                    .unwrap())
            })
        },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    )
    .await
    .unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf)).await;
    let response = String::from_utf8_lossy(&buf);

    // Handler was called but timed out - server should return 504 or 500
    assert!(
        handler_called.load(Ordering::Relaxed),
        "handler should be called before timeout"
    );
    assert!(
        response.starts_with("HTTP/1.1 504") || response.starts_with("HTTP/1.1 500"),
        "expected 504 or 500 on handler timeout, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn body_timeout_before_handler_timeout() {
    // Body timeout (50ms) expires before handler timeout (10s)
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .handler_timeout(Duration::from_secs(10))
        .body_read_timeout(Duration::from_millis(50))
        .build()
        .unwrap();

    let (handle, _tmp) = start_server(
        config,
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
        |_req: Request| {
            Box::pin(async move { unreachable!("handler should not be called on body timeout") })
        },
    )
    .await;
    let addr = handle.local_addr();

    // Send headers claiming a large body, then don't send the body
    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n",
    )
    .await
    .unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf)).await;
    let response = String::from_utf8_lossy(&buf);

    // Body timeout should return 408 or connection close
    assert!(
        response.starts_with("HTTP/1.1 408") || response.is_empty(),
        "expected 408 or connection close on body timeout, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn graceful_shutdown_waits_for_body_completion() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let body_completed = Arc::new(AtomicBool::new(false));
    let body_completed_clone = body_completed.clone();

    let (handle, _tmp) = start_server(
        config,
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
        move |req: Request| {
            let completed = body_completed_clone.clone();
            Box::pin(async move {
                let (_head, body) = req.into_head_and_body();
                let data = body.read_all().await.unwrap();
                completed.store(true, Ordering::Relaxed);
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(data.to_vec()))
                    .unwrap())
            })
        },
    )
    .await;
    let addr = handle.local_addr();

    // Send body and wait for response
    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    )
    .await
    .unwrap();
    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "expected 200, got: {}",
        response
    );

    // Body should have been consumed
    assert!(
        body_completed.load(Ordering::Relaxed),
        "body should be consumed before response"
    );

    // Graceful shutdown should complete cleanly
    handle.shutdown();
}

#[tokio::test]
async fn forced_shutdown_during_body_ingestion() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let handler_called = Arc::new(AtomicBool::new(false));
    let handler_called_clone = handler_called.clone();

    let (handle, _tmp) = start_server(
        config,
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
        move |req: Request| {
            let called = handler_called_clone.clone();
            Box::pin(async move {
                called.store(true, Ordering::Relaxed);
                // Slow handler
                tokio::time::sleep(Duration::from_secs(10)).await;
                let (_head, body) = req.into_head_and_body();
                let _ = body.read_all().await;
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"ok".to_vec()))
                    .unwrap())
            })
        },
    )
    .await;
    let addr = handle.local_addr();

    // Send a request
    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    )
    .await
    .unwrap();

    // Wait a bit for the handler to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Force shutdown while handler is running
    let result = handle.force_shutdown(Duration::from_secs(1)).await;
    assert!(
        matches!(
            result,
            Ok(eggserve_core::server::errors::ShutdownResult::Forced)
                | Ok(eggserve_core::server::errors::ShutdownResult::Timeout)
                | Err(_)
        ),
        "forced shutdown should complete, got: {:?}",
        result
    );
}

#[tokio::test]
async fn partial_body_read_then_shutdown() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let (handle, _tmp) = start_server(
        config,
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
        |req: Request| {
            Box::pin(async move {
                let (_head, mut body) = req.into_head_and_body();
                // Read only one chunk, don't consume the rest
                let _first = body.next_chunk().await;
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"partial".to_vec()))
                    .unwrap())
            })
        },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\nConnection: close\r\n\r\n",
    )
    .await
    .unwrap();
    conn.write_all(&[b'x'; 100]).await.unwrap();
    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "expected 200, got: {}",
        response
    );

    // Server should still be responsive after partial consumption
    let mut conn2 = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn2
        .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf2 = Vec::new();
    conn2.read_to_end(&mut buf2).await.unwrap();
    let response2 = String::from_utf8_lossy(&buf2);
    assert!(
        response2.starts_with("HTTP/1.1 200"),
        "server should be responsive after partial consumption, got: {}",
        response2
    );

    handle.shutdown();
}

#[tokio::test]
async fn repeated_requests_after_body_timeout() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_millis(100))
        .build()
        .unwrap();

    let request_count = Arc::new(AtomicUsize::new(0));
    let request_count_clone = request_count.clone();

    let (handle, _tmp) = start_server(
        config,
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
        move |_req: Request| {
            let count = request_count_clone.clone();
            Box::pin(async move {
                count.fetch_add(1, Ordering::Relaxed);
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"ok".to_vec()))
                    .unwrap())
            })
        },
    )
    .await;
    let addr = handle.local_addr();

    // First: body timeout
    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n",
    )
    .await
    .unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf)).await;

    // Second: normal request on new connection
    let mut conn2 = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn2.write_all(
        b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    )
    .await
    .unwrap();
    let mut buf2 = Vec::new();
    conn2.read_to_end(&mut buf2).await.unwrap();
    let response2 = String::from_utf8_lossy(&buf2);
    assert!(
        response2.starts_with("HTTP/1.1 200"),
        "second request should succeed, got: {}",
        response2
    );

    // Third: normal request on new connection
    let mut conn3 = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn3.write_all(
        b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc",
    )
    .await
    .unwrap();
    let mut buf3 = Vec::new();
    conn3.read_to_end(&mut buf3).await.unwrap();
    let response3 = String::from_utf8_lossy(&buf3);
    assert!(
        response3.starts_with("HTTP/1.1 200"),
        "third request should succeed, got: {}",
        response3
    );

    assert_eq!(
        request_count.load(Ordering::Relaxed),
        2,
        "handler should be called twice (not for timeout)"
    );

    handle.shutdown();
}
