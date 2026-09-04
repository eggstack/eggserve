//! Embed the canonical HTTP/1 pipeline over a caller-owned byte stream.
//!
//! Demonstrates the transport-neutral connection driver
//! (`server::connection::serve_http1_connection`): the same pipeline that
//! serves TCP/TLS also serves any `AsyncRead + AsyncWrite` stream with an
//! explicit [`ConnectionContext`] (no fabricated socket addresses) and a
//! shared [`RuntimeState`] admission pool. This is the qualification proxy
//! for an anonymity-network router handing EggServe an established stream:
//! the caller owns the transport, EggServe owns HTTP parsing, framing,
//! timeouts, and response privacy.
//!
//! No listener is bound; a `tokio::io::duplex` pair stands in for the
//! caller-owned transport. One request is driven through it and the raw
//! response bytes are printed.
//!
//! Usage: cargo run --example caller_owned_stream -p eggserve-core

use std::sync::Arc;

use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::connection_info::Scheme;
use eggserve_core::server::connection::{
    serve_http1_connection, ConnectionContext, ConnectionShutdown,
};
use eggserve_core::server::{service_fn, Request, RuntimeConfig, RuntimeState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A deliberately tiny custom service: this is the transport/service
    // boundary, not routing or an application framework.
    let service = service_fn(|request: Request| async move {
        match request.head().target().path() {
            "/health" => Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"ok\n".to_vec()))
                .map_err(|e| eggserve_core::server::ServiceError::internal(e.to_string())),
            _ => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(ResponseBody::Bytes(b"not found\n".to_vec()))
                .map_err(|e| eggserve_core::server::ServiceError::internal(e.to_string())),
        }
    });

    let config = Arc::new(RuntimeConfig::default());
    // One admission pool shared across all caller-owned streams, exactly as
    // a multi-stream embedding (many duplex pairs, one router) must share
    // one `Arc<RuntimeState>`.
    let runtime_state = Arc::new(RuntimeState::new(&config));
    // Non-socket transport: scheme asserted by the caller, no socket
    // endpoints fabricated (services observe `None` addresses).
    let context = ConnectionContext::for_non_socket(Scheme::Http, None);
    let shutdown = ConnectionShutdown::new();

    let (mut client, server) = tokio::io::duplex(64 * 1024);
    let driver = tokio::spawn(async move {
        serve_http1_connection(
            server,
            service,
            Arc::clone(&config),
            context,
            Arc::clone(&runtime_state),
            &shutdown,
        )
        .await
    });

    client
        .write_all(b"GET /health HTTP/1.1\r\nHost: example\r\nConnection: close\r\n\r\n")
        .await?;
    let mut raw = Vec::new();
    client.read_to_end(&mut raw).await?;
    let outcome = driver.await?;

    println!("outcome: {outcome:?}");
    println!("{}", String::from_utf8_lossy(&raw));
    Ok(())
}
