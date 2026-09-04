//! Downstream application-server consumer qualification (Plan 175).
//!
//! External-consumer proof that the public EggServe API is sufficient to build
//! the HTTP half of a real event-driven application server without importing
//! Hyper internals, the Python facade, or crate-private modules.
//!
//! # Import boundary (Track A)
//!
//! This file is an integration test outside the `eggserve-core` module tree.
//! It imports ONLY:
//!
//! - `eggserve_core::primitives` (stable facade)
//! - `eggserve_core::server` (experimental runtime/service boundary)
//! - ordinary downstream dependencies (`tokio`, `bytes`, `futures-util`)
//! - `std`, plus `rcgen`/`rustls`/`tokio-rustls` for the TLS parity case
//!
//! It must NOT import:
//!
//! - `eggserve_core::response`, `::path`, `::fs`, or other crate-private modules
//! - `hyper`, `http::HeaderValue`, or any Hyper request/response/service type
//! - internal connection activity/state types
//! - the Python package or `eggserve.lowlevel`
//!
//! Where the public API intentionally exposes a low-level conversion adapter
//! that mentions Hyper (`to_hyper_response` / `try_from_hyper`), this fixture
//! does not use it. The point is to prove the canonical `Service`/runtime path.
//!
//! # Bridge shape (Track B)
//!
//! ```text
//! EggServe Service::call(Request)
//!        |
//!        +--> app task owns RequestBody + RequestLifecycle
//!        |         |
//!        |         +--> bounded request/event adaptation
//!        |         +--> produces response-start
//!        |
//!        +<-- response-start
//!        |
//!        +--> return ResponseBody::Stream
//!                  |
//!                  +<-- bounded response chunks from app task
//! ```
//!
//! Event names below are local to this fixture only. They are not exposed
//! from EggServe. All coordination uses bounded channels with small
//! capacities so tests exercise real backpressure. The main qualification
//! path never calls `read_all()`.
//!
//! EggServe itself is not an application server, ASGI/WSGI runtime,
//! framework, proxy, or WebSocket implementation. This fixture is a
//! consumer test, not a maintained second server product.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::connection_info::Scheme;
use eggserve_core::primitives::header_block::{HeaderName, HeaderValue};
use eggserve_core::primitives::request::Request;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::primitives::request_lifecycle::{RequestCancellationReason, RequestLifecycle};
use eggserve_core::primitives::response_stream::ResponseStreamError;
use eggserve_core::primitives::ResponseStream;
use eggserve_core::server::connection::{
    serve_http1_connection, ConnectionContext, ConnectionShutdown,
};
use eggserve_core::server::{
    service_fn_with_policy, RuntimeConfig, RuntimeState, Server, Service, ServiceError,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ---------------------------------------------------------------------------
// Fixture-local event protocol (Track B)
// ---------------------------------------------------------------------------

/// Fixture-local request event. Not exposed from EggServe.
#[derive(Debug)]
enum AppRequestEvent {
    Body(Bytes),
    End,
    Disconnected,
}

/// Snapshot of canonical request metadata available to a downstream server.
#[derive(Debug, Clone)]
struct CapturedMeta {
    method: String,
    version: String,
    scheme: String,
    path: String,
    query: Option<String>,
    raw: Vec<u8>,
    path_bytes: Vec<u8>,
    query_bytes: Option<Vec<u8>>,
    /// Ordered duplicate-preserving headers as byte pairs.
    headers: Vec<(Vec<u8>, Vec<u8>)>,
    local: Option<SocketAddr>,
    remote: Option<SocketAddr>,
    tls_present: bool,
    tls_protocol: Option<String>,
    tls_sni: Option<String>,
}

fn capture_meta(req: &Request) -> CapturedMeta {
    let head = req.head();
    let conn = req.connection();
    CapturedMeta {
        method: head.method().as_str().to_owned(),
        version: head.version().as_str().to_owned(),
        scheme: conn.scheme.as_str().to_owned(),
        path: head.target().path().to_owned(),
        query: head.target().query().map(str::to_owned),
        raw: head.target().raw_bytes().to_vec(),
        path_bytes: head.target().path_bytes().to_vec(),
        query_bytes: head.target().query_bytes().map(|q| q.to_vec()),
        headers: head
            .headers()
            .iter()
            .map(|f| {
                (
                    f.name.as_str().as_bytes().to_vec(),
                    f.value.as_bytes().to_vec(),
                )
            })
            .collect(),
        local: conn.local_addr,
        remote: conn.remote_addr,
        tls_present: conn.tls.is_some(),
        tls_protocol: conn.tls.as_ref().and_then(|t| t.protocol_version.clone()),
        tls_sni: conn.tls.as_ref().and_then(|t| t.server_name.clone()),
    }
}

fn stream_policy() -> RequestBodyPolicy {
    RequestBodyPolicy::Stream {
        max_bytes: 1024 * 1024,
    }
}

fn test_config() -> Arc<RuntimeConfig> {
    Arc::new(
        RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(1024 * 1024)
            .body_read_timeout(Duration::from_secs(5))
            .handler_timeout(Duration::from_secs(5))
            .connection_total_timeout(Duration::from_secs(30))
            .graceful_shutdown_timeout(Duration::from_secs(5))
            .build()
            .unwrap(),
    )
}

async fn read_response_headers(client: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut acc = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        client.read_exact(&mut byte).await.unwrap();
        acc.push(byte[0]);
        if acc.ends_with(b"\r\n\r\n") {
            break;
        }
        assert!(acc.len() < 16384, "headers too large");
    }
    acc
}

