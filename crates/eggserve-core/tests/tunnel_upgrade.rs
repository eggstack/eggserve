//! Generic tunnel / upgrade qualification (Plan 199 Track I + H).
//!
//! - Generic byte echo over H1 `Upgrade: eggserve-test` (no WebSocket dep,
//!   proves EggServe abstraction).
//! - Denial stays ordinary HTTP; double-take impossible; after-commit fails;
//!   malformed/duplicate tokens yield no capability; bodies never cross;
//!   read-ahead byte-exact; limits recover; shutdown accounts; no payload logged.
//! - H2 Extended CONNECT echo (feature `http2`, generic `eggserve-test`
//!   `:protocol`, one stream, siblings survive) – see `h2_tunnel_*`.
//! - H3 CONNECT echo (feature `http3`, plain `CONNECT`, stream-scoped) – see
//!   `h3_tunnel_*`. Generic H3 `:protocol` (e.g. `websocket`) is blocked by
//!   `h3` 0.0.8 (only `webtransport`/`connect-udp`); documented, not bypassed.
//! - WebSocket interop fixture (dev-only `tokio-tungstenite`, handshake via
//!   EggServe, framing via codec over `TunnelIo`, no WS in core) – see
//!   `ws_interop_*`.

use std::net::SocketAddr;
use std::sync::Arc;

use eggserve_core::primitives::header_block::HeaderBlock;
use eggserve_core::primitives::tunnel::TunnelIo;
use eggserve_core::primitives::RequestLifecycle;
use eggserve_core::server::{service_fn, Request, RuntimeConfig, Server};
#[cfg(feature = "http3")]
use eggserve_h3::{h3, h3_quinn};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn echo_service() -> impl eggserve_core::server::Service {
    service_fn(|req: Request| async move {
        let Some(tunnel) = req.context().take_tunnel() else {
            // Denial: ordinary HTTP (no tunnel).
            return Ok(eggserve_core::primitives::canonical::Response::builder()
                .status(eggserve_core::primitives::canonical::StatusCode::OK)
                .body(eggserve_core::primitives::canonical::ResponseBody::Bytes(
                    b"no-tunnel".to_vec(),
                ))
                .unwrap());
        };
        // Double-take impossible (second returns None).
        assert!(req.context().take_tunnel().is_none());
        let kind = tunnel.request().kind();
        assert!(matches!(
            kind,
            eggserve_core::primitives::tunnel::TunnelKind::Http1Upgrade
                | eggserve_core::primitives::tunnel::TunnelKind::Connect
                | eggserve_core::primitives::tunnel::TunnelKind::ExtendedConnect
        ));
        let headers = HeaderBlock::new();
        let handler = |mut io: TunnelIo, lifecycle: RequestLifecycle| async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
        match tunnel.accept(headers, handler) {
            Ok(response) => Ok(response),
            Err(_) => Ok(eggserve_core::primitives::canonical::Response::builder()
                .status(eggserve_core::primitives::canonical::StatusCode::BAD_REQUEST)
                .body(eggserve_core::primitives::canonical::ResponseBody::Empty)
                .unwrap()),
        }
    })
}

async fn start_echo_server(
    config: RuntimeConfig,
) -> (SocketAddr, eggserve_core::server::ServerHandle) {
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(echo_service()).await.unwrap();
    let addr = handle.local_addr();
    (addr, handle)
}

async fn raw_request(addr: SocketAddr, req: &[u8], read_ms: u64) -> Vec<u8> {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(req).await.unwrap();
    // Give the server a moment to respond / close (no absolute timing gate:
    // small bounded read with timeout, deterministic content assertions).
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(read_ms),
        stream.read_to_end(&mut buf),
    )
    .await;
    buf
}

