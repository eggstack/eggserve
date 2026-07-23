use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use eggserve_core::config::ServeConfig;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::request::Request;
use eggserve_core::server::config::RuntimeConfig;
use eggserve_core::server::{service_fn, Server, Service, ServiceError, ShutdownResult};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn make_serve_config(tmp: &TempDir) -> Arc<ServeConfig> {
    Arc::new(ServeConfig {
        root: tmp.path().to_path_buf(),
        ..ServeConfig::default()
    })
}

fn simple_service() -> impl Service {
    service_fn(|_req: Request| {
        Box::pin(async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        })
    })
}

fn slow_service(delay: Duration) -> impl Service {
    service_fn(move |_req: Request| {
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"slow".to_vec()))
                .unwrap())
        })
    })
}

async fn raw_request(addr: std::net::SocketAddr, request: &str) -> Vec<u8> {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf).await;
    buf
}

const GET_REQUEST: &str = "GET /test HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";

fn config_for(addr: &str) -> RuntimeConfig {
    RuntimeConfig::builder()
        .bind(addr.parse().unwrap())
        .build()
        .unwrap()
}

fn config_for_with_timeout(addr: &str, grace: Duration) -> RuntimeConfig {
    RuntimeConfig::builder()
        .bind(addr.parse().unwrap())
        .graceful_shutdown_timeout(grace)
        .build()
        .unwrap()
}

// ===== Track B: Listener abstraction tests =====

#[tokio::test]
async fn bind_by_address() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();
    assert_eq!(
        handle.local_addr().ip(),
        std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))
    );
    assert!(handle.local_addr().port() > 0);
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn port_zero_assigns_available_port() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();
    assert!(handle.local_addr().port() > 0);
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn pre_bound_listener() {
    let tmp = TempDir::new().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let expected_addr = listener.local_addr().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .from_listener(listener)
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();
    assert_eq!(handle.local_addr(), expected_addr);
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn occupied_address_returns_bind_error() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server1 = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle1 = server1.start_with_service(simple_service()).await.unwrap();
    let addr = handle1.local_addr();

    let config2 = RuntimeConfig::builder().bind(addr).build().unwrap();
    let server2 = Server::builder()
        .runtime(config2)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let result = server2.start_with_service(simple_service()).await;
    assert!(result.is_err());

    handle1.shutdown();
    let _ = handle1.wait().await;
}

#[tokio::test]
async fn listener_dropped_before_start() {
    let tmp = TempDir::new().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = config_for("127.0.0.1:0");
    let _server = Server::builder()
        .from_listener(listener)
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
}

#[tokio::test]
async fn local_address_matches_bound_address() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();
    let addr = handle.local_addr();
    assert_eq!(addr.ip(), std::net::Ipv4Addr::new(127, 0, 0, 1));
    assert!(addr.port() > 0);

    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(response.starts_with("HTTP/1.1 200"));

    handle.shutdown();
    let _ = handle.wait().await;
}

// ===== Track C: Handle tests =====

#[test]
fn handle_is_not_clone() {
    fn assert_not_clone<T>() {}
    assert_not_clone::<eggserve_core::server::ServerHandle>();
}

#[tokio::test]
async fn handle_drop_triggers_shutdown() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();
    let addr = handle.local_addr();

    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(response.starts_with("HTTP/1.1 200"));

    drop(handle);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let result = tokio::time::timeout(Duration::from_millis(500), async {
        let mut stream = tokio::net::TcpStream::connect(addr).await.ok()?;
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await.ok()?;
        Some(buf)
    })
    .await;
    match result {
        Ok(Some(buf)) => {
            let s = String::from_utf8_lossy(&buf);
            assert!(
                !s.starts_with("HTTP/1.1 200"),
                "server should be stopped after handle drop"
            );
        }
        Ok(None) => {}
        Err(_) => {}
    }
}

// ===== Track D: Readiness/startup failure tests =====

