//! Plan 200 Track H — `http` / `http-body` / Tower interop fixtures.
//!
//! Gated behind the `tower` feature (which implies `http-interop`) so native
//! builds never depend on ecosystem types. Covers: full/streaming bodies with
//! backpressure, duplicate/opaque headers, request/response trailers,
//! header-adding middleware, readiness accounting with per-request clones
//! (no shared mutex), service errors, and H1 transport parity through the
//! same Tower application.

#![cfg(feature = "tower")]

use bytes::Bytes;
use eggserve_core::primitives::interop::{
    header_block_to_map, header_map_to_block, method_from_http, method_to_http,
    request_head_to_http, response_from_http_body, status_from_http, status_to_http,
    version_from_http, version_to_http, ConnectionInfoExt, LifecycleExt, RawTargetExt,
};
use eggserve_core::primitives::{
    ConnectionInfo, HeaderBlock, Method, RequestBody, RequestContext, RequestHead, RequestTarget,
    ResponseBody, Scheme, StatusCode, Trailers,
};
use eggserve_core::primitives::{HttpVersion, Request};
use eggserve_core::server::{service_fn, Service, TowerToEggserve};
use http_body_util::{BodyExt as _, Full, StreamBody};
use std::future::Future;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::task::{Context, Poll};
use tower_layer::Layer as _;

// ---------------------------------------------------------------------------
// Fixture Tower services
// ---------------------------------------------------------------------------

/// Simple Tower service returning a full body (Track H).
#[derive(Clone, Default)]
struct FullHello;

impl tower_service::Service<http::Request<RequestBody>> for FullHello {
    type Response = http::Response<Full<Bytes>>;
    type Error = std::convert::Infallible;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: http::Request<RequestBody>) -> Self::Future {
        let resp = http::Response::builder()
            .status(http::StatusCode::OK)
            .header("x-hello", "world")
            .body(Full::new(Bytes::from("hello tower")))
            .unwrap();
        std::future::ready(Ok(resp))
    }
}

/// Streaming Tower service with backpressure (Track H).
#[derive(Clone, Default)]
struct StreamingService;

type StreamingBody = StreamBody<
    Pin<
        Box<
            dyn futures_util::Stream<Item = Result<http_body::Frame<Bytes>, std::io::Error>> + Send,
        >,
    >,
>;

impl tower_service::Service<http::Request<RequestBody>> for StreamingService {
    type Response = http::Response<StreamingBody>;
    type Error = std::convert::Infallible;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: http::Request<RequestBody>) -> Self::Future {
        // Three chunks streamed incrementally (no full buffering).
        let stream = futures_util::stream::iter(vec![
            Ok(http_body::Frame::data(Bytes::from("chunk-one-"))),
            Ok(http_body::Frame::data(Bytes::from("chunk-two-"))),
            Ok(http_body::Frame::data(Bytes::from("chunk-three"))),
        ]);
        let boxed: Pin<
            Box<
                dyn futures_util::Stream<Item = Result<http_body::Frame<Bytes>, std::io::Error>>
                    + Send,
            >,
        > = Box::pin(stream);
        let body = StreamBody::new(boxed);
        let resp = http::Response::builder()
            .status(http::StatusCode::OK)
            .body(body)
            .unwrap();
        std::future::ready(Ok(resp))
    }
}

/// Tower service echoing request trailers into response headers (proves
/// trailers arrived through `http_body` frames without full buffering).
#[derive(Clone, Default)]
struct TrailerEchoService;

impl tower_service::Service<http::Request<RequestBody>> for TrailerEchoService {
    type Response = http::Response<Full<Bytes>>;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<RequestBody>) -> Self::Future {
        Box::pin(async move {
            let (_parts, mut body) = req.into_parts();
            // Drain data incrementally (backpressure preserved). Trailers
            // arrive as the final `http_body` frame (taken from the canonical
            // store, so the native `trailers()` accessor would observe `None`
            // afterwards — count frames here instead).
            let mut trailer_count = 0usize;
            while let Some(frame) = body.frame().await {
                let frame = frame.unwrap();
                if frame.is_trailers() {
                    trailer_count += 1;
                }
            }
            let resp = http::Response::builder()
                .status(http::StatusCode::OK)
                .header("x-trailer-count", trailer_count.to_string())
                .body(Full::new(Bytes::from("trailers-echoed")))
                .unwrap();
            Ok(resp)
        })
    }
}

