//! Embed a deliberately small custom service without importing Hyper.
//!
//! Usage: cargo run --example custom_service -p eggserve-core -- [BIND]

use std::env;

use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::server::{service_fn, Request, RuntimeConfig, Server, ServiceError};

fn response(status: StatusCode, body: &'static [u8]) -> Result<Response, ServiceError> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .and_then(|builder| builder.header("content-length", body.len().to_string()))
        .map_err(|error| ServiceError::internal(error.to_string()))?
        .body(ResponseBody::Bytes(body.to_vec()))
        .map_err(|error| ServiceError::internal(error.to_string()))
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
        let head = request.head();
        match (head.method().as_str(), head.target().path()) {
            ("GET", "/health") => response(StatusCode::OK, b"ok\n"),
            ("GET", "/") => response(StatusCode::OK, b"EggServe custom service\n"),
            _ => response(StatusCode::NOT_FOUND, b"not found\n"),
        }
    });

    let handle = server.start_with_service(service).await?;
    handle.ready().await?;
    println!("Serving on http://{}", handle.local_addr());

    tokio::signal::ctrl_c().await?;
    handle.shutdown();
    handle.wait().await?;
    Ok(())
}
