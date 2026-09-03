//! Serve static files with custom default content type and extra response headers.
//!
//! Demonstrates the `default_content_type` and `extra_response_headers` static
//! metadata hooks. Extra headers are emitted only on final 200 responses and
//! cannot override runtime-owned metadata (Content-Length, ETag, etc.).
//!
//! Usage: cargo run --example custom_headers -p eggserve-core -- [ROOT] [BIND]

use std::env;
use std::sync::Arc;

use eggserve_core::config::ServeConfig;
use eggserve_core::limits::Limits;
use eggserve_core::policy::StaticPolicy;
use eggserve_core::server::{RuntimeConfig, Server};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let root = args.next().unwrap_or_else(|| ".".to_owned());
    let bind = args
        .next()
        .unwrap_or_else(|| "127.0.0.1:8000".to_owned())
        .parse()?;

    let serve_config = Arc::new(ServeConfig {
        root: root.into(),
        bind,
        limits: Limits::default(),
        static_policy: StaticPolicy::safe_default(),
        default_content_type: "application/octet-stream".to_string(),
        extra_response_headers: vec![
            ("X-Served-By".to_string(), "eggserve".to_string()),
            ("Cache-Control".to_string(), "no-cache".to_string()),
        ],
        ..ServeConfig::default()
    });

    let server = Server::builder()
        .runtime(RuntimeConfig::builder().bind(bind).build()?)
        .serve_config(serve_config)
        .build()?;
    let handle = server.start().await?;
    handle.ready().await?;
    println!("Serving on http://{}", handle.local_addr());

    tokio::signal::ctrl_c().await?;
    handle.shutdown();
    handle.wait().await?;
    Ok(())
}
