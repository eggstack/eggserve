//! Reusable HTTP runtime and service boundary.
//!
//! This module provides a transport-owning HTTP runtime that downstream Rust
//! projects can embed without importing internal modules or depending directly
//! on Hyper.
//!
//! # Architecture
//!
//! ```text
//! Server::builder()
//!     .runtime(RuntimeConfig)
//!     .service(my_service)
//!     .build()?
//!     .start()
//!     .await?;
//!
//! handle.ready().await?;
//! // server is accepting connections
//!
//! handle.shutdown().await?;
//! // server drains and stops
//! ```
//!
//! The runtime owns:
//! - Listener acceptance
//! - HTTP/1 parsing
//! - Request conversion to canonical types
//! - Response normalization
//! - Timeout enforcement
//! - Connection and file-stream permits
//! - Connection/task tracking
//! - Graceful shutdown with drain deadline
//! - Forced shutdown with task cancellation
//!
//! Services own:
//! - Request handling logic
//! - Response construction
//!
//! # Public types
//!
//! - [`Server`] — the main entry point for embedding
//! - [`ServerBuilder`] — configured builder for the server
//! - [`ServerHandle`] — control handle for a running server
//! - [`RuntimeConfig`] — transport-level configuration
//! - [`Service`] — the service trait
//! - [`service_fn`] — create a service from a closure
//! - [`StaticService`] — hardened static file service
//! - [`ServerError`] — startup and lifecycle errors
//! - [`ServiceError`] — per-request service errors
//! - [`ShutdownResult`] — outcome of a shutdown operation
//! - [`LifecycleState`] — server lifecycle state

pub mod config;
pub mod connection;
pub mod errors;
pub mod handle;
pub mod lifecycle;
pub mod service;
pub mod static_service;

pub use crate::primitives::request::Request;
pub use config::{RuntimeConfig, RuntimeConfigBuilder};
pub use errors::{ServerError, ShutdownResult};
pub use handle::ServerHandle;
pub use lifecycle::LifecycleState;
pub use service::{
    service_fn, service_fn_head, service_fn_with_policy, Service, ServiceError, ServiceFn,
};
pub use static_service::{StaticService, StaticServiceBuilder};

use std::sync::Arc;

use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::broadcast;

use crate::config::{ServeConfig, ServeState};
use crate::server::lifecycle::Lifecycle;

/// A reusable HTTP runtime server.
///
/// This type is experimental and its API may change without notice.
///
/// The server binds a TCP listener, accepts connections, and dispatches them
/// to a [`Service`] implementation. It owns the full connection lifecycle:
/// parsing, normalization, timeouts, connection tracking, and graceful shutdown.
///
/// # Example
///
/// ```ignore
/// use eggserve_core::server::{Server, RuntimeConfig, service_fn, Request};
/// use eggserve_core::primitives::canonical::{Response, StatusCode, ResponseBody};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let server = Server::builder()
///     .runtime(RuntimeConfig::builder()
///         .bind("127.0.0.1:8000".parse().unwrap())
///         .build()?)
///     .build()?;
///
/// let handle = server.start_with_service(service_fn(|_req: Request| async {
///     Ok(Response::builder()
///         .status(StatusCode::OK)
///         .body(ResponseBody::Bytes(b"hello".to_vec()))
///         .unwrap())
/// })).await?;
/// handle.ready().await?;
/// println!("listening on {}", handle.local_addr());
///
/// // ... serve requests ...
///
/// handle.shutdown().await?;
/// handle.wait().await?;
/// # Ok(())
/// # }
/// ```
pub struct Server {
    config: RuntimeConfig,
    serve_config: Arc<ServeConfig>,
    lifecycle: Arc<Lifecycle>,
    listener_source: Option<ListenerSource>,
}

/// Source for the TCP listener.
#[derive(Debug)]
enum ListenerSource {
    /// Bind to this address on start.
    Bind(std::net::SocketAddr),
    /// Use this pre-bound listener.
    Listener(TcpListener),
}

impl Server {
    /// Create a new server builder with default configuration.
    pub fn builder() -> ServerBuilder {
        ServerBuilder {
            runtime_config: None,
            serve_config: None,
            listener_source: None,
        }
    }
}