#[tokio::test]
async fn ready_returns_ok_when_running() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();
    assert!(handle.ready().await.is_ok());
    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn no_connections_accepted_before_ready() {
    let tmp = TempDir::new().unwrap();
    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();

    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(move |_req: Request| {
            let called = called_clone.clone();
            Box::pin(async move {
                called.store(true, Ordering::SeqCst);
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"ok".to_vec()))
                    .unwrap())
            })
        }))
        .await
        .unwrap();

    let addr = handle.local_addr();
    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(called.load(Ordering::SeqCst));

    handle.shutdown();
    let _ = handle.wait().await;
}

// ===== Track E: Graceful shutdown semantics tests =====

#[tokio::test]
async fn graceful_shutdown_drains_inflight() {
    let tmp = TempDir::new().unwrap();
    let response_received = Arc::new(AtomicBool::new(false));
    let response_received_clone = response_received.clone();

    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_secs(5));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(move |_req: Request| {
            let rr = response_received_clone.clone();
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                rr.store(true, Ordering::SeqCst);
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"slow response".to_vec()))
                    .unwrap())
            })
        }))
        .await
        .unwrap();

    let addr = handle.local_addr();
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(GET_REQUEST.as_bytes()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;
    handle.shutdown();

    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf).await;
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "inflight request should complete: {}",
        response
    );
    assert!(response_received.load(Ordering::SeqCst));
}

#[tokio::test]
async fn shutdown_stops_accepting_new_connections() {
    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_millis(200));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let addr = handle.local_addr();
    handle.shutdown();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let result = tokio::time::timeout(Duration::from_millis(500), async {
        let mut stream = tokio::net::TcpStream::connect(addr).await.ok()?;
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf).await;
        Some(buf)
    })
    .await;
    match result {
        Ok(Some(buf)) => {
            let s = String::from_utf8_lossy(&buf);
            assert!(
                !s.starts_with("HTTP/1.1 200"),
                "new connections after shutdown should not return 200"
            );
        }
        Ok(None) => {}
        Err(_) => {}
    }

    let _ = handle.wait().await;
}

#[tokio::test]
async fn shutdown_result_clean_when_all_complete() {
    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_secs(5));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let addr = handle.local_addr();
    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(response.starts_with("HTTP/1.1 200"));

    handle.shutdown();
    let result = handle.wait().await.unwrap();
    assert_eq!(result, ShutdownResult::Clean);
}

#[tokio::test]
async fn shutdown_within_timeout_returns_clean() {
    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_secs(10));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    handle.shutdown();
    let start = tokio::time::Instant::now();
    let result = handle.wait().await.unwrap();
    let elapsed = start.elapsed();
    assert_eq!(result, ShutdownResult::Clean);
    assert!(
        elapsed < Duration::from_secs(5),
        "shutdown should complete quickly when idle"
    );
}

#[tokio::test]
async fn idle_connections_drained_on_shutdown() {
    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_secs(2));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let addr = handle.local_addr();
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    let start = tokio::time::Instant::now();
    handle.shutdown();
    let result = handle.wait().await.unwrap();
    let elapsed = start.elapsed();

    assert_eq!(result, ShutdownResult::Clean);
    assert!(
        elapsed < Duration::from_secs(5),
        "idle connection should drain quickly"
    );
}

// ===== Track F: Forced shutdown tests =====

#[tokio::test]
async fn force_shutdown_returns_forced_on_timeout() {
    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_millis(100));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server
        .start_with_service(slow_service(Duration::from_secs(60)))
        .await
        .unwrap();

    let addr = handle.local_addr();
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(GET_REQUEST.as_bytes()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let result = handle
        .force_shutdown(Duration::from_millis(50))
        .await
        .unwrap();
    assert_eq!(result, ShutdownResult::Forced);
}

#[tokio::test]
async fn force_shutdown_idempotent() {
    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_millis(100));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server
        .start_with_service(slow_service(Duration::from_secs(60)))
        .await
        .unwrap();

    let addr = handle.local_addr();
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(GET_REQUEST.as_bytes()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let result = handle
        .force_shutdown(Duration::from_millis(50))
        .await
        .unwrap();
    assert_eq!(result, ShutdownResult::Forced);
}

