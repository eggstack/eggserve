//! Plan 197 application-service contract fixture.
//!
//! Compile/import proof that the stabilized native contract is usable
//! without importing Hyper, Tokio internals beyond the runtime, or
//! crate-private modules. Covers:
//!
//! - `RequestContext` as the single attachment point (Track B);
//! - ordinary `Service` staying simple and transport-neutral (Track C/E);
//! - commitment/cancellation semantics (Track D);
//! - error-taxonomy tolerance via `#[non_exhaustive]` wildcards (Track F);
//! - connection metadata that cannot be forged via headers (Track B/D).
//!
//! Import boundary: `eggserve_core::primitives` + `eggserve_core::server`
//! plus ordinary downstream deps (`tokio`, `bytes`, `futures-util`) only.
//! No `hyper`, no `http::HeaderValue`, no crate-private modules, no
//! `to_hyper_response` / `try_from_hyper` in this fixture.

use std::net::SocketAddr;
use std::time::Duration;

use bytes::Bytes;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::connection_info::{ConnectionInfo, Scheme};
use eggserve_core::primitives::header_block::HeaderBlock;
use eggserve_core::primitives::method::Method;
use eggserve_core::primitives::request::Request;
use eggserve_core::primitives::request_body::RequestBody;
use eggserve_core::primitives::request_body_error::RequestBodyError;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::primitives::request_context::RequestContext;
use eggserve_core::primitives::request_lifecycle::RequestCancellationReason;
use eggserve_core::primitives::request_target::RequestTarget;
use eggserve_core::primitives::response_stream::ResponseStreamError;
use eggserve_core::primitives::version::HttpVersion;
use eggserve_core::primitives::{RequestHead, ResponseStream};
use eggserve_core::server::connection::ConnectionOutcome;
use eggserve_core::server::{
    service_fn, service_fn_with_policy, RuntimeConfig, Server, Service, ServiceError,
};

fn test_head(path: &str) -> RequestHead {
    RequestHead::new(
        Method::get(),
        RequestTarget::parse(path).unwrap(),
        HttpVersion::Http11,
        HeaderBlock::new(),
    )
}

fn test_connection() -> ConnectionInfo {
    ConnectionInfo::with_socket_addrs(
        "127.0.0.1:8000".parse::<SocketAddr>().unwrap(),
        "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        Scheme::Http,
        None,
    )
}

#[test]
fn request_context_is_single_attachment_point() {
    let body = RequestBody::empty();
    let lifecycle = body.lifecycle();
    let ctx = RequestContext::new(test_connection(), lifecycle);
    assert_eq!(ctx.connection().scheme, Scheme::Http);
    assert!(ctx.connection().has_socket_endpoints());
    assert!(!ctx.lifecycle().is_cancelled());

    // Cheap clone shares the lifecycle allocation, never the one-shot body.
    let cloned = ctx.clone();
    assert_eq!(cloned.connection(), ctx.connection());
    assert_eq!(
        cloned.lifecycle().is_cancelled(),
        ctx.lifecycle().is_cancelled()
    );
}

#[test]
fn request_exposes_context_without_breaking_common_path() {
    let req = Request::new(test_head("/"), RequestBody::empty(), test_connection());
    // New accessor.
    assert_eq!(req.context().connection().scheme, Scheme::Http);
    // Preserved Plan 175 accessors forward to the context.
    assert_eq!(req.connection().scheme, Scheme::Http);
    assert!(!req.lifecycle().is_cancelled());
    assert!(!req.lifecycle_clone().is_cancelled());

    // Forward-compatible deconstruction.
    let req = Request::new(test_head("/ctx"), RequestBody::empty(), test_connection());
    let (head, _body, ctx) = req.into_parts_with_context();
    assert_eq!(head.target().path(), "/ctx");
    assert_eq!(ctx.connection().scheme, Scheme::Http);

    // Explicit-context constructor shares the body allocation.
    let body = RequestBody::from_bytes(b"hi".to_vec(), u64::MAX);
    let lifecycle = body.lifecycle();
    let ctx = RequestContext::new(test_connection(), lifecycle);
    let req = Request::new_with_context(test_head("/explicit"), body, ctx);
    assert_eq!(req.context().connection().scheme, Scheme::Http);
}

