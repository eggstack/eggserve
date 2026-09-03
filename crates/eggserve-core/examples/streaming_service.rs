//! Embed a minimal streaming service without importing Hyper.
//!
//! Demonstrates transport-independent response bodies: known-length and
//! unknown-length streams with runtime-owned framing. The service never
//! touches `Content-Length`/`Transfer-Encoding`; the runtime does.
//!
//! Usage: cargo run --example streaming_service -p eggserve-core -- [BIND]

use std::env;

use bytes::Bytes;
use eggserve_core::primitives::canonical::{
    Response, ResponseBody, ResponseStream, ResponseStreamError, StatusCode,
};
use eggserve_core::server::{service_fn, Request, RuntimeConfig, Server, ServiceError};
use futures_util::stream;

fn known_stream() -> ResponseBody {
    // Exact 11-byte representation: runtime sends `Content-Length: 11`.
    let chunks = vec![
        Ok::<_, ResponseStreamError>(Bytes::from_static(b"hello ")),
        Ok::<_, ResponseStreamError>(Bytes::from_static(b"world")),
    ];
    ResponseBody::Stream(ResponseStream::with_known_length(stream::iter(chunks), 11))
}

fn unknown_stream() -> ResponseBody {
    // Unknown length: runtime omits `Content-Length`, HTTP/1 uses chunked.
    let chunks = vec![
        Ok::<_, ResponseStreamError>(Bytes::from_static(b"tick ")),
        Ok::<_, ResponseStreamError>(Bytes::from_static(b"tock")),
    ];
    ResponseBody::Stream(ResponseStream::new(stream::iter(chunks)))
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
    let service = service_fn(|request: Request| async move {
        match (
            request.head().method().as_str(),
            request.head().target().path(),
        ) {
            ("GET", "/known") => Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/plain; charset=utf-8")
                .map_err(|e| ServiceError::internal(e.to_string()))?
                .body(known_stream())
                .map_err(|e| ServiceError::internal(e.to_string())),
            ("GET", "/stream") => Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/plain; charset=utf-8")
                .map_err(|e| ServiceError::internal(e.to_string()))?
                .body(unknown_stream())
                .map_err(|e| ServiceError::internal(e.to_string())),
            _ => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(ResponseBody::Bytes(b"not found\n".to_vec()))
                .map_err(|e| ServiceError::internal(e.to_string())),
        }
    });

    let handle = server.start_with_service(service).await?;
    handle.ready().await?;
    println!("Serving streaming demo on http://{}", handle.local_addr());
    println!("  GET /known  — known-length stream (Content-Length)");
    println!("  GET /stream — unknown-length stream (chunked)");

    tokio::signal::ctrl_c().await?;
    handle.shutdown();
    handle.wait().await?;
    Ok(())
}
