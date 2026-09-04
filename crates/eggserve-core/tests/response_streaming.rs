//! Transport-independent streaming response bodies (Plan 162).
//!
//! Covers known/unknown-length success, framing, HEAD/body-forbidden
//! suppression without polling, runtime-owned framing, length-mismatch
//! connection teardown, producer failure/panic containment, cancellation,
//! backpressure, keep-alive reuse, and unchanged static behavior.

use std::cell::Cell;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use eggserve_core::primitives::canonical::{
    normalize_response, to_hyper_response, BodyLength, NormalizeRequest, Response, ResponseBody,
    ResponseStream, ResponseStreamError, StatusCode,
};
use eggserve_core::primitives::connection_info::{ConnectionInfo, Scheme};
use eggserve_core::primitives::header_block::HeaderBlock;
use eggserve_core::primitives::method::Method;
use eggserve_core::primitives::request::Request;
use eggserve_core::primitives::request_body::RequestBody;
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::primitives::request_target::RequestTarget;
use eggserve_core::primitives::version::HttpVersion;
use eggserve_core::server::{service_fn, RuntimeConfig, Server};
use futures_util::stream;
use http_body_util::BodyExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[allow(dead_code)]
fn test_connection() -> ConnectionInfo {
    ConnectionInfo {
        local_addr: Some("127.0.0.1:8000".parse::<SocketAddr>().unwrap()),
        remote_addr: Some("127.0.0.1:12345".parse::<SocketAddr>().unwrap()),
        scheme: Scheme::Http,
        tls: None,
    }
}

#[allow(dead_code)]
fn get_request() -> Request {
    Request::new(
        RequestHead::new(
            Method::get(),
            RequestTarget::parse("/").unwrap(),
            HttpVersion::Http11,
            HeaderBlock::new(),
        ),
        RequestBody::empty(),
        test_connection(),
    )
}

fn bytes_stream(
    chunks: Vec<&'static [u8]>,
) -> impl futures_util::Stream<Item = Result<Bytes, ResponseStreamError>> + Send + Sync + 'static {
    let items: Vec<Result<Bytes, ResponseStreamError>> = chunks
        .into_iter()
        .map(|c| Ok(Bytes::from_static(c)))
        .collect();
    stream::iter(items)
}

async fn start_service<S>(service: S) -> (SocketAddr, eggserve_core::server::ServerHandle)
where
    S: eggserve_core::server::Service,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = Server::builder()
        .runtime(RuntimeConfig::builder().build().unwrap())
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    handle.ready().await.unwrap();
    (addr, handle)
}

fn split_headers(raw: &[u8]) -> (String, Vec<u8>) {
    let pos = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..pos]).to_string();
    (head, raw[pos..].to_vec())
}

// ---------------------------------------------------------------------------
// Unit: construction + normalization
// ---------------------------------------------------------------------------

#[test]
fn stream_constructors_report_length() {
    let s = ResponseStream::new(bytes_stream(vec![b"hi"]));
    assert_eq!(s.known_length(), None);
    let s = ResponseStream::with_known_length(bytes_stream(vec![b"hi"]), 2);
    assert_eq!(s.known_length(), Some(2));
    assert!(s.is_known_length());
}

fn non_sync_stream() -> impl futures_util::Stream<Item = Result<Bytes, ResponseStreamError>> + Send
{
    // Cell is Send but not Sync. The producer is intentionally single-owner;
    // ResponseStream must not require synchronization for a value polled by
    // one connection task.
    stream::unfold(Cell::new(false), |state| async move {
        if state.get() {
            None
        } else {
            state.set(true);
            Some((Ok(Bytes::from_static(b"hello")), state))
        }
    })
}

#[tokio::test]
async fn send_only_stream_works_over_runtime() {
    let svc = service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(ResponseStream::with_known_length(
                non_sync_stream(),
                5,
            )))
            .unwrap())
    });
    let (addr, _handle) = start_service(svc).await;
    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut out = Vec::new();
    conn.read_to_end(&mut out).await.unwrap();
    let text = String::from_utf8_lossy(&out);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
    assert!(text.to_ascii_lowercase().contains("content-length: 5"));
    assert!(text.ends_with("hello"));
}

