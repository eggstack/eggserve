//! Serve static files over HTTPS using EggServe's Rust TLS backend.
//!
//! Requires the `tls` feature. Generate a self-signed certificate for local
//! testing:
//!
//!     openssl req -x509 -newkey rsa:2048 -nodes \
//!         -keyout key.pem -out cert.pem -days 30 \
//!         -subj '/CN=localhost'
//!
//! Usage: cargo run --example https_server -p eggserve-core --features tls -- [ROOT] [BIND]
//!
//! The default bind is `127.0.0.1:8443`. The certificate and key are loaded
//! from `cert.pem` and `key.pem` in the current directory.

#[cfg(not(feature = "tls"))]
fn main() {
    eprintln!("This example requires the `tls` feature. Re-run with:");
    eprintln!("  cargo run --example https_server -p eggserve-core --features tls");
}

#[cfg(feature = "tls")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::env;
    use std::path::PathBuf;
    use std::sync::Arc;

    use eggserve_core::config::ServeConfig;
    use eggserve_core::limits::Limits;
    use eggserve_core::policy::StaticPolicy;
    use eggserve_core::server::{RuntimeConfig, Server};
    use eggserve_core::tls::load_tls_config;

    let mut args = env::args().skip(1);
    let root = args.next().unwrap_or_else(|| ".".to_owned());
    let bind = args
        .next()
        .unwrap_or_else(|| "127.0.0.1:8443".to_owned())
        .parse()?;

    let cert_path = PathBuf::from("cert.pem");
    let key_path = PathBuf::from("key.pem");

    let tls_config =
        load_tls_config(&cert_path, &key_path).map_err(|e| format!("TLS config error: {e}"))?;

    let serve_config = Arc::new(ServeConfig {
        root: root.into(),
        bind,
        limits: Limits::default(),
        static_policy: StaticPolicy::safe_default(),
        default_content_type: "application/octet-stream".to_string(),
        extra_response_headers: Vec::new(),
    });

    let runtime_config = RuntimeConfig::builder()
        .bind(bind)
        .tls_config(tls_config)
        .build()?;

    let server = Server::builder()
        .runtime(runtime_config)
        .serve_config(serve_config)
        .build()?;
    let handle = server.start().await?;
    handle.ready().await?;
    println!("Serving on https://{}", handle.local_addr());

    tokio::signal::ctrl_c().await?;
    handle.shutdown();
    handle.wait().await?;
    Ok(())
}