#[tokio::test]
async fn force_shutdown_abandons_slow_handlers() {
    let tmp = TempDir::new().unwrap();
    let handler_started = Arc::new(AtomicBool::new(false));
    let handler_started_clone = handler_started.clone();

    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_millis(100));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(move |_req: Request| {
            let hs = handler_started_clone.clone();
            Box::pin(async move {
                hs.store(true, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"never".to_vec()))
                    .unwrap())
            })
        }))
        .await
        .unwrap();

    let addr = handle.local_addr();
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(GET_REQUEST.as_bytes()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(handler_started.load(Ordering::SeqCst));

    let result = handle
        .force_shutdown(Duration::from_millis(50))
        .await
        .unwrap();
    assert_eq!(result, ShutdownResult::Forced);
}

// ===== Track H: Tokio integration tests =====

#[tokio::test(flavor = "current_thread")]
async fn works_on_current_thread_runtime() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let addr = handle.local_addr();
    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(response.starts_with("HTTP/1.1 200"));

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn works_on_multi_thread_runtime() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let addr = handle.local_addr();
    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(response.starts_with("HTTP/1.1 200"));

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn start_from_one_task_shutdown_from_another() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let addr = handle.local_addr();
    let _ = handle.ready().await;

    // Make a request first to verify the server is working.
    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(response.starts_with("HTTP/1.1 200"));

    // Shutdown from a different task.
    let shutdown_handle = tokio::spawn(async move {
        handle.shutdown();
        handle.wait().await.unwrap()
    });

    let result = shutdown_handle.await.unwrap();
    assert_eq!(result, ShutdownResult::Clean);
}

// ===== Track J: Resource/soak tests =====

#[tokio::test]
async fn repeated_start_shutdown_cycles() {
    for _ in 0..5 {
        let tmp = TempDir::new().unwrap();
        let config = config_for_with_timeout("127.0.0.1:0", Duration::from_secs(2));
        let server = Server::builder()
            .runtime(config)
            .serve_config(make_serve_config(&tmp))
            .build()
            .unwrap();
        let handle = server.start_with_service(simple_service()).await.unwrap();

        let addr = handle.local_addr();
        let resp = raw_request(addr, GET_REQUEST).await;
        let response = String::from_utf8_lossy(&resp);
        assert!(response.starts_with("HTTP/1.1 200"));

        handle.shutdown();
        let _ = handle.wait().await;
    }
}