/// Builder for constructing a [`Server`].
///
/// This type is experimental and its API may change without notice.
///
/// # Example
///
/// ```ignore
/// use eggserve_core::server::{Server, RuntimeConfig, StaticService};
///
/// let server = Server::builder()
///     .runtime(RuntimeConfig::default())
///     .static_service("/var/www")
///     .build()?;
/// ```
#[derive(Debug)]
#[must_use]
pub struct ServerBuilder {
    runtime_config: Option<RuntimeConfig>,
    serve_config: Option<Arc<ServeConfig>>,
    listener_source: Option<ListenerSource>,
}

impl ServerBuilder {
    /// Set the runtime configuration.
    pub fn runtime(mut self, config: RuntimeConfig) -> Self {
        self.runtime_config = Some(config);
        self
    }

    /// Set a pre-built serve configuration.
    ///
    /// This bridges the CLI/Python configuration model. The runtime config
    /// is derived from the serve config's limits and bind address.
    pub fn serve_config(mut self, config: Arc<ServeConfig>) -> Self {
        self.serve_config = Some(config);
        self
    }

    /// Set the bind address for the listener.
    ///
    /// This overrides the bind address from `RuntimeConfig`. The server will
    /// bind to this address when `start()` is called.
    pub fn bind(mut self, addr: std::net::SocketAddr) -> Self {
        self.listener_source = Some(ListenerSource::Bind(addr));
        self
    }

    /// Use a pre-bound TCP listener instead of binding on start.
    ///
    /// The listener must already be bound to an address. The runtime will
    /// take ownership of the listener after a successful `start()`.
    ///
    /// # Blocking/nonblocking
    ///
    /// The listener should be in nonblocking mode (as returned by
    /// [`TcpListener::bind`] and [`TcpListener::from_std`]).
    /// The runtime will normalize to nonblocking if needed.
    ///
    /// # Ownership
    ///
    /// After `start()`, the runtime owns the listener. The caller must not
    /// use the listener after passing it to the builder.
    pub fn from_listener(mut self, listener: TcpListener) -> Self {
        self.listener_source = Some(ListenerSource::Listener(listener));
        self
    }

    /// Build the server with the built-in static file service.
    ///
    /// Creates a [`StaticService`] from the serve configuration's root and
    /// policy. The serve config must have been set via [`ServerBuilder::serve_config`].
    pub fn build(self) -> Result<Server, ServerError> {
        let serve_config = self.serve_config.ok_or_else(|| {
            ServerError::Config("serve configuration required for static service".into())
        })?;
        let config = self
            .runtime_config
            .unwrap_or_else(|| RuntimeConfig::from(&*serve_config));
        Ok(Server {
            config,
            serve_config,
            lifecycle: Arc::new(Lifecycle::new()),
            listener_source: self.listener_source,
        })
    }

    /// Build the server with a static service rooted at the given path.
    ///
    /// Convenience method that creates both the serve config and runtime config.
    pub fn static_service(self, root: impl AsRef<std::path::Path>) -> Result<Server, ServerError> {
        let serve_config = Arc::new(ServeConfig {
            root: root.as_ref().to_path_buf(),
            ..ServeConfig::default()
        });
        let config = self
            .runtime_config
            .unwrap_or_else(|| RuntimeConfig::from(&*serve_config));
        Ok(Server {
            config,
            serve_config,
            lifecycle: Arc::new(Lifecycle::new()),
            listener_source: self.listener_source,
        })
    }
}

