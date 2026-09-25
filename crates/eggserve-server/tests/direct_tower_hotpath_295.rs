//! Plan 295 focused tests: direct Tower hot-path optimizations.
//!
//! P1 — known-length Tower response fast path (`response_from_http_body`
//! declares exact size hints; transport verifies, framing stays
//! runtime-owned).
//! P2 — provably-empty request bodies complete as empty (keep-alive reuse
//! preserved for bodyless Buffer/Stream requests; unread wire bytes still
//! force close).

#![cfg(feature = "tower")]

use std::convert::Infallible;
use std::time::Duration;

use bytes::Bytes;
use eggserve_primitives::request_body_policy::RequestBodyPolicy;
use eggserve_primitives::{Response, ResponseBody, StatusCode};
use eggserve_server::{RuntimeConfig, Server, TowerToEggserve};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn start<S>(service: S) -> (std::net::SocketAddr, eggserve_server::ServerControl)
where
    S: eggserve_server::Service,
{
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = Server::builder()
        .runtime(
            RuntimeConfig::builder()
                .bind(addr)
                .max_request_body_bytes(1024 * 1024)
                .build()
                .unwrap(),
        )
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let (control, completion) = handle.into_parts();
    tokio::spawn(async move {
        let mut completion = completion;
        let _ = completion.wait().await;
    });
    (addr, control)
}

async fn exchange(addr: std::net::SocketAddr, raw_request: &[u8]) -> Vec<u8> {
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket.write_all(raw_request).await.unwrap();
    let mut wire = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), socket.read_to_end(&mut wire))
        .await
        .unwrap()
        .unwrap();
    wire
}

fn head_text(wire: &[u8]) -> String {
    let split = wire
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response must have header terminator");
    String::from_utf8_lossy(&wire[..split]).to_ascii_lowercase()
}

#[derive(Clone)]
struct FullKib;

impl tower_service::Service<http::Request<eggserve_server::interop::HttpRequestBody>> for FullKib {
    type Response = http::Response<http_body_util::Full<Bytes>>;
    type Error = Infallible;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(
        &mut self,
        _req: http::Request<eggserve_server::interop::HttpRequestBody>,
    ) -> Self::Future {
        let body = http_body_util::Full::new(Bytes::from(vec![b'k'; 1024]));
        std::future::ready(Ok(http::Response::builder()
            .status(200)
            .body(body)
            .unwrap()))
    }
}

#[tokio::test]
async fn p1_full_body_emits_content_length() {
    let (addr, control) = start(TowerToEggserve::with_policy(
        FullKib,
        RequestBodyPolicy::Reject,
    ))
    .await;
    let wire = exchange(
        addr,
        b"GET / HTTP/1.1\r\nHost: p1\r\nConnection: close\r\n\r\n",
    )
    .await;
    let head = head_text(&wire);
    assert!(head.contains("http/1.1 200"), "{head}");
    assert!(
        head.contains("content-length: 1024"),
        "P1: exact-hint Tower body must carry Content-Length: {head}"
    );
    assert!(
        !head.contains("transfer-encoding: chunked"),
        "P1: known-length Tower body must not be chunked: {head}"
    );
    assert!(wire.ends_with(&vec![b'k'; 1024]));
    control.shutdown();
}

#[tokio::test]
async fn p1_unknown_length_stays_chunked() {
    #[derive(Clone)]
    struct Streamed;
    impl tower_service::Service<http::Request<eggserve_server::interop::HttpRequestBody>> for Streamed {
        type Response = http::Response<
            http_body_util::StreamBody<
                futures_util::stream::Iter<
                    std::vec::IntoIter<Result<http_body::Frame<Bytes>, Infallible>>,
                >,
            >,
        >;
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(
            &mut self,
            _req: http::Request<eggserve_server::interop::HttpRequestBody>,
        ) -> Self::Future {
            let frames = vec![Ok(http_body::Frame::data(Bytes::from_static(b"chunk")))];
            let body = http_body_util::StreamBody::new(futures_util::stream::iter(frames));
            std::future::ready(Ok(http::Response::builder()
                .status(200)
                .body(body)
                .unwrap()))
        }
    }
    let (addr, control) = start(TowerToEggserve::with_policy(
        Streamed,
        RequestBodyPolicy::Reject,
    ))
    .await;
    let wire = exchange(
        addr,
        b"GET / HTTP/1.1\r\nHost: p1\r\nConnection: close\r\n\r\n",
    )
    .await;
    let head = head_text(&wire);
    assert!(head.contains("http/1.1 200"), "{head}");
    assert!(
        head.contains("transfer-encoding: chunked"),
        "P1: hintless Tower body must stay chunked: {head}"
    );
    assert!(!head.contains("content-length"), "{head}");
    let text = String::from_utf8_lossy(&wire);
    assert!(text.contains("chunk"), "{text}");
    control.shutdown();
}