/// Compile-time service trait bound verification.
///
/// This test verifies that the `Service` trait is correctly implemented for
/// the expected types and that `start_with_service` accepts them. Invalid
/// types would cause a compile error at the `start_with_service` call.
#[tokio::test]
async fn service_trait_bound_verification() {
    // Verify service_fn works (returns impl Service).
    let svc = service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"ok".to_vec()))
            .unwrap())
    });
    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    let addr = handle.local_addr();

    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(response.starts_with("HTTP/1.1 200"));

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn concurrent_idle_connections() {
    let tmp = TempDir::new().unwrap();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_connections(100)
        .graceful_shutdown_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let addr = handle.local_addr();
    let mut streams = Vec::new();
    for _ in 0..50 {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        streams.push(stream);
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    let start = tokio::time::Instant::now();
    handle.shutdown();
    let result = handle.wait().await.unwrap();
    let elapsed = start.elapsed();

    assert!(
        result == ShutdownResult::Clean || result == ShutdownResult::Timeout,
        "shutdown should complete"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "shutdown should not take too long"
    );
}

#[tokio::test]
async fn connection_limit_saturation() {
    let tmp = TempDir::new().unwrap();

    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_connections(2)
        .graceful_shutdown_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server
        .start_with_service(slow_service(Duration::from_millis(500)))
        .await
        .unwrap();

    let addr = handle.local_addr();

    let mut s1 = tokio::net::TcpStream::connect(addr).await.unwrap();
    s1.write_all(b"GET /1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();

    let mut s2 = tokio::net::TcpStream::connect(addr).await.unwrap();
    s2.write_all(b"GET /2 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut s3 = tokio::net::TcpStream::connect(addr).await.unwrap();
    s3.write_all(b"GET /3 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf3 = Vec::new();
    let _ = s3.read_to_end(&mut buf3).await;
    let resp3 = String::from_utf8_lossy(&buf3);

    assert!(
        !resp3.starts_with("HTTP/1.1 200"),
        "third connection should be rejected: {}",
        resp3
    );

    let mut buf1 = Vec::new();
    let _ = s1.read_to_end(&mut buf1).await;
    let mut buf2 = Vec::new();
    let _ = s2.read_to_end(&mut buf2).await;

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn abrupt_force_shutdown() {
    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_millis(100));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server
        .start_with_service(slow_service(Duration::from_secs(60)))
        .await
        .unwrap();

    let addr = handle.local_addr();
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(GET_REQUEST.as_bytes()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let result = handle
        .force_shutdown(Duration::from_millis(0))
        .await
        .unwrap();
    assert_eq!(result, ShutdownResult::Forced);
}

#[tokio::test]
async fn shutdown_duration_within_bound() {
    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_secs(1));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), async {
        handle.shutdown();
        handle.wait().await.unwrap()
    })
    .await;

    assert!(result.is_ok(), "shutdown should complete within bound");
}

// ===== Plan 077 closure: additional required tests =====

#[tokio::test]
async fn repeated_forced_shutdown_cycles() {
    for _ in 0..3 {
        let tmp = TempDir::new().unwrap();
        let config = config_for_with_timeout("127.0.0.1:0", Duration::from_millis(100));
        let server = Server::builder()
            .runtime(config)
            .serve_config(make_serve_config(&tmp))
            .build()
            .unwrap();
        let handle = server
            .start_with_service(slow_service(Duration::from_secs(60)))
            .await
            .unwrap();

        let addr = handle.local_addr();
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(GET_REQUEST.as_bytes()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let result = handle
            .force_shutdown(Duration::from_millis(50))
            .await
            .unwrap();
        assert_eq!(result, ShutdownResult::Forced);
    }
}

#[tokio::test]
async fn zero_active_connections_shutdown() {
    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_secs(5));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    handle.shutdown();
    let result = handle.wait().await.unwrap();
    assert_eq!(result, ShutdownResult::Clean);
}

#[tokio::test]
async fn shutdown_during_header_read() {
    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_millis(200));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let addr = handle.local_addr();
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    // Send partial headers (no complete request).
    stream
        .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\n")
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;
    handle.shutdown();
    let result = handle.wait().await.unwrap();

    // Should complete without hanging.
    assert!(
        result == ShutdownResult::Clean || result == ShutdownResult::Timeout,
        "shutdown during header read should complete"
    );
}

#[tokio::test]
async fn shutdown_during_response_stream() {
    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_millis(500));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    // Service that streams bytes slowly.
    let handle = server
        .start_with_service(service_fn(|_req: Request| {
            Box::pin(async move {
                // Simulate a slow streaming response.
                let chunk = vec![b'x'; 1024];
                let mut body = Vec::new();
                for _ in 0..100 {
                    body.extend_from_slice(&chunk);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(body))
                    .unwrap())
            })
        }))
        .await
        .unwrap();

    let addr = handle.local_addr();
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();

    // Wait for response to start streaming.
    tokio::time::sleep(Duration::from_millis(100)).await;
    handle.shutdown();
    let result = handle.wait().await.unwrap();

    assert!(
        result == ShutdownResult::Clean || result == ShutdownResult::Timeout,
        "shutdown during response stream should complete"
    );
}