impl Server {
    /// Start the server with the built-in static file service.
    ///
    /// Binds the TCP listener (or uses the provided pre-bound listener) and
    /// begins accepting connections. Returns a [`ServerHandle`] for
    /// controlling the running server.
    pub async fn start(self) -> Result<ServerHandle, ServerError> {
        self.lifecycle.start()?;

        let listener = match self.listener_source {
            Some(ListenerSource::Listener(l)) => l,
            Some(ListenerSource::Bind(addr)) => {
                TcpListener::bind(addr).await.map_err(ServerError::Bind)?
            }
            None => TcpListener::bind(self.config.bind)
                .await
                .map_err(ServerError::Bind)?,
        };

        let local_addr = listener.local_addr().map_err(ServerError::Bind)?;

        let state = Arc::new(
            ServeState::new(self.serve_config).map_err(|e| ServerError::Config(e.to_string()))?,
        );

        crate::ops::Logger::global().emit(crate::ops::Event::new(
            crate::ops::Severity::Info,
            crate::ops::EventKind::RootInitialized,
            "root initialized",
        ));

        let config = Arc::new(self.config);
        let connection_semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_connections));

        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let shutdown_tx_clone = shutdown_tx.clone();
        let lifecycle = self.lifecycle.clone();

        let join = tokio::spawn({
            let lifecycle = lifecycle.clone();
            async move {
                accept_loop(
                    listener,
                    config,
                    state,
                    connection_semaphore,
                    shutdown_rx,
                    lifecycle,
                )
                .await
            }
        });

        Ok(ServerHandle::new(
            local_addr,
            shutdown_tx_clone,
            join,
            lifecycle,
        ))
    }

    /// Start the server with a custom service.
    pub async fn start_with_service<S: Service>(
        self,
        service: S,
    ) -> Result<ServerHandle, ServerError> {
        self.lifecycle.start()?;

        let listener = match self.listener_source {
            Some(ListenerSource::Listener(l)) => l,
            Some(ListenerSource::Bind(addr)) => {
                TcpListener::bind(addr).await.map_err(ServerError::Bind)?
            }
            None => TcpListener::bind(self.config.bind)
                .await
                .map_err(ServerError::Bind)?,
        };

        let local_addr = listener.local_addr().map_err(ServerError::Bind)?;

        let state = Arc::new(
            ServeState::new(self.serve_config).map_err(|e| ServerError::Config(e.to_string()))?,
        );

        crate::ops::Logger::global().emit(crate::ops::Event::new(
            crate::ops::Severity::Info,
            crate::ops::EventKind::RootInitialized,
            "root initialized",
        ));
        let config = Arc::new(self.config);
        let connection_semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_connections));

        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let shutdown_tx_clone = shutdown_tx.clone();
        let lifecycle = self.lifecycle.clone();

        let join = tokio::spawn({
            let lifecycle = lifecycle.clone();
            async move {
                accept_loop_with_service(
                    listener,
                    config,
                    state,
                    connection_semaphore,
                    service,
                    shutdown_rx,
                    lifecycle,
                )
                .await
            }
        });

        Ok(ServerHandle::new(
            local_addr,
            shutdown_tx_clone,
            join,
            lifecycle,
        ))
    }
}

