//! Plan 294 baseline fixtures: identical native vs direct-Tower/Axum workloads.
//!
//! Non-ignored tests are behavior-parity controls that run in routine CI.
//! The `baseline_timing_matrix` test is `#[ignore]`d: it performs
//! same-machine loopback timing and must be run explicitly with release
//! profile; its stdout JSON is retained under
//! `benchmarks/294-direct-tower-baseline/`.

#![cfg(feature = "tower")]

use std::convert::Infallible;
use std::time::{Duration, Instant};

use axum::body::Body as AxumBody;
use axum::response::Response as AxumResponse;
use axum::routing::{get, post};
use axum::Router;
use bytes::Bytes;
use eggserve_primitives::request_body_policy::RequestBodyPolicy;
use eggserve_primitives::{Response, ResponseBody, StatusCode};
use eggserve_server::{service_fn, RuntimeConfig, Server, TowerToEggserve};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const ONE_KIB: usize = 1024;

fn one_kib_body() -> Vec<u8> {
    vec![b'k'; ONE_KIB]
}

macro_rules! native_one_kib {
    () => {
        service_fn(|_req: eggserve_server::Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(one_kib_body()))
                .unwrap())
        })
    };
}

#[derive(Clone)]
struct TowerOneKib;

impl tower_service::Service<http::Request<eggserve_server::interop::HttpRequestBody>>
    for TowerOneKib
{
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
        let body = http_body_util::Full::new(Bytes::from(one_kib_body()));
        std::future::ready(Ok(http::Response::builder()
            .status(200)
            .body(body)
            .unwrap()))
    }
}

async fn axum_one_kib() -> AxumResponse {
    AxumResponse::new(AxumBody::from(one_kib_body()))
}

fn axum_router() -> Router {
    Router::new().route("/", get(axum_one_kib))
}

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
                // Explicit direct application-server profile ceiling: the
                // runtime default is 0 (reject all bodies), so POST/streaming
                // fixtures must opt into a bounded ceiling.
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

async fn get_close(addr: std::net::SocketAddr, path: &str, extra_headers: &str) -> Vec<u8> {
    let mut socket = TcpStream::connect(addr).await.unwrap();
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: baseline\r\nConnection: close\r\n{extra_headers}\r\n"
    );
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut wire = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), socket.read_to_end(&mut wire))
        .await
        .unwrap()
        .unwrap();
    wire
}

async fn get_close_te_trailers(
    addr: std::net::SocketAddr,
    path: &str,
    extra_headers: &str,
) -> Vec<u8> {
    // RFC 9110 §6.5: the server suppresses response trailers unless the
    // client advertises `TE: trailers`.
    let mut socket = TcpStream::connect(addr).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: baseline\r\nTE: trailers\r\nConnection: close\r\n{extra_headers}\r\n");
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut wire = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), socket.read_to_end(&mut wire))
        .await
        .unwrap()
        .unwrap();
    wire
}

fn status_and_body(wire: &[u8]) -> (String, Vec<u8>) {
    let split = wire
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response must have header terminator");
    let head = String::from_utf8_lossy(&wire[..split]).to_ascii_lowercase();
    let status = head.lines().next().unwrap_or("").to_owned();
    let body = decode_body(&head, &wire[split + 4..]);
    (status, body)
}

fn decode_body(head: &str, raw: &[u8]) -> Vec<u8> {
    if head.contains("transfer-encoding: chunked") {
        let mut out = Vec::new();
        let mut rest = raw;
        loop {
            let line_end = rest
                .windows(2)
                .position(|w| w == b"\r\n")
                .expect("chunk size line");
            let size_line = std::str::from_utf8(&rest[..line_end]).unwrap();
            let size = usize::from_str_radix(size_line.trim(), 16).unwrap();
            rest = &rest[line_end + 2..];
            if size == 0 {
                break;
            }
            out.extend_from_slice(&rest[..size]);
            rest = &rest[size + 2..];
        }
        out
    } else if let Some(len) = head.lines().find_map(|line| {
        line.strip_prefix("content-length:")
            .and_then(|v| v.trim().parse::<usize>().ok())
    }) {
        raw[..len.min(raw.len())].to_vec()
    } else {
        raw.to_vec()
    }
}