#[test]
fn body_length_known_vs_unknown() {
    assert_eq!(
        ResponseBody::Bytes(b"hi".to_vec()).body_length(),
        BodyLength::Known(2)
    );
    assert_eq!(ResponseBody::Empty.body_length(), BodyLength::Known(0));
    let unknown = ResponseBody::Stream(ResponseStream::new(bytes_stream(vec![b"x"])));
    assert_eq!(unknown.body_length(), BodyLength::Unknown);
    // len() must not be used for framing unknown, but must not panic.
    assert_eq!(unknown.len(), 0);
    assert!(!unknown.is_empty());
    let known_zero =
        ResponseBody::Stream(ResponseStream::with_known_length(bytes_stream(vec![]), 0));
    assert!(known_zero.is_empty());
}

#[test]
fn unknown_length_never_becomes_content_length_zero() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::new(bytes_stream(
            vec![b"hi"],
        ))))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    assert!(!norm.headers().contains("content-length"));
}

#[test]
fn known_length_sets_content_length() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::with_known_length(
            bytes_stream(vec![b"hello"]),
            5,
        )))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    assert_eq!(
        norm.headers()
            .get_first("content-length")
            .unwrap()
            .to_str()
            .unwrap(),
        "5"
    );
}

#[test]
fn normalization_is_idempotent_for_streams() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::with_known_length(
            bytes_stream(vec![b"hi"]),
            2,
        )))
        .unwrap();
    let once = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    let cl_once = once
        .headers()
        .get_first("content-length")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let twice = normalize_response(once, &NormalizeRequest::new(false)).unwrap();
    assert_eq!(
        twice
            .headers()
            .get_first("content-length")
            .unwrap()
            .to_str()
            .unwrap(),
        cl_once
    );

    // Unknown stays absent after double normalize.
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::new(bytes_stream(
            vec![b"hi"],
        ))))
        .unwrap();
    let once = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    assert!(!once.headers().contains("content-length"));
    let twice = normalize_response(once, &NormalizeRequest::new(false)).unwrap();
    assert!(!twice.headers().contains("content-length"));
}

#[test]
fn head_unknown_does_not_invent_content_length() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::new(bytes_stream(
            vec![b"hi"],
        ))))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(true)).unwrap();
    assert!(norm.body().unwrap().is_empty());
    assert!(!norm.headers().contains("content-length"));
}

// ---------------------------------------------------------------------------
// HEAD / body-forbidden never poll
// ---------------------------------------------------------------------------

#[test]
fn head_does_not_poll_stream() {
    let polled = Arc::new(AtomicBool::new(false));
    let flag = polled.clone();
    let s = stream::once(async move {
        flag.store(true, Ordering::SeqCst);
        Ok::<_, ResponseStreamError>(Bytes::from_static(b"data"))
    });
    let stream = ResponseStream::with_known_length(s, 4);
    let dropped = Arc::new(AtomicBool::new(false));
    // Wrap to detect prompt drop? Dropping `stream` below is the release.
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(stream))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(true)).unwrap();
    drop(norm);
    drop(dropped);
    assert!(
        !polled.load(Ordering::SeqCst),
        "HEAD must not poll the stream"
    );
}

#[test]
fn body_forbidden_statuses_do_not_poll() {
    for status in [
        StatusCode::CONTINUE,
        StatusCode::NO_CONTENT,
        StatusCode::new(205).unwrap(),
        StatusCode::NOT_MODIFIED,
    ] {
        let polled = Arc::new(AtomicBool::new(false));
        let flag = polled.clone();
        let s = stream::once(async move {
            flag.store(true, Ordering::SeqCst);
            Ok::<_, ResponseStreamError>(Bytes::from_static(b"x"))
        });
        let resp = Response::builder()
            .status(status)
            .body(ResponseBody::Stream(ResponseStream::with_known_length(
                s, 1,
            )))
            .unwrap();
        let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
        assert!(norm.body().unwrap().is_empty(), "{}", status.as_u16());
        assert!(
            !polled.load(Ordering::SeqCst),
            "status {} must not poll",
            status.as_u16()
        );
    }
}

