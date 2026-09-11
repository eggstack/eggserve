#![cfg(feature = "http3")]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::{Buf, Bytes};
use eggserve_core::ops::OpsContext;
use eggserve_core::primitives::canonical::{
    Response, ResponseBody, ResponseStream, ResponseStreamError, StatusCode,
};
use eggserve_core::server::{service_fn, Http3Config, Request, RuntimeConfig, Server};
use futures_util::{future, StreamExt};
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

/// Plan 192 Track C/D (#262): an early request error must terminate only its
/// own stream. A rejected stream sends the canonical error while a sibling
/// stream on the same connection still completes normally.
#[tokio::test]
async fn h3_early_head_error_is_stream_scoped_and_sibling_survives() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let service = service_fn(|_request: Request| async move {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"sibling ok".to_vec()))
            .unwrap())
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

    // `TE: chunked` is forbidden on H3; the adapter must answer 400 without
    // invoking the service and without disturbing sibling streams.
    let mut rejected = sender
        .send_request(
            hyper::Request::builder()
                .uri("https://localhost/rejected")
                .header("te", "chunked")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    rejected.finish().await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(1), rejected.recv_response())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), hyper::StatusCode::BAD_REQUEST);
    let _ = collect_h3_body(&mut rejected).await;

    let mut sibling = sender
        .send_request(
            hyper::Request::builder()
                .uri("https://localhost/sibling")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    sibling.finish().await.unwrap();
    let response = sibling.recv_response().await.unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    assert_eq!(collect_h3_body(&mut sibling).await, b"sibling ok");

    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// Plan 192 Track D (#338 deterministic simulation): bytes for a valid
/// response that are already buffered must not be discarded when the peer
/// closes immediately after a complete exchange. The complete body is
/// observed, the detached lifecycle reports peer close, and the server stays
/// usable for a new connection.
#[tokio::test]
async fn h3_complete_response_survives_immediate_peer_close() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let service = service_fn(|_request: Request| async move {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"plan-192-data".to_vec()))
            .unwrap())
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
    let addr = handle.local_addr();
    let (endpoint, connection, mut driver, mut sender) =
        connect_h3(addr, certificate.clone()).await;
    let driver_task =
        tokio::spawn(async move { future::poll_fn(|cx| driver.poll_close(cx)).await });

    let mut request = sender
        .send_request(
            hyper::Request::builder()
                .uri("https://localhost/data")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    request.finish().await.unwrap();
    let response = request.recv_response().await.unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    assert_eq!(collect_h3_body(&mut request).await, b"plan-192-data");
    // Close immediately after the complete exchange; the already-observed
    // bytes must remain valid and the close must surface as a connection
    // event rather than retroactively failing the request.
    connection.close(VarInt::from_u32(0), b"race close");
    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;

    // The server endpoint must remain usable for a fresh connection.
    let (endpoint2, _connection2, mut driver2, mut sender2) = connect_h3(addr, certificate).await;
    let driver_task2 =
        tokio::spawn(async move { future::poll_fn(|cx| driver2.poll_close(cx)).await });
    let mut retry = sender2
        .send_request(
            hyper::Request::builder()
                .uri("https://localhost/again")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    retry.finish().await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), retry.recv_response())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    assert_eq!(collect_h3_body(&mut retry).await, b"plan-192-data");
    drop(sender2);
    endpoint2.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task2).await;

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

/// Plan 194: a stalled `ResponseStream` producer must hit the per-stream
/// `response_write_timeout` no-progress deadline (stream reset), not park the
/// H3 request task indefinitely. A sibling stream on the same connection must
/// still complete normally.
#[tokio::test]
async fn h3_stalled_response_producer_times_out_and_sibling_survives() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let service = service_fn(|request: Request| async move {
        if request.head().target().path() == "/stalled" {
            let pending = futures_util::stream::pending::<Result<Bytes, ResponseStreamError>>();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Stream(ResponseStream::new(pending)))
                .unwrap())
        } else {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"sibling ok".to_vec()))
                .unwrap())
        }
    });
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .response_write_timeout(Duration::from_millis(100))
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

    // Headers are sent before the stalled body, so the client observes 200
    // and then a stream reset once the producer no-progress deadline fires.
    let mut stalled = sender
        .send_request(
            hyper::Request::builder()
                .uri("https://localhost/stalled")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    stalled.finish().await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), stalled.recv_response())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    let stalled_outcome = tokio::time::timeout(Duration::from_secs(2), stalled.recv_data()).await;
    let stalled_outcome =
        stalled_outcome.expect("stalled H3 producer must terminate via producer timeout, not hang");
    // The stalled stream must terminate without yielding application bytes:
    // either a reset error or a clean end-of-stream, never a data chunk.
    match stalled_outcome {
        Ok(None) => {}
        Err(_) => {}
        Ok(Some(_)) => panic!("stalled H3 producer must not yield bytes"),
    }

    let mut sibling = sender
        .send_request(
            hyper::Request::builder()
                .uri("https://localhost/sibling")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    sibling.finish().await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), sibling.recv_response())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    assert_eq!(collect_h3_body(&mut sibling).await, b"sibling ok");

    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// Plan 194 Track G: a producer that yields one real chunk and then parks is
