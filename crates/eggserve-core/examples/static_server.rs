//! Embed EggServe's confined static service in a Rust application.
//!
//! Usage: cargo run --example static_server -p eggserve-core -- [ROOT] [BIND]

use std::env;

use eggserve_core::server::{RuntimeConfig, Server};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let root = args.next().unwrap_or_else(|| ".".to_owned());
    let bind = args
        .next()
        .unwrap_or_else(|| "127.0.0.1:8000".to_owned())
        .parse()?;

    let server = Server::builder()
        .runtime(RuntimeConfig::builder().bind(bind).build()?)
        .static_service(root)?;
    let handle = server.start().await?;
    handle.ready().await?;
    println!("Serving on http://{}", handle.local_addr());

    tokio::signal::ctrl_c().await?;
    handle.shutdown();
    handle.wait().await?;
    Ok(())
}