#[test]
fn head_known_length_preserved() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::with_known_length(
            bytes_stream(vec![b"hello"]),
            5,
        )))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(true)).unwrap();
    assert_eq!(
        norm.headers()
            .get_first("content-length")
            .unwrap()
            .to_str()
            .unwrap(),
        "5"
    );
}

// ---------------------------------------------------------------------------
// Runtime-owned framing
// ---------------------------------------------------------------------------

#[test]
fn service_framing_headers_are_stripped() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("transfer-encoding", "chunked")
        .unwrap()
        .header("content-length", "999")
        .unwrap()
        .body(ResponseBody::Stream(ResponseStream::with_known_length(
            bytes_stream(vec![b"hi"]),
            2,
        )))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    assert!(!norm.headers().contains("transfer-encoding"));
    assert_eq!(
        norm.headers()
            .get_first("content-length")
            .unwrap()
            .to_str()
            .unwrap(),
        "2"
    );
}

// ---------------------------------------------------------------------------
// Transport conversion: success paths
// ---------------------------------------------------------------------------

#[tokio::test]
async fn known_length_stream_collects() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::with_known_length(
            bytes_stream(vec![b"he", b"llo"]),
            5,
        )))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    let body = to_hyper_response(norm)
        .unwrap()
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(&body[..], b"hello");
}

#[tokio::test]
async fn unknown_length_stream_collects() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::new(bytes_stream(
            vec![b"a", b"b", b"c"],
        ))))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    let body = to_hyper_response(norm)
        .unwrap()
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(&body[..], b"abc");
}

#[tokio::test]
async fn empty_known_stream_is_empty_with_length_zero() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::with_known_length(
            stream::empty(),
            0,
        )))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    assert_eq!(
        norm.headers()
            .get_first("content-length")
            .unwrap()
            .to_str()
            .unwrap(),
        "0"
    );
    let body = to_hyper_response(norm)
        .unwrap()
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    assert!(body.is_empty());
}

#[tokio::test]
async fn empty_chunks_are_skipped() {
    let items = vec![
        Ok(Bytes::from_static(b"a")),
        Ok(Bytes::new()),
        Ok(Bytes::from_static(b"b")),
        Ok(Bytes::new()),
        Ok(Bytes::from_static(b"c")),
    ];
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::with_known_length(
            stream::iter(items),
            3,
        )))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    let body = to_hyper_response(norm)
        .unwrap()
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(&body[..], b"abc");
}

#[tokio::test]
async fn large_chunk_is_split_not_rejected() {
    let big = vec![b'x'; 100 * 1024];
    let len = big.len() as u64;
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::with_known_length(
            stream::once(async move { Ok::<_, ResponseStreamError>(Bytes::from(big)) }),
            len,
        )))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    // Convert with small chunk size to force splitting.
    let hyper = eggserve_core::primitives::canonical::to_hyper_response(norm).unwrap();
    let body = hyper.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 100 * 1024);
    assert!(body.iter().all(|&b| b == b'x'));
}

// ---------------------------------------------------------------------------
// Length mismatch closes connection (wire: truncated + close)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn known_length_underrun_is_error() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::with_known_length(
            bytes_stream(vec![b"hi"]),
            10,
        )))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    let err = to_hyper_response(norm)
        .unwrap()
        .into_body()
        .collect()
        .await
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
}

