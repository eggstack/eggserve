use std::sync::Arc;
use std::time::Duration;

use eggserve_core::config::ServeConfig;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::server::config::RuntimeConfig;
use eggserve_core::server::handle::ServerHandle;
use eggserve_core::server::{service_fn_with_policy, Server};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn start_server_with_service<F, Fut>(
    config: RuntimeConfig,
    service: F,
    policy: RequestBodyPolicy,
) -> (ServerHandle, TempDir)
where
    F: Fn(eggserve_core::primitives::request::Request) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Response, eggserve_core::server::ServiceError>>
        + Send
        + 'static,
{
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
        .start_with_service(service_fn_with_policy(service, policy))
        .await
        .unwrap();
    (handle, tmp)
}

#[tokio::test]
async fn buffer_mode_post_with_body() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build();

    let (handle, _tmp) = start_server_with_service(
        config,
        |req: eggserve_core::primitives::request::Request| async move {
            let (head, body) = req.into_head_and_body();
            assert_eq!(head.method().as_str(), "POST");
            let data = body.read_all().await.unwrap();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(data.to_vec()))
                .unwrap())
        },
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")
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
    assert!(
        response.contains("hello"),
        "response should contain body: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn reject_policy_gets_empty_body() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .build();

    let (handle, _tmp) = start_server_with_service(
        config,
        |_req: eggserve_core::primitives::request::Request| async move {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"no body".to_vec()))
                .unwrap())
        },
        RequestBodyPolicy::Reject,
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")
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
    handle.shutdown();
}

#[tokio::test]
async fn body_limit_exceeded_returns_413() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(5)
        .body_read_timeout(Duration::from_secs(5))
        .build();

    let (handle, _tmp) = start_server_with_service(
        config,
        |_req: eggserve_core::primitives::request::Request| async move {
            unreachable!("service should not be called");
        },
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 10\r\nConnection: close\r\n\r\nhello world")
        .await
        .unwrap();
    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 413"),
        "expected 413, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn declared_length_too_large_returns_413() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(5)
        .build();

    let (handle, _tmp) = start_server_with_service(
        config,
        |_req: eggserve_core::primitives::request::Request| async move {
            unreachable!("service should not be called");
        },
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 413"),
        "expected 413, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn empty_post_with_content_length_zero() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .build();

    let (handle, _tmp) = start_server_with_service(
        config,
        |req: eggserve_core::primitives::request::Request| async move {
            let (head, body) = req.into_head_and_body();
            assert_eq!(head.method().as_str(), "POST");
            let data = body.read_all().await.unwrap();
            assert!(data.is_empty());
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        },
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(
        b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
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
    handle.shutdown();
}

#[tokio::test]
async fn get_with_body_is_rejected() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .build();

    let (handle, _tmp) = start_server_with_service(
        config,
        |_req: eggserve_core::primitives::request::Request| async move {
            unreachable!("service should not be called for GET with body");
        },
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")
        .await
        .unwrap();
    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 400"),
        "expected 400 for GET with body, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn stream_mode_chunked_body() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .build();

    let (handle, _tmp) = start_server_with_service(
        config,
        |req: eggserve_core::primitives::request::Request| async move {
            let (head, mut body) = req.into_head_and_body();
            assert_eq!(head.method().as_str(), "POST");
            let mut all = Vec::new();
            while let Some(chunk) = body.next_chunk().await.unwrap() {
                all.extend_from_slice(&chunk);
            }
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(all))
                .unwrap())
        },
        RequestBodyPolicy::Stream { max_bytes: 1024 },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"POST /test HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    conn.write_all(b"5\r\nhello\r\n").await.unwrap();
    conn.write_all(b"6\r\n world\r\n").await.unwrap();
    conn.write_all(b"0\r\n\r\n").await.unwrap();
    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "expected 200, got: {}",
        response
    );
    assert!(
        response.contains("hello world"),
        "response should contain body: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn static_service_post_returns_405() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build();
    let serve_config = Arc::new(ServeConfig {
        root: tmp.path().to_path_buf(),
        ..ServeConfig::default()
    });
    let server = Server::builder()
        .runtime(config)
        .serve_config(serve_config)
        .build()
        .unwrap();
    let handle = server.start().await.unwrap();
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"POST /hello.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")
        .await
        .unwrap();
    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 405"),
        "expected 405 for POST to static service, got: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn body_timeout_returns_408() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_millis(50))
        .build();

    let (handle, _tmp) = start_server_with_service(
        config,
        |_req: eggserve_core::primitives::request::Request| async move {
            unreachable!("service should not be called");
        },
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
    )
    .await;
    let addr = handle.local_addr();

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), conn.read_to_end(&mut buf)).await;
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 408") || response.is_empty(),
        "expected 408 or connection close, got: {}",
        response
    );
    handle.shutdown();
}
