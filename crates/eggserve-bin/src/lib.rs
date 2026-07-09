use std::sync::Arc;
use std::time::Duration;

use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::Request;
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Semaphore};

use eggserve_core::config::{ServeConfig, ServeState};
use eggserve_core::service::handle_request;

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

    let config = Arc::new(ServeConfig {
        root: args.root,
        bind: args.bind,
        limits,
        static_policy,
    });

    let state = Arc::new(ServeState::new(config.clone()));
    let connection_semaphore = Arc::new(Semaphore::new(config.limits.max_connections));

    if !quiet {
        log_startup(&config, config.startup_summary());
        #[cfg(feature = "tls")]
        if tls_config.is_some() {
            println!(
                "TLS: enabled, certificate: {}",
                args.tls_cert.as_ref().unwrap().display()
            );
        }
    }

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let listener = TcpListener::bind(config.bind).await.unwrap_or_else(|e| {
            eprintln!("error: failed to bind to {}: {}", config.bind, e);
            std::process::exit(1);
        });

        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        tokio::spawn(shutdown::shutdown_signal(shutdown_tx));

        #[cfg(not(feature = "tls"))]
        {
            accept_loop(listener, config, state, connection_semaphore, shutdown_rx).await;
        }
        #[cfg(feature = "tls")]
        {
            accept_loop_with_tls(
                listener,
                config,
                state,
                connection_semaphore,
                tls_config,
                shutdown_rx,
            )
            .await;
        }
    });
}

#[cfg(not(feature = "tls"))]
async fn accept_loop(
    listener: TcpListener,
    config: Arc<ServeConfig>,
    state: Arc<ServeState>,
    connection_semaphore: Arc<Semaphore>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        let permit = match connection_semaphore.clone().try_acquire_owned() {
                            Ok(p) => p,
                            Err(_) => {
                                drop(stream);
                                continue;
                            }
                        };

                        let mut shutdown_rx = shutdown_rx.resubscribe();
                        let state = state.clone();
                        let header_timeout = config.limits.header_read_timeout;
                        let write_timeout = config.limits.response_write_timeout;

                        tokio::spawn(async move {
                            let _permit = permit;
                            let io = TokioIo::new(stream);
                            serve_connection(io, state, header_timeout, write_timeout, &mut shutdown_rx).await;
                        });
                    }
                    Err(e) => {
                        eprintln!("accept error: {}", e);
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        }
    }

    let shutdown_timeout = config.limits.graceful_shutdown_timeout;
    println!(
        "shutting down (grace period: {}s)",
        shutdown_timeout.as_secs()
    );
    tokio::time::timeout(shutdown_timeout, async {
        tokio::time::sleep(Duration::from_millis(100)).await;
    })
    .await
    .ok();
}

#[cfg(feature = "tls")]
async fn accept_loop_with_tls(
    listener: TcpListener,
    config: Arc<ServeConfig>,
    state: Arc<ServeState>,
    connection_semaphore: Arc<Semaphore>,
    tls_config: Option<Arc<rustls::ServerConfig>>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        let permit = match connection_semaphore.clone().try_acquire_owned() {
                            Ok(p) => p,
                            Err(_) => {
                                drop(stream);
                                continue;
                            }
                        };

                        let mut shutdown_rx = shutdown_rx.resubscribe();
                        let state = state.clone();
                        let header_timeout = config.limits.header_read_timeout;
                        let write_timeout = config.limits.response_write_timeout;
                        let tls_config = tls_config.clone();

                        tokio::spawn(async move {
                            let _permit = permit;

                            match tls_config {
                                Some(tls_config) => {
                                    let tls_accept = tokio_rustls::TlsAcceptor::from(tls_config);
                                    match tokio::time::timeout(
                                        header_timeout,
                                        tls_accept.accept(stream),
                                    )
                                    .await
                                    {
                                        Ok(Ok(tls_stream)) => {
                                            let io = TokioIo::new(tls_stream);
                                            serve_connection(io, state, header_timeout, write_timeout, &mut shutdown_rx).await;
                                        }
                                        Ok(Err(_)) => {}
                                        Err(_) => {}
                                    }
                                }
                                None => {
                                    let io = TokioIo::new(stream);
                                    serve_connection(io, state, header_timeout, write_timeout, &mut shutdown_rx).await;
                                }
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("accept error: {}", e);
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        }
    }

    let shutdown_timeout = config.limits.graceful_shutdown_timeout;
    println!(
        "shutting down (grace period: {}s)",
        shutdown_timeout.as_secs()
    );
    tokio::time::timeout(shutdown_timeout, async {
        tokio::time::sleep(Duration::from_millis(100)).await;
    })
    .await
    .ok();
}

async fn serve_connection<I>(
    io: TokioIo<I>,
    state: Arc<ServeState>,
    header_timeout: Duration,
    write_timeout: Duration,
    shutdown_rx: &mut broadcast::Receiver<()>,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
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
                Ok(Err(e)) => {
                    let _ = e;
                }
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
    use std::sync::Arc;
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