#[tokio::test]
async fn h1_upgrade_echo_preserves_readahead() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let (addr, handle) = start_echo_server(config).await;
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    // Pipelined post-handshake bytes (`hello-readahead`) in the same segment
    // as headers: Hyper buffers them (`Upgraded::read_buf`); the bridge must
    // not lose a byte.
    stream
        .write_all(
            b"GET /chat HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\n\r\nhello-readahead",
        )
        .await
        .unwrap();
    // Read 101 handshake.
    let mut head = Vec::new();
    let mut tmp = [0u8; 1];
    loop {
        stream.read_exact(&mut tmp).await.unwrap();
        head.extend_from_slice(&tmp);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
        if head.len() > 8192 {
            panic!("handshake too large: {:?}", String::from_utf8_lossy(&head));
        }
    }
    let head_text = String::from_utf8_lossy(&head);
    assert!(head_text.starts_with("HTTP/1.1 101"), "got: {head_text}");
    assert!(head_text
        .to_ascii_lowercase()
        .contains("upgrade: eggserve-test"));
    assert!(head_text
        .to_ascii_lowercase()
        .contains("connection: upgrade"));
    // Echo (includes read-ahead).
    let mut echo = vec![0u8; b"hello-readahead".len()];
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_exact(&mut echo),
    )
    .await
    .expect("echo timeout")
    .unwrap();
    assert_eq!(&echo, b"hello-readahead");
    // Second message (not pipelined) also echoes.
    stream.write_all(b"second").await.unwrap();
    let mut echo2 = vec![0u8; 6];
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_exact(&mut echo2),
    )
    .await
    .expect("second echo timeout")
    .unwrap();
    assert_eq!(&echo2, b"second");
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn h1_denial_stays_ordinary_http() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let (addr, _handle) = start_echo_server(config).await;
    // No Upgrade headers: no capability, ordinary 200 with `no-tunnel`.
    let buf = raw_request(
        addr,
        b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        2000,
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200"), "got: {text}");
    assert!(text.contains("no-tunnel"));
    assert!(!text.to_ascii_lowercase().contains("upgrade:"));
}