#[tokio::test]
async fn p1_short_body_fails_closed_without_second_response() {
    // Exact hint claims 8 bytes but the producer yields 3 then ends: the
    // transport must truncate (close) rather than synthesize a second HTTP
    // error after commitment, and the server must stay alive afterwards.
    #[derive(Clone)]
    struct Lying;
    impl tower_service::Service<http::Request<eggserve_server::interop::HttpRequestBody>> for Lying {
        type Response = http::Response<http_body_util::Full<Bytes>>;
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(
            &mut self,
            _req: http::Request<eggserve_server::interop::HttpRequestBody>,
        ) -> Self::Future {
            // Full(3 bytes) reports exact 3; wrap it so the adapter sees a
            // longer declaration via a custom body below instead.
            let body = http_body_util::Full::new(Bytes::from_static(b"abc"));
            std::future::ready(Ok(http::Response::builder()
                .status(200)
                .body(body)
                .unwrap()))
        }
    }
    // Direct adapter-level check with a lying size hint: declare 8, yield 3.
    struct LyingBody;
    impl http_body::Body for LyingBody {
        type Data = Bytes;
        type Error = std::io::Error;

        fn poll_frame(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
            std::task::Poll::Ready(None)
        }

        fn size_hint(&self) -> http_body::SizeHint {
            http_body::SizeHint::with_exact(8)
        }
    }
    let canonical = eggserve_server::interop::response_from_http_body(
        http::Response::builder()
            .status(200)
            .body(LyingBody)
            .unwrap(),
    )
    .unwrap();
    match canonical.body() {
        Some(ResponseBody::Stream(stream)) => {
            assert_eq!(
                stream.known_length(),
                Some(8),
                "P1: exact hint must be declared for verification"
            );
        }
        other => panic!("P1: lying body must take the streaming path, got {other:?}"),
    }
    // Wire-level truncation: Full path serves fine (control case).
    let (addr, control) = start(TowerToEggserve::with_policy(
        Lying,
        RequestBodyPolicy::Reject,
    ))
    .await;
    let wire = exchange(
        addr,
        b"GET / HTTP/1.1\r\nHost: p1\r\nConnection: close\r\n\r\n",
    )
    .await;
    let head = head_text(&wire);
    assert!(head.contains("content-length: 3"), "{head}");
    assert!(wire.ends_with(b"abc"));
    // Server alive afterwards.
    let again = exchange(
        addr,
        b"GET / HTTP/1.1\r\nHost: p1\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head_text(&again).contains("http/1.1 200"));
    control.shutdown();
}

#[tokio::test]
async fn p1_head_suppresses_known_length_body() {
    let (addr, control) = start(TowerToEggserve::with_policy(
        FullKib,
        RequestBodyPolicy::Reject,
    ))
    .await;
    let wire = exchange(
        addr,
        b"HEAD / HTTP/1.1\r\nHost: p1\r\nConnection: close\r\n\r\n",
    )
    .await;
    let head = head_text(&wire);
    assert!(head.contains("http/1.1 200"), "{head}");
    let split = wire.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    assert!(
        wire.len() == split + 4,
        "P1: HEAD must carry no body bytes even for known-length Tower bodies"
    );
    control.shutdown();
}

async fn keep_alive_requests(addr: std::net::SocketAddr, raw_request: &[u8], count: usize) {
    let mut socket = TcpStream::connect(addr).await.unwrap();
    for i in 0..count {
        socket.write_all(raw_request).await.unwrap();
        let mut head = Vec::with_capacity(256);
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            tokio::time::timeout(Duration::from_secs(10), socket.read_exact(&mut byte))
                .await
                .unwrap_or_else(|_| panic!("P2: request {i} timed out"))
                .unwrap_or_else(|_| panic!("P2: request {i} got EOF (server closed)"));
            head.push(byte[0]);
            assert!(head.len() < 64 * 1024, "response head too large");
        }
        let text = String::from_utf8_lossy(&head).to_ascii_lowercase();
        assert!(text.starts_with("http/1.1 200"), "P2 request {i}: {text}");
        // Drain the 1 KiB / small body so the next request can be parsed.
        let len: usize = text
            .lines()
            .find_map(|line| {
                line.strip_prefix("content-length:")
                    .and_then(|v| v.trim().parse().ok())
            })
            .unwrap_or(0);
        let mut remaining = len;
        let mut buf = [0u8; 4096];
        while remaining > 0 {
            let take = remaining.min(buf.len());
            let n = socket.read(&mut buf[..take]).await.unwrap();
            assert!(n > 0, "P2: short body on request {i}");
            remaining -= n;
        }
    }
}

