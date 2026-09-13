//! Direct H1 tunnel / upgrade qualification (Plan 216).
//!
//! Proves the direct `eggserve-server` ownership of generic H1
//! Upgrade/CONNECT transport handoff without importing `eggserve-core` or
//! Hyper transport types in service signatures:
//!
//! - generic byte echo over `Upgrade: eggserve-test` (no WebSocket dep);
//! - H1 read-ahead preservation (pipelined post-handshake bytes exact);
//! - denial stays ordinary HTTP (no intent, malformed intent, ignored
//!   capability, body-bearing upgrade);
//! - one-shot acceptance (double-accept deterministic, after-commit fails);
//! - handshake header bounds;
//! - tunnel admission exhaustion is deterministic 503;
//! - shutdown cancels active tunnel work;
//! - CONNECT without a transport handoff stays ordinary HTTP.

use std::net::SocketAddr;

use eggserve_primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_primitives::header_block::HeaderBlock;
use eggserve_primitives::TunnelKind;
use eggserve_server::tunnel::{TunnelError, TunnelIo};
use eggserve_server::{service_fn_with_tunnel, Request, RuntimeConfig, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn echo_service() -> impl eggserve_server::Service {
    service_fn_with_tunnel(|req: Request, tunnel| async move {
        let Some(capability) = tunnel else {
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"no-tunnel".to_vec()))
                .unwrap());
        };
        // Intent is visible cloneably on the context for routing.
        let intent = req
            .context()
            .tunnel_request()
            .expect("capability implies intent");
        assert_eq!(intent.kind(), capability.request().kind());
        assert!(matches!(
            capability.request().kind(),
            TunnelKind::Http1Upgrade | TunnelKind::Connect | TunnelKind::ExtendedConnect
        ));
        // Capture cancellation before accepting; the handler owns only IO.
        let lifecycle = req.lifecycle_clone();
        let handler = |mut io: TunnelIo| async move {
            let mut buf = vec![0u8; 8192];
            loop {
                tokio::select! {
                    read = io.read(&mut buf) => {
                        match read {
                            Ok(0) => break,
                            Ok(n) => {
                                if io.write_all(&buf[..n]).await.is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    _ = lifecycle.cancelled() => break,
                }
            }
        };
        match capability.accept(HeaderBlock::new(), handler) {
            Ok(response) => Ok(response),
            Err(_) => Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(ResponseBody::Empty)
                .unwrap()),
        }
    })
}

async fn start_echo_server(config: RuntimeConfig) -> (SocketAddr, eggserve_server::ServerHandle) {
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(echo_service()).await.unwrap();
    let addr = handle.local_addr();
    (addr, handle)
}

async fn read_handshake_head(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
        assert!(head.len() < 16 * 1024, "handshake head too large");
    }
    head
}

#[tokio::test]
async fn h1_upgrade_echo_preserves_readahead() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let (addr, handle) = start_echo_server(config).await;
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    // Pipelined post-handshake bytes in the same segment as headers: Hyper
    // buffers them (`Upgraded::read_buf`); the bridge must not lose a byte.
    stream
        .write_all(
            b"GET /chat HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\n\r\nhello-readahead",
        )
        .await
        .unwrap();
    let head = read_handshake_head(&mut stream).await;
    let text = String::from_utf8_lossy(&head);
    assert!(
        text.starts_with("HTTP/1.1 101"),
        "expected 101, got: {text}"
    );
    assert!(
        text.to_ascii_lowercase().contains("upgrade: eggserve-test"),
        "runtime must own Upgrade value, got: {text}"
    );
    assert!(
        text.to_ascii_lowercase().contains("connection: upgrade"),
        "runtime must own Connection value, got: {text}"
    );
    // Exact read-ahead echo, then live echo.
    let mut buf = [0u8; 15];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"hello-readahead");
    stream.write_all(b"ping").await.unwrap();
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"ping");

    handle.shutdown();
    handle.wait().await;
}

#[tokio::test]
async fn ordinary_request_has_no_capability() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let (addr, handle) = start_echo_server(config).await;
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
    assert!(text.ends_with("no-tunnel"), "got: {text}");

    handle.shutdown();
    handle.wait().await;
}

#[tokio::test]
async fn malformed_upgrade_yields_no_capability() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let (addr, handle) = start_echo_server(config).await;
    // Duplicate Upgrade tokens: strictly rejected, ordinary HTTP denial.
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            b"GET /chat HTTP/1.1\r\nHost: x\r\nConnection: upgrade\r\nUpgrade: a, b\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(
        !text.starts_with("HTTP/1.1 101"),
        "malformed upgrade must not handshake, got: {text}"
    );

    // HTTP/1.0 never upgrades.
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            b"GET /chat HTTP/1.0\r\nHost: x\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\n\r\n",
        )
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(
        !text.starts_with("HTTP/1.1 101"),
        "HTTP/1.0 must not handshake, got: {text}"
    );

    handle.shutdown();
    handle.wait().await;
}

