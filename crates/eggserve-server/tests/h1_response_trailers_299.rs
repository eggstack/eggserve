//! Plan 299 — H1 response-trailer wire correctness (raw TCP evidence).
//!
//! Hyper's H1 encoder only serializes terminal trailer frames when the
//! initial response head declares them via `Trailer`. This suite proves the
//! runtime-owned declaration repair: opted-in H1.1 carries trailers on the
//! wire, all other paths suppress without polling, and actual fields cannot
//! escape the declared set.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use eggserve_primitives::{Response, ResponseBody, StatusCode, TrailerDeclaration, Trailers};
use eggserve_server::{service_fn, RuntimeConfig, Server};
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

async fn raw_request(addr: std::net::SocketAddr, request: &str) -> Vec<u8> {
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut wire = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), socket.read_to_end(&mut wire))
        .await
        .unwrap()
        .unwrap();
    wire
}

async fn get(
    addr: std::net::SocketAddr,
    version: &str,
    te: bool,
    extra: &str,
    method: &str,
) -> Vec<u8> {
    let te_line = if te { "TE: trailers\r\n" } else { "" };
    let request = format!(
        "{method} / HTTP/{version}\r\nHost: t\r\n{te_line}Connection: close\r\n{extra}\r\n"
    );
    raw_request(addr, &request).await
}

fn head_lower(wire: &[u8]) -> String {
    let split = wire
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("head terminator");
    String::from_utf8_lossy(&wire[..split]).to_ascii_lowercase()
}

fn has_wire_trailer(wire: &[u8], name: &[u8], value: &[u8]) -> bool {
    // Terminal section follows the zero chunk (`0\r\n`); look for the
    // field there rather than anywhere in the head.
    let zero = b"0\r\n";
    let Some(pos) = wire.windows(3).position(|w| w == zero) else {
        return false;
    };
    let tail = &wire[pos..];
    let needle: Vec<u8> = [name, b": ", value].concat();
    tail.windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(&needle))
}

fn declared_service(
    declaration_names: Vec<String>,
    trailer_pairs: Vec<(String, String)>,
    body: &'static [u8],
    known_length: Option<u64>,
    polled: Arc<AtomicBool>,
) -> impl eggserve_server::Service {
    service_fn(move |_req: eggserve_server::Request| {
        let declaration_names = declaration_names.clone();
        let trailer_pairs = trailer_pairs.clone();
        let polled = polled.clone();
        async move {
            let decl =
                TrailerDeclaration::from_names(declaration_names).expect("valid declaration");
            let items = vec![Ok::<_, eggserve_primitives::ResponseStreamError>(
                Bytes::from_static(body),
            )];
            let fut_polled = polled.clone();
            let trailer_future = async move {
                fut_polled.store(true, Ordering::SeqCst);
                let mut block = eggserve_primitives::HeaderBlock::new();
                for (n, v) in trailer_pairs {
                    block.push_str(n, v).unwrap();
                }
                let trailers = Trailers::new(block).unwrap();
                Ok(Some(trailers))
            };
            let stream = match known_length {
                Some(len) => {
                    eggserve_primitives::ResponseStream::with_known_length_and_declared_trailers(
                        futures_util::stream::iter(items),
                        len,
                        decl,
                        trailer_future,
                    )
                }
                None => eggserve_primitives::ResponseStream::with_declared_trailers(
                    futures_util::stream::iter(items),
                    decl,
                    trailer_future,
                ),
            };
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Stream(stream))
                .unwrap())
        }
    })
}

#[tokio::test]
async fn h1_11_opted_in_unknown_length_carries_wire_trailer() {
    let polled = Arc::new(AtomicBool::new(false));
    let svc = declared_service(
        vec!["x-end".to_owned()],
        vec![("x-end".to_owned(), "yes".to_owned())],
        b"trailer-body",
        None,
        polled.clone(),
    );
    let (addr, control) = start(svc).await;
    let wire = get(addr, "1.1", true, "", "GET").await;
    let head = head_lower(&wire);
    assert!(head.contains("200 ok"), "{head}");
    assert!(
        head.contains("trailer:") && head.contains("x-end"),
        "head must declare trailers: {head}"
    );
    assert!(
        !head.contains("content-length"),
        "trailer-bearing H1 must not send Content-Length: {head}"
    );
    assert!(
        head.contains("transfer-encoding: chunked"),
        "trailer-bearing H1 must be chunked: {head}"
    );
    assert!(
        has_wire_trailer(&wire, b"x-end", b"yes"),
        "terminal trailer missing on wire"
    );
    assert!(polled.load(Ordering::SeqCst), "producer must be polled");
    control.shutdown();
}