/// Read a chunked response body from a raw connection after headers.
async fn read_chunked_body(client: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut line = Vec::new();
        loop {
            let mut b = [0u8; 1];
            client.read_exact(&mut b).await.unwrap();
            line.push(b[0]);
            if line.ends_with(b"\r\n") {
                break;
            }
            assert!(line.len() < 32, "chunk-size line too large");
        }
        let size_str = String::from_utf8_lossy(&line[..line.len() - 2]).to_string();
        let size_str = size_str.split(';').next().unwrap().trim().to_owned();
        let size = usize::from_str_radix(&size_str, 16).unwrap();
        if size == 0 {
            // Consume optional trailers + final CRLF.
            let mut tail = [0u8; 2];
            client.read_exact(&mut tail).await.unwrap();
            assert_eq!(&tail, b"\r\n");
            break;
        }
        let mut chunk = vec![0u8; size];
        client.read_exact(&mut chunk).await.unwrap();
        out.extend_from_slice(&chunk);
        let mut crlf = [0u8; 2];
        client.read_exact(&mut crlf).await.unwrap();
        assert_eq!(&crlf, b"\r\n");
    }
    out
}

/// Convert a bounded `mpsc::Receiver<Bytes>` into a `ResponseStream` producer.
///
/// End-of-stream is the dropped sender (explicit `End`). Producer failure
/// details never reach the wire; truncation/close is the only signal.
fn receiver_to_stream(rx: tokio::sync::mpsc::Receiver<Bytes>) -> ResponseStream {
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|chunk| (Ok::<Bytes, ResponseStreamError>(chunk), rx))
    });
    ResponseStream::new(stream)
}

// ---------------------------------------------------------------------------
// Track C — metadata fidelity contract
// ---------------------------------------------------------------------------

/// Service capturing canonical metadata and returning duplicate + opaque
/// response headers through the byte API (no `http::HeaderValue`, no Hyper).
struct MetadataCaptureService {
    tx: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<CapturedMeta>>>>,
}

impl Service for MetadataCaptureService {
    fn request_body_policy(
        &self,
        _head: &eggserve_core::primitives::request_head::RequestHead,
    ) -> RequestBodyPolicy {
        RequestBodyPolicy::Reject
    }

    fn call(
        &self,
        request: Request,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Response, ServiceError>> + Send + '_>,
    > {
        let tx = self.tx.clone();
        Box::pin(async move {
            let meta = capture_meta(&request);
            if let Some(tx) = tx.lock().unwrap().take() {
                let _ = tx.send(meta);
            }
            // Duplicate + opaque response headers via the canonical byte API.
            let mut resp = Response::builder()
                .status(StatusCode::OK)
                .push_header(
                    HeaderName::new("x-dup").unwrap(),
                    HeaderValue::new("one").unwrap(),
                )
                .push_header(
                    HeaderName::new("x-dup").unwrap(),
                    HeaderValue::new("two").unwrap(),
                )
                .push_header(
                    HeaderName::new("x-opaque").unwrap(),
                    HeaderValue::from_bytes(b"a\xffopaque").unwrap(),
                )
                .body(ResponseBody::Bytes(b"meta-ok".to_vec()))
                .unwrap();
            let _ = &mut resp;
            Ok(resp)
        })
    }
}

#[tokio::test]
async fn bridge_metadata_round_trip_over_tcp() {
    let (tx, rx) = tokio::sync::oneshot::channel::<CapturedMeta>();
    let svc = MetadataCaptureService {
        tx: Arc::new(std::sync::Mutex::new(Some(tx))),
    };
    let config = test_config();
    let server = Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    // Raw client: ordered duplicates + percent-encoded target + empty-value hdr.
    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(
            b"GET /a%2Fb%20c?x=1%26y%3D2&x=3 HTTP/1.1\r\n\
              Host: example\r\n\
              X-Dup: first\r\n\
              X-Dup: second\r\n\
              X-Opaque: placeholder\r\n\
              Connection: close\r\n\r\n",
        )
        .await
        .unwrap();

    let mut raw = Vec::new();
    client.read_to_end(&mut raw).await.unwrap();
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(raw.len());
    let (head_bytes, body) = raw.split_at(split);
    let head_str = String::from_utf8_lossy(head_bytes);
    assert!(head_str.starts_with("HTTP/1.1 200"), "got: {head_str}");
    assert_eq!(body, b"meta-ok");

    // Duplicate response headers preserved in order on the wire.
    let dup_positions: Vec<usize> = head_str
        .to_ascii_lowercase()
        .match_indices("x-dup:")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        dup_positions.len(),
        2,
        "expected 2 x-dup headers: {head_str}"
    );
    assert!(head_str[dup_positions[0]..].contains("one"));
    assert!(head_str[dup_positions[1]..].contains("two"));
    // Opaque response byte 0xFF reaches the wire unchanged.
    assert!(
        head_bytes.windows(8).any(|w| w == b"a\xffopaque"),
        "opaque response bytes must round-trip"
    );