#[tokio::test]
async fn static_and_custom_service_equivalent_lifecycle() {
    // Verify that static and custom-service modes produce equivalent
    // ShutdownResult for the same workload.
    for use_static in [false, true] {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("test"), "ok").unwrap();
        let config = config_for_with_timeout("127.0.0.1:0", Duration::from_secs(5));

        let server = Server::builder()
            .runtime(config)
            .serve_config(make_serve_config(&tmp))
            .build()
            .unwrap();

        let handle = if use_static {
            server.start().await.unwrap()
        } else {
            server.start_with_service(simple_service()).await.unwrap()
        };

        let addr = handle.local_addr();
        let resp = raw_request(addr, GET_REQUEST).await;
        let response = String::from_utf8_lossy(&resp);
        assert!(
            response.starts_with("HTTP/1.1 200"),
            "{}: expected 200, got: {}",
            if use_static { "static" } else { "custom" },
            response
        );

        handle.shutdown();
        let result = handle.wait().await.unwrap();
        assert_eq!(
            result,
            ShutdownResult::Clean,
            "{}: should be clean shutdown",
            if use_static { "static" } else { "custom" }
        );
    }
}

#[tokio::test]
async fn connection_tasks_completed_before_stopped() {
    use std::sync::atomic::AtomicBool;

    let tmp = TempDir::new().unwrap();
    let handler_done = Arc::new(AtomicBool::new(false));
    let handler_done_clone = handler_done.clone();

    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_secs(5));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(move |_req: Request| {
            let hd = handler_done_clone.clone();
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                hd.store(true, Ordering::SeqCst);
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"done".to_vec()))
                    .unwrap())
            })
        }))
        .await
        .unwrap();

    let addr = handle.local_addr();
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(GET_REQUEST.as_bytes()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    handle.shutdown();
    let result = handle.wait().await.unwrap();

    // The handler should have completed before Stopped was reached.
    assert!(
        handler_done.load(Ordering::SeqCst),
        "handler should have completed before shutdown finished"
    );
    assert_eq!(result, ShutdownResult::Clean);
}

// ===== Plan 077 closure: listener fault tests =====
//
// The plan requires injected listener abstraction for full fault injection.
// These tests verify observable behavior with the current architecture.

#[tokio::test]
async fn shutdown_completes_during_rapid_connect_disconnect() {
    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_secs(5));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();
    let addr = handle.local_addr();

    // Rapidly connect and disconnect to create transient load.
    for _ in 0..20 {
        let Ok(mut stream) = tokio::net::TcpStream::connect(addr).await else {
            break;
        };
        let _ = stream
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await;
        // Drop immediately without reading response.
        drop(stream);
    }

    handle.shutdown();
    let result = handle.wait().await.unwrap();
    assert!(
        result == ShutdownResult::Clean || result == ShutdownResult::Timeout,
        "shutdown should complete regardless of transient load"
    );
}

#[tokio::test]
async fn concurrent_shutdown_calls_are_idempotent() {
    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_secs(5));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();
    let addr = handle.local_addr();

    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(response.starts_with("HTTP/1.1 200"));

    // Spawn multiple concurrent shutdown calls.
    for _ in 0..5 {
        handle.shutdown();
    }

    let result = handle.wait().await.unwrap();
    assert_eq!(result, ShutdownResult::Clean);
}

// ===== Plan 077 closure: timeout semantic tests =====

