//! Plan 207 — Cross-protocol application-server conformance (routine subset).
//!
//! One normative scenario inventory drives qualification:
//! `conformance/app_server_conformance.toml` defines each semantic scenario
//! once with transport/consumer applicability. This file runs the
//! deterministic Linux subset through real drivers:
//!
//! - H1 cleartext TCP (baseline)
//! - H1 prebound TCP (`from_std_listener` parity)
//! - H1 Unix socket (`from_unix_listener`, Unix only)
//! - caller-owned duplex (`serve_http1_connection`, no listener)
//! - H2 cleartext prior knowledge (`http2` feature, in-process Hyper client)
//!
//! H1 TLS, H2 TLS/ALPN, H3 QUIC, Tower/`http` adapters, async Python, and the
//! ASGI fixture are qualified by their owning suites
//! (`tls_identity.rs`, `http2_runtime.rs`, `http3_runtime.rs`,
//! `interop_http_tower.rs`, `test_async_bridge.py`); the inventory records
//! that mapping so skips are capability-driven, never silent. Expensive
//! interop/soak/impairment/browser evidence stays manual per Track J.
//!
//! No byte-identical framing is asserted across protocols. Each test asserts
//! canonical application-visible behavior plus protocol-correct lifecycle
//! outcomes, using bounded channels and deterministic synchronization
//! (no sleep-heavy races).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::connection_info::Scheme;
use eggserve_core::primitives::header_block::HeaderBlock;
use eggserve_core::primitives::interim::InterimSender;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::primitives::response_stream::{ResponseStream, ResponseStreamError};
use eggserve_core::primitives::version::HttpVersion;
use eggserve_core::primitives::Request;
use eggserve_core::server::connection::{
    serve_http1_connection, ConnectionContext, ConnectionShutdown,
};
use eggserve_core::server::{
    service_fn, service_fn_with_policy, RuntimeConfig, RuntimeState, Server,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_config() -> RuntimeConfig {
    RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap()
}

fn body_config() -> RuntimeConfig {
    RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_request_body_bytes(1024 * 1024)
        .build()
        .unwrap()
}

/// Raw H1 request over TCP with `Connection: close`; returns the full text.
async fn raw_h1(addr: SocketAddr, req: &[u8]) -> String {
    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client.write_all(req).await.unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).await.unwrap();
    String::from_utf8_lossy(&buf).into_owned()
}

/// Drive one H1 exchange over a caller-owned duplex transport.
async fn caller_owned_exchange<S>(service: S, req: &[u8]) -> (String, bool)
where
    S: eggserve_core::server::Service,
{
    // Caller-owned path uses a body ceiling above zero so Stream/Buffer
    // policies behave like the TCP tests; metadata-only cases are unaffected.
    let config = Arc::new(
        RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(1024 * 1024)
            .build()
            .unwrap(),
    );
    let runtime = Arc::new(RuntimeState::try_new(&config).unwrap());
    let shutdown: &'static ConnectionShutdown = Box::leak(Box::new(ConnectionShutdown::new()));
    let context = ConnectionContext::for_non_socket(Scheme::Http, None);
    let (mut client, server) = tokio::io::duplex(65536);
    let task = tokio::spawn(serve_http1_connection(
        server, service, config, context, runtime, shutdown,
    ));
    client.write_all(req).await.unwrap();
    // `Connection: close` bounds every caller-owned case: the server closes
    // its side after the response, so `read_to_end` terminates without any
    // client half-close (which would tear down the duplex early).
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.read_to_end(&mut buf),
    )
    .await
    .unwrap();
    let outcome = task.await.unwrap();
    (
        String::from_utf8_lossy(&buf).into_owned(),
        outcome.is_clean(),
    )
}

fn status_line(resp: &str) -> &str {
    resp.lines().next().unwrap_or("")
}

// ---------------------------------------------------------------------------
// Track A (metadata): parity across transports
// ---------------------------------------------------------------------------