/// bounded from its last meaningful progress point, not from response
/// creation. The client observes the first chunk, then the stream terminates.
#[tokio::test]
async fn h3_producer_stall_after_progress_times_out_from_last_progress() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let service = service_fn(|_request: Request| async move {
        let first = futures_util::stream::iter(vec![Ok::<_, ResponseStreamError>(
            Bytes::from_static(b"first"),
        )])
        .chain(futures_util::stream::pending());
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(ResponseStream::new(first)))
            .unwrap())
    });
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .response_write_timeout(Duration::from_millis(150))
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
                .uri("https://localhost/progress-then-stall")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    request.finish().await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), request.recv_response())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    // The first real chunk must reach the client before the stall terminates
    // the stream.
    let first = tokio::time::timeout(Duration::from_secs(2), request.recv_data())
        .await
        .expect("first chunk should arrive before the producer deadline")
        .expect("stream should still be open for the first chunk");
    assert!(first.is_some());
    // After the stall, the stream must terminate without further data.
    let stalled = tokio::time::timeout(Duration::from_secs(2), request.recv_data()).await;
    let stalled =
        stalled.expect("stalled-after-progress producer must terminate via producer timeout");
    match stalled {
        Ok(None) => {}
        Err(_) => {}
        Ok(Some(_)) => panic!("no further bytes expected after the producer stall"),
    }

    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// Plan 194 Track G: slow but steadily progressing producers never spuriously
/// time out. Total stream duration exceeds one timeout interval while every
/// inter-chunk gap stays below it.
#[tokio::test]
async fn h3_slow_progressing_producer_completes_beyond_one_timeout_interval() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let service = service_fn(|_request: Request| async move {
        let slow = futures_util::stream::unfold(0u32, |count| async move {
            if count >= 4 {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
            Some((
                Ok::<_, ResponseStreamError>(Bytes::from_static(b"x")),
                count + 1,
            ))
        });
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(ResponseStream::new(slow)))
            .unwrap())
    });
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .response_write_timeout(Duration::from_millis(200))
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
                .uri("https://localhost/slow")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    request.finish().await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(5), request.recv_response())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    // Four 60 ms gaps total ~240 ms, beyond one 200 ms interval: completion
    // proves no-progress rather than total-duration semantics.
    let body = tokio::time::timeout(Duration::from_secs(5), collect_h3_body(&mut request))
        .await
        .expect("slow-but-progressing stream must complete");
    assert_eq!(body, b"xxxx");

    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// Plan 194 Track G (E1): empty chunks followed by real data within the
/// original deadline succeed; empty chunks do not alter body accounting.
#[tokio::test]
async fn h3_empty_chunks_then_data_within_deadline_succeed() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let service = service_fn(|_request: Request| async move {
        let stream = futures_util::stream::iter(vec![
            Ok::<_, ResponseStreamError>(Bytes::new()),
            Ok::<_, ResponseStreamError>(Bytes::new()),
            Ok::<_, ResponseStreamError>(Bytes::from_static(b"data")),
        ]);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(ResponseStream::new(stream)))
            .unwrap())
    });
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .response_write_timeout(Duration::from_millis(300))
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
                .uri("https://localhost/empty-then-data")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    request.finish().await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), request.recv_response())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    let body = tokio::time::timeout(Duration::from_secs(2), collect_h3_body(&mut request))
        .await
        .expect("empty chunks followed by prompt data must succeed");
    assert_eq!(body, b"data");

    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// Plan 194 Track G (E2): empty chunks cannot refresh the producer deadline.
/// A producer that yields empty chunks and then parks times out against the
/// original meaningful-progress point.
#[tokio::test]
async fn h3_empty_chunks_do_not_refresh_producer_deadline() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let service = service_fn(|_request: Request| async move {
        let stream = futures_util::stream::iter(vec![
            Ok::<_, ResponseStreamError>(Bytes::new()),
            Ok::<_, ResponseStreamError>(Bytes::new()),
        ])
        .chain(futures_util::stream::pending());
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(ResponseStream::new(stream)))
            .unwrap())
    });
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .response_write_timeout(Duration::from_millis(150))
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
                .uri("https://localhost/empty-then-stall")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    request.finish().await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), request.recv_response())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    // Empty chunks carry no bytes, so the client must observe termination
    // (reset or EOS) rather than an indefinite live stream.
    let outcome = tokio::time::timeout(Duration::from_secs(2), request.recv_data()).await;
    let outcome = outcome.expect("empty-then-parked producer must terminate via producer timeout");
    match outcome {
        Ok(None) => {}
        Err(_) => {}
        Ok(Some(_)) => panic!("empty chunks must not yield application bytes"),
    }

    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