#[tokio::test]
async fn h1_11_opted_in_known_length_omits_cl_and_carries_trailer() {
    let polled = Arc::new(AtomicBool::new(false));
    let svc = declared_service(
        vec!["x-end".to_owned()],
        vec![("x-end".to_owned(), "yes".to_owned())],
        b"trailer-body",
        Some(12),
        polled.clone(),
    );
    let (addr, control) = start(svc).await;
    let wire = get(addr, "1.1", true, "", "GET").await;
    let head = head_lower(&wire);
    assert!(head.contains("200 ok"), "{head}");
    assert!(
        !head.contains("content-length"),
        "known-length trailer response must omit wire Content-Length: {head}"
    );
    assert!(
        head.contains("trailer:") && head.contains("x-end"),
        "head must declare trailers: {head}"
    );
    assert!(
        has_wire_trailer(&wire, b"x-end", b"yes"),
        "terminal trailer missing on wire"
    );
    control.shutdown();
}

#[tokio::test]
async fn h1_11_without_te_suppresses_without_polling() {
    let polled = Arc::new(AtomicBool::new(false));
    let svc = declared_service(
        vec!["x-end".to_owned()],
        vec![("x-end".to_owned(), "yes".to_owned())],
        b"trailer-body",
        None,
        polled.clone(),
    );
    let (addr, control) = start(svc).await;
    let wire = get(addr, "1.1", false, "", "GET").await;
    let head = head_lower(&wire);
    assert!(head.contains("200 ok"), "{head}");
    assert!(
        !head.contains("trailer:"),
        "suppressed response must not advertise Trailer: {head}"
    );
    assert!(
        !has_wire_trailer(&wire, b"x-end", b"yes"),
        "suppressed response must not carry wire trailers"
    );
    assert!(
        !polled.load(Ordering::SeqCst),
        "suppressed trailer producer must not be polled"
    );
    control.shutdown();
}

#[tokio::test]
async fn h1_10_never_carries_trailers() {
    let polled = Arc::new(AtomicBool::new(false));
    let svc = declared_service(
        vec!["x-end".to_owned()],
        vec![("x-end".to_owned(), "yes".to_owned())],
        b"trailer-body",
        None,
        polled.clone(),
    );
    let (addr, control) = start(svc).await;
    let wire = get(addr, "1.0", true, "", "GET").await;
    let head = head_lower(&wire);
    assert!(
        !head.contains("trailer:"),
        "H1.0 must not advertise: {head}"
    );
    assert!(
        !has_wire_trailer(&wire, b"x-end", b"yes"),
        "H1.0 must not carry wire trailers"
    );
    assert!(
        !polled.load(Ordering::SeqCst),
        "H1.0 trailer producer must not be polled"
    );
    control.shutdown();
}

#[tokio::test]
async fn undeclared_h1_trailer_source_is_suppressed_not_lost() {
    let polled = Arc::new(AtomicBool::new(false));
    let polled_svc = polled.clone();
    let svc = service_fn(move |_req: eggserve_server::Request| {
        let polled = polled_svc.clone();
        async move {
            let items = vec![Ok::<_, eggserve_primitives::ResponseStreamError>(
                Bytes::from_static(b"trailer-body"),
            )];
            let mut block = eggserve_primitives::HeaderBlock::new();
            block.push_str("x-end", "yes").unwrap();
            let trailers = Trailers::new(block).unwrap();
            let trailer_future = async move {
                polled.store(true, Ordering::SeqCst);
                Ok(Some(trailers))
            };
            // Legacy constructor: no head-time declaration.
            let stream = eggserve_primitives::ResponseStream::with_trailers(
                futures_util::stream::iter(items),
                trailer_future,
            );
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Stream(stream))
                .unwrap())
        }
    });
    let (addr, control) = start(svc).await;
    let wire = get(addr, "1.1", true, "", "GET").await;
    let head = head_lower(&wire);
    assert!(head.contains("200 ok"), "{head}");
    assert!(
        !head.contains("trailer:"),
        "undeclared source must not advertise: {head}"
    );
    assert!(
        !has_wire_trailer(&wire, b"x-end", b"yes"),
        "undeclared source must not emit wire trailers"
    );
    assert!(
        !polled.load(Ordering::SeqCst),
        "undeclared producer must be suppressed before polling"
    );
    control.shutdown();
}

