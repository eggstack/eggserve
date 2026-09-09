#![cfg(feature = "http3")]

use std::net::SocketAddr;

use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::server::{service_fn, Http3Config, Request, RuntimeConfig, Server};
use tempfile::TempDir;
use tokio::net::UdpSocket;

fn write_identity(dir: &TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
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
    (cert_path, key_path)
}

#[tokio::test]
async fn http3_binds_udp_to_the_tcp_port_and_shuts_down_together() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity_dir = TempDir::new().unwrap();
    let (cert_path, key_path) = write_identity(&identity_dir);
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
