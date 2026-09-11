//! Listener accept loop (Plan 206 Track G).
//!
//! Owns feature-specific listener/protocol startup:
//! `accept_loop_multi` (TCP + Unix, shared admission, bounded backoff),
//! `handle_tcp_accept`/`handle_unix_accept`, TLS/ALPN selection,
//! systemd/process-manager endpoints, H3 prebound validation.
//! `Server` orchestration stays in the parent facade.

#![allow(unused_imports)]
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::broadcast;

use super::runtime::RuntimeState;
use crate::config::ServeConfig;
use crate::ops::{Event, EventKind, OpsContext, Severity};
#[cfg(feature = "http2")]
use crate::server::config::Http2Config;
#[cfg(feature = "http3")]
use crate::server::config::Http3Config;
use crate::server::config::{RuntimeConfig, RuntimeConfigBuilder};
use crate::server::connection::{
    serve_http1_connection, ConnectionContext, ConnectionOutcome, ConnectionShutdown,
};
use crate::server::errors::{ServerError, ShutdownResult};
use crate::server::handle::ServerHandle;
use crate::server::lifecycle::{Lifecycle, LifecycleState};
use crate::server::listener::BoundEndpoint;
use crate::server::response_policy::{DatePolicy, ResponsePolicy};
use crate::server::service::{Service, ServiceError};

/// Source for the TCP listener (Plan 201 Track B).
#[derive(Debug)]
pub(super) enum TcpListenerSource {
    /// Bind to this address on start.
    Bind(std::net::SocketAddr),
    /// Use this pre-bound listener (no duplicate bind).
    Listener(TcpListener),
}

/// Source for the Unix-domain listener (Plan 201 Track C, Unix only).
#[cfg(unix)]
#[derive(Debug)]
pub(super) enum UnixListenerSource {
    /// Use this pre-bound Unix listener. Filesystem path ownership stays
    /// with the caller; EggServe never unlinks.
    Listener(tokio::net::UnixListener),
}

#[cfg(feature = "http3")]
pub(super) fn config_http3_enabled(config: &RuntimeConfig) -> bool {
    config.http3.enabled
}

