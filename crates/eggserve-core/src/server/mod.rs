//! Reusable HTTP runtime and service boundary.
//!
//! This module provides a transport-owning HTTP runtime that downstream Rust
//! projects can embed without importing internal modules or depending directly
//! on Hyper.
//!
//! # Architecture
//!
//! ```text
//! let server = Server::builder()
//!     .runtime(RuntimeConfig::default())
//!     .build()?;
//! let handle = server.start_with_service(my_service).await?;
//!
//! handle.ready().await?;
//! // server is accepting connections
//!
//! handle.shutdown();
//! // server drains and stops
//! handle.wait().await?;
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
pub use config::{try_from_serve_config, RuntimeConfig, RuntimeConfigBuilder};
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

use crate::config::ServeConfig;
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
/// ```no_run
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
/// handle.shutdown();
/// handle.wait().await?;
/// # Ok(())
/// # }
/// ```
pub struct Server {
    config: RuntimeConfig,
    builtin_static_service: Option<StaticService>,
    lifecycle: Arc<Lifecycle>,
    listener_source: Option<ListenerSource>,
}

/// Transport state shared by every connection in one running server.
///
/// In particular, file-stream admission is created once here and cloned into
/// connection tasks. Static services never own or acquire this semaphore.
#[derive(Debug)]
pub struct RuntimeState {
    pub(crate) file_stream_semaphore: Arc<tokio::sync::Semaphore>,
}

impl RuntimeState {
    pub(crate) fn new(config: &RuntimeConfig) -> Self {
        Self {
            file_stream_semaphore: Arc::new(tokio::sync::Semaphore::new(config.max_file_streams)),
        }
    }

    /// Construct an explicit admission context for legacy adapter migration
    /// and low-level tests. Running servers must obtain their context from
    /// [`Server::start`] or [`Server::start_with_service`].
    #[doc(hidden)]
    pub fn new_for_testing(max_file_streams: usize) -> Self {
        Self {
            file_stream_semaphore: Arc::new(tokio::sync::Semaphore::new(max_file_streams)),
        }
    }

    /// Return the server-wide file-stream admission pool.
    pub fn file_stream_semaphore(&self) -> &Arc<tokio::sync::Semaphore> {
        &self.file_stream_semaphore
    }
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
/// ```no_run
/// use eggserve_core::server::{RuntimeConfig, Server};
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
///
/// let server = Server::builder()
///     .runtime(RuntimeConfig::default())
///     .static_service("/var/www")?;
/// # Ok(())
/// # }
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

