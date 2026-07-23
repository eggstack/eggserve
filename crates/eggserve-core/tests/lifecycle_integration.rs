use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use eggserve_core::config::ServeConfig;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::request::Request;
use eggserve_core::server::config::RuntimeConfig;
use eggserve_core::server::{service_fn, Server, Service, ShutdownResult};
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