#[tokio::test]
async fn head_and_body_forbidden_do_not_advertise_or_poll() {
    for (status, body_status) in [
        (StatusCode::NO_CONTENT, "204"),
        (StatusCode::NOT_MODIFIED, "304"),
    ] {
        let polled = Arc::new(AtomicBool::new(false));
        let polled_svc = polled.clone();
        let svc = service_fn(move |_req: eggserve_server::Request| {
            let polled = polled_svc.clone();
            async move {
                let decl = TrailerDeclaration::from_names(vec!["x-end"]).unwrap();
                let items = vec![Ok::<_, eggserve_primitives::ResponseStreamError>(
                    Bytes::from_static(b"data"),
                )];
                let trailer_future = async move {
                    polled.store(true, Ordering::SeqCst);
                    let mut block = eggserve_primitives::HeaderBlock::new();
                    block.push_str("x-end", "yes").unwrap();
                    Ok(Some(Trailers::new(block).unwrap()))
                };
                let stream = eggserve_primitives::ResponseStream::with_declared_trailers(
                    futures_util::stream::iter(items),
                    decl,
                    trailer_future,
                );
                Ok(Response::builder()
                    .status(status)
                    .body(ResponseBody::Stream(stream))
                    .unwrap())
            }
        });
        let (addr, control) = start(svc).await;
        let wire = get(addr, "1.1", true, "", "GET").await;
        let head = head_lower(&wire);
        assert!(head.contains(body_status), "{head}");
        assert!(
            !head.contains("trailer:"),
            "{body_status} must not advertise Trailer: {head}"
        );
        assert!(
            !polled.load(Ordering::SeqCst),
            "{body_status} must not poll trailer producer"
        );
        control.shutdown();
    }

    // HEAD request with declared trailers on 200.
    let polled = Arc::new(AtomicBool::new(false));
    let svc = declared_service(
        vec!["x-end".to_owned()],
        vec![("x-end".to_owned(), "yes".to_owned())],
        b"trailer-body",
        None,
        polled.clone(),
    );
    let (addr, control) = start(svc).await;
    let wire = get(addr, "1.1", true, "", "HEAD").await;
    let head = head_lower(&wire);
    assert!(head.contains("200 ok"), "{head}");
    assert!(
        !head.contains("trailer:"),
        "HEAD must not advertise Trailer: {head}"
    );
    assert!(
        !polled.load(Ordering::SeqCst),
        "HEAD must not poll trailer producer"
    );
    control.shutdown();
}

#[tokio::test]
async fn undeclared_actual_field_fails_closed_without_leak() {
    let svc = service_fn(|_req: eggserve_server::Request| async {
        let decl = TrailerDeclaration::from_names(vec!["x-end"]).unwrap();
        let items = vec![Ok::<_, eggserve_primitives::ResponseStreamError>(
            Bytes::from_static(b"data"),
        )];
        let trailer_future = async move {
            let mut block = eggserve_primitives::HeaderBlock::new();
            block.push_str("x-evil", "secret").unwrap();
            Ok(Some(Trailers::new(block).unwrap()))
        };
        let stream = eggserve_primitives::ResponseStream::with_declared_trailers(
            futures_util::stream::iter(items),
            decl,
            trailer_future,
        );
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(stream))
            .unwrap())
    });
    let (addr, control) = start(svc).await;
    let wire = get(addr, "1.1", true, "", "GET").await;
    let text = String::from_utf8_lossy(&wire).to_ascii_lowercase();
    assert!(
        !text.contains("x-evil"),
        "undeclared trailer value must never reach the wire"
    );
    assert!(
        !has_wire_trailer(&wire, b"x-evil", b"secret"),
        "undeclared trailer must fail closed"
    );
    control.shutdown();
}

#[test]
fn declaration_validation_rejects_forbidden_and_malformed() {
    assert!(TrailerDeclaration::from_names(Vec::<String>::new()).is_err());
    assert!(TrailerDeclaration::from_names(vec!["content-length"]).is_err());
    assert!(TrailerDeclaration::from_names(vec!["transfer-encoding"]).is_err());
    assert!(TrailerDeclaration::from_names(vec!["trailer"]).is_err());
    assert!(TrailerDeclaration::from_names(vec![""]).is_err());
    assert!(TrailerDeclaration::from_names(vec!["not a name!"]).is_err());
    assert!(TrailerDeclaration::parse_header_value("").is_err());
    assert!(TrailerDeclaration::parse_header_value("content-length").is_err());
    // Duplicates canonicalize deterministically.
    let decl = TrailerDeclaration::from_names(vec!["x-a", "X-A", "x-b"]).expect("dedup");
    assert_eq!(decl.len(), 2);
    assert!(decl.contains("x-a"));
    assert!(decl.contains("X-A"));
    assert_eq!(decl.header_value(), "x-a, x-b");
}