#[tokio::test]
async fn baseline_294_native_vs_tower_1kib_parity() {
    let (native_addr, native_control) = start(native_one_kib!()).await;
    let (tower_addr, tower_control) = start(TowerToEggserve::with_policy(
        TowerOneKib,
        RequestBodyPolicy::Stream {
            max_bytes: 1024 * 1024,
        },
    ))
    .await;
    let (axum_addr, axum_control) = start(TowerToEggserve::with_policy(
        axum_router(),
        RequestBodyPolicy::Stream {
            max_bytes: 1024 * 1024,
        },
    ))
    .await;

    let native = get_close(native_addr, "/", "").await;
    let tower = get_close(tower_addr, "/", "").await;
    let axum = get_close(axum_addr, "/", "").await;
    let (native_status, native_body) = status_and_body(&native);
    let (tower_status, tower_body) = status_and_body(&tower);
    let (axum_status, axum_body) = status_and_body(&axum);
    assert!(native_status.contains("200"), "{native_status}");
    assert_eq!(tower_status, native_status);
    assert_eq!(axum_status, native_status);
    assert_eq!(native_body.len(), ONE_KIB);
    assert_eq!(tower_body, native_body);
    assert_eq!(axum_body, native_body);

    native_control.shutdown();
    tower_control.shutdown();
    axum_control.shutdown();
}

#[tokio::test]
async fn baseline_294_header_count_sensitivity_parity() {
    let echo = service_fn(|req: eggserve_server::Request| async move {
        let count = req.head().headers().len();
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(count.to_string().into_bytes()))
            .unwrap())
    });
    #[derive(Clone)]
    struct TowerEcho;
    impl tower_service::Service<http::Request<eggserve_server::interop::HttpRequestBody>>
        for TowerEcho
    {
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
            req: http::Request<eggserve_server::interop::HttpRequestBody>,
        ) -> Self::Future {
            // `http::HeaderMap` exposes one entry per stored value; matches the
            // canonical count for distinct header names used here.
            let count = req.headers().len();
            let body = http_body_util::Full::new(Bytes::from(count.to_string()));
            std::future::ready(Ok(http::Response::builder()
                .status(200)
                .body(body)
                .unwrap()))
        }
    }

    let (native_addr, native_control) = start(echo).await;
    let (tower_addr, tower_control) = start(TowerToEggserve::with_policy(
        TowerEcho,
        RequestBodyPolicy::Reject,
    ))
    .await;

    for header_count in [1usize, 16, 64] {
        let mut extra = String::new();
        for i in 0..header_count {
            extra.push_str(&format!("x-h-{i}: v{i}\r\n"));
        }
        let (_, native_body) = status_and_body(&get_close(native_addr, "/", &extra).await);
        let (_, tower_body) = status_and_body(&get_close(tower_addr, "/", &extra).await);
        // Parity control: both profiles must observe the same header set.
        // Absolute counts are runtime-normalization-internal (hop-by-hop
        // stripping); what matters is native/tower convergence.
        let native_text = String::from_utf8(native_body).unwrap();
        let tower_text = String::from_utf8(tower_body).unwrap();
        assert_eq!(
            tower_text, native_text,
            "tower must observe the same headers as native for {header_count} sent headers"
        );
        assert!(
            native_text.parse::<usize>().is_ok(),
            "echo service must return a header count"
        );
    }

    native_control.shutdown();
    tower_control.shutdown();
}

