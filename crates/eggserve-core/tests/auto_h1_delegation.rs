//! Plan 249 Track D: compatibility `Auto` H1 reaches the direct authority.
//!
//! These tests drive ordinary cleartext connections through the normal
//! compatibility accept path (`ServerBuilder`, which enters
//! `WireProtocol::Auto`) and prove H1 is served. Combined with the topology
//! gate (which forbids any core Hyper H1 builder/connection/driver), passing
//! wire behavior through the normal path proves delegation to
//! `eggserve-server`: no second core H1 implementation exists to serve them.
//!
//! The control case drives the explicit public direct entry point over a
//! duplex pair, distinguishing "direct works" from "normal accept path
//! delegates". Composition cases cover prebound TCP, PROXY-prefixed
//! cleartext, and Unix-domain H1. TLS ALPN H1 delegates through the unchanged
//! explicit-`Http1` branch and stays covered by the existing TLS suites.

use std::sync::Arc;
use std::time::Duration;

use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::proxy::{IpPrefix, TrustedProxyConfig};
use eggserve_core::server::{service_fn, Request, RuntimeConfig, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn hello_service() -> impl eggserve_core::server::Service {
    service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap())
    })
}

fn echo_service() -> impl eggserve_core::server::Service {
    eggserve_core::server::service_fn_with_policy(
        |req: Request| async move {
            let (_head, body) = req.into_head_and_body();
            let bytes = body
                .read_all()
                .await
                .map_err(|e| eggserve_core::server::ServiceError::internal(e.to_string()))?;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(bytes.to_vec()))
                .unwrap())
        },
        eggserve_core::primitives::RequestBodyPolicy::Buffer {
            max_bytes: 1024 * 1024,
        },
    )
}

fn test_config() -> RuntimeConfig {
    RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024 * 1024)
        .build()
        .unwrap()
}

async fn tcp_get(addr: std::net::SocketAddr, request: &[u8]) -> String {
    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client.write_all(request).await.unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut buf))
        .await
        .expect("server must answer")
        .unwrap();
    String::from_utf8_lossy(&buf).into_owned()
}

/// Control: the explicit public direct entry point serves H1 over duplex.
#[tokio::test]
async fn explicit_direct_entry_serves_h1() {
    let (mut client, server_io) = tokio::io::duplex(64 * 1024);
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let config = Arc::new(eggserve_server::config::RuntimeConfig::default());
    let state = Arc::new(eggserve_server::runtime::RuntimeState::new(&config));
    let shutdown = eggserve_server::connection::ConnectionShutdown::new();
    let ctx = eggserve_server::connection::ConnectionContext::for_non_socket(
        eggserve_primitives::connection_info::Scheme::Http,
        None,
    );
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        eggserve_server::connection::serve_http1_connection(
            server_io,
            eggserve_server::service_fn(|_req| async {
                Ok(eggserve_primitives::canonical::Response::builder()
                    .status(eggserve_primitives::canonical::StatusCode::OK)
                    .body(eggserve_primitives::canonical::ResponseBody::Bytes(
                        b"hello".to_vec(),
                    ))
                    .unwrap())
            }),
            config,
            ctx,
            state,
            &shutdown,
        ),
    )
    .await
    .expect("direct driver must terminate");
    assert_eq!(outcome.to_string(), "normal");
    let mut raw = Vec::new();
    client.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
    assert!(text.contains("hello"));
}