/// Unified multi-listener accept loop (Plan 201 Tracks A/C/F/G).
///
/// One loop drives every adopted stream listener (TCP and, on Unix, UDS)
/// through the same admission, TLS, protocol-selection, and lifecycle
/// pipeline — not a second accept loop. Connection-semaphore admission uses
/// `try_acquire` so accepted sockets never accumulate unboundedly; accept
/// errors share bounded backoff + observability; shutdown wakes the loop
/// promptly and undispatched transports are dropped.
#[allow(clippy::too_many_arguments)]
pub(super) async fn accept_loop_multi<S: Service>(
    tcp_listener: Option<TcpListener>,
    tcp_addr: Option<std::net::SocketAddr>,
    #[cfg(unix)] unix_listener: Option<tokio::net::UnixListener>,
    endpoints: Vec<crate::server::listener::BoundEndpoint>,
    config: Arc<RuntimeConfig>,
    runtime_state: Arc<RuntimeState>,
    connection_semaphore: Arc<tokio::sync::Semaphore>,
    service: S,
    mut shutdown_rx: broadcast::Receiver<()>,
    lifecycle: Arc<Lifecycle>,
) -> ShutdownResult {
    let service = Arc::new(service);

    // Signal that we're running (listeners bound, accept loop about to poll).
    // If shutdown raced before this point, `drain()` has already transitioned
    // `Starting` → `Stopped` and `mark_running()` will fail. `mark_failed()`
    // is a no-op in that terminal state, so we return `Clean`.
    if lifecycle.mark_running().is_err() {
        let _ = lifecycle.mark_failed();
        return ShutdownResult::Clean;
    }

    let ops = runtime_state.ops().clone();
    let endpoint_summary = endpoints
        .iter()
        .map(|ep| ep.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    ops.emit(
        crate::ops::Event::new(
            crate::ops::Severity::Info,
            crate::ops::EventKind::ListenerReady,
            format!("accept loop started: {endpoint_summary}"),
        )
        .field(crate::ops::Field::Str(
            "endpoints".into(),
            endpoint_summary.clone(),
        )),
    );

    // Track spawned connection tasks for graceful drain.
    let mut tasks = tokio::task::JoinSet::new();
    let mut backoff_idx: usize = 0;
    let mut error_repeat_count: usize = 0;
    let mut last_error_kind: Option<String> = None;

    // Readiness (Track G) already means every endpoint above was adopted and
    // protocol configuration validated before this task spawned.
    #[cfg(unix)]
    let has_unix = unix_listener.is_some();
    #[cfg(not(unix))]
    let has_unix = false;
    let has_tcp = tcp_listener.is_some();
    debug_assert!(has_tcp || has_unix, "accept loop needs a listener");

    loop {
        // `pending()` branches keep `select!` well-formed when a family is
        // absent (Unix-only or TCP-only servers).
        let tcp_accept = async {
            match &tcp_listener {
                Some(l) => l.accept().await.map(|(s, p)| (Some(s), p)),
                None => {
                    std::future::pending::<
                        Result<
                            (Option<tokio::net::TcpStream>, std::net::SocketAddr),
                            std::io::Error,
                        >,
                    >()
                    .await
                }
            }
        };
        #[cfg(unix)]
        let unix_accept = async {
            match &unix_listener {
                Some(l) => l.accept().await.map(|(s, _cred)| s),
                None => {
                    std::future::pending::<Result<tokio::net::UnixStream, std::io::Error>>().await
                }
            }
        };
        #[cfg(not(unix))]
        let unix_accept: std::future::Pending<Result<(), std::io::Error>> = std::future::pending();

        tokio::select! {
            result = tcp_accept => {
                match result {
                    Ok((Some(stream), peer_addr)) => {
                        let tcp_bind = tcp_addr.expect("tcp listener has an addr");
                        handle_tcp_accept(
                            stream,
                            peer_addr,
                            tcp_bind,
                            "tcp-0",
                            &config,
                            &runtime_state,
                            &connection_semaphore,
                            &service,
                            &shutdown_rx,
                            &mut tasks,
                            &ops,
                        );
                        backoff_idx = 0;
                        error_repeat_count = 0;
                        last_error_kind = None;
                    }
                    Ok((None, _)) => {}
                    Err(e) => {
                        let fatal = classify_accept_error(&e, &mut shutdown_rx, &mut backoff_idx, &mut error_repeat_count, &mut last_error_kind, &ops).await;
                        if fatal {
                            break;
                        }
                    }
                }
            }
            result = unix_accept => {
                #[cfg(unix)]
                match result {
                    Ok(stream) => {
                        handle_unix_accept(
                            stream,
                            "unix-0",
                            &config,
                            &runtime_state,
                            &connection_semaphore,
                            &service,
                            &shutdown_rx,
                            &mut tasks,
                            &ops,
                        );
                        backoff_idx = 0;
                        error_repeat_count = 0;
                        last_error_kind = None;
                    }
                    Err(e) => {
                        let fatal = classify_accept_error(&e, &mut shutdown_rx, &mut backoff_idx, &mut error_repeat_count, &mut last_error_kind, &ops).await;
                        if fatal {
                            break;
                        }
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = result;
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        }
    }

    ops.emit(crate::ops::Event::new(
        crate::ops::Severity::Info,
        crate::ops::EventKind::ShutdownRequested,
        "shutdown requested",
    ));

    // Transition to Draining.
    let _ = lifecycle.drain_with_ops(&ops);

    // Wait for in-flight connections to drain.
    let drain_timeout = config.graceful_shutdown_timeout;
    let deadline = tokio::time::Instant::now() + drain_timeout;
    let mut timed_out = false;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            timed_out = true;
            break;
        }
        match tokio::time::timeout(remaining, tasks.join_next()).await {
            Ok(Some(result)) => {
                if let Err(e) = result {
                    if e.is_panic() {
                        ops.counters()
                            .connection_panics
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        ops.emit(crate::ops::Event::new(
                            crate::ops::Severity::Error,
                            crate::ops::EventKind::ConnectionPanic,
                            "connection task panicked during drain",
                        ));
                    }
                }
            }
            Ok(None) => break,
            Err(_) => {
                timed_out = true;
                break;
            }
        }
    }

    let mut abort_count = 0usize;

    if timed_out {
        ops.emit(crate::ops::Event::new(
            crate::ops::Severity::Warn,
            crate::ops::EventKind::ForcedShutdownStarted,
            "grace deadline exceeded, aborting remaining tasks",
        ));
        tasks.abort_all();
        while let Some(result) = tasks.join_next().await {
            abort_count += 1;
            if let Err(e) = result {
                if e.is_panic() {
                    ops.counters()
                        .connection_panics
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    ops.emit(crate::ops::Event::new(
                        crate::ops::Severity::Error,
                        crate::ops::EventKind::ConnectionPanic,
                        "connection task panicked during forced shutdown",
                    ));
                }
            }
        }
    }

    let _ = lifecycle.mark_stopped();

    let result = if timed_out {
        ops.counters()
            .forced_shutdowns
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ShutdownResult::Timeout
    } else {
        ops.counters()
            .graceful_shutdowns
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ShutdownResult::Clean
    };

    ops.emit(crate::ops::Event::new(
        crate::ops::Severity::Info,
        crate::ops::EventKind::ShutdownComplete,
        format!("shutdown complete: {:?} (aborted={})", result, abort_count),
    ));

    result
}

/// Admit and dispatch one accepted TCP connection (Plan 201 Track F).
///
/// Shared by every TCP listener source (address-bound, prebound, systemd).
/// Admission uses `try_acquire` so saturation drops (closes) the accepted
/// socket instead of queueing unboundedly. Rejected connections never skew
/// the active-connection gauge. The spawned task owns TLS handshake (bounded
/// by `tls_handshake_timeout`), protocol selection, and the canonical
/// service pipeline.
#[allow(clippy::too_many_arguments)]
fn handle_tcp_accept<S: Service>(
    stream: tokio::net::TcpStream,
    peer_addr: std::net::SocketAddr,
    tcp_bind: std::net::SocketAddr,
    listener_id: &'static str,
    config: &Arc<RuntimeConfig>,
    runtime_state: &Arc<RuntimeState>,
    connection_semaphore: &Arc<tokio::sync::Semaphore>,
    service: &Arc<S>,
    shutdown_rx: &broadcast::Receiver<()>,
    tasks: &mut tokio::task::JoinSet<()>,
    ops: &crate::ops::OpsContext,
) {
    let _ = stream.set_nodelay(true);
    let conn_id = ops.next_connection_id();
    ops.counters()
        .connections_accepted
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    ops.emit(
        crate::ops::Event::new(
            crate::ops::Severity::Debug,
            crate::ops::EventKind::ConnectionAccepted,
            format!("connection accepted ({listener_id})"),
        )
        .connection_id(conn_id)
        .field(crate::ops::Field::Str(
            "listener".into(),
            listener_id.into(),
        )),
    );

    let permit = match connection_semaphore.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            ops.counters()
                .connections_rejected
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Debug,
                    crate::ops::EventKind::ConnectionRejected,
                    "connection rejected: admission limit",
                )
                .connection_id(conn_id)
                .field(crate::ops::Field::Str(
                    "listener".into(),
                    listener_id.into(),
                )),
            );
            drop(stream);
            return;
        }
    };

    let runtime_state = runtime_state.clone();
    let conn_ops = ops.clone();
    let config = config.clone();
    let service = service.clone();
    let remote_addr = peer_addr;
    let local_addr_pre_tls = stream.local_addr().unwrap_or(tcp_bind);

    // Count the connection as active only after it has been admitted;
    // rejected connections must not skew the gauge.
    conn_ops
        .counters()
        .active_connections
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let forwarder_rx = shutdown_rx.resubscribe();
    tasks.spawn(async move {
        let _permit = permit;
        let _active_connection = ActiveConnectionGuard {
            ops: conn_ops.clone(),
        };

        // Bridge the server broadcast shutdown to the canonical
        // per-connection token so TCP/TLS and caller-owned streams share one
        // driver pipeline.
        let conn_shutdown = super::connection::ConnectionShutdown::new();
        let forwarder_shutdown = conn_shutdown.clone();
        let mut forwarder_rx = forwarder_rx;
        tokio::spawn(async move {
            let _ = forwarder_rx.recv().await;
            forwarder_shutdown.shutdown();
        });

        // Plan 202 Track C: optional PROXY preamble before TLS/HTTP.
        // Disabled listeners interpret bytes normally (existing path below
        // unchanged). Enabled listeners require trust and a bounded preamble;
        // malformed/untrusted input closes before TLS/HTTP and never reaches
        // a service. Order: TCP accept -> PROXY -> TLS (optional) -> HTTP.
        if config.trusted_proxy.proxy_protocol.enabled {
            use std::sync::atomic::Ordering as ProxyOrdering;

            if !config.trusted_proxy.is_trusted_peer(&remote_addr) {
                conn_ops
                    .counters()
                    .proxy_rejected
                    .fetch_add(1, ProxyOrdering::Relaxed);
                conn_ops.emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Warn,
                        crate::ops::EventKind::ProxyProtocolRejected,
                        "proxy preamble rejected: untrusted peer",
                    )
                    .connection_id(conn_id)
                    .field(crate::ops::Field::Str(
                        "listener".into(),
                        listener_id.into(),
                    ))
                    .field(crate::ops::Field::Str(
                        "peer".into(),
                        remote_addr.to_string(),
                    ))
                    .field(crate::ops::Field::Str(
                        "category".into(),
                        "untrusted_peer".into(),
                    )),
                );
                return;
            }

            let mut tcp_stream = stream;
            let (proxy_source, proxy_destination, proxy_kind, proxy_leftover) =
                match crate::server::proxy::read_proxy_preamble(
                    &mut tcp_stream,
                    config.trusted_proxy.proxy_protocol.timeout,
                )
                .await
                {
                    Ok((endpoints, leftover)) => {
                        conn_ops
                            .counters()
                            .proxy_accepted
                            .fetch_add(1, ProxyOrdering::Relaxed);
                        let effective = endpoints
                            .source
                            .map(|addr| addr.to_string())
                            .unwrap_or_else(|| "none".to_owned());
                        conn_ops.emit(
                            crate::ops::Event::new(
                                crate::ops::Severity::Debug,
                                crate::ops::EventKind::ProxyProtocolAccepted,
                                format!("proxy preamble accepted ({})", endpoints.kind),
                            )
                            .connection_id(conn_id)
                            .field(crate::ops::Field::Str(
                                "listener".into(),
                                listener_id.into(),
                            ))
                            .field(crate::ops::Field::Str(
                                "peer".into(),
                                remote_addr.to_string(),
                            ))
                            .field(crate::ops::Field::Str(
                                "source".into(),
                                endpoints.kind.as_str().to_owned(),
                            ))
                            .field(crate::ops::Field::Str("effective".into(), effective)),
                        );
                        (
                            endpoints.source,
                            endpoints.destination,
                            endpoints.kind,
                            leftover,
                        )
                    }
                    Err(error) => {
                        let category = match error {
                            crate::server::proxy::ProxyReadError::Timeout => "timeout",
                            crate::server::proxy::ProxyReadError::TooLong => "too_long",
                            crate::server::proxy::ProxyReadError::Invalid => "invalid",
                            crate::server::proxy::ProxyReadError::Io => "io",
                        };
                        conn_ops
                            .counters()
                            .proxy_rejected
                            .fetch_add(1, ProxyOrdering::Relaxed);
                        conn_ops.emit(
                            crate::ops::Event::new(
                                crate::ops::Severity::Warn,
                                crate::ops::EventKind::ProxyProtocolRejected,
                                format!("proxy preamble rejected: {category}"),
                            )
                            .connection_id(conn_id)
                            .field(crate::ops::Field::Str(
                                "listener".into(),
                                listener_id.into(),
                            ))
                            .field(crate::ops::Field::Str(
                                "peer".into(),
                                remote_addr.to_string(),
                            ))
                            .field(crate::ops::Field::Str(
                                "category".into(),
                                category.to_owned(),
                            )),
                        );
                        return;
                    }
                };

            #[cfg(feature = "tls")]
            {
                if let Some(tls_config) = current_tls_config(&config) {
                    let tls_acceptor = tokio_rustls::TlsAcceptor::from(tls_config);
                    #[cfg(feature = "http2")]
                    let h2_enabled = config.http2.enabled;
                    #[cfg(not(feature = "http2"))]
                    let h2_enabled = false;
                    let expose_chain = config.tls_expose_peer_chain;
                    let prefixed =
                        super::connection::driver::PrefixedIo::new(proxy_leftover, tcp_stream);
                    match accept_tls(
                        prefixed,
                        &tls_acceptor,
                        config.tls_handshake_timeout,
                        h2_enabled,
                        expose_chain,
                        conn_id,
                        &conn_ops,
                    )
                    .await
                    {
                        Some((tls_stream, tls_info, protocol)) => {
                            conn_ops.emit(
                                crate::ops::Event::new(
                                    crate::ops::Severity::Debug,
                                    crate::ops::EventKind::TlsHandshakeSuccess,
                                    "TLS handshake completed",
                                )
                                .connection_id(conn_id),
                            );
                            let context = super::connection::ConnectionContext::for_tcp(
                                local_addr_pre_tls,
                                remote_addr,
                                Some(tls_info),
                            )
                            .with_proxy_endpoints(
                                proxy_source,
                                proxy_destination,
                                proxy_kind,
                            );
                            let _ = super::connection::serve_http_connection_with_id_and_protocol(
                                tls_stream,
                                ArcService(service),
                                config.clone(),
                                context,
                                runtime_state.clone(),
                                &conn_shutdown,
                                conn_id,
                                protocol,
                            )
                            .await;
                            return;
                        }
                        None => {
                            return;
                        }
                    }
                }
            }

            // Cleartext (or TLS feature disabled) with replayed preamble bytes.
            {
                let prefixed =
                    super::connection::driver::PrefixedIo::new(proxy_leftover, tcp_stream);
                let context = super::connection::ConnectionContext::for_tcp(
                    local_addr_pre_tls,
                    remote_addr,
                    None,
                )
                .with_proxy_endpoints(proxy_source, proxy_destination, proxy_kind);
                let _ = super::connection::serve_http_connection_with_id_and_protocol(
                    prefixed,
                    ArcService(service),
                    config.clone(),
                    context,
                    runtime_state.clone(),
                    &conn_shutdown,
                    conn_id,
                    super::connection::driver::WireProtocol::Auto,
                )
                .await;
                return;
            }
        }

        #[cfg(feature = "tls")]
        {
            if let Some(tls_config) = current_tls_config(&config) {
                let tls_acceptor = tokio_rustls::TlsAcceptor::from(tls_config);
                #[cfg(feature = "http2")]
                let h2_enabled = config.http2.enabled;
                #[cfg(not(feature = "http2"))]
                let h2_enabled = false;
                let expose_chain = config.tls_expose_peer_chain;
                match accept_tls(
                    stream,
                    &tls_acceptor,
                    config.tls_handshake_timeout,
                    h2_enabled,
                    expose_chain,
                    conn_id,
                    &conn_ops,
                )
                .await
                {
                    Some((tls_stream, tls_info, protocol)) => {
                        conn_ops.emit(
                            crate::ops::Event::new(
                                crate::ops::Severity::Debug,
                                crate::ops::EventKind::TlsHandshakeSuccess,
                                "TLS handshake completed",
                            )
                            .connection_id(conn_id),
                        );
                        let context = super::connection::ConnectionContext::for_tcp(
                            local_addr_pre_tls,
                            remote_addr,
                            Some(tls_info),
                        );
                        let _ = super::connection::serve_http_connection_with_id_and_protocol(
                            tls_stream,
                            ArcService(service),
                            config.clone(),
                            context,
                            runtime_state.clone(),
                            &conn_shutdown,
                            conn_id,
                            protocol,
                        )
                        .await;
                        return;
                    }
                    None => {
                        return;
                    }
                }
            }
        }

        let context =
            super::connection::ConnectionContext::for_tcp(local_addr_pre_tls, remote_addr, None);
        let _ = super::connection::serve_http_connection_with_id_and_protocol(
            stream,
            ArcService(service),
            config.clone(),
            context,
            runtime_state.clone(),
            &conn_shutdown,
            conn_id,
            super::connection::driver::WireProtocol::Auto,
        )
        .await;
    });
}

