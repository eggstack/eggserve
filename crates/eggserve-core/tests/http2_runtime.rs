#![cfg(feature = "http2")]

use std::sync::Arc;

use bytes::Bytes;
use eggserve_core::config::ServeConfig;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::connection_info::Scheme;
use eggserve_core::server::connection::{
    serve_http_connection, ConnectionContext, ConnectionShutdown,
};
use eggserve_core::server::{
    service_fn, Http2Config, Request, RuntimeConfig, RuntimeState, Server,
};
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http2;
use hyper::Request as HyperRequest;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(feature = "tls")]
fn init_tls() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[cfg(feature = "tls")]
fn tls_configs(
    server_alpn: Vec<Vec<u8>>,
    client_alpn: Vec<Vec<u8>>,
) -> (Arc<rustls::ServerConfig>, Arc<rustls::ClientConfig>) {
    init_tls();
    let key_pair = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    let cert = params.self_signed(&key_pair).unwrap();
    let cert_der: rustls::pki_types::CertificateDer<'static> = cert.into();
    let key_der = rustls::pki_types::PrivatePkcs8KeyDer::from(key_pair.serialize_der());
    let mut server = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key_der.into())
        .unwrap();
    server.alpn_protocols = server_alpn;

    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert_der).unwrap();
    let mut client = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    client.alpn_protocols = client_alpn;
    (Arc::new(server), Arc::new(client))
}

fn runtime_config() -> RuntimeConfig {
    RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .http2(Http2Config {
            max_concurrent_streams: 8,
            ..Http2Config::default()
        })
        .build()
        .unwrap()
}

fn serve_config(root: &TempDir) -> Arc<ServeConfig> {
    Arc::new(ServeConfig {
        root: root.path().to_path_buf(),
        ..ServeConfig::default()
    })
}

#[tokio::test]
async fn cleartext_prior_knowledge_multiplexes_and_maps_metadata() {
    let root = TempDir::new().unwrap();
    let (seen_tx, mut seen_rx) = tokio::sync::mpsc::channel(8);
    let service = service_fn(move |request: Request| {
        let seen_tx = seen_tx.clone();
        async move {
            let _ = seen_tx
                .send((
                    request.head().version(),
                    request.head().authority().cloned(),
                    request.head().target().path().to_owned(),
                ))
                .await;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"h2".to_vec()))
                .unwrap())
        }
    });
    let server = Server::builder()
        .runtime(runtime_config())
        .serve_config(serve_config(&root))
        .build()
        .unwrap();
    let handle = server.start_with_service(service).await.unwrap();

    let stream = tokio::net::TcpStream::connect(handle.local_addr())
        .await
        .unwrap();
    let (sender, connection) =
        http2::handshake::<_, _, Full<Bytes>>(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .unwrap();
    let connection_task = tokio::spawn(connection);

    let requests = ["/one", "/two", "/three", "/four"].into_iter().map(|path| {
        let mut sender = sender.clone();
        async move {
            sender.ready().await.unwrap();
            let request = HyperRequest::builder()
                .method("GET")
                .uri(format!("http://example.test{path}"))
                .header("host", "example.test")
                .body(Full::new(Bytes::new()))
                .unwrap();
            sender.send_request(request).await.unwrap()
        }
    });
    let responses = futures_util::future::join_all(requests).await;
    for response in responses {
        assert_eq!(response.status(), hyper::StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"h2");
    }

    for expected in ["/one", "/two", "/three", "/four"] {
        let (version, authority, path) = seen_rx.recv().await.unwrap();
        assert_eq!(version, eggserve_core::primitives::HttpVersion::Http2);
        assert_eq!(authority.unwrap().as_str(), "example.test");
        assert_eq!(path, expected);
    }

    drop(sender);
    let _ = connection_task.await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn caller_owned_h1_entry_remains_strict() {
    let config = Arc::new(RuntimeConfig::default());
    let runtime = Arc::new(RuntimeState::new(&config));
    let shutdown: &'static ConnectionShutdown = Box::leak(Box::new(ConnectionShutdown::new()));
    let context = ConnectionContext::for_non_socket(Scheme::Http, None);
    let (mut client, server) = tokio::io::duplex(4096);
    let task = tokio::spawn(serve_http_connection(
        server,
        service_fn(|_request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        }),
        config,
        context,
        runtime,
        shutdown,
    ));
    client
        .write_all(
            b"GET / HTTP/1.1\r\nHost: example.test\r\nConnection: Upgrade, close\r\nUpgrade: h2c\r\n\r\n",
        )
        .await
        .unwrap();
    let mut output = Vec::new();
    client.read_to_end(&mut output).await.unwrap();
    assert!(String::from_utf8_lossy(&output).starts_with("HTTP/1.1 200"));
    assert!(!String::from_utf8_lossy(&output).starts_with("HTTP/2"));
    assert!(task.await.unwrap().is_clean());
}