/// Normal compatibility accept path serves cleartext H1 (Auto → direct).
#[tokio::test]
async fn normal_accept_path_serves_cleartext_h1() {
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(hello_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    let body = tcp_get(
        addr,
        b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(body.starts_with("HTTP/1.1 200 OK"), "{body}");
    assert!(body.contains("hello"));
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// Normal accept path echoes a buffered POST body (Auto → direct body path).
#[tokio::test]
async fn normal_accept_path_echoes_buffered_body() {
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(echo_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    let body = tcp_get(
        addr,
        b"POST /echo HTTP/1.1\r\nHost: h\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    )
    .await;
    assert!(body.starts_with("HTTP/1.1 200 OK"), "{body}");
    assert!(body.ends_with("hello"), "{body}");
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// Normal accept path answers HEAD with length and no body.
#[tokio::test]
async fn normal_accept_path_answers_head() {
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(hello_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    let body = tcp_get(
        addr,
        b"HEAD / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(body.starts_with("HTTP/1.1 200 OK"), "{body}");
    assert!(body.contains("content-length: 5"), "{body}");
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// Prebound TCP listener serves H1 through the same Auto path.
#[tokio::test]
async fn prebound_tcp_serves_h1() {
    let std_bound = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std_bound.set_nonblocking(true).unwrap();
    let tokio_listener = tokio::net::TcpListener::from_std(std_bound).unwrap();
    let expected = tokio_listener.local_addr().unwrap();
    let server = Server::builder()
        .runtime(test_config())
        .from_listener(tokio_listener)
        .build()
        .unwrap();
    let handle = server.start_with_service(hello_service()).await.unwrap();
    handle.ready().await.unwrap();
    assert_eq!(handle.local_addr(), expected);
    let body = tcp_get(
        expected,
        b"GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(body.starts_with("HTTP/1.1 200 OK"), "{body}");
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// PROXY-prefixed cleartext H1 replays into the direct driver.
#[tokio::test]
async fn proxy_prefixed_cleartext_serves_h1() {
    let mut trusted = TrustedProxyConfig::default();
    trusted.peers.push(IpPrefix::parse("127.0.0.1/32").unwrap());
    trusted.proxy_protocol.enabled = true;
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .trusted_proxy(trusted)
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(hello_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"PROXY TCP4 192.168.0.1 192.168.0.11 56324 443\r\n")
        .await
        .unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf))
        .await
        .expect("server must answer")
        .unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
    assert!(text.contains("hello"));
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// Unix-domain H1 serves through the same Auto path (Unix only).
#[cfg(unix)]
#[tokio::test]
async fn unix_domain_serves_h1() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auto-h1.sock");
    let unix_listener = tokio::net::UnixListener::bind(&path).unwrap();
    let server = Server::builder()
        .runtime(test_config())
        .from_unix_listener(unix_listener)
        .build()
        .unwrap();
    let handle = server.start_with_service(hello_service()).await.unwrap();
    handle.ready().await.unwrap();

    let mut client = tokio::net::UnixStream::connect(&path).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
    assert!(text.contains("hello"));
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// Many short sequential connections leave no per-connection shutdown
/// state behind: the active gauge returns to zero and shutdown drains.
#[tokio::test]
async fn repeated_short_connections_drain_cleanly() {
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(hello_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    for _ in 0..32 {
        let body = tcp_get(
            addr,
            b"GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(body.starts_with("HTTP/1.1 200 OK"), "{body}");
    }
    // Allow connection tasks to finish dropping their guards.
    for _ in 0..50 {
        if handle.ops_snapshot().active_connections == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        handle.ops_snapshot().active_connections,
        0,
        "active gauge must return to zero"
    );
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// Multiprotocol caller-owned entry resolves H1 bytes to H1 (http2 only).
///
/// `serve_http_connection` enters `WireProtocol::Auto`; H1 bytes must be
/// answered as H1 rather than misrouted to the H2 path.
#[cfg(feature = "http2")]
#[tokio::test]
async fn multiprotocol_entry_resolves_h1_bytes_to_h1() {
    use eggserve_core::server::connection::ConnectionContext;

    let (mut client, server_io) = tokio::io::duplex(64 * 1024);
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let config = Arc::new(RuntimeConfig::builder().build().unwrap());
    let state = Arc::new(eggserve_core::server::RuntimeState::new(&config));
    let shutdown = eggserve_core::server::connection::ConnectionShutdown::new();
    let ctx = ConnectionContext::for_non_socket(
        eggserve_core::primitives::connection_info::Scheme::Http,
        None,
    );
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        eggserve_core::server::connection::serve_http_connection(
            server_io,
            hello_service(),
            config,
            ctx,
            state,
            &shutdown,
        ),
    )
    .await
    .expect("multiprotocol driver must terminate");
    assert_eq!(outcome.to_string(), "normal");
    let mut raw = Vec::new();
    client.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
    assert!(text.contains("hello"));
}