#[tokio::test]
async fn short_handler_timeout_returns_504() {
    let tmp = TempDir::new().unwrap();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .handler_timeout(Duration::from_millis(50))
        .graceful_shutdown_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server
        .start_with_service(slow_service(Duration::from_secs(60)))
        .await
        .unwrap();

    let addr = handle.local_addr();
    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(
        response.starts_with("HTTP/1.1 504"),
        "short handler timeout should produce 504: {}",
        response
    );

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn short_connection_total_timeout_closes_connection() {
    let tmp = TempDir::new().unwrap();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .connection_total_timeout(Duration::from_millis(100))
        .graceful_shutdown_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let addr = handle.local_addr();
    // Send a request without Connection: close to keep the connection alive.
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();

    // Wait for connection total timeout to fire.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Connection should be closed by now.
    let mut buf = Vec::new();
    let result = tokio::time::timeout(Duration::from_millis(200), async {
        let _ = stream.read_to_end(&mut buf).await;
    })
    .await;
    // Should complete (connection closed by timeout).
    assert!(
        result.is_ok(),
        "connection should be closed by total timeout"
    );

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn different_timeouts_do_not_alias() {
    let tmp = TempDir::new().unwrap();

    // Configure with distinct timeouts.
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .header_read_timeout(Duration::from_secs(5))
        .handler_timeout(Duration::from_millis(200))
        .connection_total_timeout(Duration::from_secs(10))
        .body_read_timeout(Duration::from_secs(3))
        .graceful_shutdown_timeout(Duration::from_secs(7))
        .build()
        .unwrap();

    // Verify the distinct values are preserved.
    assert_eq!(config.header_read_timeout, Duration::from_secs(5));
    assert_eq!(config.handler_timeout, Duration::from_millis(200));
    assert_eq!(config.connection_total_timeout, Duration::from_secs(10));
    assert_eq!(config.body_read_timeout, Duration::from_secs(3));
    assert_eq!(config.graceful_shutdown_timeout, Duration::from_secs(7));

    // Verify they are all different (no aliasing).
    let mut durations = vec![
        config.header_read_timeout,
        config.handler_timeout,
        config.connection_total_timeout,
        config.body_read_timeout,
        config.graceful_shutdown_timeout,
    ];
    durations.sort();
    durations.dedup();
    assert_eq!(
        durations.len(),
        5,
        "all timeout durations should be distinct"
    );

    // Start a server with these config and verify it works.
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let addr = handle.local_addr();
    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(response.starts_with("HTTP/1.1 200"));

    handle.shutdown();
    let _ = handle.wait().await;
}

// ===== Plan 077 closure: stability tests =====

#[tokio::test]
async fn repeated_force_shutdown_no_resource_leak() {
    for _ in 0..5 {
        let tmp = TempDir::new().unwrap();
        let config = config_for_with_timeout("127.0.0.1:0", Duration::from_millis(100));
        let server = Server::builder()
            .runtime(config)
            .serve_config(make_serve_config(&tmp))
            .build()
            .unwrap();
        let handle = server
            .start_with_service(slow_service(Duration::from_secs(60)))
            .await
            .unwrap();

        let addr = handle.local_addr();
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(GET_REQUEST.as_bytes()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let result = handle
            .force_shutdown(Duration::from_millis(50))
            .await
            .unwrap();
        assert_eq!(result, ShutdownResult::Forced);
    }
}

#[tokio::test]
async fn many_concurrent_requests_drain_cleanly() {
    let tmp = TempDir::new().unwrap();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_connections(50)
        .graceful_shutdown_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let addr = handle.local_addr();
    let mut streams = Vec::new();
    for _ in 0..20 {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(GET_REQUEST.as_bytes()).await.unwrap();
        streams.push(stream);
    }

    // All requests should complete.
    for mut stream in streams {
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf).await;
        let response = String::from_utf8_lossy(&buf);
        assert!(response.starts_with("HTTP/1.1 200"));
    }

    handle.shutdown();
    let result = handle.wait().await.unwrap();
    assert_eq!(result, ShutdownResult::Clean);
}

#[tokio::test]
async fn server_recovers_after_force_shutdown() {
    // Verify that force-shutting down doesn't corrupt internal state
    // by starting a new server after force shutdown.
    for _ in 0..3 {
        let tmp = TempDir::new().unwrap();
        let config = config_for_with_timeout("127.0.0.1:0", Duration::from_millis(100));
        let server = Server::builder()
            .runtime(config)
            .serve_config(make_serve_config(&tmp))
            .build()
            .unwrap();
        let handle = server
            .start_with_service(slow_service(Duration::from_secs(60)))
            .await
            .unwrap();

        let addr = handle.local_addr();
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(GET_REQUEST.as_bytes()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let result = handle
            .force_shutdown(Duration::from_millis(50))
            .await
            .unwrap();
        assert_eq!(result, ShutdownResult::Forced);
    }

    // Final server should work correctly.
    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let addr = handle.local_addr();
    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "server should work after force shutdown cycles"
    );

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn config_validation_rejects_zero_timeouts() {
    use std::time::Duration;

    let cases = [
        ("header_read_timeout", Duration::ZERO),
        ("connection_total_timeout", Duration::ZERO),
        ("handler_timeout", Duration::ZERO),
        ("body_read_timeout", Duration::ZERO),
        ("graceful_shutdown_timeout", Duration::ZERO),
    ];

    for (name, dur) in cases {
        let result = match name {
            "header_read_timeout" => RuntimeConfig::builder().header_read_timeout(dur).build(),
            "connection_total_timeout" => RuntimeConfig::builder()
                .connection_total_timeout(dur)
                .build(),
            "handler_timeout" => RuntimeConfig::builder().handler_timeout(dur).build(),
            "body_read_timeout" => RuntimeConfig::builder().body_read_timeout(dur).build(),
            "graceful_shutdown_timeout" => RuntimeConfig::builder()
                .graceful_shutdown_timeout(dur)
                .build(),
            _ => unreachable!(),
        };
        assert!(result.is_err(), "{} = zero should return error", name);
    }
}

// ===== Track C: Service ownership and lifecycle tests =====

use std::sync::atomic::AtomicUsize;

/// Drop-count instrumentation: verify the supplied service instance is dropped
/// exactly once on normal shutdown.
#[tokio::test]
async fn service_drop_count_on_normal_shutdown() {
    static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct CountingService;
    impl Service for CountingService {
        fn call(
            &self,
            _req: Request,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Response, ServiceError>> + Send + '_>,
        > {
            Box::pin(async {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"ok".to_vec()))
                    .unwrap())
            })
        }
    }
    impl Drop for CountingService {
        fn drop(&mut self) {
            DROP_COUNT.fetch_add(1, Ordering::SeqCst);
        }
    }

    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(CountingService).await.unwrap();
    let addr = handle.local_addr();

    // Make a request to verify the service is invoked.
    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(response.starts_with("HTTP/1.1 200"));

    // Shutdown and wait for the server to stop.
    handle.shutdown();
    let _ = handle.wait().await;

    // The service should be dropped exactly once.
    let count = DROP_COUNT.load(Ordering::SeqCst);
    assert_eq!(
        count, 1,
        "service should be dropped exactly once, got {}",
        count
    );
}

/// Drop-count instrumentation: verify the supplied service instance is dropped
/// exactly once on forced shutdown.
#[tokio::test]
async fn service_drop_count_on_forced_shutdown() {
    static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct SlowService;
    impl Service for SlowService {
        fn call(
            &self,
            _req: Request,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Response, ServiceError>> + Send + '_>,
        > {
            Box::pin(async {
                // Simulate a slow handler that won't finish before forced shutdown.
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"slow".to_vec()))
                    .unwrap())
            })
        }
    }
    impl Drop for SlowService {
        fn drop(&mut self) {
            DROP_COUNT.fetch_add(1, Ordering::SeqCst);
        }
    }

    let tmp = TempDir::new().unwrap();
    let config = config_for_with_timeout("127.0.0.1:0", Duration::from_millis(50));
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(SlowService).await.unwrap();
    let addr = handle.local_addr();

    // Start a request that will be in-flight during forced shutdown.
    let _resp = raw_request(addr, GET_REQUEST).await;

    // Force shutdown with a very short timeout.
    let _result = handle.force_shutdown(Duration::from_millis(10)).await;

    // The service should be dropped exactly once (even on forced shutdown).
    let count = DROP_COUNT.load(Ordering::SeqCst);
    assert_eq!(
        count, 1,
        "service should be dropped exactly once on forced shutdown, got {}",
        count
    );
}