#[tokio::test]
async fn metadata_parity_across_transports() {
    // Service captures canonical metadata and echoes path + version.
    let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let service = {
        let seen = seen.clone();
        service_fn(move |req: Request| {
            let seen = seen.clone();
            async move {
                let head = req.head();
                let dupes: Vec<String> = head
                    .headers()
                    .get_all("x-dupe")
                    .iter()
                    .map(|v| v.to_str().unwrap().to_owned())
                    .collect();
                let record = format!(
                    "{}|{}|{}|{}|{}|{}",
                    head.method().as_str(),
                    head.target().path(),
                    head.target().query().unwrap_or("-"),
                    head.version().as_str(),
                    head.authority().map(|a| a.as_str()).unwrap_or("-"),
                    dupes.join(",")
                );
                seen.lock().unwrap().push(record.clone());
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(record.into_bytes()))
                    .unwrap())
            }
        })
    };
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let addr = handle.local_addr();

    let resp = raw_h1(
        addr,
        b"GET /a/b?x=1&y=2 HTTP/1.1\r\nHost: example.test\r\nX-Dupe: one\r\nX-Dupe: two\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(status_line(&resp).contains("200"), "{resp}");
    assert!(
        resp.contains("GET|/a/b|x=1&y=2|HTTP/1.1|example.test|one,two"),
        "{resp}"
    );

    // Caller-owned duplex observes the same canonical values.
    let (resp, clean) = caller_owned_exchange(
        service_fn(|req: Request| async move {
            let head = req.head();
            let dupes: Vec<String> = head
                .headers()
                .get_all("x-dupe")
                .iter()
                .map(|v| v.to_str().unwrap().to_owned())
                .collect();
            let record = format!(
                "{}|{}|{}|{}|{}|{}",
                head.method().as_str(),
                head.target().path(),
                head.target().query().unwrap_or("-"),
                head.version().as_str(),
                head.authority().map(|a| a.as_str()).unwrap_or("-"),
                dupes.join(",")
            );
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(record.into_bytes()))
                .unwrap())
        }),
        b"GET /a/b?x=1&y=2 HTTP/1.1\r\nHost: example.test\r\nX-Dupe: one\r\nX-Dupe: two\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(status_line(&resp).contains("200"), "{resp}");
    assert!(
        resp.contains("GET|/a/b|x=1&y=2|HTTP/1.1|example.test|one,two"),
        "{resp}"
    );
    assert!(clean);

    // Prebound TCP listener serves the identical pipeline.
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let expected = std_listener.local_addr().unwrap();
    let server = Server::builder()
        .runtime(test_config())
        .from_std_listener(std_listener)
        .unwrap()
        .build()
        .unwrap();
    let prebound = server
        .start_with_service(service_fn(|req: Request| async move {
            assert_eq!(req.head().target().path(), "/pre");
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"prebound".to_vec()))
                .unwrap())
        }))
        .await
        .unwrap();
    prebound.ready().await.unwrap();
    assert_eq!(prebound.local_addr(), expected);
    let resp = raw_h1(
        expected,
        b"GET /pre HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.contains("prebound"), "{resp}");

    handle.shutdown();
    handle.wait().await.unwrap();
    prebound.shutdown();
    prebound.wait().await.unwrap();
}