    let meta = tokio::time::timeout(Duration::from_secs(3), rx)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(meta.method, "GET");
    assert_eq!(meta.version, "HTTP/1.1");
    assert_eq!(meta.scheme, "http");
    assert_eq!(meta.path, "/a%2Fb%20c");
    assert_eq!(meta.query, Some("x=1%26y%3D2&x=3".to_owned()));
    // Truthful byte accessors over accepted origin-form bytes.
    assert_eq!(
        meta.raw,
        b"/a%2Fb%20c?x=1%26y%3D2&x=3".to_vec(),
        "raw_bytes must equal accepted wire target"
    );
    assert_eq!(meta.path_bytes, b"/a%2Fb%20c".to_vec());
    assert_eq!(
        meta.query_bytes,
        Some(b"x=1%26y%3D2&x=3".to_vec()),
        "query_bytes must preserve percent-encoding"
    );
    // Ordered duplicate request headers as byte values.
    let dups: Vec<Vec<u8>> = meta
        .headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case(b"x-dup"))
        .map(|(_, v)| v.clone())
        .collect();
    assert_eq!(dups, vec![b"first".to_vec(), b"second".to_vec()]);
    // Socket endpoints present on TCP.
    assert!(meta.local.is_some());
    assert!(meta.remote.is_some());
    assert!(!meta.tls_present);
    assert_eq!(meta.tls_protocol, None);
    assert_eq!(meta.tls_sni, None);

    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn bridge_opaque_request_byte_preserved() {
    let (tx, rx) = tokio::sync::oneshot::channel::<CapturedMeta>();
    let svc = MetadataCaptureService {
        tx: Arc::new(std::sync::Mutex::new(Some(tx))),
    };
    let config = test_config();
    let server = Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    // Legal obs-text byte 0xFF in a field value over the real parser path.
    let mut raw_req = b"GET /opaque HTTP/1.1\r\nHost: x\r\nX-Opaque: a".to_vec();
    raw_req.push(0xFF);
    raw_req.extend_from_slice(b"\r\nConnection: close\r\n\r\n");
    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client.write_all(&raw_req).await.unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).await.unwrap();
    assert!(String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"));

    let meta = tokio::time::timeout(Duration::from_secs(3), rx)
        .await
        .unwrap()
        .unwrap();
    let opaque: Vec<Vec<u8>> = meta
        .headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case(b"x-opaque"))
        .map(|(_, v)| v.clone())
        .collect();
    assert_eq!(opaque.len(), 1);
    assert_eq!(opaque[0], vec![b'a', 0xFF]);

    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn bridge_empty_query_canonicalizes_to_none() {
    let (tx, rx) = tokio::sync::oneshot::channel::<CapturedMeta>();
    let svc = MetadataCaptureService {
        tx: Arc::new(std::sync::Mutex::new(Some(tx))),
    };
    let config = test_config();
    let server = Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    // `/path` vs `/path?` deliberately canonicalize: empty query -> None.
    // A downstream server must omit optional metadata rather than fabricate it.
    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET /path? HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).await.unwrap();
    assert!(String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"));

    let meta = tokio::time::timeout(Duration::from_secs(3), rx)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(meta.path, "/path");
    assert_eq!(meta.query, None);
    assert_eq!(meta.query_bytes, None);

    handle.shutdown();
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// Track D1 — early response while upload continues (full-duplex bridge)
// ---------------------------------------------------------------------------

/// Full-duplex bridge with bounded channels in BOTH directions:
///
/// pump task owns `RequestBody` -> bounded `AppRequestEvent` channel (cap 2)
///   -> app task -> response-start oneshot + bounded response `Bytes`
///   channel (cap 2) -> `ResponseBody::Stream`.
///
/// No `read_all()` anywhere on this path.
struct FullDuplexBridge {
    /// Observed counters for assertions (downstream-owned).
    request_bytes: Arc<AtomicUsize>,
    response_chunks: usize,
}

impl Service for FullDuplexBridge {
    fn request_body_policy(
        &self,
        _head: &eggserve_core::primitives::request_head::RequestHead,
    ) -> RequestBodyPolicy {
        stream_policy()
    }

    fn call(
        &self,
        request: Request,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Response, ServiceError>> + Send + '_>,
    > {
        let request_bytes = self.request_bytes.clone();
        let response_chunks = self.response_chunks;
        Box::pin(async move {
            let lifecycle = request.lifecycle_clone();
            let (_head, body) = request.into_head_and_body();

            // Bounded channels with deliberately small capacities.
            let (req_tx, mut req_rx) = tokio::sync::mpsc::channel::<AppRequestEvent>(2);
            let (resp_tx, resp_rx) = tokio::sync::mpsc::channel::<Bytes>(2);
            let (start_tx, start_rx) = tokio::sync::oneshot::channel::<()>();
            let start_tx = Arc::new(std::sync::Mutex::new(Some(start_tx)));

            // Request-pump task: owns the body, forwards bounded events.
            let pump_lifecycle = lifecycle.clone();
            tokio::spawn(async move {
                let mut body = body;
                loop {
                    let next = tokio::select! {
                        biased;
                        _ = pump_lifecycle.cancelled() => None,
                        chunk = body.next_chunk() => match chunk {
                            Ok(c) => c,
                            Err(_) => {
                                let _ = req_tx.send(AppRequestEvent::Disconnected).await;
                                break;
                            }
                        },
                    };
                    match next {
                        Some(chunk) => {
                            // Backpressure-aware send: prompt exit on cancel.
                            tokio::select! {
                                biased;
                                _ = pump_lifecycle.cancelled() => break,
                                res = req_tx.send(AppRequestEvent::Body(chunk)) => {
                                    if res.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                        None => {
                            // EOF or cancelled while idle: End only on clean EOF.
                            if pump_lifecycle.is_cancelled() {
                                let _ = req_tx.send(AppRequestEvent::Disconnected).await;
                            } else {
                                let _ = req_tx.send(AppRequestEvent::End).await;
                            }
                            break;
                        }
                    }
                }
            });

            // Application task: consumes bounded request events, produces
            // response-start then bounded response chunks after return.
            let app_lifecycle = lifecycle.clone();
            tokio::spawn(async move {
                let mut start_tx = start_tx.lock().unwrap().take();
                let mut seen_first = false;
                let mut total: usize = 0;
                loop {
                    let event = tokio::select! {
                        biased;
                        _ = app_lifecycle.cancelled() => Some(AppRequestEvent::Disconnected),
                        ev = req_rx.recv() => ev,
                    };
                    match event {
                        Some(AppRequestEvent::Body(chunk)) => {
                            total += chunk.len();
                            request_bytes.fetch_add(chunk.len(), Ordering::Relaxed);
                            if !seen_first {
                                seen_first = true;
                                if let Some(tx) = start_tx.take() {
                                    let _ = tx.send(());
                                }
                                // First response chunk promptly after start.
                                let send = resp_tx.send(Bytes::from_static(b"chunk1-"));
                                tokio::select! {
                                    biased;
                                    _ = app_lifecycle.cancelled() => break,
                                    res = send => {
                                        if res.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        Some(AppRequestEvent::End) => {
                            if !seen_first {
                                if let Some(tx) = start_tx.take() {
                                    let _ = tx.send(());
                                }
                            }
                            // Emit remaining configured chunks, then End.
                            for i in 1..response_chunks {
                                let payload: Bytes = if i == response_chunks - 1 {
                                    Bytes::from_static(b"chunk2")
                                } else {
                                    Bytes::from_static(b"mid-")
                                };
                                let send = resp_tx.send(payload);
                                tokio::select! {
                                    biased;
                                    _ = app_lifecycle.cancelled() => break,
                                    res = send => {
                                        if res.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                            let _ = total;
                            break;
                        }
                        Some(AppRequestEvent::Disconnected) | None => {
                            break;
                        }
                    }
                }
            });

            // Wait only until response-start (bounded by runtime handler timeout).
            tokio::time::timeout(Duration::from_secs(4), start_rx)
                .await
                .map_err(|_| ServiceError::internal("app response-start timeout"))?
                .map_err(|_| ServiceError::internal("app task dropped before start"))?;

            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Stream(receiver_to_stream(resp_rx)))
                .unwrap())
        })
    }
}

#[tokio::test]
async fn bridge_early_response_full_duplex_and_reuse() {
    let svc = FullDuplexBridge {
        request_bytes: Arc::new(AtomicUsize::new(0)),
        response_chunks: 2,
    };
    let request_bytes = svc.request_bytes.clone();
    let config = test_config();
    let server = Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client.set_nodelay(true).unwrap();
    client
        .write_all(b"POST /upload HTTP/1.1\r\nHost: x\r\nContent-Length: 11\r\n\r\n")
        .await
        .unwrap();
    client.write_all(b"hello ").await.unwrap();
    client.flush().await.unwrap();

    // Response-start must arrive before request EOF (chunked, unknown length).
    let headers = tokio::time::timeout(Duration::from_secs(3), read_response_headers(&mut client))
        .await
        .expect("response-start must arrive before upload completes");
    let head_str = String::from_utf8_lossy(&headers);
    assert!(
        head_str.starts_with("HTTP/1.1 200"),
        "expected early 200, got: {head_str}"
    );
    assert!(
        head_str.to_ascii_lowercase().contains("chunked"),
        "streaming bridge must use runtime chunked framing: {head_str}"
    );

    // Complete the upload AFTER response-start while the response streams.
    client.write_all(b"world").await.unwrap();
    client.flush().await.unwrap();

    let body = tokio::time::timeout(Duration::from_secs(3), read_chunked_body(&mut client))
        .await
        .expect("chunked response body must complete");
    assert_eq!(body, b"chunk1-chunk2");

    // Remaining request body reached the downstream task.
    tokio::time::timeout(Duration::from_secs(3), async {
        while request_bytes.load(Ordering::Relaxed) < 11 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("all 11 upload bytes must reach the app task");
    assert_eq!(request_bytes.load(Ordering::Relaxed), 11);

    // Keep-alive reuse: second request succeeds after deferred completion.
    tokio::time::sleep(Duration::from_millis(200)).await;
    client
        .write_all(b"GET /second HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut rest = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut rest))
        .await
        .expect("second request must complete")
        .unwrap();
    // Second POST-shaped upload is not needed; the bridge answers GET with an
    // empty-body Start + chunks. Assert framing, not application routing.
    assert!(
        String::from_utf8_lossy(&rest).starts_with("HTTP/1.1 200"),
        "keep-alive reuse failed: {}",
        String::from_utf8_lossy(&rest)
    );

    handle.shutdown();
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// Track D2 — abandon prevents reuse
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bridge_abandon_prevents_reuse_safely() {
    let done = Arc::new(AtomicBool::new(false));
    let done_clone = done.clone();
    let svc = service_fn_with_policy(
        move |req: Request| {
            let done = done_clone.clone();
            async move {
                let (_head, mut body) = req.into_head_and_body();
                tokio::spawn(async move {
                    // Read one chunk, then deliberately abandon the rest.
                    let _ = body.next_chunk().await;
                    // Drop without EOF -> Abandoned. Mark task exit promptly.
                    done.store(true, Ordering::SeqCst);
                });
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"OK".to_vec()))
                    .unwrap())
            }
        },
        stream_policy(),
    );
    let config = test_config();
    let server = Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"POST /upload HTTP/1.1\r\nHost: x\r\nContent-Length: 100\r\n\r\n")
        .await
        .unwrap();
    client.write_all(b"hello ").await.unwrap();
    client.flush().await.unwrap();

    // Current response remains correctly framed.
    let headers = tokio::time::timeout(Duration::from_secs(3), read_response_headers(&mut client))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&headers).starts_with("HTTP/1.1 200"));
    let mut body = [0u8; 2];
    client.read_exact(&mut body).await.unwrap();
    assert_eq!(&body, b"OK");

    // Downstream task ownership terminates promptly.
    tokio::time::timeout(Duration::from_secs(3), async {
        while !done.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("abandoning task must exit promptly");

    tokio::time::sleep(Duration::from_millis(300)).await;
    // Trailing bytes are never parsed as a second request: connection closes.
    let _ = client
        .write_all(b"GET /second HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await;
    let mut buf = Vec::new();
    let read_res = tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut buf)).await;
    if let Ok(Ok(_)) = read_res {
        let text = String::from_utf8_lossy(&buf);
        assert!(
            !text.contains("chunk1") && !text.matches("HTTP/1.1 200").count().ge(&2),
            "abandoned body must not allow reuse, got: {text}"
        );
    }

    handle.shutdown();
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// Track D3 — long-poll disconnect wakes idle waiter
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bridge_long_poll_disconnect_wakes_idle_waiter() {
    let (tx, rx) = tokio::sync::oneshot::channel::<RequestCancellationReason>();
    let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
    let svc = service_fn_with_policy(
        move |req: Request| {
            let tx = tx.clone();
            async move {
                let lc: RequestLifecycle = req.lifecycle_clone();
                // Idle waiter: not polling body/response, only lifecycle.
                tokio::spawn(async move {
                    lc.cancelled().await;
                    if let Some(reason) = lc.cancellation_reason() {
                        if let Some(tx) = tx.lock().unwrap().take() {
                            let _ = tx.send(reason);
                        }
                    }
                });
                // Long-poll: wait longer than the test disconnect.
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"late".to_vec()))
                    .unwrap())
            }
        },
        RequestBodyPolicy::Reject,
    );
    let config = Arc::new(
        RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .handler_timeout(Duration::from_secs(30))
            .connection_total_timeout(Duration::from_secs(60))
            .build()
            .unwrap(),
    );
    let server = Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET /poll HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    drop(client);

    let reason = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("lifecycle must wake idle waiter without raw-socket probe")
        .unwrap();
    assert_eq!(reason, RequestCancellationReason::PeerDisconnected);

    handle.shutdown();
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// Track D4 — send-side disconnect terminates without deadlock
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bridge_send_side_disconnect_terminates() {
    let produced = Arc::new(AtomicUsize::new(0));
    let produced_clone = produced.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();
    let svc = service_fn_with_policy(
        move |req: Request| {
            let produced = produced_clone.clone();
            let cancelled = cancelled_clone.clone();
            async move {
                let lc = req.lifecycle_clone();
                let (_head, _body) = req.into_head_and_body();
                // Watch lifecycle in the background: send-side failure may
                // precede lifecycle, but cancellation follows promptly.
                tokio::spawn(async move {
                    lc.cancelled().await;
                    cancelled.store(true, Ordering::SeqCst);
                });
                let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(2);
                tokio::spawn(async move {
                    for i in 0..64u32 {
                        // Bounded backpressure: stop when consumer drops.
                        if tx
                            .send(Bytes::from(format!("chunk-{i:02}-0123456789")))
                            .await
                            .is_err()
                        {
                            break;
                        }
                        produced.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                });
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Stream(receiver_to_stream(rx)))
                    .unwrap())
            }
        },
        RequestBodyPolicy::Reject,
    );
    let config = test_config();
    let server = Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    // Read only the headers + a little body, then close (stop reading).
    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET /stream HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let headers = tokio::time::timeout(Duration::from_secs(3), read_response_headers(&mut client))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&headers).starts_with("HTTP/1.1 200"));
    // Read a small prefix of the chunked body, then abandon the connection.
    let mut prefix = vec![0u8; 64];
    let _ = tokio::time::timeout(Duration::from_secs(3), client.read(&mut prefix)).await;
    drop(client);

    // Producer must stop (no deadlock on either signal order) and lifecycle
    // cancellation must become observable. No second error response is
    // attempted after commitment: the server simply stays healthy.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        produced.load(Ordering::SeqCst) < 64,
        "producer must stop after consumer drop"
    );
    // Lifecycle may race send-side failure; allow generous settle.
    let mut saw_cancel = cancelled.load(Ordering::SeqCst);
    for _ in 0..20 {
        if saw_cancel {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        saw_cancel = cancelled.load(Ordering::SeqCst);
    }
    assert!(
        saw_cancel,
        "lifecycle cancellation must follow send-side drop"
    );

    // Server remains healthy for a new connection (no poisoned state).
    let mut probe = tokio::net::TcpStream::connect(addr).await.unwrap();
    probe
        .write_all(b"GET /probe HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), probe.read_to_end(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"));

    handle.shutdown();
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// Track D5 — shutdown drains/cancels bridge tasks deterministically
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bridge_shutdown_cancels_waiting_and_streaming_tasks() {
    let wait_reason = Arc::new(std::sync::Mutex::new(None::<RequestCancellationReason>));
    let wait_reason_clone = wait_reason.clone();
    let stream_exited = Arc::new(AtomicBool::new(false));
    let stream_exited_clone = stream_exited.clone();

    // One service covering two shapes by path:
    // /wait -> blocked before response-start (lifecycle waiter)
    // /stream -> active streaming response + deferred body not yet complete
    let svc = service_fn_with_policy(
        move |req: Request| {
            let wait_reason = wait_reason_clone.clone();
            let stream_exited = stream_exited_clone.clone();
            async move {
                let path = req.head().target().path().to_owned();
                if path == "/wait" {
                    let lc = req.lifecycle_clone();
                    let (_head, _body) = req.into_head_and_body();
                    // Block before response-start until shutdown cancels.
                    lc.cancelled().await;
                    if let Some(reason) = lc.cancellation_reason() {
                        *wait_reason.lock().unwrap() = Some(reason);
                    }
                    return Err::<Response, _>(ServiceError::internal("cancelled"));
                }
                // /stream: return a pending-ish stream the shutdown must drop.
                let lc = req.lifecycle_clone();
                let (_head, body) = req.into_head_and_body();
                // Deferred body consumer watching lifecycle.
                tokio::spawn(async move {
                    let mut body = body;
                    tokio::select! {
                        _ = async {
                            while let Ok(Some(_)) = body.next_chunk().await {}
                        } => {},
                        _ = lc.cancelled() => {},
                    }
                });
                let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(2);
                tokio::spawn(async move {
                    // Never send; hold until the transport drops the stream.
                    let _tx = tx;
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    stream_exited.store(true, Ordering::SeqCst);
                });
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Stream(receiver_to_stream(rx)))
                    .unwrap())
            }
        },
        stream_policy(),
    );
    let config = Arc::new(
        RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(1024 * 1024)
            .body_read_timeout(Duration::from_secs(5))
            .handler_timeout(Duration::from_secs(10))
            .graceful_shutdown_timeout(Duration::from_secs(3))
            .connection_total_timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
    );
    let server = Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    // Waiting request blocked before response-start.
    let mut waiter = tokio::net::TcpStream::connect(addr).await.unwrap();
    waiter
        .write_all(b"GET /wait HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    // Streaming request with active upload.
    let mut streamer = tokio::net::TcpStream::connect(addr).await.unwrap();
    streamer
        .write_all(b"POST /stream HTTP/1.1\r\nHost: x\r\nContent-Length: 100\r\n\r\n")
        .await
        .unwrap();
    streamer.write_all(b"partial").await.unwrap();
    streamer.flush().await.unwrap();
    // Streaming response-start arrives, body held open.
    let headers =
        tokio::time::timeout(Duration::from_secs(3), read_response_headers(&mut streamer))
            .await
            .unwrap();
    assert!(String::from_utf8_lossy(&headers).starts_with("HTTP/1.1 200"));

    handle.shutdown();
    tokio::time::timeout(Duration::from_secs(6), handle.wait())
        .await
        .expect("graceful shutdown must drain/cancel bridge tasks")
        .unwrap();

    assert_eq!(
        *wait_reason.lock().unwrap(),
        Some(RequestCancellationReason::ServerShutdown),
        "waiting task must observe shutdown"
    );
    // Streaming holder never reached its 30s sleep (transport dropped it).
    assert!(
        !stream_exited.load(Ordering::SeqCst),
        "streaming task must be cancelled, not run to completion"
    );
}

// ---------------------------------------------------------------------------
// Track E — timeout + admission composition
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bridge_handler_timeout_split_from_body_timeout() {
    // handler_timeout bounds response-start; body timeout is separate.
    // A slow Start must fail fast even with a generous body deadline.
    let svc = service_fn_with_policy(
        |req: Request| async move {
            let (_head, _body) = req.into_head_and_body();
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"late".to_vec()))
                .unwrap())
        },
        stream_policy(),
    );
    let config = Arc::new(
        RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(1024 * 1024)
            .body_read_timeout(Duration::from_secs(10))
            .handler_timeout(Duration::from_millis(300))
            .connection_total_timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
    );
    let server = Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET /slow HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut buf))
        .await
        .unwrap()
        .unwrap();
    let text = String::from_utf8_lossy(&buf);
    // Runtime maps handler timeout to 504 without waiting for the 5s app sleep.
    assert!(
        text.starts_with("HTTP/1.1 504"),
        "handler timeout must bound response-start, got: {text}"
    );

    handle.shutdown();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn bridge_downstream_admission_distinct_and_bounded() {
    // EggServe `max_in_flight_requests` bounds pre-response Service::call;
    // the fixture owns a separate bounded app-task semaphore for work that
    // continues after response-start. Saturation maps deterministically
    // without changing core policy, and permits return on cancellation.
    let app_budget = Arc::new(tokio::sync::Semaphore::new(1));
    let app_budget_clone = app_budget.clone();
    let entered = Arc::new(AtomicUsize::new(0));
    let entered_clone = entered.clone();

    let svc = service_fn_with_policy(
        move |req: Request| {
            let budget = app_budget_clone.clone();
            let entered = entered_clone.clone();
            async move {
                // Downstream admission: non-blocking acquire; saturation maps
                // to a deterministic fixture response (503) rather than an
                // unbounded queue.
                let _permit = match budget.clone().try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => {
                        return Err::<Response, _>(ServiceError::rejected(503, "app saturated"));
                    }
                };
                entered.fetch_add(1, Ordering::SeqCst);
                let lc = req.lifecycle_clone();
                let (_head, _body) = req.into_head_and_body();
                // Hold the downstream permit past response-start via a
                // bounded streaming response.
                let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(2);
                tokio::spawn(async move {
                    // Hold ~400ms or until cancelled; permit returns either way.
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(400)) => {},
                        _ = lc.cancelled() => {},
                    }
                    let _ = tx.send(Bytes::from_static(b"done")).await;
                    drop(_permit);
                });
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Stream(receiver_to_stream(rx)))
                    .unwrap())
            }
        },
        RequestBodyPolicy::Reject,
    );
    let config = Arc::new(
        RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            // Core admission stays wide open: saturation below is purely
            // downstream-owned, proving the split.
            .max_in_flight_requests(64)
            .handler_timeout(Duration::from_secs(5))
            .connection_total_timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
    );
    let server = Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    // First request occupies the single downstream permit.
    let mut first = tokio::net::TcpStream::connect(addr).await.unwrap();
    first
        .write_all(b"GET /one HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let first_headers =
        tokio::time::timeout(Duration::from_secs(3), read_response_headers(&mut first))
            .await
            .unwrap();
    assert!(String::from_utf8_lossy(&first_headers).starts_with("HTTP/1.1 200"));

    // Second concurrent request must deterministically saturate downstream.
    let mut second = tokio::net::TcpStream::connect(addr).await.unwrap();
    second
        .write_all(b"GET /two HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut second_raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), second.read_to_end(&mut second_raw))
        .await
        .unwrap()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&second_raw).starts_with("HTTP/1.1 503"),
        "downstream saturation must map deterministically, got: {}",
        String::from_utf8_lossy(&second_raw)
    );

    // First stream completes; the permit returns (no leak).
    let first_body = tokio::time::timeout(Duration::from_secs(3), read_chunked_body(&mut first))
        .await
        .unwrap();
    assert_eq!(first_body, b"done");
    assert_eq!(entered.load(Ordering::SeqCst), 1);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        app_budget.available_permits(),
        1,
        "downstream permit must return after completion"
    );

    // A third request after recovery succeeds (no stuck queue).
    let mut third = tokio::net::TcpStream::connect(addr).await.unwrap();
    third
        .write_all(b"GET /three HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut third_raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), third.read_to_end(&mut third_raw))
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&third_raw).starts_with("HTTP/1.1 200"));

    handle.shutdown();
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// Track F — transport parity (TCP covered above; caller-owned + TLS here)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bridge_caller_owned_transport_parity() {
    let (tx, rx) = tokio::sync::oneshot::channel::<CapturedMeta>();
    let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
    let svc = service_fn_with_policy(
        move |req: Request| {
            let tx = tx.clone();
            async move {
                // Caller-owned transports expose no fabricated socket addrs.
                assert!(req.connection().local_addr.is_none());
                assert!(req.connection().remote_addr.is_none());
                if let Some(tx) = tx.lock().unwrap().take() {
                    let _ = tx.send(capture_meta(&req));
                }
                let (_head, body) = req.into_head_and_body();
                // Deferred ownership works without sockets.
                tokio::spawn(async move {
                    let mut body = body;
                    let mut total = 0usize;
                    while let Ok(Some(chunk)) = body.next_chunk().await {
                        total += chunk.len();
                    }
                    assert_eq!(total, 11);
                });
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"OK".to_vec()))
                    .unwrap())
            }
        },
        stream_policy(),
    );
    let config: Arc<RuntimeConfig> = Arc::new(
        RuntimeConfig::builder()
            .max_request_body_bytes(1024 * 1024)
            .body_read_timeout(Duration::from_secs(5))
            .handler_timeout(Duration::from_secs(5))
            .build()
            .unwrap(),
    );
    let runtime = Arc::new(RuntimeState::new(&config));
    let (mut client, server_io) = tokio::io::duplex(64 * 1024);
    let shutdown: &'static ConnectionShutdown = Box::leak(Box::new(ConnectionShutdown::new()));
    let context = ConnectionContext::for_non_socket(Scheme::Http, None);
    let driver = tokio::spawn(serve_http1_connection(
        server_io, svc, config, context, runtime, shutdown,
    ));

    client
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 11\r\n\r\nhello ")
        .await
        .unwrap();
    // Early response headers arrive before the rest of the upload.
    let mut acc = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        client.read_exact(&mut byte).await.unwrap();
        acc.push(byte[0]);
        if acc.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    assert!(String::from_utf8_lossy(&acc).starts_with("HTTP/1.1 200"));
    let mut body = [0u8; 2];
    client.read_exact(&mut body).await.unwrap();
    assert_eq!(&body, b"OK");
    client.write_all(b"world").await.unwrap();
    client.flush().await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Keep-alive reuse on the caller-owned stream.
    client
        .write_all(b"GET /second HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"));

    let meta = tokio::time::timeout(Duration::from_secs(3), rx)
        .await
        .unwrap()
        .unwrap();
    // Identical application-visible model except truthful connection metadata.
    assert_eq!(meta.method, "POST");
    assert_eq!(meta.scheme, "http");
    assert_eq!(meta.local, None);
    assert_eq!(meta.remote, None);
    assert!(!meta.tls_present);

    let _ = driver.await.unwrap();
}

