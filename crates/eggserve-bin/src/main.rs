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

mod args;
mod shutdown;

#[tokio::main]
async fn main() {
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
    let config = Arc::new(ServeConfig {
        root: args.root,
        bind: args.bind,
        limits,
        static_policy,
    });

    let state = Arc::new(ServeState::new(config.clone()));
    let connection_semaphore = Arc::new(Semaphore::new(config.limits.max_connections));

    telemetry::log_startup(&config);

    let listener = TcpListener::bind(config.bind).await.unwrap_or_else(|e| {
        eprintln!("error: failed to bind to {}: {}", config.bind, e);
        std::process::exit(1);
    });

    let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);

    tokio::spawn(shutdown::shutdown_signal(shutdown_tx));

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        let permit = match connection_semaphore.clone().try_acquire_owned() {
                            Ok(p) => p,
                            Err(_) => {
                                // Connection limit reached; drop the stream
                                drop(stream);
                                continue;
                            }
                        };

                        let mut shutdown_rx = shutdown_rx.resubscribe();
                        let state = state.clone();
                        let config = config.clone();
                        let header_timeout = config.limits.header_read_timeout;
                        let write_timeout = config.limits.response_write_timeout;

                        tokio::spawn(async move {
                            let _permit = permit;
                            let io = TokioIo::new(stream);
                            let service = service_fn(move |req: Request<Incoming>| {
                                let state = state.clone();
                                async move {
                                    Ok::<_, std::convert::Infallible>(handle_request(req, &state).await)
                                }
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
        // Wait briefly for in-flight tasks to complete
        tokio::time::sleep(Duration::from_millis(100)).await;
    })
    .await
    .ok();
}
