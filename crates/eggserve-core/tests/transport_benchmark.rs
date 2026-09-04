//! Ignored Plan 170 microbenchmark for the caller-owned connection seam.
//!
//! This deliberately uses only the public canonical service/driver API and a
//! Tokio duplex transport. It is evidence for gross embedding-path overhead,
//! not a network throughput score and not part of routine CI.

use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use eggserve_core::primitives::canonical::{Response, ResponseBody, ResponseStream, StatusCode};
use eggserve_core::primitives::connection_info::Scheme;
use eggserve_core::server::connection::{
    serve_http1_connection, ConnectionContext, ConnectionShutdown,
};
use eggserve_core::server::{service_fn, Request, RuntimeConfig, RuntimeState, ServiceError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const SIZE: usize = 1024 * 1024;
const CHUNK: usize = 8192;

fn body_stream(
    size: usize,
) -> impl futures_util::Stream<
    Item = Result<Bytes, eggserve_core::primitives::canonical::ResponseStreamError>,
> {
    futures_util::stream::unfold(size, |remaining| async move {
        if remaining == 0 {
            None
        } else {
            let len = remaining.min(CHUNK);
            Some((Ok(Bytes::from(vec![b'x'; len])), remaining - len))
        }
    })
}

fn response(path: &str) -> Result<Response, ServiceError> {
    let body = match path {
        "/bytes" => ResponseBody::Bytes(vec![b'x'; SIZE]),
        "/stream" => ResponseBody::Stream(ResponseStream::with_known_length(
            body_stream(SIZE),
            SIZE as u64,
        )),
        _ => ResponseBody::Bytes(b"not found".to_vec()),
    };
    Response::builder()
        .status(if path == "/bytes" || path == "/stream" {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        })
        .body(body)
        .map_err(|error| ServiceError::internal(error.to_string()))
}

async fn read_response_body(
    client: &mut (impl AsyncReadExt + Unpin),
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut headers = Vec::new();
    let mut byte = [0; 1];
    while !headers.ends_with(b"\r\n\r\n") {
        client.read_exact(&mut byte).await?;
        headers.push(byte[0]);
    }
    let text = String::from_utf8(headers)?;
    let length = text
        .lines()
        .find_map(|line| {
            line.strip_prefix("content-length:")
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    let mut body = vec![0; length];
    client.read_exact(&mut body).await?;
    Ok(body.len())
}

async fn measure(path: &'static str, iterations: usize) -> Result<f64, Box<dyn std::error::Error>> {
    let config = Arc::new(
        RuntimeConfig::builder()
            .bind("127.0.0.1:0".parse()?)
            .connection_total_timeout(std::time::Duration::from_secs(120))
            .max_requests_per_connection(Some((iterations + 1) as u64))
            .build()?,
    );
    let runtime = Arc::new(RuntimeState::new(&config));
    let service =
        service_fn(move |request: Request| async move { response(request.head().target().path()) });
    let shutdown = Arc::new(ConnectionShutdown::new());
    let (mut client, server) = tokio::io::duplex(2 * 1024 * 1024);
    let driver_shutdown = Arc::clone(&shutdown);
    let driver = tokio::spawn(async move {
        serve_http1_connection(
            server,
            service,
            config,
            ConnectionContext::for_non_socket(Scheme::Http, None),
            runtime,
            &driver_shutdown,
        )
        .await
    });
    let request = format!("GET {path} HTTP/1.1\r\nHost: benchmark\r\n\r\n");
    let start = Instant::now();
    for _ in 0..iterations {
        client.write_all(request.as_bytes()).await?;
        let body_size = read_response_body(&mut client).await?;
        assert_eq!(body_size, SIZE);
    }
    client
        .write_all(b"GET /bytes HTTP/1.1\r\nHost: benchmark\r\nConnection: close\r\n\r\n")
        .await?;
    let _ = read_response_body(&mut client).await?;
    let _ = driver.await?;
    Ok(iterations as f64 / start.elapsed().as_secs_f64())
}

#[tokio::test]
#[ignore = "manual Plan 170 evidence; absolute timing is not a CI gate"]
async fn caller_owned_duplex_benchmark() -> Result<(), Box<dyn std::error::Error>> {
    let iterations = std::env::var("EGGSERVE_BENCH_ITERATIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);
    let buffered = measure("/bytes", iterations).await?;
    let streamed = measure("/stream", iterations).await?;
    println!("{{\"iterations\":{iterations},\"response_size\":{SIZE},\"buffered_rps\":{buffered:.3},\"streamed_rps\":{streamed:.3},\"transport\":\"tokio::io::duplex\"}}");
    Ok(())
}