#[tokio::test]
async fn duplicate_trailer_values_preserve_order() {
    let svc = service_fn(|_req: eggserve_server::Request| async {
        let decl = TrailerDeclaration::from_names(vec!["x-dup"]).unwrap();
        let items = vec![Ok::<_, eggserve_primitives::ResponseStreamError>(
            Bytes::from_static(b"data"),
        )];
        let trailer_future = async move {
            let mut block = eggserve_primitives::HeaderBlock::new();
            block.push_str("x-dup", "a").unwrap();
            block.push_str("x-dup", "b").unwrap();
            Ok(Some(Trailers::new(block).unwrap()))
        };
        let stream = eggserve_primitives::ResponseStream::with_declared_trailers(
            futures_util::stream::iter(items),
            decl,
            trailer_future,
        );
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(stream))
            .unwrap())
    });
    let (addr, control) = start(svc).await;
    let wire = get(addr, "1.1", true, "", "GET").await;
    assert!(has_wire_trailer(&wire, b"x-dup", b"a"));
    assert!(has_wire_trailer(&wire, b"x-dup", b"b"));
    control.shutdown();
}

#[cfg(feature = "tower")]
#[tokio::test]
async fn tower_declared_trailers_converge_with_native() {
    use eggserve_primitives::request_body_policy::RequestBodyPolicy;
    use eggserve_server::TowerToEggserve;

    #[derive(Clone)]
    struct TowerDeclared;
    impl tower_service::Service<http::Request<eggserve_server::interop::HttpRequestBody>>
        for TowerDeclared
    {
        type Response = http::Response<
            http_body_util::StreamBody<
                futures_util::stream::Iter<
                    std::vec::IntoIter<
                        Result<http_body::Frame<bytes::Bytes>, std::convert::Infallible>,
                    >,
                >,
            >,
        >;
        type Error = std::convert::Infallible;
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
                Ok(http_body::Frame::data(Bytes::from_static(b"trailer-body"))),
                Ok(http_body::Frame::trailers(map)),
            ];
            let body = http_body_util::StreamBody::new(futures_util::stream::iter(frames));
            std::future::ready(Ok(http::Response::builder()
                .status(200)
                .header("trailer", "x-end")
                .body(body)
                .unwrap()))
        }
    }

    let polled = Arc::new(AtomicBool::new(false));
    let native = declared_service(
        vec!["x-end".to_owned()],
        vec![("x-end".to_owned(), "yes".to_owned())],
        b"trailer-body",
        None,
        polled,
    );
    let (native_addr, native_control) = start(native).await;
    let (tower_addr, tower_control) = start(TowerToEggserve::with_policy(
        TowerDeclared,
        RequestBodyPolicy::Reject,
    ))
    .await;
    let native_wire = get(native_addr, "1.1", true, "", "GET").await;
    let tower_wire = get(tower_addr, "1.1", true, "", "GET").await;
    for (name, wire) in [("native", &native_wire), ("tower", &tower_wire)] {
        let head = head_lower(wire);
        assert!(head.contains("trailer:"), "{name}: {head}");
        assert!(!head.contains("content-length"), "{name}: {head}");
        assert!(
            has_wire_trailer(wire, b"x-end", b"yes"),
            "{name} must carry wire trailer"
        );
    }
    native_control.shutdown();
    tower_control.shutdown();
}

#[cfg(feature = "tower")]
#[tokio::test]
async fn tower_without_declaration_is_suppressed() {
    use eggserve_primitives::request_body_policy::RequestBodyPolicy;
    use eggserve_server::TowerToEggserve;

    #[derive(Clone)]
    struct TowerUndeclared;
    impl tower_service::Service<http::Request<eggserve_server::interop::HttpRequestBody>>
        for TowerUndeclared
    {
        type Response = http::Response<
            http_body_util::StreamBody<
                futures_util::stream::Iter<
                    std::vec::IntoIter<
                        Result<http_body::Frame<bytes::Bytes>, std::convert::Infallible>,
                    >,
                >,
            >,
        >;
        type Error = std::convert::Infallible;
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
                Ok(http_body::Frame::data(Bytes::from_static(b"trailer-body"))),
                Ok(http_body::Frame::trailers(map)),
            ];
            let body = http_body_util::StreamBody::new(futures_util::stream::iter(frames));
            // No `Trailer` declaration header: H1 must suppress, not lose.
            std::future::ready(Ok(http::Response::builder()
                .status(200)
                .body(body)
                .unwrap()))
        }
    }

    let (addr, control) = start(TowerToEggserve::with_policy(
        TowerUndeclared,
        RequestBodyPolicy::Reject,
    ))
    .await;
    let wire = get(addr, "1.1", true, "", "GET").await;
    let head = head_lower(&wire);
    assert!(head.contains("200 ok"), "{head}");
    assert!(
        !head.contains("trailer:"),
        "undeclared Tower source must not advertise: {head}"
    );
    assert!(
        !has_wire_trailer(&wire, b"x-end", b"yes"),
        "undeclared Tower source must not emit wire trailers"
    );
    control.shutdown();
}
