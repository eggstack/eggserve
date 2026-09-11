//! Plan 201 listener ownership and process-manager integration.
//!
//! Covers address-bound vs prebound TCP parity, standard-library listener
//! adoption, Unix-domain serving with truthful endpoint metadata (Unix
//! only), combined TCP+Unix endpoints with stable IDs, explicit TLS/H3
//! Unix rules, and H3 prebound-UDP validation.

use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::request::Request;
use eggserve_core::server::{service_fn, RuntimeConfig, Server};

fn hello_service() -> impl eggserve_core::server::Service {
    service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap())
    })
}

fn test_config() -> RuntimeConfig {
    RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap()
}

async fn tcp_get(addr: std::net::SocketAddr) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).await.unwrap();
    String::from_utf8_lossy(&buf).into_owned()
}

#[tokio::test]
async fn address_bound_and_prebound_tcp_have_parity() {
    // Address-bound baseline.
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(hello_service()).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();
    let body = tcp_get(addr).await;
    assert!(body.starts_with("HTTP/1.1 200 OK"), "{body}");
    assert!(body.contains("hello"));
    assert_eq!(handle.endpoints().len(), 1);
    assert_eq!(handle.endpoints()[0].id(), "tcp-0");
    handle.shutdown();
    handle.wait().await.unwrap();

    // Prebound Tokio listener: same pipeline, actual addr in readiness.
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
    let body = tcp_get(expected).await;
    assert!(body.starts_with("HTTP/1.1 200 OK"), "{body}");
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn std_listener_adoption_preserves_socket() {
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let expected = std_listener.local_addr().unwrap();
    let server = Server::builder()
        .runtime(test_config())
        .from_std_listener(std_listener)
        .unwrap()
        .build()
        .unwrap();
    let handle = server.start_with_service(hello_service()).await.unwrap();
    handle.ready().await.unwrap();
    // No duplicate bind: the advertised address is the caller's address.
    assert_eq!(handle.local_addr(), expected);
    let body = tcp_get(expected).await;
    assert!(body.starts_with("HTTP/1.1 200 OK"), "{body}");
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_only_serves_http_with_truthful_endpoints() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("eggserve.sock");
    let unix_listener = tokio::net::UnixListener::bind(&path).unwrap();

    let server = Server::builder()
        .runtime(test_config())
        .from_unix_listener(unix_listener)
        .build()
        .unwrap();
    let handle = server.start_with_service(hello_service()).await.unwrap();
    handle.ready().await.unwrap();

    // Unix-only: no fabricated IP endpoints.
    assert_eq!(handle.endpoints().len(), 1);
    assert_eq!(handle.endpoints()[0].id(), "unix-0");
    assert!(handle.tcp_local_addr().is_none());

    let mut client = tokio::net::UnixStream::connect(&path).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).await.unwrap();
    let body = String::from_utf8_lossy(&buf);
    assert!(body.starts_with("HTTP/1.1 200 OK"), "{body}");
    assert!(body.contains("hello"));

    handle.shutdown();
    handle.wait().await.unwrap();
    // Caller owns path removal; EggServe never unlinks.
    assert!(path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn std_unix_listener_adoption_serves() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("std.sock");
    let std_listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

    let server = Server::builder()
        .runtime(test_config())
        .from_std_unix_listener(std_listener)
        .unwrap()
        .build()
        .unwrap();
    let handle = server.start_with_service(hello_service()).await.unwrap();
    handle.ready().await.unwrap();

    let mut client = tokio::net::UnixStream::connect(&path).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).await.unwrap();
    let body = String::from_utf8_lossy(&buf);
    assert!(body.starts_with("HTTP/1.1 200 OK"), "{body}");

    handle.shutdown();
    handle.wait().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn tcp_and_unix_combined_expose_stable_ids() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("combo.sock");
    let unix_listener = tokio::net::UnixListener::bind(&path).unwrap();
    let tcp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tcp_addr = tcp_listener.local_addr().unwrap();

    let server = Server::builder()
        .runtime(test_config())
        .from_listener(tcp_listener)
        .from_unix_listener(unix_listener)
        .build()
        .unwrap();
    let handle = server.start_with_service(hello_service()).await.unwrap();
    handle.ready().await.unwrap();

    assert_eq!(handle.endpoints().len(), 2);
    assert_eq!(handle.endpoints()[0].id(), "tcp-0");
    assert_eq!(handle.endpoints()[1].id(), "unix-0");
    assert_eq!(handle.local_addr(), tcp_addr);

    let tcp_body = tcp_get(tcp_addr).await;
    assert!(tcp_body.starts_with("HTTP/1.1 200 OK"));

    let mut client = tokio::net::UnixStream::connect(&path).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).await.unwrap();
    assert!(String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200 OK"));

    handle.shutdown();
    handle.wait().await.unwrap();
}

#[cfg(all(unix, feature = "tls"))]
#[tokio::test]
async fn unix_only_with_tls_fails_closed() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tls.sock");
    let unix_listener = tokio::net::UnixListener::bind(&path).unwrap();

    // Build a minimal self-signed identity for the config check.
    let key_pair = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    let cert = params.self_signed(&key_pair).unwrap();
    let certs = vec![cert.der().clone()];
    let key = rustls_pki_types::PrivateKeyDer::Pkcs8(rustls_pki_types::PrivatePkcs8KeyDer::from(
        key_pair.serialize_der(),
    ));
    let tls_config = std::sync::Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .unwrap(),
    );
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .tls_config(tls_config)
        .build()
        .unwrap();
    let err = match Server::builder()
        .runtime(config)
        .from_unix_listener(unix_listener)
        .build()
    {
        Err(e) => e,
        Ok(_) => panic!("unix-only TLS build must fail"),
    };
    assert!(err.to_string().contains("TLS requires a TCP listener"));
}

#[cfg(feature = "http3")]
#[tokio::test]
async fn prebound_udp_without_h3_enabled_fails_closed() {
    let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let server = Server::builder()
        .runtime(test_config())
        .http3_socket(udp)
        .build()
        .unwrap();
    let err = server
        .start_with_service(hello_service())
        .await
        .unwrap_err();
    assert!(err
        .to_string()
        .contains("prebound H3 UDP socket supplied but http3 is not enabled"));
}

#[cfg(feature = "http3")]
#[tokio::test]
async fn prebound_udp_port_mismatch_fails_with_validation() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let key_pair = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    let certificate = params.self_signed(&key_pair).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let cert_path = dir.path().join("cert.pem");
    let key_path = dir.path().join("key.pem");
    std::fs::write(&cert_path, certificate.pem()).unwrap();
    std::fs::write(&key_path, key_pair.serialize_pem()).unwrap();

    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tcp_port = tcp.local_addr().unwrap().port();
    // Bind UDP to a port guaranteed different from TCP.
    let mut udp_port = tcp_port.wrapping_add(1);
    if udp_port == 0 {
        udp_port = 1;
    }
    let udp = std::net::UdpSocket::bind(format!("127.0.0.1:{udp_port}")).unwrap();

    let http3 = eggserve_core::server::Http3Config {
        enabled: true,
        ..Default::default()
    };
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .http3(http3)
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(config)
        .from_listener(tcp)
        .http3_identity(&cert_path, &key_path)
        .http3_socket(udp)
        .build()
        .unwrap();
    let err = server
        .start_with_service(hello_service())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("does not match TCP port"));
}