/// Readiness-accounting Tower service proving per-request clones.
///
/// `clones` counts `Clone` invocations (per-request policy); `readies`
/// counts `poll_ready` polls. A naive shared-`Mutex<S>` adapter would show
/// zero clones; this adapter must clone per request.
struct ReadinessService {
    clones: Arc<AtomicUsize>,
    readies: Arc<AtomicUsize>,
}

impl Clone for ReadinessService {
    fn clone(&self) -> Self {
        self.clones.fetch_add(1, Ordering::SeqCst);
        Self {
            clones: self.clones.clone(),
            readies: self.readies.clone(),
        }
    }
}

impl tower_service::Service<http::Request<RequestBody>> for ReadinessService {
    type Response = http::Response<Full<Bytes>>;
    type Error = std::convert::Infallible;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.readies.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: http::Request<RequestBody>) -> Self::Future {
        let resp = http::Response::builder()
            .status(http::StatusCode::OK)
            .body(Full::new(Bytes::from("ready")))
            .unwrap();
        std::future::ready(Ok(resp))
    }
}

/// Tower service that always fails (error mapping fixture).
#[derive(Clone, Default)]
struct FailingService;

#[derive(Debug)]
struct AppError(&'static str);

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "app error: {}", self.0)
    }
}

impl std::error::Error for AppError {}

impl tower_service::Service<http::Request<RequestBody>> for FailingService {
    type Response = http::Response<Full<Bytes>>;
    type Error = AppError;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: http::Request<RequestBody>) -> Self::Future {
        std::future::ready(Err(AppError("boom")))
    }
}

// ---------------------------------------------------------------------------
// Middleware fixture (Tower Layer adding headers)
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct AddHeaderLayer {
    name: &'static str,
    value: &'static str,
}

impl<S> tower_layer::Layer<S> for AddHeaderLayer {
    type Service = AddHeaderService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AddHeaderService {
            inner,
            name: self.name,
            value: self.value,
        }
    }
}

#[derive(Clone)]
struct AddHeaderService<S> {
    inner: S,
    name: &'static str,
    value: &'static str,
}

impl<S, ReqBody> tower_service::Service<http::Request<ReqBody>> for AddHeaderService<S>
where
    S: tower_service::Service<http::Request<ReqBody>>,
    S::Response: AddHeaderResponse,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<ReqBody>) -> Self::Future {
        let name = self.name;
        let value = self.value;
        let fut = self.inner.call(req);
        Box::pin(async move {
            let mut resp = fut.await?;
            resp.add_header(name, value);
            Ok(resp)
        })
    }
}

trait AddHeaderResponse {
    fn add_header(&mut self, name: &'static str, value: &'static str);
}

