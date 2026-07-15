#![cfg(feature = "tls")]

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

async fn start_tls_server(
    config: RuntimeConfig,
    policy: RequestBodyPolicy,
    _ctx: &TlsContext,
) -> (ServerHandle, TempDir) {
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

async fn raw_tls_request_with_body(
    addr: std::net::SocketAddr,
    headers: &str,
    body: &[u8],
    ctx: &TlsContext,
) -> Vec<u8> {
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(ctx.client_config.clone());
    let domain = "localhost".try_into().unwrap();
    let tls_stream = connector.connect(domain, tcp).await.unwrap();
    let (mut reader, mut writer) = tokio::io::split(tls_stream);
    writer.write_all(headers.as_bytes()).await.unwrap();
    if !body.is_empty() {
        writer.write_all(body).await.unwrap();
    }
    let mut buf = Vec::new();
    let _ = reader.read_to_end(&mut buf).await;
    buf
}

#[tokio::test]
async fn tls_post_with_body() {
    let ctx = make_tls_context();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .tls_config(ctx.server_config.clone())
        .build();
    let (handle, _tmp) = start_tls_server(
        config,
        RequestBodyPolicy::Buffer {
            max_bytes: 1024 * 1024,
        },
        &ctx,
    )
    .await;
    let addr = handle.local_addr();

    let response = raw_tls_request_with_body(
        addr,
        "POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\n",
        b"hello",
        &ctx,
    )
    .await;
    let response = String::from_utf8_lossy(&response);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "TLS POST with body should succeed: {}",
        response
    );
    assert!(
        response.contains("hello"),
        "response should echo body: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn tls_chunked_body() {
    let ctx = make_tls_context();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .tls_config(ctx.server_config.clone())
        .build();
    let (handle, _tmp) = start_tls_server(
        config,
        RequestBodyPolicy::Buffer {
            max_bytes: 1024 * 1024,
        },
        &ctx,
    )
    .await;
    let addr = handle.local_addr();

    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(ctx.client_config.clone());
    let domain = "localhost".try_into().unwrap();
    let tls_stream = connector.connect(domain, tcp).await.unwrap();
    let (mut reader, mut writer) = tokio::io::split(tls_stream);

    writer
        .write_all(
            b"POST /test HTTP/1.1\r\n\
              Host: localhost\r\n\
              Transfer-Encoding: chunked\r\n\
              Connection: close\r\n\
              \r\n",
        )
        .await
        .unwrap();
    writer.write_all(b"5\r\nhello\r\n").await.unwrap();
    writer.write_all(b"1\r\n \r\n").await.unwrap();
    writer.write_all(b"5\r\nworld\r\n").await.unwrap();
    writer.write_all(b"0\r\n\r\n").await.unwrap();

    let mut buf = Vec::new();
    let _ = reader.read_to_end(&mut buf).await;
    let response = String::from_utf8_lossy(&buf);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "TLS chunked body should succeed: {}",
        response
    );
    assert!(
        response.contains("hello world"),
        "response should contain reassembled body: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn tls_body_limit_exceeded() {
    let ctx = make_tls_context();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(5)
        .body_read_timeout(Duration::from_secs(5))
        .tls_config(ctx.server_config.clone())
        .build();
    let (handle, _tmp) = start_tls_server(
        config,
        RequestBodyPolicy::Buffer {
            max_bytes: 1024 * 1024,
        },
        &ctx,
    )
    .await;
    let addr = handle.local_addr();

    let response = raw_tls_request_with_body(
        addr,
        "POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 10\r\nConnection: close\r\n\r\n",
        b"hello world",
        &ctx,
    )
    .await;
    let response = String::from_utf8_lossy(&response);
    assert!(
        response.starts_with("HTTP/1.1 413"),
        "TLS body limit exceeded should return 413: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn tls_body_timeout() {
    let ctx = make_tls_context();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_millis(50))
        .tls_config(ctx.server_config.clone())
        .build();
    let (handle, _tmp) = start_tls_server(
        config,
        RequestBodyPolicy::Buffer {
            max_bytes: 1024 * 1024,
        },
        &ctx,
    )
    .await;
    let addr = handle.local_addr();

    let response = raw_tls_request_with_body(
        addr,
        "POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\nConnection: close\r\n\r\n",
        b"",
        &ctx,
    )
    .await;
    let response = String::from_utf8_lossy(&response);
    assert!(
        response.starts_with("HTTP/1.1 408") || response.is_empty(),
        "TLS body timeout should return 408 or close: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn tls_get_with_body_rejected() {
    let ctx = make_tls_context();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .tls_config(ctx.server_config.clone())
        .build();
    let (handle, _tmp) = start_tls_server(
        config,
        RequestBodyPolicy::Buffer {
            max_bytes: 1024 * 1024,
        },
        &ctx,
    )
    .await;
    let addr = handle.local_addr();

    let response = raw_tls_request_with_body(
        addr,
        "GET /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\n",
        b"hello",
        &ctx,
    )
    .await;
    let response = String::from_utf8_lossy(&response);
    assert!(
        response.starts_with("HTTP/1.1 400"),
        "TLS GET with body should return 400: {}",
        response
    );
    handle.shutdown();
}

#[tokio::test]
async fn tls_partial_body_close_policy() {
    let ctx = make_tls_context();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024)
        .body_read_timeout(Duration::from_secs(5))
        .incomplete_body_policy(
            eggserve_core::primitives::incomplete_body_policy::IncompleteBodyPolicy::Close,
        )
        .tls_config(ctx.server_config.clone())
        .build();

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
                let (_head, mut body) = req.into_head_and_body();
                // Read only first chunk.
                let _chunk = body.next_chunk().await.unwrap();
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"ok".to_vec()))
                    .unwrap())
            },
            RequestBodyPolicy::Stream {
                max_bytes: 1024 * 1024,
            },
        ))
        .await
        .unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let response = raw_tls_request_with_body(
        addr,
        "POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 10\r\nConnection: close\r\n\r\n",
        b"helloworld",
        &ctx,
    )
    .await;
    let response = String::from_utf8_lossy(&response);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "TLS partial body with close policy should succeed: {}",
        response
    );
    handle.shutdown();
}
