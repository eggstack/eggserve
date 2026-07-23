#![cfg(feature = "tls")]

use std::sync::Arc;
use std::time::Duration;

use eggserve_core::config::ServeConfig;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::server::config::RuntimeConfig;
use eggserve_core::server::{service_fn, Server, Service, ShutdownResult};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

use rustls::pki_types::PrivatePkcs8KeyDer;

fn make_tls_context() -> TlsContext {
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

fn make_serve_config(tmp: &TempDir) -> Arc<ServeConfig> {
    Arc::new(ServeConfig {
        root: tmp.path().to_path_buf(),
        ..ServeConfig::default()
    })
}

fn simple_service() -> impl Service {
    service_fn(|_req: RequestHead| {
        Box::pin(async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        })
    })
}

fn slow_service(delay: Duration) -> impl Service {
    service_fn(move |_req: RequestHead| {
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"slow".to_vec()))
                .unwrap())
        })
    })
}

const GET_REQUEST: &str = "GET /test HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";

fn config_for_tls(addr: &str, ctx: &TlsContext) -> RuntimeConfig {
    RuntimeConfig::builder()
        .bind(addr.parse().unwrap())
        .tls_config(ctx.server_config.clone())
        .build()
        .unwrap()
}

async fn raw_request(addr: std::net::SocketAddr, request: &str) -> Vec<u8> {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf).await;
    buf
}

async fn raw_tls_request(addr: std::net::SocketAddr, request: &str, ctx: &TlsContext) -> Vec<u8> {
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(ctx.client_config.clone());
    let domain = "localhost".try_into().unwrap();
    let tls_stream = connector.connect(domain, tcp).await.unwrap();
    let (mut reader, mut writer) = tokio::io::split(tls_stream);
    writer.write_all(request.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = reader.read_to_end(&mut buf).await;
    buf
}

#[tokio::test]
async fn custom_service_fn_over_plaintext() {
    let tmp = TempDir::new().unwrap();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
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
        "plaintext: {}",
        response
    );

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn custom_service_fn_over_tls() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_tls_context();
    let config = config_for_tls("127.0.0.1:0", &ctx);
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();

    let addr = handle.local_addr();
    let resp = raw_tls_request(addr, GET_REQUEST, &ctx).await;
    let response = String::from_utf8_lossy(&resp);
    assert!(response.starts_with("HTTP/1.1 200"), "TLS: {}", response);

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn static_service_over_plaintext() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();

    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start().await.unwrap();

    let addr = handle.local_addr();
    let resp = raw_request(
        addr,
        "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    let response = String::from_utf8_lossy(&resp);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "plaintext static: {}",
        response
    );
    assert!(response.contains("hello world"), "body: {}", response);

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn static_service_over_tls() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();

    let ctx = make_tls_context();
    let config = config_for_tls("127.0.0.1:0", &ctx);
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start().await.unwrap();

    let addr = handle.local_addr();
    let resp = raw_tls_request(
        addr,
        "GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        &ctx,
    )
    .await;
    let response = String::from_utf8_lossy(&resp);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "TLS static: {}",
        response
    );
    assert!(response.contains("hello world"), "body: {}", response);

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn tls_handshake_failure_does_not_invoke_service() {
    let tmp = TempDir::new().unwrap();
    let service_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let service_called_clone = service_called.clone();

    let svc = service_fn(move |_req: RequestHead| {
        service_called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"should not reach".to_vec()))
                .unwrap())
        })
    });

    let ctx = make_tls_context();
    let config = config_for_tls("127.0.0.1:0", &ctx);
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    let addr = handle.local_addr();

    // Connect with a plain TCP stream (no TLS handshake) — the server should
    // reject the connection during TLS handshake without invoking the service.
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        let mut stream = tokio::net::TcpStream::connect(addr).await.ok()?;
        let _ = stream
            .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await;
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
                "service should not be invoked on TLS handshake failure: {}",
                s
            );
        }
        Ok(None) => {}
        Err(_) => {}
    }

    assert!(
        !service_called.load(std::sync::atomic::Ordering::SeqCst),
        "service must not be called on TLS handshake failure"
    );

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn tls_handshake_timeout_does_not_hang() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_tls_context();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .header_read_timeout(Duration::from_secs(1))
        .tls_config(ctx.server_config.clone())
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(simple_service()).await.unwrap();
    let addr = handle.local_addr();

    let start = std::time::Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = tokio::net::TcpStream::connect(addr).await.ok()?;
        let _ = stream
            .write_all(b"GET /test HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await;
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf).await;
        Some(buf)
    })
    .await;

    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(4),
        "TLS handshake timeout should not hang: {:?}",
        elapsed
    );
    assert!(result.is_ok(), "should complete within deadline");

    handle.shutdown();
    let _ = handle.wait().await;
}