/// Admit and dispatch one accepted Unix-domain connection (Plan 201 Track C).
///
/// Same admission/backoff/observability as TCP: `try_acquire` (no unbounded
/// queue), stable `listener` field, prompt close of undispatched transports
/// on shutdown via the drain below. No TLS handshake (Unix is plaintext by
/// explicit policy) and no fabricated IP endpoints (`for_unix()`); the H1/H2
/// selector still applies over the byte stream.
#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn handle_unix_accept<S: Service>(
    stream: tokio::net::UnixStream,
    listener_id: &'static str,
    config: &Arc<RuntimeConfig>,
    runtime_state: &Arc<RuntimeState>,
    connection_semaphore: &Arc<tokio::sync::Semaphore>,
    service: &Arc<S>,
    shutdown_rx: &broadcast::Receiver<()>,
    tasks: &mut tokio::task::JoinSet<()>,
    ops: &crate::ops::OpsContext,
) {
    let conn_id = ops.next_connection_id();
    ops.counters()
        .connections_accepted
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ops.emit(
        crate::ops::Event::new(
            crate::ops::Severity::Debug,
            crate::ops::EventKind::ConnectionAccepted,
            format!("connection accepted ({listener_id})"),
        )
        .connection_id(conn_id)
        .field(crate::ops::Field::Str(
            "listener".into(),
            listener_id.into(),
        )),
    );

    let permit = match connection_semaphore.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            ops.counters()
                .connections_rejected
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Debug,
                    crate::ops::EventKind::ConnectionRejected,
                    "connection rejected: admission limit",
                )
                .connection_id(conn_id)
                .field(crate::ops::Field::Str(
                    "listener".into(),
                    listener_id.into(),
                )),
            );
            drop(stream);
            return;
        }
    };

    let runtime_state = runtime_state.clone();
    let conn_ops = ops.clone();
    let config = config.clone();
    let service = service.clone();
    conn_ops
        .counters()
        .active_connections
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let forwarder_rx = shutdown_rx.resubscribe();
    tasks.spawn(async move {
        let _permit = permit;
        let _active_connection = ActiveConnectionGuard {
            ops: conn_ops.clone(),
        };
        let conn_shutdown = super::connection::ConnectionShutdown::new();
        let forwarder_shutdown = conn_shutdown.clone();
        let mut forwarder_rx = forwarder_rx;
        tokio::spawn(async move {
            let _ = forwarder_rx.recv().await;
            forwarder_shutdown.shutdown();
        });
        let context = super::connection::ConnectionContext::for_unix();
        let _ = super::connection::serve_http_connection_with_id_and_protocol(
            stream,
            ArcService(service),
            config.clone(),
            context,
            runtime_state.clone(),
            &conn_shutdown,
            conn_id,
            super::connection::driver::WireProtocol::Auto,
        )
        .await;
    });
}