/// Accept loop for the built-in static file service.
///
/// Tracks spawned connection tasks for graceful drain and cleanup.
async fn accept_loop(
    listener: TcpListener,
    config: Arc<RuntimeConfig>,
    state: Arc<ServeState>,
    connection_semaphore: Arc<tokio::sync::Semaphore>,
    mut shutdown_rx: broadcast::Receiver<()>,
    lifecycle: Arc<Lifecycle>,
) -> ShutdownResult {
    // Signal that we're running (listener bound, accept loop about to poll).
    if lifecycle.mark_running().is_err() {
        return ShutdownResult::Clean;
    }

    crate::ops::Logger::global().emit(crate::ops::Event::new(
        crate::ops::Severity::Info,
        crate::ops::EventKind::ListenerReady,
        "accept loop started",
    ));

    let correlation = crate::ops::CorrelationId::new();
    let counters = crate::ops::global_counters();

    // Track spawned connection tasks for graceful drain.
    let mut tasks = tokio::task::JoinSet::new();
    let mut backoff_idx: usize = 0;
    let mut error_repeat_count: usize = 0;
    let mut last_error_kind: Option<String> = None;

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer_addr)) => {
                        backoff_idx = 0;
                        error_repeat_count = 0;
                        last_error_kind = None;
                        let conn_id = correlation.next();
                        counters.connections_accepted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        counters.active_connections.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                        crate::ops::Logger::global().emit(
                            crate::ops::Event::new(
                                crate::ops::Severity::Debug,
                                crate::ops::EventKind::ConnectionAccepted,
                                "connection accepted",
                            )
                            .connection_id(conn_id),
                        );

                        let permit = match connection_semaphore.clone().try_acquire_owned() {
                            Ok(p) => p,
                            Err(_) => {
                                counters.connections_rejected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                counters.active_connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                crate::ops::Logger::global().emit(
                                    crate::ops::Event::new(
                                        crate::ops::Severity::Debug,
                                        crate::ops::EventKind::ConnectionRejected,
                                        "connection rejected: admission limit",
                                    )
                                    .connection_id(conn_id),
                                );
                                drop(stream);
                                continue;
                            }
                        };

                        let mut shutdown_rx = shutdown_rx.resubscribe();
                        let state = state.clone();
                        let config = config.clone();
                        let remote_addr = peer_addr;
                        let local_addr = stream.local_addr().unwrap_or(config.bind);

                        tasks.spawn(async move {
                            let _permit = permit;

                            #[cfg(feature = "tls")]
                            {
                                if let Some(tls_config) = &config.tls_config {
                                    let tls_acceptor = tokio_rustls::TlsAcceptor::from(tls_config.clone());
                                    match accept_tls(stream, &tls_acceptor, config.header_read_timeout, conn_id).await {
                                        Some((tls_stream, tls_info)) => {
                                            crate::ops::Logger::global().emit(
                                                crate::ops::Event::new(
                                                    crate::ops::Severity::Debug,
                                                    crate::ops::EventKind::TlsHandshakeSuccess,
                                                    "TLS handshake completed",
                                                )
                                                .connection_id(conn_id),
                                            );
                                            let io = TokioIo::new(tls_stream);
                                            let tls_info = std::sync::Arc::new(Some(tls_info));
                                            let svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                                let state = state.clone();
                                                let tls_info = tls_info.clone();
                                                let local_addr = local_addr;
                                                let remote_addr = remote_addr;
                                                async move {
                                                    Ok::<_, std::convert::Infallible>(
                                                        crate::service::handle_request_with_metadata(req, &state, local_addr, remote_addr, (*tls_info).clone()).await,
                                                    )
                                                }
                                            });
                                            connection::serve_connection(io, svc, &config, &mut shutdown_rx, conn_id).await;
                                            counters.active_connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                            return;
                                        }
                                        None => {
                                            counters.active_connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                            return;
                                        }
                                    }
                                }
                            }

                            let io = TokioIo::new(stream);
                            let svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                let state = state.clone();
                                let local_addr = local_addr;
                                let remote_addr = remote_addr;
                                async move {
                                    Ok::<_, std::convert::Infallible>(
                                        crate::service::handle_request_with_metadata(req, &state, local_addr, remote_addr, None).await,
                                    )
                                }
                            });
                            connection::serve_connection(io, svc, &config, &mut shutdown_rx, conn_id).await;
                            counters.active_connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                        });
                    }
                    Err(e) => {
                        let fatal = classify_accept_error(&e, &mut shutdown_rx, &mut backoff_idx, &mut error_repeat_count, &mut last_error_kind).await;
                        if fatal {
                            break;
                        }
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        }
    }

    crate::ops::Logger::global().emit(crate::ops::Event::new(
        crate::ops::Severity::Info,
        crate::ops::EventKind::ShutdownRequested,
        "shutdown requested",
    ));

    // Transition to Draining.
    let _ = lifecycle.drain();

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
                        crate::ops::Logger::global().emit(crate::ops::Event::new(
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
        crate::ops::Logger::global().emit(crate::ops::Event::new(
            crate::ops::Severity::Warn,
            crate::ops::EventKind::ForcedShutdownStarted,
            "grace deadline exceeded, aborting remaining tasks",
        ));
        tasks.abort_all();
        while let Some(result) = tasks.join_next().await {
            abort_count += 1;
            if let Err(e) = result {
                if e.is_panic() {
                    crate::ops::Logger::global().emit(crate::ops::Event::new(
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
        counters
            .forced_shutdowns
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ShutdownResult::Timeout
    } else {
        counters
            .graceful_shutdowns
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ShutdownResult::Clean
    };

    crate::ops::Logger::global().emit(crate::ops::Event::new(
        crate::ops::Severity::Info,
        crate::ops::EventKind::ShutdownComplete,
        format!("shutdown complete: {:?} (aborted={})", result, abort_count),
    ));

    result
}

/// Accept loop with a custom service.
async fn accept_loop_with_service<S: Service>(
    listener: TcpListener,
    config: Arc<RuntimeConfig>,
    state: Arc<ServeState>,
    connection_semaphore: Arc<tokio::sync::Semaphore>,
    service: S,
    mut shutdown_rx: broadcast::Receiver<()>,
    lifecycle: Arc<Lifecycle>,
) -> ShutdownResult {
    let service = Arc::new(service);

    // Signal that we're running.
    if lifecycle.mark_running().is_err() {
        return ShutdownResult::Clean;
    }

    crate::ops::Logger::global().emit(crate::ops::Event::new(
        crate::ops::Severity::Info,
        crate::ops::EventKind::ListenerReady,
        "accept loop started",
    ));

    let correlation = crate::ops::CorrelationId::new();
    let counters = crate::ops::global_counters();

    let mut tasks = tokio::task::JoinSet::new();
    let mut backoff_idx: usize = 0;
    let mut error_repeat_count: usize = 0;
    let mut last_error_kind: Option<String> = None;

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer_addr)) => {
                        backoff_idx = 0;
                        error_repeat_count = 0;
                        last_error_kind = None;
                        let conn_id = correlation.next();
                        counters.connections_accepted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        counters.active_connections.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                        crate::ops::Logger::global().emit(
                            crate::ops::Event::new(
                                crate::ops::Severity::Debug,
                                crate::ops::EventKind::ConnectionAccepted,
                                "connection accepted",
                            )
                            .connection_id(conn_id),
                        );

                        let permit = match connection_semaphore.clone().try_acquire_owned() {
                            Ok(p) => p,
                            Err(_) => {
                                counters.connections_rejected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                counters.active_connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                crate::ops::Logger::global().emit(
                                    crate::ops::Event::new(
                                        crate::ops::Severity::Debug,
                                        crate::ops::EventKind::ConnectionRejected,
                                        "connection rejected: admission limit",
                                    )
                                    .connection_id(conn_id),
                                );
                                drop(stream);
                                continue;
                            }
                        };

                        let mut shutdown_rx = shutdown_rx.resubscribe();
                        let state = state.clone();
                        let config = config.clone();
                        let service = service.clone();
                        let remote_addr = peer_addr;
                        let local_addr_pre_tls = stream.local_addr().unwrap_or(config.bind);

                        tasks.spawn(async move {
                            let _permit = permit;

                            #[cfg(feature = "tls")]
                            {
                                if let Some(tls_config) = &config.tls_config {
                                    let tls_acceptor = tokio_rustls::TlsAcceptor::from(tls_config.clone());
                                    match accept_tls(stream, &tls_acceptor, config.header_read_timeout, conn_id).await {
                                        Some((tls_stream, tls_info)) => {
                                            crate::ops::Logger::global().emit(
                                                crate::ops::Event::new(
                                                    crate::ops::Severity::Debug,
                                                    crate::ops::EventKind::TlsHandshakeSuccess,
                                                    "TLS handshake completed",
                                                )
                                                .connection_id(conn_id),
                                            );
                                            let io = TokioIo::new(tls_stream);
                                            connection::serve_connection_with_service(
                                                io,
                                                ArcService(service),
                                                &config,
                                                &state,
                                                &mut shutdown_rx,
                                                conn_id,
                                                local_addr_pre_tls,
                                                remote_addr,
                                                true,
                                                Some(tls_info),
                                            ).await;
                                            counters.active_connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                            return;
                                        }
                                        None => {
                                            counters.active_connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                            return;
                                        }
                                    }
                                }
                            }

                            let io = TokioIo::new(stream);
                            connection::serve_connection_with_service(
                                io,
                                ArcService(service),
                                &config,
                                &state,
                                &mut shutdown_rx,
                                conn_id,
                                local_addr_pre_tls,
                                remote_addr,
                                false,
                                None,
                            ).await;
                            counters.active_connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                        });
                    }
                    Err(e) => {
                        let fatal = classify_accept_error(&e, &mut shutdown_rx, &mut backoff_idx, &mut error_repeat_count, &mut last_error_kind).await;
                        if fatal {
                            break;
                        }
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        }
    }

    crate::ops::Logger::global().emit(crate::ops::Event::new(
        crate::ops::Severity::Info,
        crate::ops::EventKind::ShutdownRequested,
        "shutdown requested",
    ));

    let _ = lifecycle.drain();

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
                        crate::ops::Logger::global().emit(crate::ops::Event::new(
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
        crate::ops::Logger::global().emit(crate::ops::Event::new(
            crate::ops::Severity::Warn,
            crate::ops::EventKind::ForcedShutdownStarted,
            "grace deadline exceeded, aborting remaining tasks",
        ));
        tasks.abort_all();
        while let Some(result) = tasks.join_next().await {
            abort_count += 1;
            if let Err(e) = result {
                if e.is_panic() {
                    crate::ops::Logger::global().emit(crate::ops::Event::new(
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
        counters
            .forced_shutdowns
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ShutdownResult::Timeout
    } else {
        counters
            .graceful_shutdowns
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ShutdownResult::Clean
    };

    crate::ops::Logger::global().emit(crate::ops::Event::new(
        crate::ops::Severity::Info,
        crate::ops::EventKind::ShutdownComplete,
        format!("shutdown complete: {:?} (aborted={})", result, abort_count),
    ));

    result
}

/// Accept a TLS connection with timeout.
///
/// Returns the TLS stream and TLS session metadata on success, or `None` if
/// the handshake failed or timed out. Emits `TlsHandshakeFailure` or
/// `TlsHandshakeTimeout` events on failure.
#[cfg(feature = "tls")]
async fn accept_tls(
    stream: tokio::net::TcpStream,
    tls_acceptor: &tokio_rustls::TlsAcceptor,
    timeout: std::time::Duration,
    conn_id: u64,
) -> Option<(
    tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    crate::primitives::connection_info::TlsInfo,
)> {
    match tokio::time::timeout(timeout, tls_acceptor.accept(stream)).await {
        Ok(Ok(tls_stream)) => {
            let tls_info = extract_tls_info(&tls_stream);
            Some((tls_stream, tls_info))
        }
        Ok(Err(_)) => {
            crate::ops::Logger::global().emit(
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
            crate::ops::Logger::global().emit(
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

/// Extract TLS session metadata from a completed TLS stream.
#[cfg(feature = "tls")]
fn extract_tls_info(
    tls_stream: &tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
) -> crate::primitives::connection_info::TlsInfo {
    use crate::primitives::connection_info::TlsInfo;

    let (_io, conn) = tls_stream.get_ref();
    let protocol_version = conn.protocol_version().map(|v| format!("{v:?}"));
    let server_name = conn.server_name().map(|n| n.to_owned());
    TlsInfo {
        protocol_version,
        server_name,
    }
}

/// Classify an accept loop error, emit a structured log event, and apply
/// bounded exponential backoff for transient errors. The backoff is
/// interruptible by shutdown via the provided receiver.
///
/// Rate-limits repeated identical errors: emits the first occurrence, then
/// a summary every 10 consecutive identical errors, resetting on success
/// or a different error kind.
///
/// Returns `true` if the error is fatal and the accept loop should terminate.
#[allow(clippy::collapsible_match)]
async fn classify_accept_error(
    e: &std::io::Error,
    shutdown_rx: &mut broadcast::Receiver<()>,
    backoff_idx: &mut usize,
    error_repeat_count: &mut usize,
    last_error_kind: &mut Option<String>,
) -> bool {
    use crate::ops::{Event, EventKind, Logger, Severity};

    let err_str = e.to_string();
    let kind = e.kind();

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
        std::io::ErrorKind::OutOfMemory => {
            if err_str.contains("too many open files")
                || err_str.contains("EMFILE")
                || err_str.contains("ENFILE")
            {
                (Severity::Error, EventKind::ResourceExhaustion, true, false)
            } else {
                (
                    Severity::Error,
                    EventKind::ListenerPersistentError,
                    false,
                    true,
                )
            }
        }
        std::io::ErrorKind::Other => {
            if err_str.contains("too many open files")
                || err_str.contains("EMFILE")
                || err_str.contains("ENFILE")
            {
                (Severity::Error, EventKind::ResourceExhaustion, true, false)
            } else {
                (
                    Severity::Error,
                    EventKind::ListenerPersistentError,
                    false,
                    true,
                )
            }
        }
        _ => (
            Severity::Error,
            EventKind::ListenerPersistentError,
            false,
            true,
        ),
    };

    crate::ops::global_counters()
        .listener_errors
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // Rate-limit repeated identical errors.
    let current_kind = format!("{}", event_kind);
    let is_same_kind = last_error_kind.as_deref() == Some(&current_kind);
    if is_same_kind {
        *error_repeat_count += 1;
    } else {
        *error_repeat_count = 1;
        *last_error_kind = Some(current_kind);
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
        Logger::global().emit(Event::new(severity, event_kind, message).field(
            crate::ops::Field::Str("error_kind".into(), format!("{:?}", kind)),
        ));
    }

    if should_backoff {
        static BACKOFF_MS: [u64; 5] = [1, 2, 4, 8, 50];
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

/// Wrapper to implement `Service` for `Arc<S>`.
struct ArcService<S>(Arc<S>);

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