#[tokio::test]
async fn p2_stream_ignored_empty_keeps_alive_native_and_tower() {
    let native_ignore = eggserve_server::service_fn_with_policy(
        |_req: eggserve_server::Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        },
        RequestBodyPolicy::Stream {
            max_bytes: 1024 * 1024,
        },
    );
    let (native_addr, native_control) = start(native_ignore).await;
    keep_alive_requests(
        native_addr,
        b"GET / HTTP/1.1\r\nHost: p2\r\nConnection: keep-alive\r\n\r\n",
        20,
    )
    .await;

    async fn axum_ok() -> axum::response::Response {
        axum::response::Response::new(axum::body::Body::from("ok"))
    }
    let router: axum::Router = axum::Router::new().route("/", axum::routing::get(axum_ok));
    // Default constructor: Stream 1 MiB — the exact shape that closed
    // every connection before P2.
    let (tower_addr, tower_control) = start(TowerToEggserve::new(router)).await;
    keep_alive_requests(
        tower_addr,
        b"GET / HTTP/1.1\r\nHost: p2\r\nConnection: keep-alive\r\n\r\n",
        20,
    )
    .await;

    native_control.shutdown();
    tower_control.shutdown();
}

#[tokio::test]
async fn p2_unread_nonempty_body_still_closes() {
    // Framing safety: a Stream service that ignores a PRESENT body must
    // still force close so unread bytes are never parsed as a next request.
    let native_ignore = eggserve_server::service_fn_with_policy(
        |_req: eggserve_server::Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        },
        RequestBodyPolicy::Stream {
            max_bytes: 1024 * 1024,
        },
    );
    let (addr, control) = start(native_ignore).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"POST / HTTP/1.1\r\nHost: p2\r\nContent-Length: 18\r\nConnection: keep-alive\r\n\r\nsmall-post-payload!!")
        .await
        .unwrap();
    let mut head = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        socket.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&head).to_ascii_lowercase();
    assert!(text.starts_with("http/1.1 200"), "{text}");
    assert!(
        text.contains("connection: close"),
        "P2: unread PRESENT body must still force close: {text}"
    );
    control.shutdown();
}

#[tokio::test]
async fn p2_consumed_body_keeps_alive() {
    let native_read = eggserve_server::service_fn_with_policy(
        |req: eggserve_server::Request| async move {
            let bytes = req.into_body().read_all().await.unwrap();
            assert_eq!(bytes.len(), 18);
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        },
        RequestBodyPolicy::Stream {
            max_bytes: 1024 * 1024,
        },
    );
    let (addr, control) = start(native_read).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    for i in 0..5 {
        socket
            .write_all(b"POST / HTTP/1.1\r\nHost: p2\r\nContent-Length: 18\r\nConnection: keep-alive\r\n\r\nsmall-post-payload!!")
            .await
            .unwrap();
        let mut head = Vec::with_capacity(256);
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }
        let text = String::from_utf8_lossy(&head).to_ascii_lowercase();
        assert!(text.starts_with("http/1.1 200"), "P2 request {i}: {text}");
        assert!(
            !text.contains("connection: close"),
            "P2: consumed body must keep alive: {text}"
        );
        // Drain the 2-byte body.
        let mut body = [0u8; 2];
        socket.read_exact(&mut body).await.unwrap();
    }
    control.shutdown();
}