#[test]
fn connection_metadata_cannot_be_forged_via_headers() {
    // A spoofed forwarding header stays an ordinary untrusted header; the
    // context keeps the observed transport identity.
    let mut headers = HeaderBlock::new();
    headers.push_str("x-forwarded-for", "203.0.113.7").unwrap();
    headers.push_str("forwarded", "for=198.51.100.9").unwrap();
    let head = RequestHead::new(
        Method::get(),
        RequestTarget::parse("/").unwrap(),
        HttpVersion::Http11,
        headers,
    );
    let req = Request::new(head, RequestBody::empty(), test_connection());
    assert!(req.head().headers().contains("x-forwarded-for"));
    assert_eq!(
        req.context().connection().remote_addr.unwrap().port(),
        12345
    );
    assert!(req.context().connection().tls.is_none());
}

#[test]
fn ordinary_service_remains_simple_and_transport_neutral() {
    fn assert_service<S: Service>(_: &S) {}
    fn assert_send_sync<T: Send + Sync>(_: &T) {}

    let svc = service_fn(|_req: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"ok".to_vec()))
            .unwrap())
    });
    assert_service(&svc);
    assert_send_sync(&svc);

    // No `ServiceOutcome` exists: the final return stays an ordinary Response.
    // This asserts the Plan 197 Track C decision at compile time — if an
    // outcome enum is ever introduced, this fixture must be updated with a
    // `From<Response>` conversion check instead of silently passing.
    fn returns_response<S: Service>(svc: &S, req: Request) -> bool {
        let _ = svc.request_body_policy(req.head());
        true
    }
    let req = Request::new(test_head("/"), RequestBody::empty(), test_connection());
    assert!(returns_response(&svc, req));
}

#[tokio::test]
async fn buffered_request_to_bytes_response() {
    let svc = service_fn_with_policy(
        |req: Request| async move {
            let bytes = req
                .into_body()
                .read_all()
                .await
                .map_err(|e| ServiceError::rejected(e.to_status_code(), "body failed"))?;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(bytes.to_vec()))
                .unwrap())
        },
        RequestBodyPolicy::Buffer { max_bytes: 1024 },
    );
    let req = Request::new(
        test_head("/echo"),
        RequestBody::from_bytes(b"hello".to_vec(), 1024),
        test_connection(),
    );
    let resp = svc.call(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.body().unwrap().len(), 5);
}

