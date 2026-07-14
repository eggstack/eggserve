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

pub use config::{RuntimeConfig, RuntimeConfigBuilder};
pub use errors::{ServerError, ShutdownResult};
pub use handle::ServerHandle;
pub use lifecycle::LifecycleState;
pub use service::{service_fn, Service, ServiceError, ServiceFn};
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
/// use eggserve_core::server::{Server, RuntimeConfig, service_fn};
/// use eggserve_core::primitives::canonical::{Response, StatusCode, ResponseBody};
/// use eggserve_core::primitives::request_head::RequestHead;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let server = Server::builder()
///     .runtime(RuntimeConfig::builder()
///         .bind("127.0.0.1:8000".parse().unwrap())
///         .build())
///     .service(service_fn(|_req: RequestHead| async {
///         Ok(Response::builder()
///             .status(StatusCode::OK)
///             .body(ResponseBody::Bytes(b"hello".to_vec()))
///             .unwrap())
///     }))
///     .build()?;
///
/// let handle = server.start().await?;
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

    /// Build the server with a custom service.
    ///
    /// Uses the provided service for request handling. The runtime config
    /// must have been set via [`ServerBuilder::runtime`].
    pub fn build_with_service<S: service::Service>(
        self,
        _service: S,
    ) -> Result<Server, ServerError> {
        let config = self
            .runtime_config
            .ok_or_else(|| ServerError::Config("runtime configuration required".into()))?;
        let serve_config = self.serve_config.unwrap_or_else(|| {
            Arc::new(ServeConfig {
                bind: config.bind,
                ..ServeConfig::default()
            })
        });
        Ok(Server {
            config,
            serve_config,
            lifecycle: Arc::new(Lifecycle::new()),
            listener_source: self.listener_source,
        })
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

        let state = Arc::new(ServeState::new(self.serve_config));
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

        let state = Arc::new(ServeState::new(self.serve_config));
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

    // Track spawned connection tasks for graceful drain.
    let mut tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();

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
                        let config = config.clone();

                        let join = tokio::spawn(async move {
                            let _permit = permit;

                            #[cfg(feature = "tls")]
                            {
                                if let Some(tls_config) = &config.tls_config {
                                    let tls_acceptor = tokio_rustls::TlsAcceptor::from(tls_config.clone());
                                    match accept_tls(stream, &tls_acceptor, config.header_read_timeout).await {
                                        Some(tls_stream) => {
                                            let io = TokioIo::new(tls_stream);
                                            let svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                                let state = state.clone();
                                                async move {
                                                    Ok::<_, std::convert::Infallible>(
                                                        crate::service::handle_request(req, &state).await,
                                                    )
                                                }
                                            });
                                            connection::serve_connection(io, svc, &config, &mut shutdown_rx).await;
                                            return;
                                        }
                                        None => return,
                                    }
                                }
                            }

                            let io = TokioIo::new(stream);
                            let svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                let state = state.clone();
                                async move {
                                    Ok::<_, std::convert::Infallible>(
                                        crate::service::handle_request(req, &state).await,
                                    )
                                }
                            });
                            connection::serve_connection(io, svc, &config, &mut shutdown_rx).await;
                        });
                        tasks.retain(|t| !t.is_finished());
                        tasks.push(join);
                    }
                    Err(_e) => {}
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        }
    }

    // Transition to Draining.
    let _ = lifecycle.drain();

    // Wait for in-flight connections to drain.
    let drain_timeout = config.graceful_shutdown_timeout;
    let deadline = tokio::time::Instant::now() + drain_timeout;
    let mut timed_out = false;

    while !tasks.is_empty() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            timed_out = true;
            break;
        }
        let mut task = tasks.pop().unwrap();
        if tokio::time::timeout(remaining, &mut task).await.is_err() {
            timed_out = true;
            task.abort();
        }
    }

    let _ = lifecycle.mark_stopped();

    if timed_out {
        ShutdownResult::Timeout
    } else {
        ShutdownResult::Clean
    }
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

    let mut tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();

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
                        let config = config.clone();
                        let service = service.clone();

                        let join = tokio::spawn(async move {
                            let _permit = permit;

                            #[cfg(feature = "tls")]
                            {
                                if let Some(tls_config) = &config.tls_config {
                                    let tls_acceptor = tokio_rustls::TlsAcceptor::from(tls_config.clone());
                                    match accept_tls(stream, &tls_acceptor, config.header_read_timeout).await {
                                        Some(tls_stream) => {
                                            let io = TokioIo::new(tls_stream);
                                            connection::serve_connection_with_service(
                                                io,
                                                ArcService(service),
                                                &config,
                                                &state,
                                                &mut shutdown_rx,
                                            ).await;
                                            return;
                                        }
                                        None => return,
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
                            ).await;
                        });
                        tasks.retain(|t| !t.is_finished());
                        tasks.push(join);
                    }
                    Err(_e) => {}
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        }
    }

    let _ = lifecycle.drain();

    let drain_timeout = config.graceful_shutdown_timeout;
    let deadline = tokio::time::Instant::now() + drain_timeout;
    let mut timed_out = false;

    while !tasks.is_empty() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            timed_out = true;
            break;
        }
        let mut task = tasks.pop().unwrap();
        if tokio::time::timeout(remaining, &mut task).await.is_err() {
            timed_out = true;
            task.abort();
        }
    }

    let _ = lifecycle.mark_stopped();

    if timed_out {
        ShutdownResult::Timeout
    } else {
        ShutdownResult::Clean
    }
}

/// Accept a TLS connection with timeout.
///
/// Returns the TLS stream on success, or `None` if the handshake failed or timed out.
#[cfg(feature = "tls")]
async fn accept_tls(
    stream: tokio::net::TcpStream,
    tls_acceptor: &tokio_rustls::TlsAcceptor,
    timeout: std::time::Duration,
) -> Option<tokio_rustls::server::TlsStream<tokio::net::TcpStream>> {
    match tokio::time::timeout(timeout, tls_acceptor.accept(stream)).await {
        Ok(Ok(tls_stream)) => Some(tls_stream),
        Ok(Err(_)) | Err(_) => None,
    }
}

/// Wrapper to implement `Service` for `Arc<S>`.
struct ArcService<S>(Arc<S>);

impl<S: Service> Service for ArcService<S> {
    fn call(
        &self,
        request: crate::primitives::request_head::RequestHead,
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
