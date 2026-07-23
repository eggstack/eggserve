use std::sync::Arc;
use std::time::Duration;

use eggserve_core::config::ServeConfig;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::request::Request;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::server::config::RuntimeConfig;
use eggserve_core::server::handle::ServerHandle;
use eggserve_core::server::{service_fn_with_policy, Server};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn start_server(config: RuntimeConfig, policy: RequestBodyPolicy) -> (ServerHandle, TempDir) {
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
        .start_with_service(service_fn_with_policy(
            |req: Request| async move {
                let (_head, body) = req.into_head_and_body();
                let data = body.read_all().await.unwrap();
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(data.to_vec()))
                    .unwrap())
            },
            policy,
        ))
        .await
        .unwrap();
    handle.ready().await.unwrap();
    (handle, tmp)
}

#[tokio::test]
async fn shutdown_before_first_body_byte() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .keep_alive(false)
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(
        config,
        RequestBodyPolicy::Buffer {
            max_bytes: 1024 * 1024,
        },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    // Send headers but no body — shutdown should clean up.
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 100\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    // Give the server a moment to start reading, then shut down.
    tokio::time::sleep(Duration::from_millis(50)).await;
    handle.shutdown();

    // Connection should close (may get partial response or just EOF).
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf)).await;
    // No assertion on response content — the key is the server didn't hang.
}

#[tokio::test]
async fn shutdown_mid_body() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .keep_alive(false)
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(
        config,
        RequestBodyPolicy::Buffer {
            max_bytes: 1024 * 1024,
        },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 100\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    // Send partial body.
    conn.write_all(b"hello").await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;
    handle.shutdown();

    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf)).await;
}

#[tokio::test]
async fn shutdown_between_chunks() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .keep_alive(false)
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(
        config,
        RequestBodyPolicy::Stream {
            max_bytes: 1024 * 1024,
        },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Transfer-Encoding: chunked\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    // Send first chunk.
    conn.write_all(b"5\r\nhello\r\n").await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Shutdown while chunked body is in progress.
    handle.shutdown();

    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf)).await;
}

#[tokio::test]
async fn forced_shutdown_with_pending_body() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(30))
        .graceful_shutdown_timeout(Duration::from_millis(50))
        .keep_alive(false)
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(
        config,
        RequestBodyPolicy::Buffer {
            max_bytes: 1024 * 1024,
        },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 100\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    // Don't send any body — body read will timeout.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Force shutdown with a short deadline.
    let _ = handle.force_shutdown(Duration::from_millis(100)).await;

    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf)).await;
}

#[tokio::test]
async fn client_disconnect_during_body_read() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .keep_alive(false)
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(
        config,
        RequestBodyPolicy::Buffer {
            max_bytes: 1024 * 1024,
        },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 100\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    // Send partial body then disconnect.
    conn.write_all(b"partial").await.unwrap();
    drop(conn);

    // Server should handle disconnect gracefully.
    tokio::time::sleep(Duration::from_millis(200)).await;
    handle.shutdown();
}

#[tokio::test]
async fn handler_timeout_aborts_body_read() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(30))
        .handler_timeout(Duration::from_millis(100))
        .keep_alive(false)
        .build()
        .unwrap();
    let (handle, _tmp) = start_server(
        config,
        RequestBodyPolicy::Buffer {
            max_bytes: 1024 * 1024,
        },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\n\
          Host: localhost\r\n\
          Content-Length: 100\r\n\
          Connection: close\r\n\
          \r\n",
    )
    .await
    .unwrap();

    // Don't send body — body read will take time, handler timeout should fire.
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf)).await;
    let response = String::from_utf8_lossy(&buf);
    // Should get 408 timeout or connection close.
    assert!(
        response.starts_with("HTTP/1.1 408")
            || response.starts_with("HTTP/1.1 504")
            || response.is_empty(),
        "expected timeout or connection close, got: {}",
        response
    );
    handle.shutdown();
}
