use std::sync::Arc;

use eggserve_core::config::ServeConfig;
use eggserve_core::server::{RuntimeConfig, Server};
use tokio::sync::broadcast;

pub mod args;
mod shutdown;
#[cfg(feature = "tls")]
pub mod tls;

pub fn run() {
    let args = match args::Args::parse() {
        Ok(a) => a,
        Err(e) if e == "help" => {
            args::print_usage();
            return;
        }
        Err(e) if e == "version" => {
            args::print_version();
            return;
        }
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };

    let static_policy = args.static_policy();
    let limits = args.limits();
    let quiet = args.quiet || args.log_format == args::LogFormat::None;

    #[cfg(feature = "tls")]
    let tls_config = match (&args.tls_cert, &args.tls_key) {
        (Some(cert), Some(key)) => match tls::load_tls_config(cert, key) {
            Ok(config) => Some(config),
            Err(e) => {
                eprintln!("error: {}", e);
                std::process::exit(1);
            }
        },
        _ => None,
    };

    let serve_config = Arc::new(ServeConfig {
        root: args.root,
        bind: args.bind,
        limits,
        static_policy,
    });

    // Print startup banner.
    if !quiet {
        log_startup(&serve_config, serve_config.startup_summary());
        #[cfg(feature = "tls")]
        if tls_config.is_some() {
            println!(
                "TLS: enabled, certificate: {}",
                args.tls_cert.as_ref().unwrap().display()
            );
        }
    }

    let rt = tokio::runtime::Runtime::new().unwrap();

    #[cfg(not(feature = "tls"))]
    {
        let runtime_config = RuntimeConfig::from(&*serve_config);
        let shutdown_timeout = serve_config.limits.graceful_shutdown_timeout;

        rt.block_on(async {
            let server = Server::builder()
                .runtime(runtime_config)
                .serve_config(serve_config)
                .build()
                .unwrap();

            let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

            // Start signal handler.
            tokio::spawn(shutdown::shutdown_signal(shutdown_tx));

            match server.start().await {
                Ok(handle) => {
                    if !quiet {
                        println!("Listening: http://{}", handle.local_addr());
                    }

                    // Wait for first signal: graceful shutdown.
                    let mut signal_rx = shutdown_rx;
                    let _ = signal_rx.recv().await;

                    println!(
                        "shutting down (grace period: {}s)",
                        shutdown_timeout.as_secs()
                    );

                    handle.shutdown();

                    // Wait for drain with configured timeout.
                    match tokio::time::timeout(shutdown_timeout, handle.wait()).await {
                        Ok(Ok(result)) => {
                            if !quiet {
                                println!("{}", result);
                            }
                        }
                        Ok(Err(e)) => {
                            eprintln!("shutdown error: {}", e);
                        }
                        Err(_) => {
                            eprintln!("shutdown timed out, forcing");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    std::process::exit(1);
                }
            }
        });
    }

    #[cfg(feature = "tls")]
    {
        let mut runtime_config = RuntimeConfig::from(&*serve_config);
        runtime_config.tls_config = tls_config;

        let shutdown_timeout = serve_config.limits.graceful_shutdown_timeout;

        rt.block_on(async {
            let server = Server::builder()
                .runtime(runtime_config)
                .serve_config(serve_config)
                .build()
                .unwrap();

            let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
            tokio::spawn(shutdown::shutdown_signal(shutdown_tx));

            match server.start().await {
                Ok(handle) => {
                    if !quiet {
                        println!("Listening: https://{}", handle.local_addr());
                    }

                    let mut signal_rx = shutdown_rx;
                    let _ = signal_rx.recv().await;

                    println!(
                        "shutting down (grace period: {}s)",
                        shutdown_timeout.as_secs()
                    );
                    handle.shutdown();

                    match tokio::time::timeout(shutdown_timeout, handle.wait()).await {
                        Ok(Ok(result)) => {
                            if !quiet {
                                println!("{}", result);
                            }
                        }
                        Ok(Err(e)) => {
                            eprintln!("shutdown error: {}", e);
                        }
                        Err(_) => {
                            eprintln!("shutdown timed out, forcing");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    std::process::exit(1);
                }
            }
        });
    }
}

#[cfg(test)]
async fn serve_connection<I>(
    io: hyper_util::rt::TokioIo<I>,
    state: Arc<eggserve_core::config::ServeState>,
    header_timeout: std::time::Duration,
    write_timeout: std::time::Duration,
    shutdown_rx: &mut broadcast::Receiver<()>,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use hyper::body::Incoming;
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::Request;
    use hyper_util::rt::TokioTimer;

    use eggserve_core::service::handle_request;

    let service = service_fn(move |req: Request<Incoming>| {
        let state = state.clone();
        async move { Ok::<_, std::convert::Infallible>(handle_request(req, &state).await) }
    });
    let conn = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(header_timeout)
        .serve_connection(io, service)
        .with_upgrades();
    let mut conn = std::pin::pin!(conn);
    tokio::select! {
        result = tokio::time::timeout(write_timeout, &mut conn) => {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(_e)) => {}
                Err(_elapsed) => {
                    conn.as_mut().graceful_shutdown();
                }
            }
        }
        _ = shutdown_rx.recv() => {
            conn.as_mut().graceful_shutdown();
        }
    }
}

fn log_startup(config: &ServeConfig, summary: eggserve_core::config::StartupSummary) {
    println!("eggserve {}", env!("CARGO_PKG_VERSION"));
    println!("Serving root: {}", config.root.display());
    println!("Listening: http://{}", config.bind);
    println!("Methods: GET, HEAD");
    println!(
        "Directory listing: {}",
        if summary.directory_listing_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "Symlinks: {}",
        if summary.symlinks_followed {
            "follow"
        } else {
            "denied"
        }
    );
    println!(
        "Dotfiles: {}",
        if summary.dotfiles_served {
            "serve"
        } else {
            "denied"
        }
    );
    println!("Max connections: {}", summary.max_connections);
    println!("Max file streams: {}", summary.max_file_streams);

    if summary.bind_is_unspecified {
        eprintln!("WARNING: public bind enabled");
    }
    if summary.symlinks_followed {
        eprintln!("WARNING: symlink following enabled");
    }
    if summary.dotfiles_served {
        eprintln!("WARNING: dotfile serving enabled");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggserve_core::config::ServeState;
    use hyper_util::rt::TokioIo;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn build_state(tmp: &TempDir) -> Arc<ServeState> {
        let config = Arc::new(ServeConfig {
            root: tmp.path().to_path_buf(),
            ..ServeConfig::default()
        });
        Arc::new(ServeState::new(config))
    }

    #[tokio::test]
    async fn serve_connection_handles_get_without_panicking_on_timer() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
        let state = build_state(&tmp);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, _rx) = broadcast::channel::<()>(1);

        let state_clone = state.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            let mut shutdown_rx = tx.subscribe();
            serve_connection(
                io,
                state_clone,
                Duration::from_secs(10),
                Duration::from_secs(60),
                &mut shutdown_rx,
            )
            .await;
        });

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();

        let _ = server.await;

        let response = String::from_utf8_lossy(&buf);
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "unexpected response: {}",
            response
        );
        assert!(response.contains("hello"), "missing body: {}", response);
    }

    #[tokio::test]
    async fn serve_connection_handles_range_request() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
        let state = build_state(&tmp);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, _rx) = broadcast::channel::<()>(1);

        let state_clone = state.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            let mut shutdown_rx = tx.subscribe();
            serve_connection(
                io,
                state_clone,
                Duration::from_secs(10),
                Duration::from_secs(60),
                &mut shutdown_rx,
            )
            .await;
        });

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(
                b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();

        let _ = server.await;

        let response = String::from_utf8_lossy(&buf);
        assert!(
            response.starts_with("HTTP/1.1 206 Partial Content"),
            "unexpected response: {}",
            response
        );
        assert!(response.contains("content-range: bytes 0-4/11"));
    }
}