#[tokio::test]
#[cfg(feature = "tls")]
async fn bridge_tls_transport_parity() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let key_pair = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    let cert = params.self_signed(&key_pair).unwrap();
    let cert_der: rustls::pki_types::CertificateDer<'static> = cert.into();
    let key_der = rustls::pki_types::PrivatePkcs8KeyDer::from(key_pair.serialize_der());
    let server_tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key_der.into())
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert_der).unwrap();
    let client_tls = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let (tx, rx) = tokio::sync::oneshot::channel::<CapturedMeta>();
    let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
    let svc = service_fn_with_policy(
        move |req: Request| {
            let tx = tx.clone();
            async move {
                if let Some(tx) = tx.lock().unwrap().take() {
                    let _ = tx.send(capture_meta(&req));
                }
                let (_head, body) = req.into_head_and_body();
                tokio::spawn(async move {
                    let mut body = body;
                    while let Ok(Some(_)) = body.next_chunk().await {}
                });
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"OK".to_vec()))
                    .unwrap())
            }
        },
        stream_policy(),
    );
    let config = Arc::new(
        RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .max_request_body_bytes(1024 * 1024)
            .body_read_timeout(Duration::from_secs(5))
            .handler_timeout(Duration::from_secs(5))
            .tls_config(Arc::new(server_tls))
            .build()
            .unwrap(),
    );
    let server = Server::builder()
        .runtime((*config).clone())
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(client_tls));
    let mut client = connector
        .connect("localhost".try_into().unwrap(), tcp)
        .await
        .unwrap();
    client
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 11\r\n\r\nhello ")
        .await
        .unwrap();
    client.flush().await.unwrap();
    let mut acc = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        client.read_exact(&mut byte).await.unwrap();
        acc.push(byte[0]);
        if acc.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    assert!(String::from_utf8_lossy(&acc).starts_with("HTTP/1.1 200"));
    let mut body = [0u8; 2];
    client.read_exact(&mut body).await.unwrap();
    client.write_all(b"world").await.unwrap();
    client.flush().await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    client
        .write_all(b"GET /second HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"));

    let meta = tokio::time::timeout(Duration::from_secs(3), rx)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(meta.scheme, "https");
    assert!(meta.tls_present);
    assert!(meta.local.is_some());
    // TLS session metadata crosses the canonical boundary truthfully.
    assert!(meta.tls_protocol.is_some());
    assert_eq!(meta.tls_sni.as_deref(), Some("localhost"));

    handle.shutdown();
    handle.wait().await.unwrap();
}