/// Current TLS snapshot for new handshakes (Plan 203 Track F).
///
/// When a reload handle is configured it wins atomically; otherwise the
/// legacy single-identity `tls_config` is used. `None` means plaintext.
#[cfg(feature = "tls")]
fn current_tls_config(config: &RuntimeConfig) -> Option<std::sync::Arc<rustls::ServerConfig>> {
    if let Some(handle) = &config.tls_reload_handle {
        return Some(handle.current());
    }
    config.tls_config.clone()
}

/// Accept a TLS connection with timeout.
///
/// Returns the TLS stream and TLS session metadata on success, or `None` if
/// the handshake failed or timed out. Emits `TlsHandshakeFailure` or
/// `TlsHandshakeTimeout` events on failure.
///
/// Ordering (Plan 203 Track E): accept permit → optional PROXY preamble
/// (Plan 202) → TLS handshake deadline → ALPN protocol selection → HTTP.
/// Handshake errors use fixed sanitized categories and never echo rustls
/// internals to clients; the connection is closed and permits released.
///
/// Generic over the transport so Plan 202 PROXY-preamble replay
/// (`PrefixedIo<TcpStream>`) shares the same handshake path as plain
/// `TcpStream` with no behavior change when PROXY is disabled.
#[cfg(feature = "tls")]
async fn accept_tls<S>(
    stream: S,
    tls_acceptor: &tokio_rustls::TlsAcceptor,
    timeout: std::time::Duration,
    h2_enabled: bool,
    expose_peer_chain: bool,
    conn_id: u64,
    ops: &crate::ops::OpsContext,
) -> Option<(
    tokio_rustls::server::TlsStream<S>,
    crate::primitives::connection_info::TlsInfo,
    super::connection::driver::WireProtocol,
)>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    match tokio::time::timeout(timeout, tls_acceptor.accept(stream)).await {
        Ok(Ok(tls_stream)) => {
            let protocol = {
                let (_io, conn) = tls_stream.get_ref();
                #[cfg(feature = "http2")]
                if conn
                    .alpn_protocol()
                    .is_some_and(|protocol| protocol == b"h2")
                {
                    if !h2_enabled {
                        ops.emit(
                            crate::ops::Event::new(
                                crate::ops::Severity::Warn,
                                crate::ops::EventKind::TlsHandshakeFailure,
                                "TLS negotiated disabled HTTP/2 protocol",
                            )
                            .connection_id(conn_id),
                        );
                        return None;
                    }
                    super::connection::driver::WireProtocol::Http2
                } else {
                    super::connection::driver::WireProtocol::Http1
                }
                #[cfg(not(feature = "http2"))]
                {
                    let _ = h2_enabled;
                    if conn
                        .alpn_protocol()
                        .is_some_and(|protocol| protocol == b"h2")
                    {
                        ops.emit(
                            crate::ops::Event::new(
                                crate::ops::Severity::Warn,
                                crate::ops::EventKind::TlsHandshakeFailure,
                                "TLS negotiated unavailable HTTP/2 protocol",
                            )
                            .connection_id(conn_id),
                        );
                        return None;
                    }
                    super::connection::driver::WireProtocol::Http1
                }
            };
            let tls_info = extract_tls_info(&tls_stream, expose_peer_chain);
            Some((tls_stream, tls_info, protocol))
        }
        Ok(Err(_)) => {
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Warn,
                    crate::ops::EventKind::TlsHandshakeFailure,
                    "TLS handshake failed",
                )
                .connection_id(conn_id),
            );
            None
        }
        Err(_) => {
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Warn,
                    crate::ops::EventKind::TlsHandshakeTimeout,
                    "TLS handshake timeout",
                )
                .connection_id(conn_id),
            );
            None
        }
    }
}