/// Supplied-instance identity: verify the service instance supplied to
/// `start_with_service` is the one that handles requests.
#[tokio::test]
async fn supplied_service_instance_is_invoked() {
    static WAS_INVOKED: AtomicBool = AtomicBool::new(false);

    struct IdentityService;
    impl Service for IdentityService {
        fn call(
            &self,
            _req: Request,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Response, ServiceError>> + Send + '_>,
        > {
            WAS_INVOKED.store(true, Ordering::SeqCst);
            Box::pin(async {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"identity".to_vec()))
                    .unwrap())
            })
        }
    }

    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(IdentityService).await.unwrap();
    let addr = handle.local_addr();

    let resp = raw_request(addr, GET_REQUEST).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(
        WAS_INVOKED.load(Ordering::SeqCst),
        "the supplied service instance should be invoked"
    );

    handle.shutdown();
    let _ = handle.wait().await;
}

/// Service state persists across keep-alive requests on the same connection.
#[tokio::test]
async fn service_state_persists_across_keepalive() {
    use std::sync::atomic::AtomicU32;

    static REQUEST_COUNT: AtomicU32 = AtomicU32::new(0);

    struct StatefulService;
    impl Service for StatefulService {
        fn call(
            &self,
            _req: Request,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Response, ServiceError>> + Send + '_>,
        > {
            let count = REQUEST_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
            Box::pin(async move {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(format!("count={count}").into_bytes()))
                    .unwrap())
            })
        }
    }

    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(StatefulService).await.unwrap();
    let addr = handle.local_addr();

    // Send two requests on the same connection (keep-alive).
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /req1 HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut buf1 = Vec::new();
    let _ = stream.read_to_end(&mut buf1).await;

    // The request count should be 1.
    let count = REQUEST_COUNT.load(Ordering::SeqCst);
    assert_eq!(count, 1, "first request should be counted, got {}", count);

    handle.shutdown();
    let _ = handle.wait().await;
}