#[tokio::test]
async fn baseline_294_sse_like_streaming_parity() {
    const CHUNKS: usize = 32;
    let native_stream = service_fn(|_req: eggserve_server::Request| async move {
        let items: Vec<Result<Bytes, eggserve_primitives::ResponseStreamError>> = (0..CHUNKS)
            .map(|i| Ok(Bytes::from(format!("data:{i}\n"))))
            .collect();
        let stream = eggserve_primitives::ResponseStream::new(futures_util::stream::iter(items));
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(stream))
            .unwrap())
    });
    async fn axum_sse() -> AxumResponse {
        let items: Vec<Result<Bytes, Infallible>> = (0..32)
            .map(|i| Ok(Bytes::from(format!("data:{i}\n"))))
            .collect();
        AxumResponse::new(AxumBody::from_stream(futures_util::stream::iter(items)))
    }
    let router: Router = Router::new().route("/sse", get(axum_sse));

    let (native_addr, native_control) = start(native_stream).await;
    let (tower_addr, tower_control) = start(TowerToEggserve::with_policy(
        router,
        RequestBodyPolicy::Reject,
    ))
    .await;

    let (_, native_body) = status_and_body(&get_close(native_addr, "/sse", "").await);
    let (_, tower_body) = status_and_body(&get_close(tower_addr, "/sse", "").await);
    let expected: Vec<u8> = (0..CHUNKS)
        .flat_map(|i| format!("data:{i}\n").into_bytes())
        .collect();
    assert_eq!(native_body, expected);
    assert_eq!(tower_body, expected);

    native_control.shutdown();
    tower_control.shutdown();
}

#[tokio::test]
async fn baseline_294_post_body_parity() {
    let native_echo = eggserve_server::service_fn_with_policy(
        |req: eggserve_server::Request| async move {
            let bytes = req.into_body().read_all().await.unwrap();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(bytes.to_vec()))
                .unwrap())
        },
        RequestBodyPolicy::Stream {
            max_bytes: 64 * 1024,
        },
    );
    async fn axum_echo(body: AxumBody) -> AxumResponse {
        use futures_util::StreamExt as _;
        let mut stream = body.into_data_stream();
        let mut out = Vec::new();
        while let Some(chunk) = stream.next().await {
            out.extend_from_slice(&chunk.unwrap());
        }
        AxumResponse::new(AxumBody::from(out))
    }
    let router: Router = Router::new().route("/echo", post(axum_echo));

    async fn post_close(addr: std::net::SocketAddr, payload: &[u8]) -> Vec<u8> {
        let mut socket = TcpStream::connect(addr).await.unwrap();
        let request = format!(
            "POST /echo HTTP/1.1\r\nHost: baseline\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        socket.write_all(request.as_bytes()).await.unwrap();
        socket.write_all(payload).await.unwrap();
        let mut wire = Vec::new();
        tokio::time::timeout(Duration::from_secs(10), socket.read_to_end(&mut wire))
            .await
            .unwrap()
            .unwrap();
        wire
    }

    let (native_addr, native_control) = start(native_echo).await;
    let (tower_addr, tower_control) = start(TowerToEggserve::with_policy(
        router,
        RequestBodyPolicy::Stream {
            max_bytes: 64 * 1024,
        },
    ))
    .await;

    let payload = b"small-post-payload";
    let (_, native_body) = status_and_body(&post_close(native_addr, payload).await);
    let (_, tower_body) = status_and_body(&post_close(tower_addr, payload).await);
    assert_eq!(native_body, payload);
    assert_eq!(tower_body, payload);

    native_control.shutdown();
    tower_control.shutdown();
}

