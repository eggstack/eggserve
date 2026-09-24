#![cfg(feature = "tower")]

use axum::body::Body;
use axum::extract::State;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use bytes::Bytes;
use eggserve_core::primitives::interop::HttpRequestBody;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::server::TowerToEggserve;
use eggserve_server::{RuntimeConfig, Server};
use futures_util::StreamExt;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

struct StreamGate {
    first_polled: Mutex<Option<oneshot::Sender<()>>>,
    release_second: Mutex<Option<oneshot::Receiver<()>>>,
    upload_first: Mutex<Option<oneshot::Sender<()>>>,
    cancel_drop: Mutex<Option<oneshot::Sender<()>>>,
}

struct DropNotice(Option<oneshot::Sender<()>>);

impl Drop for DropNotice {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

async fn middleware_marker(request: http::Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .append("x-middleware", "ran".parse().unwrap());
    response
}

async fn streamed(State(gate): State<Arc<StreamGate>>) -> Response {
    let first = gate.first_polled.lock().unwrap().take();
    let release = gate.release_second.lock().unwrap().take().unwrap();
    let stream = futures_util::stream::unfold(
        (0u8, release, first),
        move |(state, mut release, first_signal)| async move {
            match state {
                0 => {
                    if let Some(signal) = first_signal {
                        let _ = signal.send(());
                    }
                    Some((
                        Ok::<Bytes, Infallible>(Bytes::from_static(b"first")),
                        (1, release, None),
                    ))
                }
                1 => {
                    (&mut release).await.ok()?;
                    Some((Ok(Bytes::from_static(b"second")), (2, release, None)))
                }
                _ => None,
            }
        },
    );
    let mut response = Response::new(Body::from_stream(stream));
    response
        .headers_mut()
        .append("x-repeat", "one".parse().unwrap());
    response
        .headers_mut()
        .append("x-repeat", "two".parse().unwrap());
    response
}

async fn upload(State(gate): State<Arc<StreamGate>>, body: Body) -> Response {
    let mut stream = body.into_data_stream();
    let first = stream.next().await.unwrap().unwrap();
    if let Some(signal) = gate.upload_first.lock().unwrap().take() {
        let _ = signal.send(());
    }
    let mut bytes = first.to_vec();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk.unwrap());
    }
    Response::new(Body::from(bytes))
}

async fn cancellation_stream(State(gate): State<Arc<StreamGate>>) -> Response {
    let notice = gate.cancel_drop.lock().unwrap().take().unwrap();
    let stream = futures_util::stream::unfold(
        (0u64, DropNotice(Some(notice))),
        |(count, notice)| async move {
            Some((
                Ok::<Bytes, Infallible>(Bytes::from_static(b"cancel-chunk")),
                (count + 1, notice),
            ))
        },
    );
    Response::new(Body::from_stream(stream))
}

#[tokio::test]
async fn axum_router_composes_with_direct_server_and_streaming_bodies() {
    let (first_tx, first_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let (upload_tx, upload_rx) = oneshot::channel();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let gate = Arc::new(StreamGate {
        first_polled: Mutex::new(Some(first_tx)),
        release_second: Mutex::new(Some(release_rx)),
        upload_first: Mutex::new(Some(upload_tx)),
        cancel_drop: Mutex::new(Some(cancel_tx)),
    });
    let router: Router = Router::new()
        .route("/stream", get(streamed))
        .route("/cancel", get(cancellation_stream))
        .route("/upload", axum::routing::post(upload))
        .with_state(gate)
        .layer(middleware::from_fn(middleware_marker));
    let _: TowerToEggserve<Router> = TowerToEggserve::with_policy(
        router.clone(),
        RequestBodyPolicy::Stream {
            max_bytes: 1024 * 1024,
        },
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = Server::builder()
        .runtime(
            RuntimeConfig::builder()
                .bind(address)
                .max_request_body_bytes(1024 * 1024)
                .build()
                .unwrap(),
        )
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(TowerToEggserve::with_policy(
            router,
            RequestBodyPolicy::Stream {
                max_bytes: 1024 * 1024,
            },
        ))
        .await
        .unwrap();
    let (control, mut completion) = handle.into_parts();
    let mut socket = TcpStream::connect(address).await.unwrap();
    socket
        .write_all(b"GET /stream HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !response.windows(5).any(|window| window == b"first") {
            let mut chunk = [0u8; 1024];
            let count = socket.read(&mut chunk).await.unwrap();
            assert_ne!(count, 0, "connection closed before first stream item");
            response.extend_from_slice(&chunk[..count]);
        }
    })
    .await
    .unwrap();
    first_rx.await.unwrap();
    assert!(!response.windows(6).any(|window| window == b"second"));
    let _ = release_tx.send(());
    socket.read_to_end(&mut response).await.unwrap();
    let response = String::from_utf8_lossy(&response);
    assert!(response.contains("200 OK"));
    assert!(response.contains("first"));
    assert!(response.contains("second"));
    assert!(response.contains("x-middleware: ran"));
    assert!(response.contains("x-repeat: one\r\nx-repeat: two"));

    let mut upload_socket = TcpStream::connect(address).await.unwrap();
    upload_socket.write_all(b"POST /upload HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), upload_rx)
        .await
        .unwrap()
        .unwrap();
    upload_socket
        .write_all(b"3\r\ndef\r\n0\r\n\r\n")
        .await
        .unwrap();
    let mut upload_response = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(5),
        upload_socket.read_to_end(&mut upload_response),
    )
    .await
    .unwrap()
    .unwrap();
    let upload_response = String::from_utf8_lossy(&upload_response);
    assert!(upload_response.contains("200 OK"), "{upload_response}");
    assert!(upload_response.contains("abcdef"));

    let mut cancel_socket = TcpStream::connect(address).await.unwrap();
    cancel_socket
        .write_all(b"GET /cancel HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut cancel_response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !cancel_response
            .windows(12)
            .any(|window| window == b"cancel-chunk")
        {
            let mut chunk = [0u8; 1024];
            let count = cancel_socket.read(&mut chunk).await.unwrap();
            assert_ne!(count, 0, "connection closed before streaming body began");
            cancel_response.extend_from_slice(&chunk[..count]);
        }
    })
    .await
    .unwrap();
    drop(cancel_socket);
    tokio::time::timeout(Duration::from_secs(5), cancel_rx)
        .await
        .unwrap()
        .unwrap();
    control.shutdown();
    tokio::time::timeout(Duration::from_secs(5), completion.wait())
        .await
        .unwrap()
        .unwrap();

    // Compile-time contract: Router directly accepts the public body adapter.
    fn accepts(_: http::Request<HttpRequestBody>) {}
    let _ = accepts;
}