/// Extract verified TLS session metadata from a completed TLS stream.
///
/// Only verified data is exposed: SNI as supplied/accepted (bounded), ALPN,
/// and client-auth state derived from the verified peer chain. Raw DER chain
/// exposure is opt-in and bounded (8 × 64 KiB); oversized chains suppress to
/// `None`. Never logs key material (callers must only emit sanitized
/// categories).
#[cfg(feature = "tls")]
fn extract_tls_info<S>(
    tls_stream: &tokio_rustls::server::TlsStream<S>,
    expose_peer_chain: bool,
) -> crate::primitives::connection_info::TlsInfo
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use crate::primitives::connection_info::TlsInfo;

    let (_io, conn) = tls_stream.get_ref();
    let protocol_version = conn.protocol_version().map(|v| format!("{v:?}"));
    let server_name = conn.server_name().and_then(|n| {
        // Bound before observability/application use (Plan 203 Track B).
        if n.len() > TlsInfo::MAX_SERVER_NAME_LEN {
            None
        } else {
            Some(n.to_owned())
        }
    });
    let alpn = conn
        .alpn_protocol()
        .map(|p| String::from_utf8_lossy(p).into_owned());
    let peer_certs = conn.peer_certificates();
    let peer_certificates_present = peer_certs.is_some_and(|c| !c.is_empty());
    // Present implies verified (rustls/WebPKI already enforced Required, and
    // Optional only sets present when verification succeeded; handshake would
    // have failed otherwise).
    let client_authenticated = peer_certificates_present;
    let peer_certificate_chain = if expose_peer_chain && peer_certificates_present {
        peer_certs.and_then(|chain| {
            if chain.len() > TlsInfo::MAX_PEER_CERTIFICATES {
                return None;
            }
            let mut out = Vec::with_capacity(chain.len());
            for cert in chain {
                let bytes = cert.as_ref();
                if bytes.len() > TlsInfo::MAX_PEER_CERT_BYTES {
                    return None;
                }
                out.push(bytes.to_vec());
            }
            Some(out)
        })
    } else {
        None
    };
    TlsInfo {
        protocol_version,
        server_name,
        alpn,
        client_authenticated,
        peer_certificates_present,
        peer_certificate_chain,
    }
}