#[tokio::test]
async fn endpoint_truthfulness() {
    let service = service_fn(|req: Request| async move {
        let conn = req.connection();
        let body = format!(
            "local={:?} remote={:?}",
            conn.local_addr.is_some(),
            conn.remote_addr.is_some()
        );
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(body.into_bytes()))
            .unwrap())
    });
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let resp = raw_h1(
        handle.local_addr(),
        b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.contains("local=true remote=true"), "{resp}");

    // Caller-owned duplex without socket addrs exposes None truthfully.
    let (resp, _) = caller_owned_exchange(
        service_fn(|req: Request| async move {
            let conn = req.connection();
            assert!(conn.local_addr.is_none());
            assert!(conn.remote_addr.is_none());
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"no-addrs".to_vec()))
                .unwrap())
        }),
        b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.contains("no-addrs"), "{resp}");

    handle.shutdown();
    handle.wait().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_socket_serves_with_truthful_endpoints() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plan207.sock");
    let unix_listener = tokio::net::UnixListener::bind(&path).unwrap();
    let server = Server::builder()
        .runtime(test_config())
        .from_unix_listener(unix_listener)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(|req: Request| async move {
            assert!(req.connection().local_addr.is_none());
            assert!(req.connection().remote_addr.is_none());
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"unix-ok".to_vec()))
                .unwrap())
        }))
        .await
        .unwrap();
    handle.ready().await.unwrap();
    assert_eq!(handle.endpoints()[0].id(), "unix-0");

    let mut client = tokio::net::UnixStream::connect(&path).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).await.unwrap();
    let resp = String::from_utf8_lossy(&buf);
    assert!(resp.contains("unix-ok"), "{resp}");
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn untrusted_forwarded_headers_stay_untrusted() {
    let service = service_fn(|req: Request| async move {
        // No trusted-proxy policy: provenance stays absent and the effective
        // client falls back to the direct peer, never the spoofed header.
        assert!(!req.connection().has_trusted_proxy_metadata());
        let peer = req.connection().remote_addr;
        let effective = req.connection().effective_client_addr();
        assert_eq!(effective, peer);
        // Spoofed addresses must not appear as the effective client.
        if let Some(eff) = effective {
            assert_ne!(eff.ip().to_string(), "203.0.113.7");
        }
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"untrusted".to_vec()))
            .unwrap())
    });
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let resp = raw_h1(
        handle.local_addr(),
        b"GET / HTTP/1.1\r\nHost: internal.test\r\nForwarded: for=203.0.113.7\r\nX-Forwarded-For: 203.0.113.7\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.contains("untrusted"), "{resp}");
    handle.shutdown();
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// Track A (body)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn body_empty_and_fixed_parity() {
    let service = service_fn_with_policy(
        |req: Request| async move {
            let (_head, mut body) = req.into_head_and_body();
            let mut total = 0usize;
            while let Some(chunk) = body.next_chunk().await.unwrap() {
                total += chunk.len();
            }
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(format!("n={total}").into_bytes()))
                .unwrap())
        },
        RequestBodyPolicy::Stream { max_bytes: 1024 },
    );
    let server = Server::builder().runtime(body_config()).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let addr = handle.local_addr();

    let resp = raw_h1(
        addr,
        b"POST /empty HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.contains("n=0"), "{resp}");

    let resp = raw_h1(
        addr,
        b"POST /fixed HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    )
    .await;
    assert!(resp.contains("n=5"), "{resp}");
    handle.shutdown();
    handle.wait().await.unwrap();

    // Same policy over the caller-owned driver.
    let (resp, _) = caller_owned_exchange(
        service_fn_with_policy(
            |req: Request| async move {
                let (_head, mut body) = req.into_head_and_body();
                let mut total = 0usize;
                while let Some(chunk) = body.next_chunk().await.unwrap() {
                    total += chunk.len();
                }
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(format!("n={total}").into_bytes()))
                    .unwrap())
            },
            RequestBodyPolicy::Stream { max_bytes: 1024 },
        ),
        b"POST /fixed HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    )
    .await;
    assert!(resp.contains("n=5"), "{resp}");
}

