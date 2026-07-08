use std::sync::Arc;

use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::Request;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::broadcast;

use eggserve_core::config::ServeConfig;
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

    let config = Arc::new(ServeConfig {
        root: args.root,
        bind: args.bind,
        ..ServeConfig::default()
    });

    telemetry::log_startup(&config.bind, &config.root);

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
                        let mut shutdown_rx = shutdown_rx.resubscribe();
                        let config = config.clone();
                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);
                            let service = service_fn(move |req: Request<Incoming>| {
                                let config = config.clone();
                                async move {
                                    Ok::<_, std::convert::Infallible>(handle_request(req, &config))
                                }
                            });
                            let conn = http1::Builder::new()
                                .serve_connection(io, service)
                                .with_upgrades();
                            let mut conn = std::pin::pin!(conn);
                            tokio::select! {
                                result = &mut conn => {
                                    if let Err(e) = result {
                                        eprintln!("connection error: {}", e);
                                    }
                                }
                                _ = shutdown_rx.recv() => {
                                    conn.graceful_shutdown();
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

    println!("shutting down");
}
