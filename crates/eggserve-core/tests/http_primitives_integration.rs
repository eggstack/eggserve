use std::fs;

use eggserve_core::policy::StaticPolicy;
use eggserve_core::server::{Server, StaticService};
use http_body_util::BodyExt;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use tempfile::TempDir;
use tokio::net::TcpListener;

async fn start_server(
    tmp: &TempDir,
    policy: StaticPolicy,
) -> (SocketAddr, eggserve_core::server::ServerHandle) {
    let svc = StaticService::builder(tmp.path())
        .policy(policy)
        .build()
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = Server::builder()
        .runtime(
            eggserve_core::server::RuntimeConfig::builder()
                .build()
                .unwrap(),
        )
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server.start_with_service(svc).await.unwrap();

    (addr, handle)
}

async fn send_request(addr: SocketAddr, req: Request<Full<Bytes>>) -> Response<Incoming> {
    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    sender.send_request(req).await.unwrap()
}

fn get_req(path: &str) -> Request<Full<Bytes>> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

fn head_req(path: &str) -> Request<Full<Bytes>> {
    Request::builder()
        .method(Method::HEAD)
        .uri(path)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

fn post_req(path: &str) -> Request<Full<Bytes>> {
    Request::builder()
        .method(Method::POST)
        .uri(path)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

fn req_with_header(
    method: Method,
    path: &str,
    header_name: &str,
    header_value: &str,
) -> Request<Full<Bytes>> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header_name, header_value)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

async fn body_bytes(resp: Response<Incoming>) -> Bytes {
    resp.into_body().collect().await.unwrap().to_bytes()
}

#[tokio::test]
async fn live_get_existing_file_returns_200() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    let resp = send_request(addr, get_req("/hello.txt")).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(&body[..], b"hello world");
}

#[tokio::test]
async fn live_head_existing_file_returns_200_no_body() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    let resp = send_request(addr, head_req("/hello.txt")).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body.len(), 0);
}

#[tokio::test]
async fn live_get_missing_returns_404() {
    let tmp = TempDir::new().unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    let resp = send_request(addr, get_req("/nope.txt")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn live_dotfile_returns_403() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(".env"), "secret").unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    let resp = send_request(addr, get_req("/.env")).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn live_directory_without_index_returns_403() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    // Directory without trailing slash redirects first.
    let resp = send_request(addr, get_req("/subdir")).await;
    assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(location, "/subdir/");

    // After redirect, no index → 403.
    let resp = send_request(addr, get_req("/subdir/")).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn live_directory_with_index_returns_200() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    fs::write(
        tmp.path().join("subdir").join("index.html"),
        "<html>hi</html>",
    )
    .unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    // Directory without trailing slash redirects first.
    let resp = send_request(addr, get_req("/subdir")).await;
    assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(location, "/subdir/");

    // After redirect, index found → 200.
    let resp = send_request(addr, get_req("/subdir/")).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn live_directory_redirect_encodes_location() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("my dir")).unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    // The Location value must remain a valid URI-reference: encoded
    // characters from the request target stay percent-encoded.
    let resp = send_request(addr, get_req("/my%20dir")).await;
    assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(location, "/my%20dir/");
}

#[tokio::test]
async fn live_post_returns_405_with_allow_header() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    let resp = send_request(addr, post_req("/hello.txt")).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(resp.headers().get("allow").unwrap(), "GET, HEAD");
}

#[tokio::test]
async fn live_malformed_percent_returns_400() {
    let tmp = TempDir::new().unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    let resp = send_request(addr, get_req("/%ZZ")).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn live_traversal_returns_403() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    let resp = send_request(addr, get_req("/../etc/passwd")).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn live_get_with_content_length_returns_413() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    let resp = send_request(
        addr,
        req_with_header(Method::GET, "/hello.txt", "content-length", "1024"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn live_get_with_invalid_content_length_returns_400() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    let resp = send_request(
        addr,
        req_with_header(Method::GET, "/hello.txt", "content-length", "not-a-number"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn live_range_returns_206() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    let resp = send_request(
        addr,
        req_with_header(Method::GET, "/hello.txt", "range", "bytes=0-4"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(resp.headers().get("content-range").unwrap(), "bytes 0-4/11");
    let body = body_bytes(resp).await;
    assert_eq!(&body[..], b"hello");
}

#[tokio::test]
async fn live_unsatisfiable_range_returns_416() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    let resp = send_request(
        addr,
        req_with_header(Method::GET, "/hello.txt", "range", "bytes=100-200"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
}

#[tokio::test]
async fn live_conditional_etag_returns_304() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    let etag = eggserve_core::primitives::planner::generate_etag(
        &fs::metadata(tmp.path().join("hello.txt")).unwrap(),
    )
    .unwrap();

    let resp = send_request(
        addr,
        req_with_header(Method::GET, "/hello.txt", "if-none-match", &etag),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(resp.headers().get("etag").unwrap(), &etag);
}

#[tokio::test]
async fn live_head_range_returns_206_no_body() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    let (addr, _handle) = start_server(&tmp, StaticPolicy::safe_default()).await;

    let resp = send_request(
        addr,
        req_with_header(Method::HEAD, "/hello.txt", "range", "bytes=0-2"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(resp.headers().get("content-length").unwrap(), "3");
    let body = body_bytes(resp).await;
    assert!(body.is_empty());
}