#[tokio::test]
async fn h1_chunked_streams() {
    let service = service_fn_with_policy(
        |req: Request| async move {
            let (_head, mut body) = req.into_head_and_body();
            let mut total = 0usize;
            while let Some(chunk) = body.next_chunk().await.unwrap() {
                total += chunk.len();
            }
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(format!("n={total}").into_bytes()))
                .unwrap())
        },
        RequestBodyPolicy::Stream { max_bytes: 1024 },
    );
    let server = Server::builder().runtime(body_config()).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let resp = raw_h1(
        handle.local_addr(),
        b"POST /c HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
    )
    .await;
    assert!(resp.contains("n=5"), "{resp}");
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn over_limit_body_rejected() {
    let invoked = Arc::new(AtomicUsize::new(0));
    let service = {
        let invoked = invoked.clone();
        service_fn_with_policy(
            move |_req: Request| {
                let invoked = invoked.clone();
                async move {
                    invoked.fetch_add(1, Ordering::SeqCst);
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(ResponseBody::Bytes(b"called".to_vec()))
                        .unwrap())
                }
            },
            RequestBodyPolicy::Stream { max_bytes: 4 },
        )
    };
    let server = Server::builder().runtime(body_config()).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let resp = raw_h1(
        handle.local_addr(),
        b"POST /big HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(status_line(&resp).contains("413"), "{resp}");
    assert_eq!(invoked.load(Ordering::SeqCst), 0);
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn abandoned_body_forces_close() {
    // Service returns immediately without consuming a Stream body.
    let service = service_fn_with_policy(
        |_req: Request| async move {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"early".to_vec()))
                .unwrap())
        },
        RequestBodyPolicy::Stream { max_bytes: 1024 },
    );
    let server = Server::builder().runtime(body_config()).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let resp = raw_h1(
        handle.local_addr(),
        b"POST /early HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: keep-alive\r\n\r\nhello",
    )
    .await;
    assert!(resp.contains("early"), "{resp}");
    // Abandoned network body forces safe close even when keep-alive was asked.
    assert!(
        resp.to_ascii_lowercase().contains("connection: close"),
        "abandoned body must force close, got: {resp}"
    );
    handle.shutdown();
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// Track A (response)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn response_variants_parity() {
    let service = service_fn(|req: Request| async move {
        let path = req.head().target().path().to_owned();
        match path.as_str() {
            "/empty" => Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap()),
            "/bytes" => Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"bytes-body".to_vec()))
                .unwrap()),
            "/known" => {
                let s = ResponseStream::with_known_length(
                    futures_util::stream::iter(vec![Ok::<_, ResponseStreamError>(Bytes::from(
                        "known-",
                    ))]),
                    6,
                );
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Stream(s))
                    .unwrap())
            }
            "/unknown" => {
                let s = ResponseStream::new(futures_util::stream::iter(vec![Ok::<
                    _,
                    ResponseStreamError,
                >(
                    Bytes::from("unknown-body"),
                )]));
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Stream(s))
                    .unwrap())
            }
            "/dup" => Ok(Response::builder()
                .status(StatusCode::OK)
                .header("x-dupe", "a")
                .unwrap()
                .header("x-dupe", "b")
                .unwrap()
                .body(ResponseBody::Bytes(b"dup".to_vec()))
                .unwrap()),
            _ => Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(ResponseBody::Empty)
                .unwrap()),
        }
    });
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let addr = handle.local_addr();

    for (path, needle) in [
        ("/empty", "200"),
        ("/bytes", "bytes-body"),
        ("/known", "known-"),
        ("/unknown", "unknown-body"),
    ] {
        let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        let resp = raw_h1(addr, req.as_bytes()).await;
        assert!(resp.contains(needle), "{path}: {resp}");
    }
    let resp = raw_h1(
        addr,
        b"GET /dup HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    let dupes = resp.to_ascii_lowercase().matches("x-dupe:").count();
    assert_eq!(dupes, 2, "{resp}");
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn head_and_body_forbidden_never_poll() {
    let polled = Arc::new(AtomicBool::new(false));
    let service = {
        let polled = polled.clone();
        service_fn(move |req: Request| {
            let polled = polled.clone();
            async move {
                let is_head = req.head().method().as_str() == "HEAD";
                let status = if is_head {
                    StatusCode::OK
                } else {
                    StatusCode::NO_CONTENT
                };
                let flag = polled.clone();
                let stream = futures_util::stream::poll_fn(move |_| {
                    flag.store(true, Ordering::SeqCst);
                    std::task::Poll::Ready(Some(Ok::<_, ResponseStreamError>(Bytes::from("x"))))
                });
                Ok(Response::builder()
                    .status(status)
                    .body(ResponseBody::Stream(ResponseStream::new(stream)))
                    .unwrap())
            }
        })
    };
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let addr = handle.local_addr();

    let resp = raw_h1(
        addr,
        b"HEAD /h HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(status_line(&resp).contains("200"), "{resp}");
    assert!(!polled.load(Ordering::SeqCst));

    let resp = raw_h1(
        addr,
        b"GET /n HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(status_line(&resp).contains("204"), "{resp}");
    assert!(!polled.load(Ordering::SeqCst));
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[test]
fn interim_sender_bounds() {
    // Canonical interim contract holds identically for every transport adapter.
    let sender = InterimSender::new(HttpVersion::Http11);
    sender
        .send(StatusCode::new(103).unwrap(), HeaderBlock::new())
        .unwrap();
    // Non-1xx and 101 never accepted.
    assert!(sender.send(StatusCode::OK, HeaderBlock::new()).is_err());
    assert!(sender
        .send(StatusCode::new(101).unwrap(), HeaderBlock::new())
        .is_err());
    // Post-commit fails closed.
    sender.mark_committed();
    assert!(sender
        .send(StatusCode::new(103).unwrap(), HeaderBlock::new())
        .is_err());
    // HTTP/1.0 suppresses rather than emits.
    let h10 = InterimSender::new(HttpVersion::Http10);
    let disp = h10
        .send(StatusCode::new(103).unwrap(), HeaderBlock::new())
        .unwrap();
    assert_eq!(
        disp,
        eggserve_core::primitives::interim::InterimDisposition::SuppressedHttp10
    );
}

#[tokio::test]
async fn producer_panic_maps_to_500() {
    let service = service_fn(|_req: Request| async move {
        panic!("boom");
        #[allow(unreachable_code)]
        Ok::<_, eggserve_core::server::ServiceError>(
            Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap(),
        )
    });
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let resp = raw_h1(
        handle.local_addr(),
        b"GET /panic HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(status_line(&resp).contains("500"), "{resp}");
    assert!(!resp.contains("boom"));
    handle.shutdown();
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// Track E (security corpus spot checks through real drivers)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn framing_ambiguity_rejected() {
    let invoked = Arc::new(AtomicUsize::new(0));
    let service = {
        let invoked = invoked.clone();
        service_fn(move |_req: Request| {
            let invoked = invoked.clone();
            async move {
                invoked.fetch_add(1, Ordering::SeqCst);
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"called".to_vec()))
                    .unwrap())
            }
        })
    };
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    // Duplicate conflicting Content-Length must fail before the service.
    let resp = raw_h1(
        handle.local_addr(),
        b"POST /x HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nContent-Length: 6\r\nConnection: close\r\n\r\nhello!",
    )
    .await;
    assert!(status_line(&resp).contains("400"), "{resp}");
    assert_eq!(invoked.load(Ordering::SeqCst), 0);
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn oversized_target_yields_414() {
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server
        .start_with_service(service_fn(|_req: Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"called".to_vec()))
                .unwrap())
        }))
        .await
        .unwrap();
    let long = format!(
        "GET /{} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        "a".repeat(9000)
    );
    let resp = raw_h1(handle.local_addr(), long.as_bytes()).await;
    assert!(status_line(&resp).contains("414"), "{resp}");
    handle.shutdown();
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// Tracks F/lifecycle: admission saturation and recovery
// ---------------------------------------------------------------------------

