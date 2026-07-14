//! Example: embedding eggserve's runtime with a custom service.
//!
//! Run with: cargo run --example server_embedding -p eggserve-core

use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::server::{service_fn, RuntimeConfig, Server};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = Server::builder()
        .runtime(
            RuntimeConfig::builder()
                .bind("127.0.0.1:3000".parse().unwrap())
                .build(),
        )
        .build_with_service(service_fn(|req: RequestHead| async move {
            let body = format!(
                "Hello from custom service!\nRequest: {} {}",
                req.method(),
                req.target().path()
            );
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(body.into_bytes()))
                .unwrap())
        }))?;

    let handle = server.start().await?;
    println!("Listening on {}", handle.local_addr());
    handle.wait().await?;
    Ok(())
}
