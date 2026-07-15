use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use eggserve_core::config::{ServeConfig, ServeState};
use eggserve_core::primitives::canonical::{
    normalize_response, NormalizeRequest, Response, ResponseBody, StatusCode,
};
use eggserve_core::primitives::request::Request;
use eggserve_core::server::config::RuntimeConfig;
use eggserve_core::server::connection::serve_connection_with_service;
use eggserve_core::server::{service_fn, Server};
use hyper_util::rt::TokioIo;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast;

fn build_state(tmp: &TempDir) -> Arc<ServeState> {
    let config = Arc::new(ServeConfig {
        root: tmp.path().to_path_buf(),
        ..ServeConfig::default()
    });
    Arc::new(ServeState::new(config))
}

#[tokio::test]
async fn panic_in_service_returns_500() {
    let tmp = TempDir::new().unwrap();
    let state = build_state(&tmp);
    let config = RuntimeConfig::default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, _rx) = broadcast::channel::<()>(1);

    let state_clone = state.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let io = TokioIo::new(stream);
        let mut shutdown_rx = tx.subscribe();
        let svc = service_fn(|_req: Request| async {
            panic!("intentional panic");
        });
        serve_connection_with_service(io, svc, &config, &state_clone, &mut shutdown_rx).await;
    });

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let _ = client.read_to_end(&mut buf).await;
    let _ = server.await;

    // When a service panics, the connection task panics and the TCP
    // connection is dropped. The client sees a connection reset with no
    // complete HTTP response.
    let response = String::from_utf8_lossy(&buf);
    assert!(
        !response.starts_with("HTTP/1.1 200"),
        "service panic should not produce 200: {}",
        response
    );
}

#[tokio::test]
async fn slow_handler_returns_504() {
    let tmp = TempDir::new().unwrap();
    let state = build_state(&tmp);
    let config = RuntimeConfig::builder()
        .handler_timeout(Duration::from_millis(50))
        .build();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, _rx) = broadcast::channel::<()>(1);

    let state_clone = state.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let io = TokioIo::new(stream);
        let mut shutdown_rx = tx.subscribe();
        let svc = service_fn(|_req: Request| async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        });
        serve_connection_with_service(io, svc, &config, &state_clone, &mut shutdown_rx).await;
    });

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).await.unwrap();
    let _ = server.await;

    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 504"),
        "expected 504, got: {}",
        response
    );
}

#[tokio::test]
async fn malformed_request_rejected_before_service() {
    let tmp = TempDir::new().unwrap();
    let state = build_state(&tmp);
    let config = RuntimeConfig::default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, _rx) = broadcast::channel::<()>(1);

    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();

    let state_clone = state.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let io = TokioIo::new(stream);
        let mut shutdown_rx = tx.subscribe();
        let svc = service_fn(move |_req: Request| {
            let called = called_clone.clone();
            async move {
                called.store(true, Ordering::SeqCst);
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Empty)
                    .unwrap())
            }
        });
        serve_connection_with_service(io, svc, &config, &state_clone, &mut shutdown_rx).await;
    });

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"NOT A VALID HTTP REQUEST\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let _ = client.read_to_end(&mut buf).await;
    let _ = server.await;

    assert!(
        !called.load(Ordering::SeqCst),
        "service should not be called for malformed requests"
    );
}

#[tokio::test]
async fn custom_service_bytes_through_pipeline() {
    let tmp = TempDir::new().unwrap();
    let state = build_state(&tmp);
    let config = RuntimeConfig::default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, _rx) = broadcast::channel::<()>(1);

    let state_clone = state.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let io = TokioIo::new(stream);
        let mut shutdown_rx = tx.subscribe();
        let svc = service_fn(|_req: Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"hello".to_vec()))
                .unwrap())
        });
        serve_connection_with_service(io, svc, &config, &state_clone, &mut shutdown_rx).await;
    });

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).await.unwrap();
    let _ = server.await;

    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "expected 200, got: {}",
        response
    );
    assert!(
        response.contains("hello"),
        "response body should contain 'hello': {}",
        response
    );
}

#[tokio::test]
async fn connection_permits_released() {
    let tmp = TempDir::new().unwrap();
    let config = RuntimeConfig::builder()
        .max_connections(1)
        .handler_timeout(Duration::from_secs(10))
        .build();

    let server = Server::builder()
        .runtime(config)
        .serve_config(Arc::new(ServeConfig {
            root: tmp.path().to_path_buf(),
            ..ServeConfig::default()
        }))
        .build()
        .unwrap();

    let handle = server
        .start_with_service(service_fn(|_req: Request| async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"done".to_vec()))
                .unwrap())
        }))
        .await
        .unwrap();

    let addr = handle.local_addr();

    // First connection holds the single permit.
    let mut conn1 = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn1
        .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();

    // Give the server time to accept the first connection and acquire the permit.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Second connection should be dropped by the accept loop (permit exhausted).
    let _result = tokio::time::timeout(Duration::from_millis(500), async {
        let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut buf = Vec::new();
        conn.read_to_end(&mut buf).await.unwrap();
        buf
    })
    .await;

    // Read the first connection's response.
    let mut buf1 = Vec::new();
    let _ = conn1.read_to_end(&mut buf1).await;

    // Wait for the first connection to complete and release the permit.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Now a new connection should succeed.
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
        "second connection after permit release should succeed: {}",
        response2
    );

    handle.shutdown();
}

#[tokio::test]
async fn hop_by_hop_headers_stripped() {
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("connection", "close")
        .unwrap()
        .header("upgrade", "websocket")
        .unwrap()
        .header("x-custom", "preserved")
        .unwrap()
        .body(ResponseBody::Bytes(b"test".to_vec()))
        .unwrap();

    let req = NormalizeRequest::new(false);
    let normalized = normalize_response(response, &req).unwrap();

    let headers = normalized.headers();
    let header_names: Vec<&str> = headers.iter().map(|f| f.name.as_str()).collect();

    assert!(
        !header_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case("connection")),
        "hop-by-hop header 'Connection' should be stripped"
    );
    assert!(
        !header_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case("upgrade")),
        "hop-by-hop header 'Upgrade' should be stripped"
    );
    assert!(
        header_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case("x-custom")),
        "non-hop-by-hop header 'X-Custom' should be preserved"
    );
}
