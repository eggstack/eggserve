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
    let code = run_cli(std::env::args().skip(1).collect());
    std::process::exit(code);
}

/// Run the packaged CLI implementation without terminating the host process.
///
/// This entry point exists for the Python wheel's extension-backed CLI. It is
/// not the general Rust embedding API: Rust applications should depend on
/// `eggserve-core` and use its public `server` or `primitives` facade. The
/// argument vector uses the same syntax as the `eggserve` executable, and the
/// return value is the process-style exit code.
#[allow(clippy::needless_return)]
pub fn run_cli(argv: Vec<String>) -> i32 {
    let args = match args::Args::parse_from(argv) {
        Ok(a) => a,
        Err(e) if e == "help" => {
            args::print_usage();
            return 0;
        }
        Err(e) if e == "version" => {
            args::print_version();
            return 0;
        }
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    let static_policy = args.static_policy();
    let limits = match args.limits() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
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
    let _ = Logger::try_init(sink);

    #[cfg(feature = "tls")]
    let tls_config = match (&args.tls_cert, &args.tls_key) {
        (Some(cert), Some(key)) => match tls::load_tls_config(cert, key) {
            Ok(config) => Some(config),
            Err(e) => {
                Logger::global().emit(Event::new(
                    Severity::Error,
                    EventKind::ProcessStarting,
                    format!("error: {}", e),
                ));
                return 1;
            }
        },
        _ => None,
    };

    let serve_config = Arc::new(ServeConfig {
        root: args.root,
        bind: args.bind,
        limits,
        static_policy,
        default_content_type: args.default_content_type,
        extra_response_headers: args.extra_response_headers,
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
    if let (Some(_), Some(cert)) = (&tls_config, &args.tls_cert) {
        Logger::global().emit(Event::new(
            Severity::Info,
            EventKind::ProcessStarting,
            format!("TLS enabled, certificate: {}", cert.display()),
        ));
    }

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            Logger::global().emit(Event::new(
                Severity::Error,
                EventKind::ProcessStarting,
                format!("failed to build runtime: {}", e),
            ));
            return 1;
        }
    };

    #[cfg(not(feature = "tls"))]
    {
        let runtime_config = match try_from_serve_config(&serve_config) {
            Ok(c) => c,
            Err(e) => {
                Logger::global().emit(Event::new(
                    Severity::Error,
                    EventKind::ProcessStarting,
                    format!("error: {}", e),
                ));
                return 1;
            }
        };
        let shutdown_timeout = serve_config.limits.graceful_shutdown_timeout;

        return rt.block_on(async {
            let server = match Server::builder()
                .runtime(runtime_config)
                .serve_config(serve_config)
                .build()
            {
                Ok(server) => server,
                Err(e) => {
                    Logger::global().emit(Event::new(
                        Severity::Error,
                        EventKind::ProcessStarting,
                        format!("failed to build server: {}", e),
                    ));
                    return 1;
                }
            };

            // The receiver is created before the signal task and remains
            // subscribed through startup, so a signal during `start()` is
            // buffered instead of being lost.
            let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

            let signal_task = tokio::spawn(shutdown::shutdown_signal(shutdown_tx));

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

                    let mut signal_rx = shutdown_rx;
                    // A closed channel means the signal-handler task died
                    // before arming its handlers; nothing can deliver a
                    // termination signal, so stop instead of waiting forever
                    // on a server nobody can stop.
                    if signal_rx.recv().await.is_err() {
                        Logger::global().emit(Event::new(
                            Severity::Warn,
                            EventKind::ShutdownRequested,
                            "shutdown signal handler unavailable; shutting down",
                        ));
                    }
                    signal_task.abort();

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
                            // A fatal runtime failure during drain is a
                            // dirty stop; signal it to supervisors.
                            return 1;
                        }
                        Err(_) => {
                            Logger::global().emit(Event::new(
                                Severity::Warn,
                                EventKind::ShutdownComplete,
                                "shutdown timed out, forcing",
                            ));
                            // A forced abort after the grace period is a
                            // dirty stop; signal it to supervisors.
                            return 1;
                        }
                    }
                    0
                }
                Err(e) => {
                    Logger::global().emit(Event::new(
                        Severity::Error,
                        EventKind::ProcessStarting,
                        format!("error: {}", e),
                    ));
                    1
                }
            }
        });
    }

    #[cfg(feature = "tls")]
    {
        let mut runtime_config = match try_from_serve_config(&serve_config) {
            Ok(c) => c,
            Err(e) => {
                Logger::global().emit(Event::new(
                    Severity::Error,
                    EventKind::ProcessStarting,
                    format!("error: {}", e),
                ));
                return 1;
            }
        };
        runtime_config.tls_config = tls_config;

        let shutdown_timeout = serve_config.limits.graceful_shutdown_timeout;
        // Log the actual serving scheme: the TLS-featured binary still
        // serves plain HTTP when no certificate was provided.
        let scheme = if runtime_config.tls_config.is_some() {
            "https"
        } else {
            "http"
        };

        return rt.block_on(async {
            let server = match Server::builder()
                .runtime(runtime_config)
                .serve_config(serve_config)
                .build()
            {
                Ok(server) => server,
                Err(e) => {
                    Logger::global().emit(Event::new(
                        Severity::Error,
                        EventKind::ProcessStarting,
                        format!("failed to build server: {}", e),
                    ));
                    return 1;
                }
            };

            // Keep this receiver alive through startup: broadcast delivers a
            // signal sent during `start()` to this already-subscribed receiver.
            let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
            let signal_task = tokio::spawn(shutdown::shutdown_signal(shutdown_tx));

            match server.start().await {
                Ok(handle) => {
                    Logger::global().emit(
                        Event::new(
                            Severity::Info,
                            EventKind::ListenerReady,
                            format!("Listening: {}://{}", scheme, handle.local_addr()),
                        )
                        .field(Field::Str("addr".into(), handle.local_addr().to_string())),
                    );

                    let mut signal_rx = shutdown_rx;
                    // A closed channel means the signal-handler task died
                    // before arming its handlers; nothing can deliver a
                    // termination signal, so stop instead of waiting forever
                    // on a server nobody can stop.
                    if signal_rx.recv().await.is_err() {
                        Logger::global().emit(Event::new(
                            Severity::Warn,
                            EventKind::ShutdownRequested,
                            "shutdown signal handler unavailable; shutting down",
                        ));
                    }
                    signal_task.abort();

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
                            // A fatal runtime failure during drain is a
                            // dirty stop; signal it to supervisors.
                            return 1;
                        }
                        Err(_) => {
                            Logger::global().emit(Event::new(
                                Severity::Warn,
                                EventKind::ShutdownComplete,
                                "shutdown timed out, forcing",
                            ));
                            // A forced abort after the grace period is a
                            // dirty stop; signal it to supervisors.
                            return 1;
                        }
                    }
                    0
                }
                Err(e) => {
                    Logger::global().emit(Event::new(
                        Severity::Error,
                        EventKind::ProcessStarting,
                        format!("error: {}", e),
                    ));
                    1
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
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