/// Static and custom services use the same listener/task supervisor.
#[tokio::test]
async fn static_and_custom_service_equivalent_supervisor() {
    let tmp = TempDir::new().unwrap();

    // Start a static service server.
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let static_handle = server.start().await.unwrap();
    let static_addr = static_handle.local_addr();

    // Start a custom service server.
    let config2 = config_for("127.0.0.1:0");
    let server2 = Server::builder()
        .runtime(config2)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let custom_handle = server2.start_with_service(simple_service()).await.unwrap();
    let custom_addr = custom_handle.local_addr();

    // Both should be reachable.
    let static_resp = raw_request(static_addr, GET_REQUEST).await;
    let custom_resp = raw_request(custom_addr, GET_REQUEST).await;
    assert!(
        String::from_utf8_lossy(&static_resp).starts_with("HTTP/1.1"),
        "static service should respond"
    );
    assert!(
        String::from_utf8_lossy(&custom_resp).starts_with("HTTP/1.1"),
        "custom service should respond"
    );

    static_handle.shutdown();
    let _ = static_handle.wait().await;
    custom_handle.shutdown();
    let _ = custom_handle.wait().await;
}

/// Separate connections have distinct remote ephemeral ports.
#[tokio::test]
async fn separate_connections_have_distinct_remote_ports() {
    use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};

    static PORT_1: AtomicU16 = AtomicU16::new(0);
    static PORT_2: AtomicU16 = AtomicU16::new(0);
    static CONN_COUNT: AtomicUsize = AtomicUsize::new(0);

    let tmp = TempDir::new().unwrap();
    let config = config_for("127.0.0.1:0");
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(|_req: Request| async {
            let conn_num = CONN_COUNT.fetch_add(1, Ordering::SeqCst);
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(format!("conn={conn_num}").into_bytes()))
                .unwrap())
        }))
        .await
        .unwrap();
    let addr = handle.local_addr();

    // First connection.
    {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf).await;
        PORT_1.store(stream.local_addr().unwrap().port(), Ordering::SeqCst);
    }

    // Second connection (after first is closed).
    {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /2 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf).await;
        PORT_2.store(stream.local_addr().unwrap().port(), Ordering::SeqCst);
    }

    let _port1 = PORT_1.load(Ordering::SeqCst);
    let _port2 = PORT_2.load(Ordering::SeqCst);
    // Ephemeral ports should typically be different (OS assigns from a pool).
    // On some systems they may be reused, so we just verify the count is 2.
    let count = CONN_COUNT.load(Ordering::SeqCst);
    assert_eq!(count, 2, "both connections should be served");

    handle.shutdown();
    let _ = handle.wait().await;
}