#[tokio::test]
async fn baseline_294_terminal_trailer_parity() {
    use http_body::Frame;

    // Native terminal-trailer response.
    let native_trailer = service_fn(|_req: eggserve_server::Request| async move {
        let items = vec![Ok::<_, eggserve_primitives::ResponseStreamError>(
            Bytes::from_static(b"trailer-body"),
        )];
        let mut block = eggserve_primitives::HeaderBlock::new();
        block.push_str("x-end", "yes").unwrap();
        let trailers = eggserve_primitives::Trailers::new(block).unwrap();
        let stream = eggserve_primitives::ResponseStream::with_trailers(
            futures_util::stream::iter(items),
            async move { Ok(Some(trailers)) },
        );
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(stream))
            .unwrap())
    });

    // Tower terminal-trailer response: one data frame then a trailers frame.
    #[derive(Clone)]
    struct TowerTrailer;
    impl tower_service::Service<http::Request<eggserve_server::interop::HttpRequestBody>>
        for TowerTrailer
    {
        type Response = http::Response<
            http_body_util::StreamBody<
                futures_util::stream::Iter<
                    std::vec::IntoIter<Result<Frame<Bytes>, std::convert::Infallible>>,
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
            let mut map = http::HeaderMap::new();
            map.insert("x-end", "yes".parse().unwrap());
            let frames = vec![
                Ok(Frame::data(Bytes::from_static(b"trailer-body"))),
                Ok(Frame::trailers(map)),
            ];
            let body = http_body_util::StreamBody::new(futures_util::stream::iter(frames));
            std::future::ready(Ok(http::Response::builder()
                .status(200)
                .body(body)
                .unwrap()))
        }
    }

    let (native_addr, native_control) = start(native_trailer).await;
    let (tower_addr, tower_control) = start(TowerToEggserve::with_policy(
        TowerTrailer,
        RequestBodyPolicy::Reject,
    ))
    .await;

    let native_wire = get_close_te_trailers(native_addr, "/", "").await;
    let tower_wire = get_close_te_trailers(tower_addr, "/", "").await;

    // Baseline finding F3 (see results.json): Hyper's H1 server only encodes
    // response trailers when the response head declares them via `Trailer`,
    // which EggServe strips as runtime-owned and never synthesizes. Both the
    // native and direct-Tower paths therefore deliver the body without wire
    // trailers today. This test pins native/Tower CONVERGENCE at that
    // boundary; restoring wire trailers needs a separate scoped correctness
    // plan (head-time field declaration), explicitly out of the 294–297
    // polish campaign.
    for (name, wire) in [("native", &native_wire), ("tower", &tower_wire)] {
        let text = String::from_utf8_lossy(wire).to_ascii_lowercase();
        assert!(text.contains("200 ok"), "{name}: {text}");
        assert!(
            text.contains("trailer-body"),
            "{name} must carry the data frame"
        );
    }
    let (_, native_body) = status_and_body(&native_wire);
    let (_, tower_body) = status_and_body(&tower_wire);
    assert_eq!(native_body, b"trailer-body");
    assert_eq!(tower_body, b"trailer-body");
    assert_eq!(
        native_wire
            .windows(b"x-end: yes".len())
            .any(|w| w.eq_ignore_ascii_case(b"x-end: yes")),
        tower_wire
            .windows(b"x-end: yes".len())
            .any(|w| w.eq_ignore_ascii_case(b"x-end: yes")),
        "native and tower must converge on wire-trailer delivery"
    );

    native_control.shutdown();
    tower_control.shutdown();
}

/// One keep-alive request/response exchange; returns the lowercase head.
/// The socket is dropped afterwards, so a server-driven `connection: close`
/// is observable in the returned head without pipelining complexity.
async fn get_keepalive_head(addr: std::net::SocketAddr, path: &str) -> String {
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: baseline\r\nConnection: keep-alive\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut head = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        tokio::time::timeout(Duration::from_secs(10), socket.read_exact(&mut byte))
            .await
            .unwrap()
            .unwrap();
        head.push(byte[0]);
        assert!(head.len() < 64 * 1024, "response head too large");
    }
    String::from_utf8_lossy(&head).to_ascii_lowercase()
}

