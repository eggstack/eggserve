//! Embed a minimal streaming service without importing Hyper.
//!
//! Demonstrates transport-independent response bodies: known-length and
//! unknown-length streams with runtime-owned framing. The service never
//! touches `Content-Length`/`Transfer-Encoding`; the runtime does.
//! Stream producers only need `Send` and are polled by one owning connection
//! task; they do not need to be `Sync`.
//!
//! Usage: cargo run --example streaming_service -p eggserve-core -- [BIND]

use std::env;

use bytes::Bytes;
use eggserve_core::primitives::canonical::{
    Response, ResponseBody, ResponseStream, ResponseStreamError, StatusCode,
};
use eggserve_core::server::{service_fn, Request, RuntimeConfig, Server, ServiceError};
use futures_util::stream;

const CHUNK_SIZE: usize = 8192;

fn generated_stream(
    size: usize,
) -> impl futures_util::Stream<Item = Result<Bytes, ResponseStreamError>> {
    stream::unfold(size, |remaining| async move {
        if remaining == 0 {
            None
        } else {
            let chunk_len = remaining.min(CHUNK_SIZE);
            Some((
                Ok(Bytes::from(vec![b'x'; chunk_len])),
                remaining - chunk_len,
            ))
        }
    })
}

fn stream_response(size: usize, known_length: bool) -> ResponseBody {
    let stream = generated_stream(size);
    if known_length {
        ResponseBody::Stream(ResponseStream::with_known_length(stream, size as u64))
    } else {
        // Unknown length: runtime omits `Content-Length`, HTTP/1 uses chunked.
        ResponseBody::Stream(ResponseStream::new(stream))
    }
}

fn tiny_response(known_length: bool) -> ResponseBody {
    let chunks = if known_length {
        vec![
            Ok::<_, ResponseStreamError>(Bytes::from_static(b"hello ")),
            Ok(Bytes::from_static(b"world")),
        ]
    } else {
        vec![
            Ok::<_, ResponseStreamError>(Bytes::from_static(b"tick ")),
            Ok(Bytes::from_static(b"tock")),
        ]
    };
    if known_length {
        ResponseBody::Stream(ResponseStream::with_known_length(stream::iter(chunks), 11))
    } else {
        ResponseBody::Stream(ResponseStream::new(stream::iter(chunks)))
    }
}

fn size_route(path: &str, prefix: &str) -> Option<usize> {
    path.strip_prefix(prefix)?.parse().ok()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8000".to_owned())
        .parse()?;

    let mut runtime = RuntimeConfig::builder().bind(bind);
    if let Ok(value) = env::var("EGGSERVE_BENCH_MAX_CONNECTIONS") {
        runtime = runtime.max_connections(value.parse()?);
    }
    if let Ok(value) = env::var("EGGSERVE_BENCH_MAX_IN_FLIGHT") {
        runtime = runtime.max_in_flight_requests(value.parse()?);
    }
    let server = Server::builder().runtime(runtime.build()?).build()?;
    let service = service_fn(|request: Request| async move {
        let method = request.head().method().as_str();
        let path = request.head().target().path();
        let body = match (method, path) {
            ("GET", "/known") => Some(tiny_response(true)),
            ("GET", "/stream") => Some(tiny_response(false)),
            ("GET", path) => size_route(path, "/bytes/")
                .map(|size| ResponseBody::Bytes(vec![b'x'; size]))
                .or_else(|| size_route(path, "/known/").map(|size| stream_response(size, true)))
                .or_else(|| size_route(path, "/stream/").map(|size| stream_response(size, false))),
            _ => None,
        };
        match body {
            Some(body) => Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/plain; charset=utf-8")
                .map_err(|e| ServiceError::internal(e.to_string()))?
                .body(body)
                .map_err(|e| ServiceError::internal(e.to_string())),
            None => Response::builder()
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
    println!("  GET /bytes/N, /known/N, /stream/N — sized benchmark/demo bodies");

    tokio::signal::ctrl_c().await?;
    handle.shutdown();
    handle.wait().await?;
    Ok(())
}