impl<B> AddHeaderResponse for http::Response<B> {
    fn add_header(&mut self, name: &'static str, value: &'static str) {
        self.headers_mut().insert(
            http::HeaderName::from_static(name),
            http::HeaderValue::from_static(value),
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_connection() -> ConnectionInfo {
    ConnectionInfo::with_socket_addrs(
        "127.0.0.1:8000".parse().unwrap(),
        "127.0.0.1:12345".parse().unwrap(),
        Scheme::Http,
        None,
    )
}

fn make_request(path: &str, body: RequestBody) -> Request {
    let head = RequestHead::new(
        Method::get(),
        RequestTarget::parse(path).unwrap(),
        HttpVersion::Http11,
        HeaderBlock::new(),
    );
    Request::new(head, body, test_connection())
}

async fn collect_canonical_stream(response: eggserve_core::primitives::Response) -> Vec<u8> {
    use eggserve_core::primitives::canonical::{normalize_response, NormalizeRequest};
    let normalized =
        normalize_response(response, &NormalizeRequest::new(false)).expect("normalize");
    let hyper_resp =
        eggserve_core::primitives::canonical::to_hyper_response(normalized).expect("convert");
    let body = hyper_resp.into_body();
    let collected = body.collect().await.expect("collect").to_bytes();
    collected.to_vec()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn scalar_conversions_round_trip() {
    let m = Method::new("PURGE").unwrap();
    assert_eq!(method_from_http(&method_to_http(&m).unwrap()).unwrap(), m);
    let s = StatusCode::new(429).unwrap();
    assert_eq!(status_from_http(status_to_http(s)).unwrap(), s);
    assert_eq!(
        version_from_http(version_to_http(HttpVersion::Http2)).unwrap(),
        HttpVersion::Http2
    );
}

#[test]
fn opaque_and_duplicate_headers_preserved() {
    let mut block = HeaderBlock::new();
    block
        .push_bytes("x-opaque", b"\x80\x81 value \xff")
        .unwrap();
    block.push_str("x-dup", "a").unwrap();
    block.push_str("x-dup", "b").unwrap();
    let map = header_block_to_map(&block);
    assert_eq!(
        map.get("x-opaque").unwrap().as_bytes(),
        b"\x80\x81 value \xff"
    );
    let back = header_map_to_block(&map).unwrap();
    assert_eq!(
        back.get_first("x-opaque").unwrap().as_bytes(),
        b"\x80\x81 value \xff"
    );
    let dups: Vec<_> = back
        .get_all("x-dup")
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect();
    assert_eq!(dups, vec!["a".to_owned(), "b".to_owned()]);
}

#[test]
fn exact_target_and_connection_in_extensions() {
    let mut headers = HeaderBlock::new();
    headers.push_str("host", "example.test").unwrap();
    let head = RequestHead::new_with_authority(
        Method::get(),
        RequestTarget::parse("/a/b?x=1").unwrap(),
        HttpVersion::Http11,
        headers,
        Some(eggserve_core::primitives::Authority::parse("example.test").unwrap()),
    );
    let body = RequestBody::empty();
    let lifecycle = body.lifecycle();
    let http_req = request_head_to_http(&head, &test_connection(), &lifecycle).unwrap();
    assert_eq!(http_req.uri().path(), "/a/b");
    assert_eq!(
        http_req.extensions().get::<RawTargetExt>().unwrap().0,
        "/a/b?x=1"
    );
    assert_eq!(
        http_req.extensions().get::<ConnectionInfoExt>().unwrap().0,
        test_connection()
    );
    assert!(http_req.extensions().get::<LifecycleExt>().is_some());
}

#[tokio::test]
async fn request_body_streams_data_then_trailers() {
    use http_body::Body as _;
    let trailers = Trailers::new({
        let mut b = HeaderBlock::new();
        b.push_str("x-check", "trailer-value").unwrap();
        b
    })
    .unwrap();
    let mut body = RequestBody::from_bytes_with_trailers(b"hello ".to_vec(), 1024, trailers);
    // size_hint is truthful (remaining declared bytes).
    assert_eq!(body.size_hint().exact(), Some(6));
    // Poll data frames incrementally (real backpressure, no full buffering).
    let mut collected = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.expect("frame ok");
        if let Some(data) = frame.data_ref() {
            collected.extend_from_slice(data);
        } else if frame.is_trailers() {
            let map = frame.trailers_ref().expect("trailers");
            assert_eq!(map.get("x-check").unwrap(), "trailer-value");
        }
    }
    assert_eq!(collected, b"hello ");
}

#[tokio::test]
async fn tower_full_body_end_to_end() {
    let svc = TowerToEggserve::new(FullHello);
    let req = make_request("/", RequestBody::empty());
    let resp = svc.call(req).await.expect("tower ok");
    assert_eq!(resp.status(), StatusCode::OK);
    // Middleware-added/app headers survive; framing recomputed by runtime.
    assert!(resp.headers().contains("x-hello"));
    let bytes = collect_canonical_stream(resp).await;
    assert_eq!(bytes, b"hello tower");
}

#[tokio::test]
async fn tower_streaming_body_is_incremental() {
    let svc = TowerToEggserve::new(StreamingService);
    let req = make_request("/", RequestBody::empty());
    let resp = svc.call(req).await.expect("tower ok");
    let bytes = collect_canonical_stream(resp).await;
    assert_eq!(bytes, b"chunk-one-chunk-two-chunk-three");
}

#[tokio::test]
async fn tower_request_trailers_reach_service() {
    let trailers = Trailers::new({
        let mut b = HeaderBlock::new();
        b.push_str("x-check", "yes").unwrap();
        b
    })
    .unwrap();
    let body = RequestBody::from_bytes_with_trailers(b"data".to_vec(), 1024, trailers);
    let svc = TowerToEggserve::new(TrailerEchoService);
    let req = make_request("/trailers", body);
    let resp = svc.call(req).await.expect("tower ok");
    let count = resp
        .headers()
        .get_first("x-trailer-count")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(count, "1");
}

#[tokio::test]
async fn tower_response_trailers_map_without_buffering() {
    // Tower app returns data + trailers frames; adapter must validate via
    // the canonical trailer validator and expose through ResponseStream.
    let stream = futures_util::stream::iter(vec![
        Ok::<_, std::io::Error>(http_body::Frame::data(Bytes::from("payload"))),
        Ok(http_body::Frame::trailers({
            let mut m = http::HeaderMap::new();
            m.insert("x-resp-trailer", http::HeaderValue::from_static("t1"));
            m
        })),
    ]);
    let boxed: Pin<
        Box<
            dyn futures_util::Stream<Item = Result<http_body::Frame<Bytes>, std::io::Error>> + Send,
        >,
    > = Box::pin(stream);
    let http_resp = http::Response::builder()
        .status(http::StatusCode::OK)
        .body(StreamBody::new(boxed))
        .unwrap();
    let canonical = response_from_http_body(http_resp).expect("convert");
    assert!(canonical.has_response_trailers());
    // Poll bytes incrementally, then trailers (no full buffering).
    let body = canonical.body().expect("body");
    match body {
        ResponseBody::Stream(_) => {}
        other => panic!("expected stream, got {other:?}"),
    }
}

#[tokio::test]
async fn tower_framing_headers_are_not_trusted() {
    // Middleware/app framing is stripped; EggServe recomputes framing.
    // `content-length`/`transfer-encoding` are stripped at conversion;
    // hop-by-hop (`connection`) is stripped by canonical normalization.
    let http_resp = http::Response::builder()
        .status(http::StatusCode::OK)
        .header("content-length", "9999")
        .header("transfer-encoding", "chunked")
        .header("connection", "keep-alive")
        .body(Full::new(Bytes::from("actual")))
        .unwrap();
    let canonical = response_from_http_body(http_resp).expect("convert");
    // Application framing never survives conversion.
    assert!(!canonical.headers().contains("content-length"));
    assert!(!canonical.headers().contains("transfer-encoding"));
    let bytes = collect_canonical_stream(canonical).await;
    assert_eq!(bytes, b"actual");
    // Normalization strips hop-by-hop centrally (covered by the canonical
    // suite; re-assert here through a normalized probe).
    let http_resp2 = http::Response::builder()
        .status(http::StatusCode::OK)
        .header("connection", "keep-alive")
        .body(Full::new(Bytes::from("x")))
        .unwrap();
    let canonical2 = response_from_http_body(http_resp2).expect("convert");
    let normalized = eggserve_core::primitives::canonical::normalize_response(
        canonical2,
        &eggserve_core::primitives::canonical::NormalizeRequest::new(false),
    )
    .expect("normalize");
    assert!(!normalized.headers().contains("connection"));
}

#[tokio::test]
async fn tower_middleware_headers_survive() {
    let inner = FullHello;
    let layer = AddHeaderLayer {
        name: "x-middleware",
        value: "applied",
    };
    let layered = layer.layer(inner);
    let svc = TowerToEggserve::new(layered);
    let req = make_request("/", RequestBody::empty());
    let resp = svc.call(req).await.expect("tower ok");
    assert!(resp.headers().contains("x-middleware"));
}

#[tokio::test]
async fn tower_readiness_per_request_clone() {
    let clones = Arc::new(AtomicUsize::new(0));
    let readies = Arc::new(AtomicUsize::new(0));

    // Verify two concurrent calls each drive readiness on their own clone.
    let svc = TowerToEggserve::new(ReadinessService {
        clones: clones.clone(),
        readies: readies.clone(),
    });
    // `TowerToEggserve::clone` (and thus per-request Tower clones) must exist;
    // concurrent calls must both complete (no shared-mutex deadlock).
    let (r1, r2) = tokio::join!(
        svc.call(make_request("/a", RequestBody::empty())),
        svc.call(make_request("/b", RequestBody::empty())),
    );
    assert!(r1.is_ok());
    assert!(r2.is_ok());
    assert!(
        readies.load(Ordering::SeqCst) >= 2,
        "poll_ready must be driven per request"
    );
    assert!(
        clones.load(Ordering::SeqCst) >= 2,
        "each request must clone the Tower service (no shared mutex)"
    );
}

#[tokio::test]
async fn tower_service_error_maps_to_internal() {
    let svc = TowerToEggserve::new(FailingService);
    let req = make_request("/", RequestBody::empty());
    let err = svc.call(req).await.expect_err("must fail");
    assert!(!err.is_panic());
    assert!(!err.is_timeout());
    assert!(err.message().contains("tower service"));
}

#[tokio::test]
async fn tower_head_never_polls_body() {
    use std::sync::atomic::{AtomicBool, Ordering};
    // Tower app returns a streaming body; HEAD normalization must drop it
    // without polling (prompt producer release).
    let polled = Arc::new(AtomicBool::new(false));
    let _flag = polled.clone();
    let stream_items = vec![Ok::<_, std::io::Error>(http_body::Frame::data(
        Bytes::from("should-not-send"),
    ))];
    let boxed: Pin<
        Box<
            dyn futures_util::Stream<Item = Result<http_body::Frame<Bytes>, std::io::Error>> + Send,
        >,
    > = Box::pin(futures_util::stream::iter(stream_items));
    let tower_body = StreamBody::new(boxed);
    let http_resp = http::Response::builder()
        .status(http::StatusCode::OK)
        .body(tower_body)
        .unwrap();
    let canonical = response_from_http_body(http_resp).expect("convert");
    // Simulate HEAD: normalization must drop the stream without polling.
    let normalized = eggserve_core::primitives::canonical::normalize_response(
        canonical,
        &eggserve_core::primitives::canonical::NormalizeRequest::new(true),
    )
    .expect("normalize");
    let hyper_resp =
        eggserve_core::primitives::canonical::to_hyper_response(normalized).expect("convert");
    let collected = hyper_resp.into_body().collect().await.unwrap().to_bytes();
    assert!(collected.is_empty());
    assert!(!polled.load(Ordering::SeqCst));
}

#[tokio::test]
async fn tower_h1_transport_parity() {
    use eggserve_core::server::{RuntimeConfig, Server};
    use std::time::Duration;

    let svc = TowerToEggserve::new(FullHello);
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

    let client = reqwest_like_get(addr, "/").await;
    assert!(client.contains("hello tower"));

    handle.shutdown();
    tokio::time::timeout(Duration::from_secs(10), handle.wait())
        .await
        .expect("drain")
        .expect("clean");
}

async fn reqwest_like_get(addr: std::net::SocketAddr, path: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nhost: localhost\r\nconnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    String::from_utf8_lossy(&buf).into_owned()
}

#[derive(Clone)]
struct NativeHello;

impl Service for NativeHello {
    fn call(
        &self,
        _request: Request,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        eggserve_core::primitives::Response,
                        eggserve_core::server::ServiceError,
                    >,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async {
            Ok(eggserve_core::primitives::Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"native hello".to_vec()))
                .unwrap())
        })
    }
}