#[tokio::test]
async fn rejected_body_is_stream_scoped_and_h2_has_no_hop_headers() {
    let root = TempDir::new().unwrap();
    let server = Server::builder()
        .runtime(runtime_config())
        .serve_config(serve_config(&root))
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(|request: Request| async move {
            assert_eq!(request.head().target().path(), "/ok");
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"sibling survived".to_vec()))
                .unwrap())
        }))
        .await
        .unwrap();

    let stream = tokio::net::TcpStream::connect(handle.local_addr())
        .await
        .unwrap();
    let (sender, connection) =
        http2::handshake::<_, _, Full<Bytes>>(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .unwrap();
    let connection_task = tokio::spawn(connection);

    let mut rejected_sender = sender.clone();
    let rejected = tokio::spawn(async move {
        rejected_sender
            .send_request(
                HyperRequest::builder()
                    .method("POST")
                    .uri("http://example.test/rejected")
                    .header("content-type", "application/octet-stream")
                    .body(Full::new(Bytes::from_static(b"body refused")))
                    .unwrap(),
            )
            .await
            .unwrap()
    });

    let mut sibling_sender = sender.clone();
    let sibling = tokio::spawn(async move {
        sibling_sender
            .send_request(
                HyperRequest::builder()
                    .method("GET")
                    .uri("http://example.test/ok")
                    .body(Full::new(Bytes::new()))
                    .unwrap(),
            )
            .await
            .unwrap()
    });

    let rejected = rejected.await.unwrap();
    assert_eq!(rejected.status(), hyper::StatusCode::PAYLOAD_TOO_LARGE);
    assert!(rejected.headers().get("connection").is_none());
    assert!(rejected.headers().get("transfer-encoding").is_none());

    let sibling = sibling.await.unwrap();
    assert_eq!(sibling.status(), hyper::StatusCode::OK);
    assert_eq!(
        sibling.into_body().collect().await.unwrap().to_bytes(),
        "sibling survived"
    );

    drop(sender);
    let _ = connection_task.await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[cfg(feature = "tls")]
#[tokio::test]
async fn tls_alpn_selects_h2_and_falls_back_to_h1() {
    let (server_tls, h2_client_tls) = tls_configs(
        vec![b"h2".to_vec(), b"http/1.1".to_vec()],
        vec![b"h2".to_vec()],
    );
    let root = TempDir::new().unwrap();
    let service = service_fn(|_request: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"tls".to_vec()))
            .unwrap())
    });
    let server = Server::builder()
        .runtime(
            RuntimeConfig::builder()
                .bind("127.0.0.1:0".parse().unwrap())
                .http2(Http2Config::default())
                .tls_config(server_tls)
                .build()
                .unwrap(),
        )
        .serve_config(serve_config(&root))
        .build()
        .unwrap();
    let handle = server.start_with_service(service).await.unwrap();

    let tcp = tokio::net::TcpStream::connect(handle.local_addr())
        .await
        .unwrap();
    let tls = tokio_rustls::TlsConnector::from(h2_client_tls)
        .connect("localhost".try_into().unwrap(), tcp)
        .await
        .unwrap();
    let (mut sender, connection) =
        http2::handshake::<_, _, Full<Bytes>>(TokioExecutor::new(), TokioIo::new(tls))
            .await
            .unwrap();
    let connection_task = tokio::spawn(connection);
    let response = sender
        .send_request(
            HyperRequest::builder()
                .uri("https://localhost/h2")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "tls"
    );
    drop(sender);
    connection_task.await.unwrap().unwrap();

    let (server_tls, h1_client_tls) = tls_configs(
        vec![b"h2".to_vec(), b"http/1.1".to_vec()],
        vec![b"http/1.1".to_vec()],
    );
    // The first server has already been drained. Start a fresh listener with
    // the same service shape to prove ALPN fallback independently.
    handle.shutdown();
    handle.wait().await.unwrap();
    let root = TempDir::new().unwrap();
    let server = Server::builder()
        .runtime(
            RuntimeConfig::builder()
                .bind("127.0.0.1:0".parse().unwrap())
                .http2(Http2Config::default())
                .tls_config(server_tls)
                .build()
                .unwrap(),
        )
        .serve_config(serve_config(&root))
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(|_request: Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"fallback".to_vec()))
                .unwrap())
        }))
        .await
        .unwrap();
    let tcp = tokio::net::TcpStream::connect(handle.local_addr())
        .await
        .unwrap();
    let tls = tokio_rustls::TlsConnector::from(h1_client_tls)
        .connect("localhost".try_into().unwrap(), tcp)
        .await
        .unwrap();
    let (mut reader, mut writer) = tokio::io::split(tls);
    writer
        .write_all(b"GET /h1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut output = Vec::new();
    reader.read_to_end(&mut output).await.unwrap();
    assert!(String::from_utf8_lossy(&output).starts_with("HTTP/1.1 200"));
    assert!(String::from_utf8_lossy(&output).contains("fallback"));
    handle.shutdown();
    handle.wait().await.unwrap();
}
