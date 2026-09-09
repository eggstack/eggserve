#![cfg(feature = "http3")]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::{Buf, Bytes};
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::server::{service_fn, Http3Config, Request, RuntimeConfig, Server};
use futures_util::future;
use h3::client;
use h3_quinn::quinn::crypto::rustls::QuicClientConfig;
use h3_quinn::quinn::{ClientConfig, Endpoint, VarInt};
use tempfile::TempDir;
use tokio::net::UdpSocket;

fn write_identity(
    dir: &TempDir,
) -> (
    std::path::PathBuf,
    std::path::PathBuf,
    rustls::pki_types::CertificateDer<'static>,
) {
    let key_pair = rcgen::KeyPair::generate().expect("generate key pair");
    let params = rcgen::CertificateParams::new(vec!["localhost".to_string()])
        .expect("certificate parameters");
    let certificate = params
        .self_signed(&key_pair)
        .expect("self-sign certificate");
    let cert_path = dir.path().join("localhost.crt");
    let key_path = dir.path().join("localhost.key");
    std::fs::write(&cert_path, certificate.pem()).expect("write certificate");
    std::fs::write(&key_path, key_pair.serialize_pem()).expect("write key");
    (cert_path, key_path, certificate.der().clone())
}

async fn connect_h3(
    addr: SocketAddr,
    certificate: rustls::pki_types::CertificateDer<'static>,
) -> (
    Endpoint,
    h3_quinn::quinn::Connection,
    h3::client::Connection<h3_quinn::Connection, Bytes>,
    h3::client::SendRequest<h3_quinn::OpenStreams, Bytes>,
) {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).unwrap();
    let mut tls = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"h3".to_vec()];
    let client_config =
        ClientConfig::new(Arc::new(QuicClientConfig::try_from(Arc::new(tls)).unwrap()));
    let mut endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(client_config);
    let connection = endpoint.connect(addr, "localhost").unwrap().await.unwrap();
    let (driver, sender) = client::new(h3_quinn::Connection::new(connection.clone()))
        .await
        .unwrap();
    (endpoint, connection, driver, sender)
}

async fn collect_h3_body<S>(stream: &mut h3::client::RequestStream<S, Bytes>) -> Vec<u8>
where
    S: h3::quic::RecvStream,
{
    let mut body = Vec::new();
    while let Some(chunk) = stream.recv_data().await.unwrap() {
        body.extend_from_slice(chunk.chunk());
    }
    body
}

#[tokio::test]
async fn http3_binds_udp_to_the_tcp_port_and_shuts_down_together() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, _) = write_identity(&identity_dir);
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .http3(Http3Config {
            enabled: true,
            ..Http3Config::default()
        })
        .build()
        .unwrap();
    let service = service_fn(|_request: Request| async move {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"h3".to_vec()))
            .unwrap())
    });
    let server = Server::builder()
        .runtime(config)
        .http3_identity(&cert_path, &key_path)
        .build()
        .unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    assert_ne!(addr.port(), 0);
    assert!(UdpSocket::bind(addr).await.is_err());

    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn http3_requires_a_quic_identity_before_startup() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .http3(Http3Config {
            enabled: true,
            ..Http3Config::default()
        })
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let service = service_fn(|_request: Request| async move {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Empty)
            .unwrap())
    });
    let error = server.start_with_service(service).await.unwrap_err();
    assert!(error.to_string().contains("QUIC certificate/key identity"));
}