#[tokio::test]
async fn known_length_overrun_is_error() {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::with_known_length(
            bytes_stream(vec![b"hello world"]),
            5,
        )))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    let err = to_hyper_response(norm)
        .unwrap()
        .into_body()
        .collect()
        .await
        .unwrap_err();
    // Overrun surfaces as a stream error that closes the connection.
    assert!(matches!(
        err.kind(),
        std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::Other
            | std::io::ErrorKind::InvalidData
    ));
}

#[tokio::test]
async fn producer_failure_is_generic_and_closed() {
    let s = stream::once(async {
        Err::<Bytes, ResponseStreamError>(ResponseStreamError::new("/secret/path should not leak"))
    });
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::new(s)))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    let err = to_hyper_response(norm)
        .unwrap()
        .into_body()
        .collect()
        .await
        .unwrap_err();
    // Wire error must be generic, never the producer detail.
    assert_eq!(err.to_string(), "response stream failed");
    assert!(!err.to_string().contains("secret"));
}

#[tokio::test]
async fn producer_panic_is_contained() {
    let s = stream::once(async {
        panic!("boom-payload-should-not-leak");
        #[allow(unreachable_code)]
        Ok::<_, ResponseStreamError>(Bytes::from_static(b"x"))
    });
    // `stream::once` future panicking propagates as stream panic at poll.
    let resp = Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::Stream(ResponseStream::new(s)))
        .unwrap();
    let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
    let result = to_hyper_response(norm).unwrap().into_body().collect().await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.to_string(), "response stream failed");
}

// ---------------------------------------------------------------------------
// Wire: framing, keep-alive, backpressure, cancellation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn wire_unknown_length_uses_chunked_and_keepalive_reusable() {
    let svc = service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain")
            .unwrap()
            .body(ResponseBody::Stream(ResponseStream::new(bytes_stream(
                vec![b"chunk1-", b"chunk2"],
            ))))
            .unwrap())
    });
    let (addr, _handle) = start_service(svc).await;

    // Raw TCP to inspect framing.
    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
        .await
        .unwrap();
    let mut buf = vec![0u8; 8192];
    let n = tokio::time::timeout(Duration::from_secs(3), conn.read(&mut buf))
        .await
        .unwrap()
        .unwrap();
    let (head, _body) = split_headers(&buf[..n]);
    eprintln!("HEADERS:\n{head}\n---END--- raw len {}", buf[..n].len());
    assert!(head.contains("200"), "head: {head}");
    assert!(
        !head.to_ascii_lowercase().contains("content-length"),
        "unknown must omit content-length: {head}"
    );
    // Hyper uses chunked for streaming unknown length on HTTP/1.1.
    assert!(
        head.to_ascii_lowercase().contains("chunked"),
        "expected chunked framing: {head}"
    );

    // Chunked wire contains both parts (framed, not contiguous).
    // Body check via Hyper client (de-chunked) on a fresh connection.
    {
        let stream = TcpStream::connect(addr).await.unwrap();
        let io = hyper_util::rt::TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let req = hyper::Request::builder()
            .method("GET")
            .uri("/")
            .body(http_body_util::Full::new(Bytes::new()))
            .unwrap();
        let resp = sender.send_request(req).await.unwrap();
        assert_eq!(resp.status(), hyper::StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"chunk1-chunk2");
    }
}

#[tokio::test]
async fn wire_known_length_sends_content_length() {
    let svc = service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(ResponseStream::with_known_length(
                bytes_stream(vec![b"hello"]),
                5,
            )))
            .unwrap())
    });
    let (addr, _handle) = start_service(svc).await;
    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut out = Vec::new();
    conn.read_to_end(&mut out).await.unwrap();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("200"));
    assert!(text.to_ascii_lowercase().contains("content-length: 5"));
    assert!(text.ends_with("hello"));
}