// ---------------------------------------------------------------------------
// Performance sanity (correctness-first, non-gating)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bridge_perf_sanity_no_pathological_overhead() {
    // Direct trivial native Service baseline vs bounded-channel bridge with
    // one response chunk vs streamed bridge with multiple chunks. Detects
    // accidental orders-of-magnitude overhead only; not a latency/RPS gate
    // and never a Uvicorn/Granian parity claim.
    async fn bench<S>(svc: S, n: usize) -> Duration
    where
        S: Service,
    {
        let server = Server::builder()
            .runtime(
                RuntimeConfig::builder()
                    .bind("127.0.0.1:0".parse().unwrap())
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let handle = server.start_with_service(svc).await.unwrap();
        handle.ready().await.unwrap();
        let addr = handle.local_addr();
        let start = std::time::Instant::now();
        for _ in 0..n {
            let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
            client
                .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            let mut buf = Vec::new();
            client.read_to_end(&mut buf).await.unwrap();
            assert!(String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"));
        }
        let elapsed = start.elapsed();
        handle.shutdown();
        handle.wait().await.unwrap();
        elapsed
    }

    let baseline = bench(
        service_fn_with_policy(
            |_req: Request| async {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"ok".to_vec()))
                    .unwrap())
            },
            RequestBodyPolicy::Reject,
        ),
        20,
    )
    .await;

    let one_chunk = bench(
        service_fn_with_policy(
            |req: Request| async move {
                let (_head, _body) = req.into_head_and_body();
                // Bounded single-chunk bridge (cap 2).
                let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(2);
                tokio::spawn(async move {
                    let _ = tx.send(Bytes::from_static(b"ok")).await;
                });
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Stream(receiver_to_stream(rx)))
                    .unwrap())
            },
            RequestBodyPolicy::Reject,
        ),
        20,
    )
    .await;

    let multi_chunk = bench(
        service_fn_with_policy(
            |req: Request| async move {
                let (_head, _body) = req.into_head_and_body();
                let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(2);
                tokio::spawn(async move {
                    for chunk in [b"a".as_slice(), b"b", b"c", b"d"] {
                        if tx.send(Bytes::copy_from_slice(chunk)).await.is_err() {
                            break;
                        }
                    }
                });
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Stream(receiver_to_stream(rx)))
                    .unwrap())
            },
            RequestBodyPolicy::Reject,
        ),
        20,
    )
    .await;

    eprintln!(
        "plan175 perf sanity (20 req each): baseline={:?} one-chunk={:?} multi-chunk={:?}",
        baseline, one_chunk, multi_chunk
    );
    // Generous non-gating sanity: each 20-request run completes quickly on
    // loopback and the bridge stays within an order of magnitude of baseline.
    // If fixture scheduling dominates, that fact is recorded, not fixed in core.
    for (name, elapsed) in [
        ("baseline", baseline),
        ("one-chunk", one_chunk),
        ("multi-chunk", multi_chunk),
    ] {
        assert!(
            elapsed < Duration::from_secs(15),
            "{name} run took {elapsed:?}, possible pathological stall"
        );
    }
    let baseline_ms = baseline.as_millis().max(1);
    assert!(
        one_chunk.as_millis() < baseline_ms * 10 + 2000,
        "bounded bridge overhead pathological: baseline={baseline:?} bridge={one_chunk:?}"
    );
}