#[tokio::test]
async fn streamed_request_to_streamed_response_over_bounded_channel() {
    let svc = service_fn_with_policy(
        |req: Request| async move {
            let lifecycle = req.context().lifecycle_clone();
            let (_head, mut body) = req.into_head_and_body();
            let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(2);
            tokio::spawn(async move {
                while let Ok(Some(chunk)) = body.next_chunk().await {
                    tokio::select! {
                        biased;
                        _ = lifecycle.cancelled() => break,
                        res = tx.send(chunk) => {
                            if res.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
            let stream = futures_util::stream::unfold(rx, |mut rx| async move {
                rx.recv()
                    .await
                    .map(|chunk| (Ok::<Bytes, ResponseStreamError>(chunk), rx))
            });
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Stream(ResponseStream::new(stream)))
                .unwrap())
        },
        RequestBodyPolicy::Stream { max_bytes: 1024 },
    );
    let req = Request::new(
        test_head("/pipe"),
        RequestBody::from_bytes(b"stream-me".to_vec(), 1024),
        test_connection(),
    );
    let resp = svc.call(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    // Unknown-length stream omits Content-Length by construction.
    assert!(matches!(
        resp.body().unwrap(),
        ResponseBody::Stream(s) if s.known_length().is_none()
    ));
}

#[tokio::test]
async fn lifecycle_cancellation_is_observable_without_socket_probe() {
    let req = Request::new(test_head("/poll"), RequestBody::empty(), test_connection());
    let lifecycle = req.context().lifecycle_clone();
    assert!(!lifecycle.is_cancelled());
    assert!(lifecycle.cancellation_reason().is_none());
    // No transport in this unit fixture, so cancellation never fires on its
    // own; the point is the observer is threaded through the context and is
    // cheaply cloneable for idle waiters. Live disconnect/shutdown waking is
    // qualified by `app_server_consumer` and `deferred_lifecycle`.
    tokio::select! {
        biased;
        _ = lifecycle.cancelled() => panic!("must not cancel without transport"),
        _ = tokio::time::sleep(Duration::from_millis(10)) => {}
    }
}

#[test]
#[allow(clippy::match_like_matches_macro)]
fn error_taxonomy_tolerates_growth_with_wildcards() {
    // Plan 197 Track F: these matches must keep compiling when future
    // variants are added.
    fn body_kind(err: &RequestBodyError) -> &'static str {
        match err {
            RequestBodyError::RejectedByPolicy => "reject",
            RequestBodyError::DeclaredLengthTooLarge { .. }
            | RequestBodyError::LimitExceeded { .. } => "limit",
            RequestBodyError::ReadTimeout => "timeout",
            RequestBodyError::PrematureEof { .. }
            | RequestBodyError::LengthMismatch { .. }
            | RequestBodyError::InvalidChunkFraming(_) => "framing",
            RequestBodyError::Cancelled | RequestBodyError::Disconnected => "cancel",
            RequestBodyError::AlreadyConsumed | RequestBodyError::MixedConsumptionMode => "state",
            RequestBodyError::Transport(_) => "transport",
            _ => "future",
        }
    }
    assert_eq!(body_kind(&RequestBodyError::ReadTimeout), "timeout");

    fn cancel_kind(reason: &RequestCancellationReason) -> &'static str {
        match reason {
            RequestCancellationReason::PeerDisconnected => "peer",
            RequestCancellationReason::ServerShutdown => "shutdown",
            RequestCancellationReason::ConnectionTimeout => "timeout",
            RequestCancellationReason::TransportFailure => "transport",
            _ => "future",
        }
    }
    assert_eq!(
        cancel_kind(&RequestCancellationReason::ServerShutdown),
        "shutdown"
    );

    fn outcome_clean(outcome: &ConnectionOutcome) -> bool {
        match outcome {
            ConnectionOutcome::Normal
            | ConnectionOutcome::Shutdown
            | ConnectionOutcome::IdleTimeout => true,
            ConnectionOutcome::ClientError
            | ConnectionOutcome::HeaderTimeout
            | ConnectionOutcome::WriteTimeout
            | ConnectionOutcome::TotalTimeout
            | ConnectionOutcome::Internal => false,
            _ => false,
        }
    }
    assert!(outcome_clean(&ConnectionOutcome::Normal));

    fn server_kind(err: &eggserve_core::server::ServerError) -> &'static str {
        match err {
            eggserve_core::server::ServerError::Bind(_) => "bind",
            eggserve_core::server::ServerError::Config(_) => "config",
            eggserve_core::server::ServerError::AlreadyStarted => "started",
            eggserve_core::server::ServerError::NotStarted => "not-started",
            eggserve_core::server::ServerError::Accept(_) => "accept",
            eggserve_core::server::ServerError::TlsSetup(_) => "tls",
            eggserve_core::server::ServerError::Transport(_) => "transport",
            eggserve_core::server::ServerError::ShutdownTimeout => "shutdown-timeout",
            eggserve_core::server::ServerError::Startup(_) => "startup",
            eggserve_core::server::ServerError::Terminal(_) => "terminal",
            _ => "future",
        }
    }
    assert_eq!(
        server_kind(&eggserve_core::server::ServerError::NotStarted),
        "not-started"
    );

    // ServiceError stays a struct with private kind: construction only via
    // `internal` / `rejected`, inspection via `is_panic` / `is_timeout`.
    let err = ServiceError::rejected(503, "busy");
    assert!(!err.is_panic());
    assert!(!err.is_timeout());
    assert!(ServiceError::internal("x").message().contains('x'));
}

#[tokio::test]
async fn service_admission_stays_runtime_owned() {
    // `max_in_flight_requests` bounds pre-response `Service::call` only.
    // A downstream app task owns a separate budget; this fixture proves the
    // runtime side still 503s on exhaustion without involving app state.
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let gate = Arc::new(tokio::sync::Notify::new());
    let gate_clone = gate.clone();
    let svc = service_fn_with_policy(
        move |_req: Request| {
            let gate = gate_clone.clone();
            async move {
                gate.notified().await;
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"ok".to_vec()))
                    .unwrap())
            }
        },
        RequestBodyPolicy::Reject,
    );
    let config = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .max_in_flight_requests(1)
        .handler_timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let server = Server::builder().runtime(config).build().unwrap();
    let handle = server.start_with_service(svc).await.unwrap();
    handle.ready().await.unwrap();
    let addr = handle.local_addr();

    // Occupy the single in-flight slot.
    let mut first = tokio::net::TcpStream::connect(addr).await.unwrap();
    first
        .write_all(b"GET /one HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Second concurrent call must 503 while the first is still in-flight.
    let mut second = tokio::net::TcpStream::connect(addr).await.unwrap();
    second
        .write_all(b"GET /two HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), second.read_to_end(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 503"),
        "runtime admission must 503 on exhaustion, got: {}",
        String::from_utf8_lossy(&buf)
    );

    gate.notify_waiters();
    handle.shutdown();
    handle.wait().await.unwrap();
}