#[tokio::test]
async fn graceful_shutdown_works_during_tls_connections() {
    let tmp = TempDir::new().unwrap();
    let response_received = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let response_received_clone = response_received.clone();

    let svc = service_fn(move |_req: RequestHead| {
        let rr = response_received_clone.clone();
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            rr.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"slow tls".to_vec()))
                .unwrap())
        })
    });

    let ctx = make_tls_context();
    let config = config_for_tls("127.0.0.1:0", &ctx);
    let server = Server::builder()
        .runtime(config)
        .serve_config(make_serve_config(&tmp))
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();

    let addr = handle.local_addr();
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(ctx.client_config.clone());
    let domain = "localhost".try_into().unwrap();
    let tls_stream = connector.connect(domain, tcp).await.unwrap();
    let (mut reader, mut writer) = tokio::io::split(tls_stream);
    writer.write_all(GET_REQUEST.as_bytes()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;
    handle.shutdown();

    let mut buf = Vec::new();
    let _ = reader.read_to_end(&mut buf).await;
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "inflight TLS request should complete: {}",
        response
    );
    assert!(response_received.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test]
async fn forced_shutdown_works_during_tls_connections() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_tls_context();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .graceful_shutdown_timeout(Duration::from_millis(100))
        .tls_config(ctx.server_config.clone())
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
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(ctx.client_config.clone());
    let domain = "localhost".try_into().unwrap();
    let tls_stream = connector.connect(domain, tcp).await.unwrap();
    let (_reader, mut writer) = tokio::io::split(tls_stream);
    writer.write_all(GET_REQUEST.as_bytes()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let result = handle
        .force_shutdown(Duration::from_millis(50))
        .await
        .unwrap();
    assert_eq!(result, ShutdownResult::Forced);
}

#[tokio::test]
async fn tls_and_plaintext_same_service_dispatch_path() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_tls_context();
    let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    for use_tls in [false, true] {
        let cc = call_count.clone();
        let svc = service_fn(move |_req: RequestHead| {
            cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"dispatched".to_vec()))
                    .unwrap())
            })
        });

        let config = if use_tls {
            config_for_tls("127.0.0.1:0", &ctx)
        } else {
            RuntimeConfig::builder()
                .bind("127.0.0.1:0".parse().unwrap())
                .build()
                .unwrap()
        };

        let server = Server::builder()
            .runtime(config)
            .serve_config(make_serve_config(&tmp))
            .build()
            .unwrap();
        let handle = server.start_with_service(svc).await.unwrap();
        let addr = handle.local_addr();

        let resp = if use_tls {
            raw_tls_request(addr, GET_REQUEST, &ctx).await
        } else {
            raw_request(addr, GET_REQUEST).await
        };
        let response = String::from_utf8_lossy(&resp);
        assert!(
            response.starts_with("HTTP/1.1 200"),
            "{}: {}",
            if use_tls { "TLS" } else { "plaintext" },
            response
        );

        handle.shutdown();
        let _ = handle.wait().await;
    }

    assert!(
        call_count.load(std::sync::atomic::Ordering::SeqCst) >= 2,
        "both plaintext and TLS should have invoked the service"
    );
}