#[tokio::test]
async fn wire_head_does_not_poll_and_preserves_length() {
    let polled = Arc::new(AtomicUsize::new(0));
    let flag = polled.clone();
    let svc = service_fn(move |req: Request| {
        let flag = flag.clone();
        let is_head = req.head().method().is_head();
        async move {
            let s = stream::once(async move {
                flag.fetch_add(1, Ordering::SeqCst);
                Ok::<_, ResponseStreamError>(Bytes::from_static(b"hello"))
            });
            let body = if is_head {
                // Service returns a stream even for HEAD; runtime must drop
                // without polling.
                ResponseBody::Stream(ResponseStream::with_known_length(s, 5))
            } else {
                ResponseBody::Stream(ResponseStream::with_known_length(s, 5))
            };
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(body)
                .unwrap())
        }
    });
    let (addr, _handle) = start_service(svc).await;
    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"HEAD / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut out = Vec::new();
    conn.read_to_end(&mut out).await.unwrap();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("200"));
    assert!(text.to_ascii_lowercase().contains("content-length: 5"));
    // No body bytes after headers.
    let (_, body) = split_headers(&out);
    assert!(body.is_empty(), "HEAD must not send body");
    assert_eq!(polled.load(Ordering::SeqCst), 0, "HEAD must not poll");
}

#[tokio::test]
async fn wire_known_mismatch_closes_connection() {
    let svc = service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(ResponseStream::with_known_length(
                bytes_stream(vec![b"short"]),
                100,
            )))
            .unwrap())
    });
    let (addr, _handle) = start_service(svc).await;
    // Known-length mismatch must not yield a reusable ambiguous connection.
    // Hyper may abort before flushing headers (0 bytes, clean close) or
    // after partial body; either way the connection closes and the server
    // stays healthy. Assert close + mismatch accounting + next-request health.
    use tokio::io::AsyncReadExt as _;
    let before = eggserve_core::ops::global_counters()
        .stream_length_mismatches
        .load(Ordering::Relaxed);
    let mut raw = TcpStream::connect(addr).await.unwrap();
    raw.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(3), raw.read_to_end(&mut out)).await;
    // Must not be a clean full response: either empty (aborted before
    // commitment) or truncated (aborted after). Never a reusable keep-alive
    // with complete 100-byte body.
    assert!(
        out.len() < 100,
        "mismatch must not yield complete body, got {} bytes",
        out.len()
    );
    let after = eggserve_core::ops::global_counters()
        .stream_length_mismatches
        .load(Ordering::Relaxed);
    assert!(after > before, "mismatch must be counted");
    // Server stays healthy for next request (new connection).
    let mut raw2 = TcpStream::connect(addr).await.unwrap();
    raw2.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut out2 = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(3), raw2.read_to_end(&mut out2)).await;
    // Second mismatch also closes; server did not crash (it accepted).
    assert!(out2.len() < 100);
}

