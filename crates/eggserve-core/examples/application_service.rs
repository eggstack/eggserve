//! Minimal native application-service contract (Plan 197).
//!
//! One deliberately small `Service` showing the stabilized request /
//! response / lifecycle ownership without any static filesystem dependency:
//!
//! - `GET /health` — buffered bytes response, no body;
//! - `POST /echo` — buffered request (`read_all`) into a bytes response;
//! - `POST /pipe` — streamed request into a streamed response over a
//!   bounded channel (no `read_all`, backpressure is real);
//! - `GET /poll` — lifecycle cancellation: an idle waiter that wakes on
//!   peer disconnect / shutdown instead of probing a socket.
//!
//! This is the transport/service boundary, not routing, middleware, or an
//! application framework. Framing (`Content-Length` / chunked), timeouts,
//! and response privacy stay runtime-owned.
//!
//! Usage: cargo run -p eggserve-core --example application_service -- [BIND]

use std::env;

use bytes::Bytes;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::primitives::response_stream::ResponseStreamError;
use eggserve_core::primitives::ResponseStream;
use eggserve_core::server::{Request, RuntimeConfig, Server, Service, ServiceError};

struct AppService;

impl Service for AppService {
    fn request_body_policy(
        &self,
        head: &eggserve_core::primitives::request_head::RequestHead,
    ) -> RequestBodyPolicy {
        match (head.method().as_str(), head.target().path()) {
            ("POST", "/echo") => RequestBodyPolicy::Buffer {
                max_bytes: 64 * 1024,
            },
            ("POST", "/pipe") => RequestBodyPolicy::Stream {
                max_bytes: 1024 * 1024,
            },
            _ => RequestBodyPolicy::Reject,
        }
    }

    fn call(
        &self,
        request: Request,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Response, ServiceError>> + Send + '_>,
    > {
        Box::pin(async move {
            // Single attachment point for transport metadata + cancellation.
            let context = request.context().clone();
            let path = request.head().target().path().to_owned();
            let method = request.head().method().as_str().to_owned();
            match (method.as_str(), path.as_str()) {
                ("GET", "/health") => ok_bytes(b"ok\n"),
                ("POST", "/echo") => {
                    // Buffered request -> bytes response.
                    let body = request.into_body().read_all().await.map_err(|e| {
                        ServiceError::rejected(e.to_status_code(), "request body failed")
                    })?;
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(ResponseBody::Bytes(body.to_vec()))
                        .map_err(|e| ServiceError::internal(e.to_string()))
                }
                ("POST", "/pipe") => {
                    // Streamed request -> streamed response over a bounded
                    // channel. The pump owns the body; the app task forwards
                    // chunks; `Service::call` returns response-start only.
                    let lifecycle = context.lifecycle_clone();
                    let (_head, mut body) = request.into_head_and_body();
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
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(ResponseBody::Stream(ResponseStream::new(stream)))
                        .map_err(|e| ServiceError::internal(e.to_string()))
                }
                ("GET", "/poll") => {
                    // Lifecycle cancellation: never probe a raw socket.
                    // Wait for disconnect/shutdown or a 30s long-poll bound.
                    let lifecycle = context.lifecycle_clone();
                    tokio::select! {
                        biased;
                        _ = lifecycle.cancelled() => {
                            // No second HTTP error after commitment is
                            // possible here: we have not committed yet, so a
                            // sanitized 500 is still truthful pre-commitment.
                            Err(ServiceError::internal("cancelled"))
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                            ok_bytes(b"poll-timeout\n")
                        }
                    }
                }
                _ => Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(ResponseBody::Bytes(b"not found\n".to_vec()))
                    .map_err(|e| ServiceError::internal(e.to_string())),
            }
        })
    }
}

fn ok_bytes(body: &[u8]) -> Result<Response, ServiceError> {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain; charset=utf-8")
        .map_err(|e| ServiceError::internal(e.to_string()))?
        .body(ResponseBody::Bytes(body.to_vec()))
        .map_err(|e| ServiceError::internal(e.to_string()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8000".to_owned())
        .parse()?;

    let server = Server::builder()
        .runtime(RuntimeConfig::builder().bind(bind).build()?)
        .build()?;
    let handle = server.start_with_service(AppService).await?;
    handle.ready().await?;
    println!("Serving application demo on http://{}", handle.local_addr());
    println!("  GET /health — bytes response");
    println!("  POST /echo — buffered echo");
    println!("  POST /pipe — streamed echo (chunked)");
    println!("  GET /poll — lifecycle long-poll (cancel on disconnect)");

    tokio::signal::ctrl_c().await?;
    handle.shutdown();
    handle.wait().await?;
    Ok(())
}