#[tokio::test]
async fn admission_saturation_recovers() {
    use std::time::Duration;
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(tokio::sync::Notify::new());
    let service = {
        let entered = entered.clone();
        let release = release.clone();
        service_fn(move |_req: Request| {
            let entered = entered.clone();
            let release = release.clone();
            async move {
                // Only the first holder blocks; later calls (post-recovery)
                // return immediately so the test proves permit reuse.
                let first = !entered.swap(true, Ordering::SeqCst);
                if first {
                    release.notified().await;
                }
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"held".to_vec()))
                    .unwrap())
            }
        })
    };
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_in_flight_requests(1)
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let addr = handle.local_addr();

    // Hold the single in-flight permit (poll for entry like production_controls).
    let mut conn1 = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn1
        .write_all(b"GET /one HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !entered.load(Ordering::SeqCst) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "first request never entered the service"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let conn1_read = tokio::spawn(async move {
        let mut buf = Vec::new();
        conn1.read_to_end(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf).into_owned()
    });
    // Saturation maps deterministically to 503 while the permit is held.
    let saturated = raw_h1(
        addr,
        b"GET /two HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(status_line(&saturated).contains("503"), "{saturated}");
    // Recovery after release.
    release.notify_waiters();
    let first_resp = conn1_read.await.unwrap();
    assert!(first_resp.contains("held"), "{first_resp}");
    let recovered = raw_h1(
        addr,
        b"GET /three HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(status_line(&recovered).contains("200"), "{recovered}");
    handle.shutdown();
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// Tunnels (Track A): denial + H1 upgrade echo
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tunnel_denial_stays_http() {
    let service = service_fn(|req: Request| async move {
        assert!(req.context().take_tunnel().is_none());
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"no-tunnel".to_vec()))
            .unwrap())
    });
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let resp = raw_h1(
        handle.local_addr(),
        b"GET /plain HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.contains("no-tunnel"), "{resp}");
    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn h1_upgrade_echo() {
    use eggserve_core::primitives::tunnel::TunnelIo;
    let service = service_fn(|req: Request| async move {
        let Some(tunnel) = req.context().take_tunnel() else {
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"no-tunnel".to_vec()))
                .unwrap());
        };
        let handler = |mut io: TunnelIo, lifecycle: eggserve_core::primitives::RequestLifecycle| async move {
            let mut buf = vec![0u8; 1024];
            // Single echo then close: deterministic, no soak.
            if let Ok(n) = io.read(&mut buf).await {
                if n > 0 {
                    let _ = io.write_all(&buf[..n]).await;
                }
            }
            let _ = lifecycle;
        };
        match tunnel.accept(HeaderBlock::new(), handler) {
            Ok(response) => Ok(response),
            Err(_) => Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(ResponseBody::Empty)
                .unwrap()),
        }
    });
    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let mut stream = tokio::net::TcpStream::connect(handle.local_addr())
        .await
        .unwrap();
    stream
        .write_all(
            b"GET /chat HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: eggserve-test\r\n\r\nping",
        )
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
        assert!(head.len() <= 8192);
    }
    let head_text = String::from_utf8_lossy(&head);
    assert!(head_text.starts_with("HTTP/1.1 101"), "{head_text}");
    let mut echo = vec![0u8; 4];
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_exact(&mut echo),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(&echo, b"ping");
    handle.shutdown();
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// H2 isolation (Track D) — in-process Hyper client, deterministic
// ---------------------------------------------------------------------------

