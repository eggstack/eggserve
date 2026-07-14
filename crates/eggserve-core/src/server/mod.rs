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
//! ```
//!
//! The runtime owns:
//! - Listener acceptance
//! - HTTP/1 parsing
//! - Request conversion to canonical types
//! - Response normalization
//! - Timeout enforcement
//! - Connection and file-stream permits
//! - Graceful shutdown
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

pub mod config;
pub mod connection;
pub mod errors;
pub mod handle;
pub mod service;
pub mod static_service;

pub use config::{RuntimeConfig, RuntimeConfigBuilder};
pub use errors::ServerError;
pub use handle::ServerHandle;
pub use service::{service_fn, Service, ServiceError, ServiceFn};
pub use static_service::{StaticService, StaticServiceBuilder};

use std::sync::Arc;

use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::broadcast;

use crate::config::{ServeConfig, ServeState};

/// A reusable HTTP runtime server.
///
/// The server binds a TCP listener, accepts connections, and dispatches them
/// to a [`Service`] implementation. It owns the full connection lifecycle:
/// parsing, normalization, timeouts, and graceful shutdown.
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
/// println!("listening on {}", handle.local_addr());
/// handle.wait().await?;
/// # Ok(())
/// # }
/// ```
pub struct Server {
    config: RuntimeConfig,
    serve_config: Arc<ServeConfig>,
}

impl Server {
    /// Create a new server builder with default configuration.
    pub fn builder() -> ServerBuilder {
        ServerBuilder {
            runtime_config: None,
            serve_config: None,
        }
    }
}

/// Builder for constructing a [`Server`].
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
        })
    }
}

impl Server {
    /// Start the server with the built-in static file service.
    ///
    /// Binds the TCP listener and begins accepting connections. Returns
    /// a [`ServerHandle`] for controlling the running server.
    pub async fn start(self) -> Result<ServerHandle, ServerError> {
        let listener = TcpListener::bind(self.config.bind)
            .await
            .map_err(ServerError::Bind)?;
        let local_addr = listener.local_addr().map_err(ServerError::Bind)?;

        let state = Arc::new(ServeState::new(self.serve_config));
        let config = Arc::new(self.config);
        let connection_semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_connections));

        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let shutdown_tx_clone = shutdown_tx.clone();

        let join = tokio::spawn(async move {
            accept_loop(listener, config, state, connection_semaphore, shutdown_rx).await;
        });

        Ok(ServerHandle::new(local_addr, shutdown_tx_clone, join))
    }

    /// Start the server with a custom service.
    pub async fn start_with_service<S: Service>(
        self,
        service: S,
    ) -> Result<ServerHandle, ServerError> {
        let listener = TcpListener::bind(self.config.bind)
            .await
            .map_err(ServerError::Bind)?;
        let local_addr = listener.local_addr().map_err(ServerError::Bind)?;

        let state = Arc::new(ServeState::new(self.serve_config));
        let config = Arc::new(self.config);
        let connection_semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_connections));

        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let shutdown_tx_clone = shutdown_tx.clone();

        let join = tokio::spawn(async move {
            accept_loop_with_service(
                listener,
                config,
                state,
                connection_semaphore,
                service,
                shutdown_rx,
            )
            .await;
        });

        Ok(ServerHandle::new(local_addr, shutdown_tx_clone, join))
    }
}

/// Accept loop for the built-in static file service.
async fn accept_loop(
    listener: TcpListener,
    config: Arc<RuntimeConfig>,
    state: Arc<ServeState>,
    connection_semaphore: Arc<tokio::sync::Semaphore>,
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
                        let config = config.clone();

                        tokio::spawn(async move {
                            let _permit = permit;
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
                    }
                    Err(_e) => {}
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        }
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
) {
    let service = Arc::new(service);
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

                        tokio::spawn(async move {
                            let _permit = permit;
                            let io = TokioIo::new(stream);
                            connection::serve_connection_with_service(
                                io,
                                ArcService(service),
                                &config,
                                &state,
                                &mut shutdown_rx,
                            ).await;
                        });
                    }
                    Err(_e) => {}
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        }
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
