use std::sync::Arc;
use std::time::Duration;

use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::Request;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Semaphore};

use eggserve_core::config::{ServeConfig, ServeState};
use eggserve_core::service::handle_request;
use eggserve_core::telemetry;

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
        telemetry::log_startup(&config);
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
                                    match tls_accept.accept(stream).await {
                                        Ok(tls_stream) => {
                                            let io = TokioIo::new(tls_stream);
                                            serve_connection(io, state, header_timeout, write_timeout, &mut shutdown_rx).await;
                                        }
                                        Err(_) => return,
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