    /// Build the server, eagerly constructing the built-in static file service
    /// when a serve configuration was supplied.
    ///
    /// Invalid static roots therefore fail during `build()`, before listener
    /// preparation or startup. The serve config must have been set via
    /// [`ServerBuilder::serve_config`] for [`Server::start`] to be available.
    pub fn build(self) -> Result<Server, ServerError> {
        let serve_config = self.serve_config;
        let config = match self.runtime_config {
            Some(c) => c,
            None => match &serve_config {
                Some(sc) => config::try_from_serve_config(sc)?,
                None => {
                    return Err(ServerError::Config(
                        "runtime configuration or serve configuration required".into(),
                    ))
                }
            },
        };
        let builtin_static_service = serve_config
            .map(StaticService::from_serve_config)
            .transpose()
            .map_err(|e| ServerError::Config(e.to_string()))?;
        Ok(Server {
            config,
            builtin_static_service,
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
        let config = match self.runtime_config {
            Some(c) => c,
            None => config::try_from_serve_config(&serve_config)?,
        };
        let builtin_static_service = StaticService::from_serve_config(serve_config)
            .map_err(|e| ServerError::Config(e.to_string()))?;
        Ok(Server {
            config,
            builtin_static_service: Some(builtin_static_service),
            lifecycle: Arc::new(Lifecycle::new()),
            listener_source: self.listener_source,
        })
    }
}

impl Server {
    /// Start the server with the built-in static file service.
    ///
    /// Starts the statically constructed service using the shared generic
    /// accept loop. The serve config must have been set via
    /// [`ServerBuilder::serve_config`].
    pub async fn start(self) -> Result<ServerHandle, ServerError> {
        let Server {
            config,
            builtin_static_service,
            lifecycle,
            listener_source,
        } = self;
        let service = builtin_static_service.ok_or_else(|| {
            ServerError::Config("serve configuration required for static service".into())
        })?;

        Server {
            config,
            builtin_static_service: None,
            lifecycle,
            listener_source,
        }
        .start_with_service(service)
        .await
    }

    /// Start the server with a custom service.
    ///
    /// The custom service does not require a static root or serve configuration.
    /// The runtime creates only transport state (semaphores, lifecycle) and
    /// passes it to the accept loop and connection pipeline.
    pub async fn start_with_service<S: Service>(
        self,
        service: S,
    ) -> Result<ServerHandle, ServerError> {
        let Server {
            config: runtime_config,
            builtin_static_service: _,
            lifecycle,
            listener_source,
        } = self;
        lifecycle.start()?;

        let listener = match listener_source {
            Some(ListenerSource::Listener(l)) => l,
            Some(ListenerSource::Bind(addr)) => {
                TcpListener::bind(addr).await.map_err(ServerError::Bind)?
            }
            None => TcpListener::bind(runtime_config.bind)
                .await
                .map_err(ServerError::Bind)?,
        };

        let local_addr = listener.local_addr().map_err(ServerError::Bind)?;

        let config = Arc::new(runtime_config);
        let connection_semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_connections));
        let runtime_state = Arc::new(RuntimeState::new(&config));

        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let shutdown_tx_clone = shutdown_tx.clone();
        let lifecycle = lifecycle.clone();

        let join = tokio::spawn({
            let lifecycle = lifecycle.clone();
            async move {
                accept_loop_generic(
                    listener,
                    local_addr,
                    config,
                    runtime_state,
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

/// Unified accept loop for both static and custom services.
///
#[allow(clippy::too_many_arguments)]
async fn accept_loop_generic<S: Service>(
    listener: TcpListener,
    local_addr: std::net::SocketAddr,
    config: Arc<RuntimeConfig>,
    runtime_state: Arc<RuntimeState>,
    connection_semaphore: Arc<tokio::sync::Semaphore>,
    service: S,
    mut shutdown_rx: broadcast::Receiver<()>,
    lifecycle: Arc<Lifecycle>,
) -> ShutdownResult {
    let service = Arc::new(service);

    // Signal that we're running (listener bound, accept loop about to poll).
    if lifecycle.mark_running().is_err() {
        let _ = lifecycle.mark_failed();
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
                        let _ = stream.set_nodelay(true);
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
                        let runtime_state = runtime_state.clone();
                        let config = config.clone();
                        let service = service.clone();
                        let remote_addr = peer_addr;
                        let local_addr_pre_tls = stream.local_addr().unwrap_or(local_addr);

                        tasks.spawn(async move {
                            let _permit = permit;
                            let _active_connection = ActiveConnectionGuard;

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
                                            connection::serve_connection_with_runtime_state(
                                                io,
                                                ArcService(service),
                                                &config,
                                                runtime_state.clone(),
                                                &mut shutdown_rx,
                                                conn_id,
                                                local_addr_pre_tls,
                                                remote_addr,
                                                true,
                                                Some(tls_info),
                                            ).await;
                                            return;
                                        }
                                        None => {
                                            return;
                                        }
                                    }
                                }
                            }

                            let io = TokioIo::new(stream);
                            connection::serve_connection_with_runtime_state(
                                io,
                                ArcService(service),
                                &config,
                                runtime_state.clone(),
                                &mut shutdown_rx,
                                conn_id,
                                local_addr_pre_tls,
                                remote_addr,
                                false,
                                None,
                            ).await;
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
                        counters
                            .connection_panics
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
                    counters
                        .connection_panics
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        std::io::ErrorKind::OutOfMemory | std::io::ErrorKind::Other if fd_exhausted => {
            (Severity::Error, EventKind::ResourceExhaustion, true, false)
        }
        std::io::ErrorKind::OutOfMemory | std::io::ErrorKind::Other => (
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

fn is_fd_exhaustion(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    if let Some(raw) = error.raw_os_error() {
        return raw == rustix::io::Errno::MFILE.raw_os_error().abs()
            || raw == rustix::io::Errno::NFILE.raw_os_error().abs();
    }

    if error.raw_os_error().is_some() {
        return false;
    }

    let message = error.to_string().to_ascii_lowercase();
    message.contains("too many open files")
        || message.contains("emfile")
        || message.contains("enfile")
}

struct ActiveConnectionGuard;

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        crate::ops::global_counters()
            .active_connections
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn classify_accept_error_uses_os_error_for_fd_exhaustion() {
        let error = std::io::Error::from_raw_os_error(libc::EMFILE);
        let (tx, mut rx) = broadcast::channel(1);
        let mut backoff = 0;
        let mut repeats = 0;
        let mut last = None;
        assert!(
            !classify_accept_error(&error, &mut rx, &mut backoff, &mut repeats, &mut last,).await
        );
        let _ = tx.send(());
    }
}