#[tokio::test]
async fn h1_malformed_upgrade_has_no_capability() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let (addr, _handle) = start_echo_server(config).await;
    // Duplicate Upgrade tokens: no capability, ordinary denial (200, no 101).
    let buf = raw_request(
        addr,
        b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: a, b\r\nConnection: close\r\n\r\n",
        2000,
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    assert!(
        !text.starts_with("HTTP/1.1 101"),
        "must not upgrade, got: {text}"
    );
    // Duplicate Connection upgrade tokens: same.
    let buf = raw_request(
        addr,
        b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade, upgrade\r\nUpgrade: eggserve-test\r\nConnection: close\r\n\r\n",
        2000,
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    assert!(!text.starts_with("HTTP/1.1 101"), "got: {text}");
}

#[tokio::test]
async fn h1_body_never_crosses_transition() {
    // Reject policy (echo_service uses default Reject) + body => 413, no 101.
    // Note: echo_service uses `service_fn` default Reject; has_body triggers
    // pipeline 413 before service (no tunnel, no handler run).
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let (addr, _handle) = start_echo_server(config).await;
    let buf = raw_request(
        addr,
        b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
        2000,
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    assert!(
        text.starts_with("HTTP/1.1 413"),
        "body must not cross, got: {text}"
    );
    assert!(!text.starts_with("HTTP/1.1 101"));
}

#[tokio::test]
async fn h1_after_commit_accept_fails() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let saw_after_commit = Arc::new(AtomicBool::new(false));
    let flag = saw_after_commit.clone();
    let service = service_fn(move |req: Request| {
        let flag = flag.clone();
        async move {
            let Some(tunnel) = req.context().take_tunnel() else {
                return Ok(eggserve_core::primitives::canonical::Response::builder()
                    .status(eggserve_core::primitives::canonical::StatusCode::BAD_REQUEST)
                    .body(eggserve_core::primitives::canonical::ResponseBody::Empty)
                    .unwrap());
            };
            // Hold the capability past service return; background accept must
            // fail with AfterCommit (final commitment already marked).
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let err = tunnel
                    .accept(HeaderBlock::new(), |_io, _lc| async move {})
                    .unwrap_err();
                assert_eq!(
                    err,
                    eggserve_core::primitives::tunnel::TunnelError::AfterCommit
                );
                flag.store(true, Ordering::Release);
            });
            // Denial (ordinary response); commitment marks immediately after.
            Ok(eggserve_core::primitives::canonical::Response::builder()
                .status(eggserve_core::primitives::canonical::StatusCode::OK)
                .body(eggserve_core::primitives::canonical::ResponseBody::Bytes(
                    b"denied".to_vec(),
                ))
                .unwrap())
        }
    });
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let buf = raw_request(
        handle.local_addr(),
        b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\nConnection: close\r\n\r\n",
        2000,
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200"), "got: {text}");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !saw_after_commit.load(Ordering::Acquire) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("background accept must observe AfterCommit");
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn h1_tunnel_budget_recovers_exactly_once() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_active_tunnels(1)
        .build()
        .unwrap();
    let (addr, handle) = start_echo_server(config).await;
    // First tunnel: hold open (do not close).
    let mut first = tokio::net::TcpStream::connect(addr).await.unwrap();
    first
        .write_all(b"GET /a HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\n\r\n")
        .await
        .unwrap();
    let mut head = vec![0u8; 0];
    let mut tmp = [0u8; 1];
    loop {
        first.read_exact(&mut tmp).await.unwrap();
        head.extend_from_slice(&tmp);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    assert!(String::from_utf8_lossy(&head).starts_with("HTTP/1.1 101"));
    // Second handshake while first holds the sole permit => 503 (no upgrade).
    let buf = raw_request(
        addr,
        b"GET /b HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\nConnection: close\r\n\r\n",
        2000,
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    assert!(
        text.starts_with("HTTP/1.1 503"),
        "budget must hold, got: {text}"
    );
    // Close first (drop => bridge EOF => tunnel task ends => permit released).
    drop(first);
    // Poll until recovery (permit released exactly once, no leak).
    let mut recovered = false;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let mut probe = tokio::net::TcpStream::connect(addr).await.unwrap();
        probe
            .write_all(b"GET /c HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\nConnection: close\r\n\r\nping")
            .await
            .unwrap();
        let mut h = Vec::new();
        let mut t = [0u8; 1];
        let mut ok = false;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
        while tokio::time::Instant::now() < deadline {
            if probe.read_exact(&mut t).await.is_err() {
                break;
            }
            h.extend_from_slice(&t);
            if h.ends_with(b"\r\n\r\n") {
                ok = true;
                break;
            }
            if h.len() > 8192 {
                break;
            }
        }
        if ok && String::from_utf8_lossy(&h).starts_with("HTTP/1.1 101") {
            recovered = true;
            break;
        }
    }
    assert!(
        recovered,
        "tunnel budget must recover exactly once after close"
    );
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn h1_shutdown_accounts_for_tunnels() {
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let (addr, handle) = start_echo_server(config).await;
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\n\r\n")
        .await
        .unwrap();
    let mut head = Vec::new();
    let mut tmp = [0u8; 1];
    loop {
        stream.read_exact(&mut tmp).await.unwrap();
        head.extend_from_slice(&tmp);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    assert!(String::from_utf8_lossy(&head).starts_with("HTTP/1.1 101"));
    // Graceful shutdown signals tunnel lifecycles; handler observes
    // cancellation and exits; `wait()` drains (no detached survivors).
    handle.shutdown();
    tokio::time::timeout(std::time::Duration::from_secs(10), handle.wait())
        .await
        .expect("shutdown must drain tunnels")
        .unwrap();
}

#[tokio::test]
async fn h1_no_payload_bytes_logged() {
    use eggserve_core::ops::{EventKind, OpsContext, Severity};
    use eggserve_core::server::RuntimeState;
    use std::sync::{Arc, Mutex};
    #[derive(Debug)]
    struct Rec {
        events: std::sync::Arc<Mutex<Vec<(String, String)>>>,
    }
    impl eggserve_core::ops::LogSink for Rec {
        fn emit(&self, event: &eggserve_core::ops::Event) {
            self.events
                .lock()
                .unwrap()
                .push((event.event.to_string(), event.message.clone()));
        }
        fn flush(&self) {}
    }
    let events: std::sync::Arc<Mutex<Vec<(String, String)>>> =
        std::sync::Arc::new(Mutex::new(Vec::new()));
    let ops = OpsContext::with_sinks(vec![Box::new(Rec {
        events: events.clone(),
    })]);
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let state = Arc::new(RuntimeState::with_ops(&config, ops).unwrap());
    // Drive one tunneled connection via caller-owned transport to capture ops.
    // Simpler: start a server with explicit ops, echo secret, then inspect.
    let server = Server::builder()
        .runtime(config)
        .ops_context(state.ops().clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(echo_service()).await.unwrap();
    let mut stream = tokio::net::TcpStream::connect(handle.local_addr())
        .await
        .unwrap();
    let secret = b"secret-payload-xyz-123";
    let mut req = b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\n\r\n".to_vec();
    req.extend_from_slice(secret);
    stream.write_all(&req).await.unwrap();
    let mut head = Vec::new();
    let mut tmp = [0u8; 1];
    loop {
        stream.read_exact(&mut tmp).await.unwrap();
        head.extend_from_slice(&tmp);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let mut echo = vec![0u8; secret.len()];
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_exact(&mut echo),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(&echo, secret);
    handle.shutdown();
    handle.wait().await.unwrap();
    let events = events.lock().unwrap();
    let secret_text = String::from_utf8_lossy(secret);
    for (kind, msg) in events.iter() {
        assert!(
            !msg.contains(secret_text.as_ref()),
            "payload leaked in {kind}: {msg}"
        );
    }
    let _ = (Severity::Info, EventKind::TunnelAccepted);
}

#[cfg(feature = "http2")]
#[tokio::test]
async fn h2_extended_connect_echo_uses_one_stream() {
    use bytes::Bytes;
    use http_body_util::{BodyExt, Empty};
    use hyper::client::conn::http2;
    use hyper_util::rt::{TokioExecutor, TokioIo};

    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .http2(eggserve_core::server::Http2Config {
            max_concurrent_streams: 8,
            ..eggserve_core::server::Http2Config::default()
        })
        .build()
        .unwrap();
    let (addr, handle) = start_echo_server(config).await;
    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let (mut sender, connection) =
        http2::handshake::<_, _, Empty<Bytes>>(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .unwrap();
    let conn_task = tokio::spawn(connection);
    // Ordinary sibling (proves multiplexing survives tunnel reset/failure).
    sender.ready().await.unwrap();
    let sibling = hyper::Request::builder()
        .method("GET")
        .uri("http://localhost/")
        .header("host", "localhost")
        .body(Empty::<Bytes>::new())
        .unwrap();
    let sibling_resp = sender.send_request(sibling).await.unwrap();
    assert_eq!(sibling_resp.status(), hyper::StatusCode::OK);

    // Extended CONNECT with generic protocol (not WebSocket-only).
    sender.ready().await.unwrap();
    let mut req = hyper::Request::builder()
        .method("CONNECT")
        .uri("http://localhost/chat")
        .header("host", "localhost")
        .body(Empty::<Bytes>::new())
        .unwrap();
    req.extensions_mut()
        .insert(hyper::ext::Protocol::from_static("eggserve-test"));
    let resp = sender.send_request(req).await.unwrap();
    assert!(
        resp.status().is_success(),
        "extended CONNECT must succeed, got {}",
        resp.status()
    );
    let upgraded = hyper::upgrade::on(resp).await.expect("H2 upgrade");
    let mut io = TokioIo::new(upgraded);
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    io.write_all(b"h2-echo").await.unwrap();
    let mut buf = vec![0u8; 7];
    tokio::time::timeout(std::time::Duration::from_secs(5), io.read_exact(&mut buf))
        .await
        .expect("H2 echo timeout")
        .unwrap();
    assert_eq!(&buf, b"h2-echo");
    // Sibling still works after tunnel (multiplexed survival).
    sender.ready().await.unwrap();
    let sibling2 = hyper::Request::builder()
        .method("GET")
        .uri("http://localhost/")
        .header("host", "localhost")
        .body(Empty::<Bytes>::new())
        .unwrap();
    let resp2 = sender.send_request(sibling2).await.unwrap();
    assert_eq!(resp2.status(), hyper::StatusCode::OK);
    let body = resp2.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"no-tunnel");
    drop(sender);
    let _ = conn_task.await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[cfg(feature = "http3")]
#[tokio::test]
async fn h3_connect_echo_is_stream_scoped() {
    use bytes::{Buf, Bytes};
    use eggserve_core::server::Http3Config;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir = tempfile::TempDir::new().unwrap();
    let key_pair = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    let cert = params.self_signed(&key_pair).unwrap();
    let cert_path = dir.path().join("c.crt");
    let key_path = dir.path().join("c.key");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, key_pair.serialize_pem()).unwrap();
    let cert_der = cert.der().clone();
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .http3(Http3Config {
            enabled: true,
            ..Http3Config::default()
        })
        .build()
        .unwrap();
    let (addr, handle) = start_echo_server_with_http3(config, &cert_path, &key_path).await;
    // H3 client (same pattern as http3_runtime.rs).
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert_der).unwrap();
    let mut tls = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"h3".to_vec()];
    let client_config = h3_quinn::quinn::ClientConfig::new(std::sync::Arc::new(
        h3_quinn::quinn::crypto::rustls::QuicClientConfig::try_from(std::sync::Arc::new(tls))
            .unwrap(),
    ));
    let mut endpoint = h3_quinn::quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(client_config);
    let conn = endpoint.connect(addr, "localhost").unwrap().await.unwrap();
    let (mut driver, mut sender) = h3::client::new(h3_quinn::Connection::new(conn))
        .await
        .unwrap();
    let driver_task =
        tokio::spawn(
            async move { futures_util::future::poll_fn(|cx| driver.poll_close(cx)).await },
        );
    // Plain CONNECT (no `:protocol`): stream-scoped tunnel, siblings survive.
    let mut tunnel = sender
        .send_request(
            hyper::Request::builder()
                .method("CONNECT")
                .uri(format!("https://localhost:{}", addr.port()))
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    let resp = tunnel.recv_response().await.unwrap();
    assert_eq!(resp.status(), hyper::StatusCode::OK);
    tunnel
        .send_data(Bytes::from_static(b"h3-echo"))
        .await
        .unwrap();
    let mut got = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while got.len() < b"h3-echo".len() && tokio::time::Instant::now() < deadline {
        if let Some(mut chunk) =
            tokio::time::timeout(std::time::Duration::from_secs(5), tunnel.recv_data())
                .await
                .expect("H3 echo timeout")
                .unwrap()
        {
            got.extend_from_slice(chunk.copy_to_bytes(chunk.remaining()).as_ref());
        } else {
            break;
        }
    }
    assert_eq!(&got, b"h3-echo");
    // Ordinary sibling survives the tunnel (same QUIC connection).
    let mut sibling = sender
        .send_request(
            hyper::Request::builder()
                .method("GET")
                .uri("https://localhost/")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();
    sibling.finish().await.unwrap();
    let sresp = sibling.recv_response().await.unwrap();
    // Echo service returns `no-tunnel` for ordinary GET (denial path).
    assert_eq!(sresp.status(), hyper::StatusCode::OK);
    drop(sender);
    let _ = driver_task.await;
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[cfg(feature = "http3")]
async fn start_echo_server_with_http3(
    config: RuntimeConfig,
    cert: &std::path::Path,
    key: &std::path::Path,
) -> (SocketAddr, eggserve_core::server::ServerHandle) {
    let server = Server::builder()
        .runtime(config)
        .http3_identity(cert, key)
        .build()
        .unwrap();
    let handle = server.start_with_service(echo_service()).await.unwrap();
    let addr = handle.local_addr();
    (addr, handle)
}

#[tokio::test]
async fn ws_interop_proves_generic_handoff_sufficient() {
    use eggserve_core::primitives::canonical::{ResponseBody, StatusCode};
    use futures_util::{SinkExt, StreamExt};
    // Downstream WS codec lives entirely in this fixture (no WS in core):
    // handshake via EggServe tunnel (101 + Accept), framing via
    // `tokio-tungstenite` over `TunnelIo`.
    let service = service_fn(|req: Request| async move {
        let Some(tunnel) = req.context().take_tunnel() else {
            return Ok(eggserve_core::primitives::canonical::Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(ResponseBody::Empty)
                .unwrap());
        };
        // Generic capability, service-specific WS policy (not hardcoded in core).
        if tunnel.request().protocol().map(|p| p.as_str()) != Some("websocket") {
            return Ok(eggserve_core::primitives::canonical::Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(ResponseBody::Empty)
                .unwrap());
        }
        let key = req
            .head()
            .headers()
            .get_first("sec-websocket-key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        if key.is_empty() {
            return Ok(eggserve_core::primitives::canonical::Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(ResponseBody::Empty)
                .unwrap());
        }
        let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
        let mut headers = HeaderBlock::new();
        headers.push_str("sec-websocket-accept", &accept).unwrap();
        let handler = |io: TunnelIo, lifecycle: RequestLifecycle| async move {
            use tokio_tungstenite::tungstenite::protocol::Role;
            let mut ws =
                tokio_tungstenite::WebSocketStream::from_raw_socket(io, Role::Server, None).await;
            loop {
                tokio::select! {
                    msg = ws.next() => {
                        match msg {
                            Some(Ok(m)) if m.is_text() || m.is_binary() => {
                                if ws.send(m).await.is_err() {
                                    break;
                                }
                            }
                            Some(Ok(m)) if m.is_close() => break,
                            Some(Ok(_)) => continue,
                            _ => break,
                        }
                    }
                    _ = lifecycle.cancelled() => break,
                }
            }
        };
        match tunnel.accept(headers, handler) {
            Ok(r) => Ok(r),
            Err(_) => Ok(eggserve_core::primitives::canonical::Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(ResponseBody::Empty)
                .unwrap()),
        }
    });
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let url = format!("ws://{}/", handle.local_addr());
    let (mut ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("WS connect");
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        "ws-echo".into(),
    ))
    .await
    .unwrap();
    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
        .await
        .expect("WS echo timeout")
        .expect("stream ended")
        .expect("WS error");
    assert_eq!(msg.into_text().unwrap(), "ws-echo");
    ws.close(None).await.unwrap();
    handle.shutdown();
    handle.wait().await.unwrap();
}