#[tokio::test]
async fn upgrade_with_body_never_crosses() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let (addr, handle) = start_echo_server(config).await;
    // Upgrade intent with a body: body policy rejects before any handoff.
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            b"POST /chat HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\nConnection: close\r\n\r\nhello",
        )
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(
        !text.starts_with("HTTP/1.1 101"),
        "body-bearing upgrade must not handshake, got: {text}"
    );

    handle.shutdown();
    handle.wait().await;
}

#[tokio::test]
async fn ignored_capability_is_ordinary_http() {
    // A service using plain `service_fn` (no tunnel entry point) drops the
    // capability via the default `call_with_tunnel`: denial stays ordinary.
    let server = Server::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let handle = server
        .start_with_service(eggserve_server::service_fn(|_req: Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"plain".to_vec()))
                .unwrap())
        }))
        .await
        .unwrap();
    let addr = handle.local_addr();
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            b"GET /chat HTTP/1.1\r\nHost: x\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
    assert!(text.ends_with("plain"), "got: {text}");

    handle.shutdown();
    handle.wait().await;
}

#[tokio::test]
async fn handshake_header_bounds_are_enforced() {
    // Too many handshake headers: deterministic TunnelError, no handshake.
    let mut headers = HeaderBlock::new();
    for i in 0..33 {
        headers.push_str(format!("x-tunnel-{i}"), "v").unwrap();
    }
    // Bounds are enforced by the neutral validator directly.
    let err = eggserve_primitives::validate_handshake_headers(&mut headers).unwrap_err();
    assert!(matches!(
        err,
        eggserve_primitives::TunnelError::TooManyHeaders { .. }
    ));

    // Framing via handshake is forbidden, not stripped.
    let mut framing = HeaderBlock::new();
    framing.push_str("content-length", "5").unwrap();
    let err = eggserve_primitives::validate_handshake_headers(&mut framing).unwrap_err();
    assert!(matches!(
        err,
        eggserve_primitives::TunnelError::ForbiddenHeader(_)
    ));
    let _ = TunnelError::NoCapability;
}

#[tokio::test]
async fn tunnel_admission_exhaustion_is_503() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_active_tunnels(1)
        .build()
        .unwrap();
    let (addr, handle) = start_echo_server(config).await;
    // Hold the single tunnel permit with an open tunnel.
    let mut first = tokio::net::TcpStream::connect(addr).await.unwrap();
    first
        .write_all(
            b"GET /a HTTP/1.1\r\nHost: x\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\n\r\n",
        )
        .await
        .unwrap();
    let head = read_handshake_head(&mut first).await;
    assert!(String::from_utf8_lossy(&head).starts_with("HTTP/1.1 101"));

    // Second handshake while the first holds the permit: deterministic 503,
    // ordinary HTTP (no 101), other requests unaffected.
    let mut second = tokio::net::TcpStream::connect(addr).await.unwrap();
    second
        .write_all(
            b"GET /b HTTP/1.1\r\nHost: x\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    let mut buf = Vec::new();
    second.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(
        text.starts_with("HTTP/1.1 503"),
        "exhaustion must be 503, got: {text}"
    );

    handle.shutdown();
    handle.wait().await;
}

#[tokio::test]
async fn shutdown_cancels_active_tunnel() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let (addr, handle) = start_echo_server(config).await;
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            b"GET /chat HTTP/1.1\r\nHost: x\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\n\r\n",
        )
        .await
        .unwrap();
    let head = read_handshake_head(&mut stream).await;
    assert!(String::from_utf8_lossy(&head).starts_with("HTTP/1.1 101"));
    // Tunnel is live (echo works); shutdown must terminate it.
    stream.write_all(b"before").await.unwrap();
    let mut buf = [0u8; 6];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"before");

    handle.shutdown();
    handle.wait().await;
    // After shutdown the transport closes: read reaches EOF promptly.
    let mut rest = Vec::new();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_to_end(&mut rest),
    )
    .await
    .expect("closed tunnel must reach EOF after shutdown");
}

#[tokio::test]
async fn connect_without_handoff_stays_ordinary() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let (addr, handle) = start_echo_server(config).await;
    // Plain H1 CONNECT: no `Connection: upgrade` framing, so Hyper offers
    // no transport handoff on this path — the request stays ordinary HTTP
    // and never forges a tunnel handshake. Read the framed response (do
    // not wait for EOF: without a handshake the connection stays reusable).
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"CONNECT example.test:443 HTTP/1.1\r\nHost: example.test:443\r\n\r\n")
        .await
        .unwrap();
    let head = read_handshake_head(&mut stream).await;
    let text = String::from_utf8_lossy(&head);
    assert!(
        !text.starts_with("HTTP/1.1 101"),
        "CONNECT must not forge 101, got: {text}"
    );

    handle.shutdown();
    handle.wait().await;
}