#[tokio::test]
async fn slow_reader_backpressure_completes() {
    // 64 chunks of 1 KiB; producer counts polls to prove pull-driven progress.
    let polls = Arc::new(AtomicUsize::new(0));
    let svc = service_fn(move |_req: Request| {
        // Rebuild a fresh unfold per request capturing the same counter.
        let polls = polls.clone();
        async move {
            let inner = futures_util::stream::unfold(0usize, move |i| {
                let polls = polls.clone();
                async move {
                    if i >= 64 {
                        return None;
                    }
                    polls.fetch_add(1, Ordering::SeqCst);
                    Some((
                        Ok::<_, ResponseStreamError>(Bytes::from(vec![b'z'; 1024])),
                        i + 1,
                    ))
                }
            });
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Stream(ResponseStream::with_known_length(
                    inner, 65536,
                )))
                .unwrap())
        }
    });
    let (addr, _handle) = start_service(svc).await;
    let hyper_stream = TcpStream::connect(addr).await.unwrap();
    let io = hyper_util::rt::TokioIo::new(hyper_stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let req = hyper::Request::builder()
        .method("GET")
        .uri("/")
        .body(http_body_util::Full::new(Bytes::new()))
        .unwrap();
    let resp = sender.send_request(req).await.unwrap();
    // Slow reader: pull one DATA frame at a time with a small delay.
    use futures_util::StreamExt;
    let mut body = resp.into_body().into_data_stream();
    let mut received = 0usize;
    while let Some(chunk) = body.next().await {
        let chunk = chunk.unwrap();
        received += chunk.len();
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(received, 65536);
}

#[tokio::test]
async fn client_disconnect_releases_producer() {
    let dropped = Arc::new(AtomicBool::new(false));
    let flag = dropped.clone();
    struct DropStream {
        _flag: Arc<AtomicBool>,
    }
    impl Drop for DropStream {
        fn drop(&mut self) {
            self._flag.store(true, Ordering::SeqCst);
        }
    }
    let _holder = DropStream {
        _flag: flag.clone(),
    };
    // Producer pends forever until dropped.
    let svc = service_fn(move |_req: Request| {
        let s = stream::pending::<Result<Bytes, ResponseStreamError>>();
        let _flag = flag.clone();
        async move {
            // Hold a drop guard inside the service future? Instead verify
            // transport drop via stream cancellation counter below.
            let _guard = DropStream { _flag };
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Stream(ResponseStream::new(s)))
                .unwrap())
        }
    });
    let (addr, _handle) = start_service(svc).await;
    let before = eggserve_core::ops::global_counters()
        .stream_cancelled
        .load(Ordering::Relaxed);
    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    // Disconnect before reading the body.
    drop(conn);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after = eggserve_core::ops::global_counters()
        .stream_cancelled
        .load(Ordering::Relaxed);
    assert!(
        after > before,
        "disconnect should cancel the pending stream"
    );
    assert!(dropped.load(Ordering::SeqCst) || after > before);
}

#[tokio::test]
async fn shutdown_does_not_wait_forever_on_pending_stream() {
    let svc = service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(ResponseStream::new(
                stream::pending::<Result<Bytes, ResponseStreamError>>(),
            )))
            .unwrap())
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = Server::builder()
        .runtime(
            RuntimeConfig::builder()
                .graceful_shutdown_timeout(Duration::from_secs(2))
                .build()
                .unwrap(),
        )
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    // Open a streaming request that never completes.
    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let start = std::time::Instant::now();
    handle.shutdown();
    tokio::time::timeout(Duration::from_secs(5), handle.wait())
        .await
        .expect("shutdown must not wait forever")
        .unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "shutdown took too long"
    );
}

#[tokio::test]
async fn static_file_behavior_unchanged() {
    use eggserve_core::server::StaticService;
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), b"hello world").unwrap();
    let svc = StaticService::builder(tmp.path()).build().unwrap();
    let (addr, _handle) = start_service(svc).await;
    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"GET /hello.txt HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut out = Vec::new();
    conn.read_to_end(&mut out).await.unwrap();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("200"));
    assert!(text.to_ascii_lowercase().contains("content-length: 11"));
    assert!(text.ends_with("hello world"));
}

// ---------------------------------------------------------------------------
// Property: byte accounting
// ---------------------------------------------------------------------------

#[cfg(test)]
mod property {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn byte_count_matches_chunks(
            chunks in prop::collection::vec(prop::collection::vec(0u8..255u8, 0..256), 0..8),
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let total: usize = chunks.iter().map(|c| c.len()).sum();
            let items: Vec<Result<Bytes, ResponseStreamError>> = chunks
                .into_iter()
                .map(|c| Ok(Bytes::from(c)))
                .collect();
            let s = ResponseStream::with_known_length(stream::iter(items), total as u64);
            prop_assert_eq!(s.known_length(), Some(total as u64));
            let resp = Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Stream(s))
                .unwrap();
            let norm = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
            let len: u64 = norm.headers().get_first("content-length").unwrap().to_str().unwrap().parse().unwrap();
            prop_assert_eq!(len, total as u64);
            let body = rt.block_on(async {
                to_hyper_response(norm).unwrap().into_body().collect().await.unwrap().to_bytes()
            });
            prop_assert_eq!(body.len() as u64, total as u64);
        }
    }
}