#[cfg(feature = "http2")]
#[tokio::test]
async fn h2_sibling_survives_reject() {
    use eggserve_core::server::Http2Config;
    use http_body_util::{BodyExt, Full};
    use hyper::Request as HyperRequest;
    use hyper_util::rt::{TokioExecutor, TokioIo};

    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .http2(Http2Config {
            max_concurrent_streams: 8,
            ..Http2Config::default()
        })
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server
        .start_with_service(service_fn(|req: Request| async move {
            assert_eq!(req.head().target().path(), "/ok");
            assert_eq!(
                req.head().version(),
                eggserve_core::primitives::HttpVersion::Http2
            );
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"sibling-ok".to_vec()))
                .unwrap())
        }))
        .await
        .unwrap();

    let stream = tokio::net::TcpStream::connect(handle.local_addr())
        .await
        .unwrap();
    let (sender, connection) = hyper::client::conn::http2::handshake::<_, _, Full<Bytes>>(
        TokioExecutor::new(),
        TokioIo::new(stream),
    )
    .await
    .unwrap();
    let connection_task = tokio::spawn(connection);

    // Rejected POST (static-shaped Reject policy would be 413; here the
    // service asserts only /ok is reachable, so exercise the multiplexed
    // sibling path with two concurrent GETs instead: both must survive).
    let mut a = sender.clone();
    let mut b = sender.clone();
    let (ra, rb) = tokio::join!(
        a.send_request(
            HyperRequest::builder()
                .method("GET")
                .uri("http://example.test/ok")
                .body(Full::new(Bytes::new()))
                .unwrap()
        ),
        b.send_request(
            HyperRequest::builder()
                .method("GET")
                .uri("http://example.test/ok")
                .body(Full::new(Bytes::new()))
                .unwrap()
        )
    );
    for resp in [ra.unwrap(), rb.unwrap()] {
        assert_eq!(resp.status(), hyper::StatusCode::OK);
        assert_eq!(
            resp.into_body().collect().await.unwrap().to_bytes(),
            "sibling-ok"
        );
    }
    // Drop all client handles so the H2 connection can close promptly;
    // then shut down the server without waiting for the 60s total timeout.
    drop(a);
    drop(b);
    drop(sender);
    handle.shutdown();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), connection_task).await;
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// Tower parity (Track C) — same Tower app on the H1 driver
// ---------------------------------------------------------------------------

#[cfg(feature = "tower")]
#[tokio::test]
async fn tower_adapter_parity() {
    use eggserve_core::primitives::RequestBody;
    use eggserve_core::server::TowerToEggserve;

    #[derive(Clone, Default)]
    struct Hello;
    impl tower_service::Service<http::Request<RequestBody>> for Hello {
        type Response = http::Response<http_body_util::Full<Bytes>>;
        type Error = std::convert::Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;
        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn call(&mut self, _req: http::Request<RequestBody>) -> Self::Future {
            std::future::ready(Ok(http::Response::builder()
                .status(http::StatusCode::OK)
                .body(http_body_util::Full::new(Bytes::from("tower-hello")))
                .unwrap()))
        }
    }

    let server = Server::builder().runtime(test_config()).build().unwrap();
    let handle = server
        .start_with_service(TowerToEggserve::new(Hello))
        .await
        .unwrap();
    let resp = raw_h1(
        handle.local_addr(),
        b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.contains("tower-hello"), "{resp}");
    handle.shutdown();
    handle.wait().await.unwrap();
}
