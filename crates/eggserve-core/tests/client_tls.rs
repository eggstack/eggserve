#![cfg(feature = "client-tls")]

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

fn init_tls() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

use bytes::Bytes;
use eggserve_core::primitives::client::{
    ClientConfig, ClientError, ClientRequestBuilder, HttpClient, Method,
};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use rcgen::{CertificateParams, KeyPair};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

struct TlsServer {
    addr: std::net::SocketAddr,
}

fn generate_cert(common_name: &str) -> (CertificateDer<'static>, PrivatePkcs8KeyDer<'static>) {
    let key_pair = KeyPair::generate().expect("generate key pair");
    let params = CertificateParams::new(vec![common_name.to_string()]).expect("create params");
    let cert = params.self_signed(&key_pair).expect("self-sign cert");
    let key_der = PrivatePkcs8KeyDer::from(key_pair.serialize_der());
    (cert.into(), key_der)
}

fn start_tls_server(
    handler: impl Fn(
            Request<Incoming>,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Response<Full<Bytes>>> + Send>>
        + Send
        + Sync
        + 'static,
) -> TlsServer {
    let (server_cert, server_key) = generate_cert("localhost");

    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![server_cert.clone()], server_key.into())
        .expect("server TLS config");

    let acceptor = TlsAcceptor::from(Arc::new(server_config));
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std_listener.set_nonblocking(true).unwrap();
    let addr = std_listener.local_addr().unwrap();

    let handler = Arc::new(handler);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let listener = TcpListener::from_std(std_listener).unwrap();
            loop {
                let (tcp_stream, _peer_addr) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(_) => continue,
                };
                let acceptor = acceptor.clone();
                let handler = handler.clone();
                tokio::spawn(async move {
                    let tls_stream = match acceptor.accept(tcp_stream).await {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    let io = TokioIo::new(tls_stream);
                    let service = service_fn(move |req| {
                        let handler = handler.clone();
                        async move { Ok::<_, Infallible>(handler(req).await) }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, service)
                        .await;
                });
            }
        });
    });

    TlsServer { addr }
}

#[test]
fn https_with_verify_tls_false_succeeds() {
    init_tls();
    let server = start_tls_server(|_req| {
        Box::pin(async {
            Response::builder()
                .status(200)
                .body(Full::new(Bytes::from("ok")))
                .unwrap()
        })
    });

    let config = ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: Some(1024 * 1024),
        verify_tls: false,
    };
    let client = HttpClient::new(config);

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("https://{}/", server.addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.text().unwrap(), "ok");
}

#[test]
fn https_with_verify_tls_true_rejects_self_signed() {
    init_tls();
    let server = start_tls_server(|_req| {
        Box::pin(async {
            Response::builder()
                .status(200)
                .body(Full::new(Bytes::from("ok")))
                .unwrap()
        })
    });

    let config = ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: Some(1024 * 1024),
        verify_tls: true,
    };
    let client = HttpClient::new(config);

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("https://{}/", server.addr))
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(result.is_err());
    match result.unwrap_err() {
        ClientError::TlsError(_) => {}
        ClientError::Timeout(_) => {}
        other => panic!("expected TlsError or Timeout for untrusted cert, got {other:?}"),
    }
}

#[test]
fn http_never_enters_tls() {
    init_tls();
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std_listener.set_nonblocking(true).unwrap();
    let addr = std_listener.local_addr().unwrap();

    let server_handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let listener = TcpListener::from_std(std_listener).unwrap();
            let (tcp, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(tcp);
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(
                    io,
                    service_fn(|_req: Request<Incoming>| async {
                        Ok::<_, Infallible>(
                            Response::builder()
                                .status(200)
                                .body(Full::new(Bytes::from("plain")))
                                .unwrap(),
                        )
                    }),
                )
                .await;
        });
    });

    let config = ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: Some(1024),
        verify_tls: true,
    };
    let client = HttpClient::new(config);

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.text().unwrap(), "plain");

    server_handle.join().unwrap();
}

#[test]
fn tls_error_is_client_error_variant() {
    init_tls();
    let config = ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: Some(1024),
        verify_tls: true,
    };
    let client = HttpClient::new(config);

    let req = ClientRequestBuilder::new(Method::Get)
        .url("https://localhost:1/ssl")
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(result.is_err());
    match result.unwrap_err() {
        ClientError::TlsError(_) => {}
        ClientError::Timeout(_) => {}
        ClientError::ConnectError(_) => {}
        other => panic!("expected TLS/Timeout/ConnectError, got {other:?}"),
    }
}

#[test]
fn verify_tls_false_bypasses_cert_verification() {
    init_tls();
    let server = start_tls_server(|_req| {
        Box::pin(async {
            Response::builder()
                .status(201)
                .body(Full::new(Bytes::from("created")))
                .unwrap()
        })
    });

    let config = ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: Some(1024 * 1024),
        verify_tls: false,
    };
    let client = HttpClient::new(config);

    let req = ClientRequestBuilder::new(Method::Post)
        .url(&format!("https://{}/resource", server.addr))
        .unwrap()
        .body(b"test data".to_vec())
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 201);
    assert_eq!(resp.text().unwrap(), "created");
}

#[test]
fn https_multiple_requests_on_same_client() {
    init_tls();
    let server = start_tls_server(|req| {
        Box::pin(async move {
            let body = match req.uri().path() {
                "/a" => "page-a",
                "/b" => "page-b",
                _ => "not-found",
            };
            Response::builder()
                .status(200)
                .body(Full::new(Bytes::from(body)))
                .unwrap()
        })
    });

    let config = ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: Some(1024 * 1024),
        verify_tls: false,
    };
    let client = HttpClient::new(config);

    for (path, expected_body) in [("/a", "page-a"), ("/b", "page-b")] {
        let req = ClientRequestBuilder::new(Method::Get)
            .url(&format!("https://{}{}", server.addr, path))
            .unwrap()
            .build()
            .unwrap();

        let resp = client.send(&req).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.text().unwrap(), expected_body);
    }
}