/// Classify an accept loop error, emit a structured log event, and apply
/// bounded exponential backoff for transient errors. The backoff is
/// interruptible by shutdown via the provided receiver.
///
/// Rate-limits repeated identical errors: emits the first occurrence, then
/// a summary every 10 consecutive identical errors, resetting on success
/// or a different error kind. Grouping is intentionally coarse: it keys on
/// `EventKind` (`ListenerTransientError` / `ResourceExhaustion` /
/// `ListenerPersistentError`) rather than `io::ErrorKind`, so a burst of
/// `ConnectionAborted` followed by `TimedOut` (both `ListenerTransientError`)
/// is seen as the same kind and rate-limited together. This is conservative
/// — no error is lost forever (first + every 10th is emitted) — and a finer
/// `format!("{:?}/{:?}", event_kind, kind)` key could be used if per-variant
/// granularity is needed.
///
/// Returns `true` if the error is fatal and the accept loop should terminate.
#[allow(clippy::collapsible_match)]
pub(super) async fn classify_accept_error(
    e: &std::io::Error,
    shutdown_rx: &mut broadcast::Receiver<()>,
    backoff_idx: &mut usize,
    error_repeat_count: &mut usize,
    last_error_kind: &mut Option<String>,
    ops: &crate::ops::OpsContext,
) -> bool {
    use crate::ops::{Event, EventKind, Severity};

    let err_str = e.to_string();
    let kind = e.kind();
    let fd_exhausted = is_fd_exhaustion(e);

    let (severity, event_kind, should_backoff, is_fatal) = match kind {
        std::io::ErrorKind::Interrupted => (
            Severity::Debug,
            EventKind::ListenerTransientError,
            true,
            false,
        ),
        std::io::ErrorKind::ConnectionRefused
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::BrokenPipe => (
            Severity::Debug,
            EventKind::ListenerTransientError,
            true,
            false,
        ),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => (
            Severity::Warn,
            EventKind::ListenerTransientError,
            true,
            false,
        ),
        // Kernel memory pressure (ENOMEM) surfaces as OutOfMemory without
        // matching is_fd_exhaustion; like fd exhaustion it can be transient,
        // so back off instead of terminating the server.
        std::io::ErrorKind::OutOfMemory => {
            (Severity::Error, EventKind::ResourceExhaustion, true, false)
        }
        std::io::ErrorKind::Other if fd_exhausted => {
            (Severity::Error, EventKind::ResourceExhaustion, true, false)
        }
        std::io::ErrorKind::Other => (
            Severity::Error,
            EventKind::ListenerPersistentError,
            false,
            true,
        ),
        _ if fd_exhausted => (Severity::Error, EventKind::ResourceExhaustion, true, false),
        _ => (
            Severity::Error,
            EventKind::ListenerPersistentError,
            false,
            true,
        ),
    };

    ops.counters()
        .listener_errors
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // Rate-limit repeated identical errors.
    let current_kind = format!("{}", event_kind);
    let is_same_kind = last_error_kind.as_deref() == Some(&current_kind);
    if is_same_kind {
        *error_repeat_count = error_repeat_count.saturating_add(1);
    } else {
        *error_repeat_count = 1;
        *last_error_kind = Some(current_kind);
        // A different error kind starts its own backoff ramp; otherwise a
        // burst of one transient kind would saddle a different kind with
        // the maximum inherited delay.
        *backoff_idx = 0;
    }

    // Emit on first occurrence, then every 10th.
    let should_emit = *error_repeat_count == 1 || (*error_repeat_count).is_multiple_of(10);
    if should_emit {
        let message = if *error_repeat_count > 1 {
            format!(
                "accept error ({} consecutive): {}",
                error_repeat_count, err_str
            )
        } else {
            format!("accept error: {}", err_str)
        };
        ops.emit(
            Event::new(severity, event_kind, message).field(crate::ops::Field::Str(
                "error_kind".into(),
                format!("{:?}", kind),
            )),
        );
    }

    if should_backoff {
        static BACKOFF_MS: [u64; 8] = [1, 2, 4, 8, 50, 100, 250, 500];
        let idx = (*backoff_idx).min(BACKOFF_MS.len() - 1);
        *backoff_idx = backoff_idx.saturating_add(1);
        let backoff = std::time::Duration::from_millis(BACKOFF_MS[idx]);
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = shutdown_rx.recv() => {}
        }
    }

    is_fatal
}

