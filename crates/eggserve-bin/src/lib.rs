use std::sync::Arc;

use eggserve_core::config::ServeConfig;
use eggserve_core::ops::{
    Event, EventKind, Field, FilteredLogSink, LogFormat as OpsLogFormat, Logger, NopLogSink,
    Severity, StderrLogSink,
};
use eggserve_core::server::{try_from_serve_config, Server};
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
    let limits = match args.limits() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };
    let quiet = args.quiet || args.log_format == args::LogFormat::None;

    // Initialize structured logger.
    let sink: Box<dyn eggserve_core::ops::LogSink> = match args.log_format {
        args::LogFormat::None => Box::new(NopLogSink),
        args::LogFormat::Json => {
            let json_sink = Box::new(StderrLogSink {
                log_format: OpsLogFormat::Json,
            });
            if quiet {
                Box::new(FilteredLogSink::new(json_sink, Severity::Warn))
            } else {
                json_sink
            }
        }
        args::LogFormat::Text => {
            let text_sink = Box::new(StderrLogSink {
                log_format: OpsLogFormat::Text,
            });
            if quiet {
                Box::new(FilteredLogSink::new(text_sink, Severity::Warn))
            } else {
                text_sink
            }
        }
    };
    Logger::init(sink);

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

    // Emit structured startup event.
    let summary = serve_config.startup_summary();
    Logger::global().emit(
        Event::new(
            Severity::Info,
            EventKind::ProcessStarting,
            format!("eggserve {}", env!("CARGO_PKG_VERSION")),
        )
        .field(Field::Str(
            "version".into(),
            env!("CARGO_PKG_VERSION").into(),
        ))
        .field(Field::Str("bind".into(), format!("{}", serve_config.bind)))
        .field(Field::Str(
            "root".into(),
            format!("{}", serve_config.root.display()),
        ))
        .field(Field::Bool(
            "directory_listing".into(),
            summary.directory_listing_enabled,
        ))
        .field(Field::Bool(
            "symlinks_followed".into(),
            summary.symlinks_followed,
        ))
        .field(Field::Bool(
            "dotfiles_served".into(),
            summary.dotfiles_served,
        ))
        .field(Field::U64(
            "max_connections".into(),
            summary.max_connections as u64,
        ))
        .field(Field::U64(
            "max_file_streams".into(),
            summary.max_file_streams as u64,
        )),
    );
    #[cfg(feature = "tls")]
    if tls_config.is_some() {
        Logger::global().emit(Event::new(
            Severity::Info,
            EventKind::ProcessStarting,
            format!(
                "TLS enabled, certificate: {}",
                args.tls_cert.as_ref().unwrap().display()
            ),
        ));
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    #[cfg(not(feature = "tls"))]
    {
        let runtime_config = match try_from_serve_config(&serve_config) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("error: {}", e);
                std::process::exit(1);
            }
        };
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
                    Logger::global().emit(
                        Event::new(
                            Severity::Info,
                            EventKind::ListenerReady,
                            format!("Listening: http://{}", handle.local_addr()),
                        )
                        .field(Field::Str("addr".into(), handle.local_addr().to_string())),
                    );

                    // Wait for first signal: graceful shutdown.
                    let mut signal_rx = shutdown_rx;
                    let _ = signal_rx.recv().await;

                    Logger::global().emit(Event::new(
                        Severity::Info,
                        EventKind::ShutdownRequested,
                        format!(
                            "shutting down (grace period: {}s)",
                            shutdown_timeout.as_secs()
                        ),
                    ));

                    handle.shutdown();

                    // Wait for drain with configured timeout.
                    match tokio::time::timeout(shutdown_timeout, handle.wait()).await {
                        Ok(Ok(result)) => {
                            Logger::global().emit(
                                Event::new(
                                    Severity::Info,
                                    EventKind::ShutdownComplete,
                                    format!("{}", result),
                                )
                                .field(Field::Str("result".into(), result.to_string())),
                            );
                        }
                        Ok(Err(e)) => {
                            Logger::global().emit(Event::new(
                                Severity::Error,
                                EventKind::ShutdownComplete,
                                format!("shutdown error: {}", e),
                            ));
                        }
                        Err(_) => {
                            Logger::global().emit(Event::new(
                                Severity::Warn,
                                EventKind::ShutdownComplete,
                                "shutdown timed out, forcing",
                            ));
                        }
                    }
                }
                Err(e) => {
                    Logger::global().emit(Event::new(
                        Severity::Error,
                        EventKind::ProcessStarting,
                        format!("error: {}", e),
                    ));
                    std::process::exit(1);
                }
            }
        });
    }

    #[cfg(feature = "tls")]
    {
        let mut runtime_config = match try_from_serve_config(&serve_config) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("error: {}", e);
                std::process::exit(1);
            }
        };
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
                    Logger::global().emit(
                        Event::new(
                            Severity::Info,
                            EventKind::ListenerReady,
                            format!("Listening: https://{}", handle.local_addr()),
                        )
                        .field(Field::Str("addr".into(), handle.local_addr().to_string())),
                    );

                    let mut signal_rx = shutdown_rx;
                    let _ = signal_rx.recv().await;

                    Logger::global().emit(Event::new(
                        Severity::Info,
                        EventKind::ShutdownRequested,
                        format!(
                            "shutting down (grace period: {}s)",
                            shutdown_timeout.as_secs()
                        ),
                    ));
                    handle.shutdown();

                    match tokio::time::timeout(shutdown_timeout, handle.wait()).await {
                        Ok(Ok(result)) => {
                            Logger::global().emit(
                                Event::new(
                                    Severity::Info,
                                    EventKind::ShutdownComplete,
                                    format!("{}", result),
                                )
                                .field(Field::Str("result".into(), result.to_string())),
                            );
                        }
                        Ok(Err(e)) => {
                            Logger::global().emit(Event::new(
                                Severity::Error,
                                EventKind::ShutdownComplete,
                                format!("shutdown error: {}", e),
                            ));
                        }
                        Err(_) => {
                            Logger::global().emit(Event::new(
                                Severity::Warn,
                                EventKind::ShutdownComplete,
                                "shutdown timed out, forcing",
                            ));
                        }
                    }
                }
                Err(e) => {
                    Logger::global().emit(Event::new(
                        Severity::Error,
                        EventKind::ProcessStarting,
                        format!("error: {}", e),
                    ));
                    std::process::exit(1);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use eggserve_core::config::ServeConfig;
    use eggserve_core::server::{RuntimeConfig, Server, StaticService};
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn start_test_server(
        tmp: &TempDir,
    ) -> (std::net::SocketAddr, eggserve_core::server::ServerHandle) {
        let svc = StaticService::builder(tmp.path()).build().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let config = RuntimeConfig::builder()
            .header_read_timeout(Duration::from_secs(10))
            .handler_timeout(Duration::from_secs(60))
            .build()
            .unwrap();
        let server = Server::builder()
            .runtime(config)
            .from_listener(listener)
            .build()
            .unwrap();
        let handle = server.start_with_service(svc).await.unwrap();
        (addr, handle)
    }

    #[tokio::test]
    async fn serve_connection_handles_get_without_panicking_on_timer() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
        let (addr, _handle) = start_test_server(&tmp).await;

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();

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
        let (addr, _handle) = start_test_server(&tmp).await;

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(
                b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-4\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();

        let response = String::from_utf8_lossy(&buf);
        assert!(
            response.starts_with("HTTP/1.1 206 Partial Content"),
            "unexpected response: {}",
            response
        );
        assert!(response.contains("content-range: bytes 0-4/11"));
    }
}