/// Behavior record: a `Stream`-policy Tower service that never polls the
/// (empty) request body.
///
/// Plan 294 recorded `connection: close` here (finding F2). Plan 295 P2
/// changed the contract: provably-empty bodies (Hyper already at
/// end-of-stream) complete as empty, so an unconsumed drop stays `Complete`
/// and keep-alive reuse is preserved — for native and Tower paths alike.
/// Bodies with potentially unread wire bytes still force close.
#[tokio::test]
async fn baseline_294_stream_policy_unconsumed_body_connection_behavior() {
    let (addr, control) = start(TowerToEggserve::with_policy(
        TowerOneKib,
        RequestBodyPolicy::Stream {
            max_bytes: 1024 * 1024,
        },
    ))
    .await;
    // Server-driven close is observable only on a keep-alive exchange
    // (`get_close` sends `Connection: close`, which the server echoes).
    let text = get_keepalive_head(addr, "/").await;
    assert!(text.contains("http/1.1 200"), "{text}");
    let (reject_addr, reject_control) = start(TowerToEggserve::with_policy(
        TowerOneKib,
        RequestBodyPolicy::Reject,
    ))
    .await;
    let reject_text = get_keepalive_head(reject_addr, "/").await;
    // Both policies keep alive on bodyless GET since 295 P2: provably-empty
    // bodies complete as empty, so the unconsumed drop stays Complete.
    assert!(
        !reject_text.contains("connection: close"),
        "reject bodyless GET must stay reusable: {reject_text}"
    );
    assert!(
        !text.contains("connection: close"),
        "stream-policy unconsumed EMPTY body must stay reusable since 295 P2: {text}"
    );
    control.shutdown();
    reject_control.shutdown();
}

#[tokio::test]
async fn baseline_294_streaming_cancellation_recovers() {
    async fn infinite() -> AxumResponse {
        use futures_util::StreamExt as _;
        let stream = futures_util::stream::iter(vec![Ok::<_, Infallible>(Bytes::from_static(
            b"cancel-chunk",
        ))])
        .chain(futures_util::stream::pending());
        AxumResponse::new(AxumBody::from_stream(stream))
    }
    let router: Router = Router::new()
        .route("/cancel", get(infinite))
        .route("/", get(axum_one_kib));
    let (addr, control) = start(TowerToEggserve::with_policy(
        router,
        RequestBodyPolicy::Reject,
    ))
    .await;

    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"GET /cancel HTTP/1.1\r\nHost: baseline\r\n\r\n")
        .await
        .unwrap();
    let mut first = vec![0u8; 4096];
    let count = tokio::time::timeout(Duration::from_secs(5), socket.read(&mut first))
        .await
        .unwrap()
        .unwrap();
    assert!(count > 0);
    drop(socket);

    // The server must still serve a fresh connection after the disconnect.
    let (_, body) = status_and_body(&get_close(addr, "/", "").await);
    assert_eq!(body.len(), ONE_KIB);
    control.shutdown();
}