pub(super) fn is_fd_exhaustion(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    if let Some(raw) = error.raw_os_error() {
        return raw == rustix::io::Errno::MFILE.raw_os_error()
            || raw == rustix::io::Errno::NFILE.raw_os_error();
    }

    // Windows accept() failures carry raw Winsock codes; WSAEMFILE and
    // WSAENFILE are the fd-exhaustion equivalents and must back off like
    // Unix EMFILE/ENFILE.
    #[cfg(windows)]
    if let Some(raw) = error.raw_os_error() {
        return raw == 10024 || raw == 10023; // WSAEMFILE || WSAENFILE
    }

    if error.raw_os_error().is_some() {
        return false;
    }

    // Fallback string match for non-OS errors (e.g., mocked accept failures).
    // `accept()` errors originate from the kernel and are not user-controlled,
    // so the misclassification risk of matching the error string is negligible.
    let message = error.to_string().to_ascii_lowercase();
    message.contains("too many open files")
        || message.contains("emfile")
        || message.contains("enfile")
}

struct ActiveConnectionGuard {
    ops: crate::ops::OpsContext,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.ops
            .counters()
            .active_connections
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Wrapper to implement `Service` for `Arc<S>`.
pub(super) struct ArcService<S>(pub(super) Arc<S>);

impl<S: Service> Service for ArcService<S> {
    fn request_body_policy(
        &self,
        head: &crate::primitives::request_head::RequestHead,
    ) -> crate::primitives::request_body_policy::RequestBodyPolicy {
        self.0.request_body_policy(head)
    }

    fn call(
        &self,
        request: crate::primitives::request::Request,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<crate::primitives::canonical::Response, ServiceError>,
                > + Send
                + '_,
        >,
    > {
        self.0.call(request)
    }
}