#[tokio::test]
async fn h3_reject_presence_probe_is_bounded_and_stream_scoped() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let invocations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let service = {
        let invocations = invocations.clone();
        service_fn(move |_request: Request| {
            let invocations = invocations.clone();
            async move {
                invocations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"sibling survived".to_vec()))
                    .unwrap())
            }
        })
    };
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .body_read_timeout(Duration::from_millis(100))
        .http3(Http3Config {
            enabled: true,
            ..Http3Config::default()
        })
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .http3_identity(&cert_path, &key_path)
        .build()
        .unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let (endpoint, _connection, mut driver, mut sender) =
        connect_h3(handle.local_addr(), certificate).await;
    let driver_task =
        tokio::spawn(async move { future::poll_fn(|cx| driver.poll_close(cx)).await });

    let mut with_data = sender
        .send_request(
            hyper::Request::builder()
                .method("POST")
                .uri("https://localhost/data")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    with_data
        .send_data(Bytes::from_static(b"body without a length"))
        .await
        .unwrap();
    with_data.finish().await.unwrap();
    let response = with_data.recv_response().await.unwrap();
    assert_eq!(response.status(), hyper::StatusCode::PAYLOAD_TOO_LARGE);
    assert!(collect_h3_body(&mut with_data).await.starts_with(b"413"));
    assert_eq!(invocations.load(std::sync::atomic::Ordering::SeqCst), 0);

    let mut zero_with_data = sender
        .send_request(
            hyper::Request::builder()
                .method("POST")
                .uri("https://localhost/zero-with-data")
                .header("content-length", "0")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    zero_with_data
        .send_data(Bytes::from_static(b"DATA despite zero"))
        .await
        .unwrap();
    zero_with_data.finish().await.unwrap();
    let response = zero_with_data.recv_response().await.unwrap();
    assert_eq!(response.status(), hyper::StatusCode::PAYLOAD_TOO_LARGE);
    let _ = collect_h3_body(&mut zero_with_data).await;
    assert_eq!(invocations.load(std::sync::atomic::Ordering::SeqCst), 0);

    let mut empty = sender
        .send_request(
            hyper::Request::builder()
                .method("POST")
                .uri("https://localhost/empty")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    empty.finish().await.unwrap();
    let response = empty.recv_response().await.unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    assert_eq!(collect_h3_body(&mut empty).await, b"sibling survived");
    assert_eq!(invocations.load(std::sync::atomic::Ordering::SeqCst), 1);

    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn h3_presence_probe_timeout_does_not_invoke_service() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let invocations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let service = {
        let invocations = invocations.clone();
        service_fn(move |_request: Request| {
            let invocations = invocations.clone();
            async move {
                invocations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Empty)
                    .unwrap())
            }
        })
    };
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .body_read_timeout(Duration::from_millis(25))
        .http3(Http3Config {
            enabled: true,
            ..Http3Config::default()
        })
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .http3_identity(&cert_path, &key_path)
        .build()
        .unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let (endpoint, _connection, mut driver, mut sender) =
        connect_h3(handle.local_addr(), certificate).await;
    let driver_task =
        tokio::spawn(async move { future::poll_fn(|cx| driver.poll_close(cx)).await });

    let mut stalled = sender
        .send_request(
            hyper::Request::builder()
                .method("POST")
                .uri("https://localhost/stalled")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(1), stalled.recv_response())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), hyper::StatusCode::REQUEST_TIMEOUT);
    let _ = collect_h3_body(&mut stalled).await;
    assert_eq!(invocations.load(std::sync::atomic::Ordering::SeqCst), 0);

    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn h3_peer_close_wakes_detached_request_lifecycle() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let (lifecycle_tx, lifecycle_rx) = tokio::sync::oneshot::channel();
    let lifecycle_tx = Arc::new(std::sync::Mutex::new(Some(lifecycle_tx)));
    let service = service_fn(move |request: Request| {
        let lifecycle = request.lifecycle_clone();
        let lifecycle_for_service = lifecycle.clone();
        let lifecycle_tx = lifecycle_tx.clone();
        async move {
            if let Some(sender) = lifecycle_tx.lock().unwrap().take() {
                let _ = sender.send(lifecycle_for_service);
            }
            lifecycle.cancelled().await;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        }
    });
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .http3(Http3Config {
            enabled: true,
            ..Http3Config::default()
        })
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .http3_identity(&cert_path, &key_path)
        .build()
        .unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let (endpoint, connection, mut driver, mut sender) =
        connect_h3(handle.local_addr(), certificate).await;
    let driver_task =
        tokio::spawn(async move { future::poll_fn(|cx| driver.poll_close(cx)).await });

    let mut request = sender
        .send_request(
            hyper::Request::builder()
                .uri("https://localhost/wait")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    request.finish().await.unwrap();
    let lifecycle = tokio::time::timeout(Duration::from_secs(1), lifecycle_rx)
        .await
        .unwrap()
        .unwrap();
    connection.close(VarInt::from_u32(0), b"peer close");
    tokio::time::timeout(Duration::from_secs(1), lifecycle.cancelled())
        .await
        .expect("peer close should wake a detached H3 lifecycle");
    assert_eq!(
        lifecycle.cancellation_reason(),
        Some(eggserve_core::primitives::RequestCancellationReason::PeerDisconnected)
    );

    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn h3_normal_completion_does_not_cancel_lifecycle() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let (lifecycle_tx, lifecycle_rx) = tokio::sync::oneshot::channel();
    let lifecycle_tx = Arc::new(std::sync::Mutex::new(Some(lifecycle_tx)));
    let service = service_fn(move |request: Request| {
        let lifecycle = request.lifecycle_clone();
        let lifecycle_tx = lifecycle_tx.clone();
        async move {
            if let Some(sender) = lifecycle_tx.lock().unwrap().take() {
                let _ = sender.send(lifecycle.clone());
            }
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"done".to_vec()))
                .unwrap())
        }
    });
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .http3(Http3Config {
            enabled: true,
            ..Http3Config::default()
        })
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .http3_identity(&cert_path, &key_path)
        .build()
        .unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let (endpoint, _connection, mut driver, mut sender) =
        connect_h3(handle.local_addr(), certificate).await;
    let driver_task =
        tokio::spawn(async move { future::poll_fn(|cx| driver.poll_close(cx)).await });

    let mut request = sender
        .send_request(
            hyper::Request::builder()
                .uri("https://localhost/done")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    request.finish().await.unwrap();
    let response = request.recv_response().await.unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    assert_eq!(collect_h3_body(&mut request).await, b"done");
    let lifecycle = lifecycle_rx.await.unwrap();
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert!(!lifecycle.is_cancelled());

    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn h3_forced_shutdown_wakes_remaining_lifecycle_waiter() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let (lifecycle_tx, lifecycle_rx) = tokio::sync::oneshot::channel();
    let lifecycle_tx = Arc::new(std::sync::Mutex::new(Some(lifecycle_tx)));
    let service = service_fn(move |request: Request| {
        let lifecycle = request.lifecycle_clone();
        let lifecycle_tx = lifecycle_tx.clone();
        async move {
            if let Some(sender) = lifecycle_tx.lock().unwrap().take() {
                let _ = sender.send(lifecycle.clone());
            }
            lifecycle.cancelled().await;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        }
    });
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .graceful_shutdown_timeout(Duration::from_millis(25))
        .http3(Http3Config {
            enabled: true,
            ..Http3Config::default()
        })
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .http3_identity(&cert_path, &key_path)
        .build()
        .unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let (endpoint, _connection, mut driver, mut sender) =
        connect_h3(handle.local_addr(), certificate).await;
    let driver_task =
        tokio::spawn(async move { future::poll_fn(|cx| driver.poll_close(cx)).await });

    let mut request = sender
        .send_request(
            hyper::Request::builder()
                .uri("https://localhost/shutdown")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    request.finish().await.unwrap();
    let lifecycle = tokio::time::timeout(Duration::from_secs(1), lifecycle_rx)
        .await
        .unwrap()
        .unwrap();
    handle.shutdown();
    handle.wait().await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), lifecycle.cancelled())
        .await
        .expect("forced H3 shutdown should wake lifecycle waiters");
    assert_eq!(
        lifecycle.cancellation_reason(),
        Some(eggserve_core::primitives::RequestCancellationReason::ServerShutdown)
    );

    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;
}