/// Plan 195 Track J: a producer no-progress timeout is observable as exactly
/// one `WriteStallTimeout` — never as `ResponseStreamCompleted` or a producer
/// error — and releases its service/file permits.
#[tokio::test]
async fn h3_stalled_producer_timeout_observes_write_stall_and_releases_permits() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let ops = OpsContext::default();
    let service = service_fn(|_request: Request| async move {
        let pending = futures_util::stream::pending::<Result<Bytes, ResponseStreamError>>();
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(ResponseStream::new(pending)))
            .unwrap())
    });
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .response_write_timeout(Duration::from_millis(100))
        .http3(Http3Config {
            enabled: true,
            ..Http3Config::default()
        })
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .http3_identity(&cert_path, &key_path)
        .ops_context(ops.clone())
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
                .uri("https://localhost/stalled")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    request.finish().await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), request.recv_response())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    let outcome = tokio::time::timeout(Duration::from_secs(2), request.recv_data()).await;
    let outcome =
        outcome.expect("stalled H3 producer must terminate via producer timeout, not hang");
    match outcome {
        Ok(None) => {}
        Err(_) => {}
        Ok(Some(_)) => panic!("stalled H3 producer must not yield bytes"),
    }

    // The post-commit failure path observes one write-stall timeout. It must
    // not look like a normal stream completion or an explicit producer error.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let snap = loop {
        let snap = ops.snapshot();
        if snap.write_stall_timeouts == 1 {
            break snap;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "producer timeout must observe exactly one write-stall timeout"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    assert_eq!(snap.streaming_completed, 0);
    assert_eq!(snap.stream_producer_errors, 0);
    assert_eq!(snap.active_service_requests, 0);
    assert_eq!(snap.active_file_streams, 0);

    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;
    handle.shutdown();
    handle.wait().await.unwrap();
    let snap = ops.snapshot();
    assert_eq!(snap.active_connections, 0);
    assert_eq!(snap.active_service_requests, 0);
    assert_eq!(snap.active_file_streams, 0);
    assert_eq!(snap.write_stall_timeouts, 1);
}

/// Plan 195 Track I: graceful shutdown started while a producer is parked
/// stays authoritative — the drain deadline bounds the wait, no request task
/// survives it, lifecycle cancellation is first-reason-wins, and the parked
/// producer timeout never fires afterwards.
#[tokio::test]
async fn h3_stalled_producer_shutdown_race_drains_without_surviving_tasks() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path, certificate) = write_identity(&identity_dir);
    let ops = OpsContext::default();
    let (lifecycle_tx, lifecycle_rx) = tokio::sync::oneshot::channel();
    let lifecycle_tx = Arc::new(std::sync::Mutex::new(Some(lifecycle_tx)));
    let service = service_fn(move |request: Request| {
        let lifecycle = request.lifecycle_clone();
        let lifecycle_tx = lifecycle_tx.clone();
        async move {
            if let Some(sender) = lifecycle_tx.lock().unwrap().take() {
                let _ = sender.send(lifecycle.clone());
            }
            let pending = futures_util::stream::pending::<Result<Bytes, ResponseStreamError>>();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Stream(ResponseStream::new(pending)))
                .unwrap())
        }
    });
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
        // Producer budget far beyond the graceful deadline so shutdown wins
        // the race deterministically; either ordering would be acceptable.
        .response_write_timeout(Duration::from_secs(10))
        .graceful_shutdown_timeout(Duration::from_millis(100))
        .http3(Http3Config {
            enabled: true,
            ..Http3Config::default()
        })
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .http3_identity(&cert_path, &key_path)
        .ops_context(ops.clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let (endpoint, _connection, mut driver, mut sender) =
        connect_h3(handle.local_addr(), certificate).await;
    let driver_task =
        tokio::spawn(async move { future::poll_fn(|cx| driver.poll_close(cx)).await });

    // Response HEADERS commit, so the producer is parked post-commit.
    let mut request = sender
        .send_request(
            hyper::Request::builder()
                .uri("https://localhost/stalled")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    request.finish().await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), request.recv_response())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), hyper::StatusCode::OK);
    let lifecycle = tokio::time::timeout(Duration::from_secs(1), lifecycle_rx)
        .await
        .unwrap()
        .unwrap();

    handle.shutdown();
    let result = tokio::time::timeout(Duration::from_secs(5), handle.wait())
        .await
        .expect("shutdown must drain the parked producer task via the graceful deadline");
    let result = result.unwrap();
    assert!(
        matches!(
            result,
            eggserve_core::server::ShutdownResult::Timeout
                | eggserve_core::server::ShutdownResult::Forced
        ),
        "parked producer must exceed the short graceful deadline, got {result}"
    );
    tokio::time::timeout(Duration::from_secs(1), lifecycle.cancelled())
        .await
        .expect("shutdown must cancel the parked request lifecycle");
    assert_eq!(
        lifecycle.cancellation_reason(),
        Some(eggserve_core::primitives::RequestCancellationReason::ServerShutdown)
    );

    // The producer timeout never fired, and nothing leaked or double-released.
    let snap = ops.snapshot();
    assert_eq!(snap.write_stall_timeouts, 0);
    assert_eq!(snap.active_connections, 0);
    assert_eq!(snap.active_service_requests, 0);
    assert_eq!(snap.active_file_streams, 0);

    drop(sender);
    endpoint.close(VarInt::from_u32(0), b"test complete");
    let _ = tokio::time::timeout(Duration::from_secs(1), driver_task).await;
}