/// Same-machine loopback timing matrix (ignored in routine CI).
///
/// Run explicitly:
/// `cargo test --release -p eggserve-server --features tower \
///   --test direct_tower_baseline_294 baseline_timing_matrix -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn baseline_timing_matrix() {
    #[derive(Debug)]
    struct Sample {
        case: &'static str,
        profile: &'static str,
        requests: usize,
        elapsed_s: f64,
        rps: f64,
        p50_ms: f64,
        p95_ms: f64,
        p99_ms: f64,
    }

    async fn keep_alive_latencies(
        addr: std::net::SocketAddr,
        raw_request: &[u8],
        requests: usize,
        case: &str,
    ) -> Vec<f64> {
        let mut socket = TcpStream::connect(addr).await.unwrap();
        let mut latencies = Vec::with_capacity(requests);
        for i in 0..requests {
            let began = Instant::now();
            if let Err(e) = socket.write_all(raw_request).await {
                panic!("{case} request {i}: write failed: {e}");
            }
            read_keepalive_response(&mut socket, case, i).await;
            latencies.push(began.elapsed().as_secs_f64() * 1000.0);
        }
        latencies
    }

    async fn read_keepalive_response(socket: &mut TcpStream, case: &str, index: usize) {
        let mut head = Vec::with_capacity(256);
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            socket
                .read_exact(&mut byte)
                .await
                .unwrap_or_else(|e| panic!("{case} response {index}: IO failed: {e}"));
            head.push(byte[0]);
            assert!(head.len() < 64 * 1024, "response head too large");
        }
        let text = String::from_utf8_lossy(&head).to_ascii_lowercase();
        assert!(
            text.starts_with("http/1.1 200 "),
            "unexpected status: {text}"
        );
        if text.contains("transfer-encoding: chunked") {
            loop {
                let mut line = Vec::new();
                loop {
                    socket
                        .read_exact(&mut byte)
                        .await
                        .unwrap_or_else(|e| panic!("{case} response {index}: IO failed: {e}"));
                    line.push(byte[0]);
                    if line.ends_with(b"\r\n") {
                        break;
                    }
                }
                let size =
                    usize::from_str_radix(std::str::from_utf8(&line).unwrap().trim(), 16).unwrap();
                if size == 0 {
                    // Consume trailers + final CRLF.
                    loop {
                        let mut trailer = Vec::new();
                        loop {
                            socket.read_exact(&mut byte).await.unwrap_or_else(|e| {
                                panic!("{case} response {index}: IO failed: {e}")
                            });
                            trailer.push(byte[0]);
                            if trailer.ends_with(b"\r\n") {
                                break;
                            }
                        }
                        if trailer == b"\r\n" {
                            break;
                        }
                    }
                    break;
                }
                let mut remaining = size;
                let mut buf = [0u8; 8 * 1024];
                while remaining > 0 {
                    let take = remaining.min(buf.len());
                    let n = socket.read(&mut buf[..take]).await.unwrap();
                    assert!(n > 0, "short chunk body");
                    remaining -= n;
                }
                let mut crlf = [0u8; 2];
                socket.read_exact(&mut crlf).await.unwrap();
            }
        } else if let Some(len) = text.lines().find_map(|line| {
            line.strip_prefix("content-length:")
                .and_then(|v| v.trim().parse::<usize>().ok())
        }) {
            let mut remaining = len;
            let mut buf = [0u8; 32 * 1024];
            while remaining > 0 {
                let take = remaining.min(buf.len());
                let n = socket.read(&mut buf[..take]).await.unwrap();
                assert!(n > 0, "short body");
                remaining -= n;
            }
        } else {
            panic!("response has neither chunked nor content-length framing");
        }
    }

    fn summarize(case: &'static str, mut latencies: Vec<f64>) -> Sample {
        latencies.sort_by(f64::total_cmp);
        let percentile = |q: f64| latencies[((latencies.len() - 1) as f64 * q).round() as usize];
        // Elapsed time is reconstructed from the sum of sequential latencies.
        let elapsed_s = latencies.iter().sum::<f64>() / 1000.0;
        Sample {
            case,
            profile: option_env!("PROFILE_294").unwrap_or("profile-not-set"),
            requests: latencies.len(),
            elapsed_s,
            rps: latencies.len() as f64 / elapsed_s,
            p50_ms: percentile(0.50),
            p95_ms: percentile(0.95),
            p99_ms: percentile(0.99),
        }
    }

    // Warm-up + measured sequential keep-alive GETs (1 KiB) per profile.
    //
    // The tower 1 KiB case uses `Reject` (bodyless GET): with a `Stream`
    // policy the adapter currently emits `connection: close` when the service
    // never polls the (empty) body, which prohibits keep-alive reuse. That
    // behavior is recorded as a 295 candidate (see results.json); the Reject
    // profile isolates pure adapter conversion cost under keep-alive.
    let mut samples = Vec::new();
    let request = b"GET / HTTP/1.1\r\nHost: baseline\r\nConnection: keep-alive\r\n\r\n";
    // Interleaved rounds alternate profile order so cold-start/order bias
    // cannot masquerade as adapter cost: even rounds run native then tower,
    // odd rounds run tower then native.
    for round in 0..3u32 {
        let order = if round % 2 == 0 {
            ["native", "tower"]
        } else {
            ["tower", "native"]
        };
        for case in order {
            let make_native = case == "native";
            let (addr, control) = if make_native {
                start(native_one_kib!()).await
            } else {
                start(TowerToEggserve::with_policy(
                    TowerOneKib,
                    RequestBodyPolicy::Reject,
                ))
                .await
            };
            keep_alive_latencies(addr, request, 50, "warmup").await; // warm-up
            let latencies = keep_alive_latencies(addr, request, 500, case).await;
            let mut sample = summarize(case, latencies);
            sample.case = match (make_native, round) {
                (true, 0) => "get-1kib-native-r0",
                (true, 1) => "get-1kib-native-r1",
                (true, _) => "get-1kib-native-r2",
                (false, 0) => "get-1kib-tower-r0",
                (false, 1) => "get-1kib-tower-r1",
                (false, _) => "get-1kib-tower-r2",
            };
            samples.push(sample);
            control.shutdown();
        }
    }
    let (axum_addr, axum_control) = start(TowerToEggserve::with_policy(
        axum_router(),
        RequestBodyPolicy::Reject,
    ))
    .await;
    keep_alive_latencies(axum_addr, request, 50, "axum-warmup").await;
    let mut axum_sample = summarize(
        "axum",
        keep_alive_latencies(axum_addr, request, 500, "get-1kib-axum").await,
    );
    axum_sample.case = "get-1kib-axum";
    samples.push(axum_sample);
    axum_control.shutdown();

    // Header-count sweep on the tower path (sequential keep-alive).
    let echo_router: Router = Router::new().route(
        "/",
        get(|| async { AxumResponse::new(AxumBody::from("ok")) }),
    );
    let (hdr_addr, hdr_control) = start(TowerToEggserve::with_policy(
        echo_router,
        RequestBodyPolicy::Reject,
    ))
    .await;
    for count in [1usize, 16, 64] {
        let mut raw = "GET / HTTP/1.1\r\nHost: baseline\r\nConnection: keep-alive\r\n".to_string();
        for i in 0..count {
            raw.push_str(&format!("x-h-{i}: v{i}\r\n"));
        }
        raw.push_str("\r\n");
        let raw = raw.into_bytes();
        keep_alive_latencies(hdr_addr, &raw, 20, "hdr-warmup").await;
        let mut sample = summarize(
            "tower-headers",
            keep_alive_latencies(hdr_addr, &raw, 200, "headers").await,
        );
        sample.case = match count {
            1 => "get-headers-1-tower",
            16 => "get-headers-16-tower",
            _ => "get-headers-64-tower",
        };
        samples.push(sample);
    }
    hdr_control.shutdown();

    // Streaming cases: fresh connection per exchange (`Connection: close`,
    // matching the SSE/token-stream deployment shape where streams are
    // long-lived and connection churn dominates differently than keep-alive).
    async fn close_exchange_ms(addr: std::net::SocketAddr, raw_request: &[u8]) -> f64 {
        let began = Instant::now();
        let mut socket = TcpStream::connect(addr).await.unwrap();
        socket.write_all(raw_request).await.unwrap();
        let mut wire = Vec::new();
        tokio::time::timeout(Duration::from_secs(30), socket.read_to_end(&mut wire))
            .await
            .unwrap()
            .unwrap();
        assert!(!wire.is_empty(), "empty response");
        began.elapsed().as_secs_f64() * 1000.0
    }

    // 1 MiB known-length stream: native exact bytes vs Tower Full (which the
    // adapter currently demotes to unknown-length chunked — finding F1).
    #[derive(Clone)]
    struct Tower1Mib {
        body: Bytes,
    }
    impl tower_service::Service<http::Request<eggserve_server::interop::HttpRequestBody>>
        for Tower1Mib
    {
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
            let body = http_body_util::Full::new(self.body.clone());
            std::future::ready(Ok(http::Response::builder()
                .status(200)
                .body(body)
                .unwrap()))
        }
    }
    let sse_request = b"GET /sse HTTP/1.1\r\nHost: baseline\r\nConnection: close\r\n\r\n".to_vec();
    let big_request = b"GET /big HTTP/1.1\r\nHost: baseline\r\nConnection: close\r\n\r\n".to_vec();

    // SSE-like 32-chunk unknown-length streams (native vs Axum-through-Tower).
    async fn axum_sse_32() -> AxumResponse {
        let items: Vec<Result<Bytes, Infallible>> = (0..32)
            .map(|i| Ok(Bytes::from(format!("data:{i}\n"))))
            .collect();
        AxumResponse::new(AxumBody::from_stream(futures_util::stream::iter(items)))
    }
    fn native_sse_32() -> impl eggserve_server::Service {
        service_fn(|_req: eggserve_server::Request| async move {
            let items: Vec<Result<Bytes, eggserve_primitives::ResponseStreamError>> = (0..32)
                .map(|i| Ok(Bytes::from(format!("data:{i}\n"))))
                .collect();
            let stream =
                eggserve_primitives::ResponseStream::new(futures_util::stream::iter(items));
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Stream(stream))
                .unwrap())
        })
    }
    let sse_router: Router = Router::new().route("/sse", get(axum_sse_32));
    let (sse_native_addr, sse_native_control) = start(native_sse_32()).await;
    let (sse_tower_addr, sse_tower_control) = start(TowerToEggserve::with_policy(
        sse_router,
        RequestBodyPolicy::Reject,
    ))
    .await;
    for i in 0..5 {
        close_exchange_ms(sse_native_addr, &sse_request).await;
        close_exchange_ms(sse_tower_addr, &sse_request).await;
        let _ = i;
    }
    let mut sse_native = Vec::new();
    let mut sse_tower = Vec::new();
    for _ in 0..50 {
        sse_native.push(close_exchange_ms(sse_native_addr, &sse_request).await);
        sse_tower.push(close_exchange_ms(sse_tower_addr, &sse_request).await);
    }
    samples.push(summarize("sse-32-native", sse_native));
    samples.push(summarize("sse-32-tower", sse_tower));
    sse_native_control.shutdown();
    sse_tower_control.shutdown();

    // 1 MiB responses (native exact vs Tower Full-through-streaming).
    async fn axum_big() -> AxumResponse {
        AxumResponse::new(AxumBody::from(vec![b'm'; 1024 * 1024]))
    }
    fn native_big() -> impl eggserve_server::Service {
        service_fn(|_req: eggserve_server::Request| async move {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(vec![b'm'; 1024 * 1024]))
                .unwrap())
        })
    }
    let big_router: Router = Router::new().route("/big", get(axum_big));
    let (big_native_addr, big_native_control) = start(native_big()).await;
    let (big_tower_addr, big_tower_control) = start(TowerToEggserve::with_policy(
        big_router,
        RequestBodyPolicy::Reject,
    ))
    .await;
    // Tower Full-body variant (no Axum routing overhead).
    let (big_full_addr, big_full_control) = start(TowerToEggserve::with_policy(
        Tower1Mib {
            body: Bytes::from(vec![b'm'; 1024 * 1024]),
        },
        RequestBodyPolicy::Reject,
    ))
    .await;
    for _ in 0..3 {
        close_exchange_ms(big_native_addr, &big_request).await;
        close_exchange_ms(big_tower_addr, &big_request).await;
        close_exchange_ms(big_full_addr, &big_request).await;
    }
    let mut big_native = Vec::new();
    let mut big_tower = Vec::new();
    let mut big_full = Vec::new();
    for _ in 0..20 {
        big_native.push(close_exchange_ms(big_native_addr, &big_request).await);
        big_tower.push(close_exchange_ms(big_tower_addr, &big_request).await);
        big_full.push(close_exchange_ms(big_full_addr, &big_request).await);
    }
    samples.push(summarize("stream-1mib-native", big_native));
    samples.push(summarize("stream-1mib-axum", big_tower));
    samples.push(summarize("stream-1mib-tower-full", big_full));
    big_native_control.shutdown();
    big_tower_control.shutdown();
    big_full_control.shutdown();

    for sample in &samples {
        println!(
            "BASELINE294 {{\"case\":\"{}\",\"profile\":\"{}\",\"requests\":{},\"elapsed_s\":{:.3},\"rps\":{:.1},\"p50_ms\":{:.3},\"p95_ms\":{:.3},\"p99_ms\":{:.3}}}",
            sample.case,
            sample.profile,
            sample.requests,
            sample.elapsed_s,
            sample.rps,
            sample.p50_ms,
            sample.p95_ms,
            sample.p99_ms
        );
    }
    assert!(samples.iter().all(|s| s.requests > 0));
}