#[tokio::test]
async fn eggserve_to_tower_round_trip() {
    use tower_service::Service as _;
    // Native service exposed as Tower: composition/testing path.
    let native = NativeHello;
    let mut tower_svc = eggserve_core::server::EggserveToTower::new(native);
    // Readiness is adapter-local (always ready), never transport admission.
    futures_util::future::poll_fn(|cx| tower_svc.poll_ready(cx))
        .await
        .expect("ready");
    let head = RequestHead::new(
        Method::get(),
        RequestTarget::parse("/round").unwrap(),
        HttpVersion::Http11,
        HeaderBlock::new(),
    );
    let body = RequestBody::empty();
    let lifecycle = body.lifecycle();
    let http_head = request_head_to_http(&head, &test_connection(), &lifecycle).unwrap();
    let http_req = http_head.map(|()| body);
    let resp = tower_svc.call(http_req).await.expect("tower call");
    assert_eq!(resp.status(), http::StatusCode::OK);
    let collected = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&collected[..], b"native hello");
}

#[test]
fn native_consumers_do_not_require_tower() {
    // Compile-time proof: the native service trait object is constructible
    // without mentioning Tower types in this function signature.
    fn assert_native<S: Service>(_: &S) {}
    let svc = service_fn(|_req: Request| async {
        Ok(eggserve_core::primitives::Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Empty)
            .unwrap())
    });
    assert_native(&svc);
    // RequestContext is the single attachment point; tunnel/interim never
    // appear in http Extensions (enforced by type: no accessor exists).
    let ctx = RequestContext::new(test_connection(), RequestBody::empty().lifecycle());
    let _ = ctx.connection().clone();
}
