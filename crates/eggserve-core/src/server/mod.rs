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
//! - Connection, file-stream, and in-flight-service permits
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
#[cfg(feature = "http3")]
mod http3;
pub mod lifecycle;
pub mod listener;
pub mod proxy;
pub mod response_policy;
pub mod service;
pub mod static_service;
#[cfg(feature = "tower")]
pub mod tower;

pub use crate::primitives::request::Request;
#[cfg(feature = "http2")]
pub use config::Http2Config;
#[cfg(feature = "http3")]
pub use config::Http3Config;
pub use config::{try_from_serve_config, RuntimeConfig, RuntimeConfigBuilder};
pub use connection::{
    serve_http1_connection, serve_http1_connection_with_id, ConnectionContext, ConnectionOutcome,
    ConnectionShutdown,
};
#[cfg(feature = "http2")]
pub use connection::{serve_http_connection, serve_http_connection_with_id};
pub use errors::{ServerError, ShutdownResult};
pub use handle::ServerHandle;
pub use lifecycle::LifecycleState;
pub use listener::BoundEndpoint;
#[cfg(unix)]
pub use listener::{clear_systemd_activation_env, systemd_activation_count};
pub use response_policy::{validate_stripped_header_name, DatePolicy, ResponsePolicy};
pub use service::{
    service_fn, service_fn_head, service_fn_with_policy, Service, ServiceError, ServiceFn,
};
pub use static_service::{StaticService, StaticServiceBuilder};
#[cfg(feature = "tower")]
pub use tower::{EggserveToTower, TowerAdapterError, TowerToEggserve};

#[cfg(feature = "http3")]
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    tcp_source: Option<TcpListenerSource>,
    #[cfg(unix)]
    unix_source: Option<UnixListenerSource>,
    ops: crate::ops::OpsContext,
    #[cfg(feature = "http3")]
    http3_identity: Option<(PathBuf, PathBuf)>,
    #[cfg(feature = "http3")]
    http3_socket: Option<std::net::UdpSocket>,
}

/// Transport state shared by every connection in one running server.
///
/// In particular, file-stream and in-flight-service admission pools are
/// created once here and cloned into connection tasks. Static services never
/// own or acquire these semaphores.
///
/// The state also owns the runtime's observability context
/// ([`crate::ops::OpsContext`]): connection correlation IDs, connection and
/// request events, and counters resolve through this context rather than the
/// process-global logger. [`RuntimeState::new`]/[`RuntimeState::try_new`]
/// clone the process-global default so existing CLI/default construction
/// keeps working; [`RuntimeState::with_ops`] attaches an explicit per-runtime
/// context for isolated embedding.
///
/// Callers driving caller-owned byte streams with
/// [`connection::serve_http1_connection`] must share one `RuntimeState`
/// across all of their connections rather than constructing one per
/// connection; otherwise file/response/service budgets become per-connection
/// instead of server-wide. Construct it with [`RuntimeState::new`] from the
/// same [`RuntimeConfig`] used for the connections. It owns only
/// transport-runtime admission (file-stream permits and in-flight service
/// permits); it never owns static filesystem state or
/// application routing state.
#[derive(Debug, Clone)]
pub struct RuntimeState {
    pub(crate) file_stream_semaphore: Arc<tokio::sync::Semaphore>,
    pub(crate) service_semaphore: Arc<tokio::sync::Semaphore>,
    pub(crate) tunnel_semaphore: Arc<tokio::sync::Semaphore>,
    ops: crate::ops::OpsContext,
}

impl RuntimeState {
    /// Create the shared admission context for a runtime configuration.
    ///
    /// Use the same [`RuntimeConfig`] that drives the connections so
    /// budgets cannot be accidentally omitted. Clone the resulting
    /// `Arc<RuntimeState>` into every
    /// [`connection::serve_http1_connection`] invocation.
    ///
    /// # Panics
    ///
    /// Panics with an actionable message when `config` fails
    /// [`RuntimeConfig::validate`]. Prefer [`RuntimeState::try_new`] when the
    /// configuration is hand-constructed or otherwise untrusted so the error
    /// is returned instead of panicking. Validation happens before any
    /// semaphore/Hyper construction so invalid values cannot trigger obscure
    /// downstream panics.
    pub fn new(config: &RuntimeConfig) -> Self {
        Self::try_new(config).expect("invalid RuntimeConfig for RuntimeState")
    }

    /// Validated constructor for the shared admission context (Plan 179 Track C).
    ///
    /// Returns [`crate::server::errors::ServerError::Config`] when a
    /// hand-constructed [`RuntimeConfig`] violates the shared runtime kernel,
    /// response policy, or semaphore bounds. Running servers obtain their
    /// context from [`Server::start`] or [`Server::start_with_service`],
    /// which validate before constructing permits.
    pub fn try_new(config: &RuntimeConfig) -> Result<Self, crate::server::errors::ServerError> {
        Self::with_ops(config, crate::ops::OpsContext::global().clone())
    }

    /// Validated constructor with an explicit observability context
    /// (Plan 181 Track C1).
    ///
    /// Same admission budgets as [`RuntimeState::try_new`], but connection
    /// correlation IDs, events, and counters resolve through `ops` instead
    /// of the process-global default. Share the resulting
    /// `Arc<RuntimeState>` across every
    /// [`connection::serve_http1_connection`](crate::server::connection::serve_http1_connection)
    /// invocation of the runtime.
    pub fn with_ops(
        config: &RuntimeConfig,
        ops: crate::ops::OpsContext,
    ) -> Result<Self, crate::server::errors::ServerError> {
        config.validate()?;
        Ok(Self {
            file_stream_semaphore: Arc::new(tokio::sync::Semaphore::new(config.max_file_streams)),
            service_semaphore: Arc::new(tokio::sync::Semaphore::new(config.max_in_flight_requests)),
            tunnel_semaphore: Arc::new(tokio::sync::Semaphore::new(config.max_active_tunnels)),
            ops,
        })
    }

    /// Construct an explicit admission context for legacy adapter migration
    /// and low-level tests. Running servers must obtain their context from
    /// [`Server::start`] or [`Server::start_with_service`].
    #[doc(hidden)]
    pub fn new_for_testing(max_file_streams: usize) -> Self {
        debug_assert!(
            max_file_streams <= tokio::sync::Semaphore::MAX_PERMITS,
            "new_for_testing: max_file_streams exceeds Semaphore::MAX_PERMITS"
        );
        Self {
            file_stream_semaphore: Arc::new(tokio::sync::Semaphore::new(max_file_streams)),
            service_semaphore: Arc::new(tokio::sync::Semaphore::new(
                crate::limits::DEFAULT_MAX_IN_FLIGHT_REQUESTS,
            )),
            tunnel_semaphore: Arc::new(tokio::sync::Semaphore::new(
                crate::runtime_limits::DEFAULT_MAX_ACTIVE_TUNNELS,
            )),
            ops: crate::ops::OpsContext::global().clone(),
        }
    }

    /// This runtime's observability context.
    ///
    /// Connection correlation IDs, events, and counters for every connection
    /// driven by this state resolve here. Cloning is cheap (shared inner).
    pub fn ops(&self) -> &crate::ops::OpsContext {
        &self.ops
    }

    /// Non-blocking, bounded snapshot of this runtime's counters (Plan 181
    /// Track E). Reads never reset; no exporter or endpoint is involved.
    pub fn ops_snapshot(&self) -> crate::ops::OpsSnapshot {
        self.ops.snapshot()
    }

    /// Return the server-wide file-stream admission pool.
    pub fn file_stream_semaphore(&self) -> &Arc<tokio::sync::Semaphore> {
        &self.file_stream_semaphore
    }

    /// Return the server-wide in-flight service admission pool.
    ///
    /// Bounds concurrent `Service::call()` executions independently of idle
    /// keep-alive connections.
    pub fn service_semaphore(&self) -> &Arc<tokio::sync::Semaphore> {
        &self.service_semaphore
    }

    /// Return the server-wide active-tunnel admission pool (Plan 199).
    ///
    /// Bounds concurrent accepted duplex tunnels independently of ordinary
    /// HTTP admission; exhaustion fails new handshakes with 503.
    pub fn tunnel_semaphore(&self) -> &Arc<tokio::sync::Semaphore> {
        &self.tunnel_semaphore
    }
}

/// Source for the TCP listener (Plan 201 Track B).
#[derive(Debug)]
enum TcpListenerSource {
    /// Bind to this address on start.
    Bind(std::net::SocketAddr),
    /// Use this pre-bound listener (no duplicate bind).
    Listener(TcpListener),
}

/// Source for the Unix-domain listener (Plan 201 Track C, Unix only).
#[cfg(unix)]
#[derive(Debug)]
enum UnixListenerSource {
    /// Use this pre-bound Unix listener. Filesystem path ownership stays
    /// with the caller; EggServe never unlinks.
    Listener(tokio::net::UnixListener),
}

impl Server {
    /// Create a new server builder with default configuration.
    pub fn builder() -> ServerBuilder {
        ServerBuilder {
            runtime_config: None,
            serve_config: None,
            tcp_source: None,
            #[cfg(unix)]
            unix_source: None,
            ops_context: None,
            #[cfg(feature = "http3")]
            http3_identity: None,
            #[cfg(feature = "http3")]
            http3_socket: None,
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
    tcp_source: Option<TcpListenerSource>,
    #[cfg(unix)]
    unix_source: Option<UnixListenerSource>,
    ops_context: Option<crate::ops::OpsContext>,
    #[cfg(feature = "http3")]
    http3_identity: Option<(PathBuf, PathBuf)>,
    #[cfg(feature = "http3")]
    http3_socket: Option<std::net::UdpSocket>,
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
        self.tcp_source = Some(TcpListenerSource::Bind(addr));
        self
    }

    /// Attach an explicit per-runtime observability context (Plan 181).
    ///
    /// Events, counters, sink-failure accounting, and connection correlation
    /// IDs for this server resolve through `ops` instead of the
    /// process-global default. When unset, the server clones
    /// [`crate::ops::OpsContext::global`], preserving zero-ceremony CLI and
    /// compatibility construction. This setter is experimental with the rest
    /// of the `server` module.
    pub fn ops_context(mut self, ops: crate::ops::OpsContext) -> Self {
        self.ops_context = Some(ops);
        self
    }

    /// Supply the certificate and private-key PEM paths used by the
    /// experimental HTTP/3 QUIC endpoint. QUIC builds a separate TLS 1.3
    /// configuration with the `h3` ALPN; the TCP rustls config is not reused.
    #[cfg(feature = "http3")]
    pub fn http3_identity(
        mut self,
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> Self {
        self.http3_identity = Some((
            cert_path.as_ref().to_path_buf(),
            key_path.as_ref().to_path_buf(),
        ));
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
        self.tcp_source = Some(TcpListenerSource::Listener(listener));
        self
    }

    /// Use a caller-bound standard-library TCP listener (Plan 201 Track B).
    ///
    /// Prefer this at process-manager boundaries to reduce Tokio coupling:
    /// nonblocking mode is normalized internally and all other socket
    /// options are preserved. No duplicate bind is performed; the listener's
    /// actual local address becomes server readiness metadata.
    ///
    /// Ownership transfers to the builder on success. Failed conversion
    /// drops (closes) the passed socket; do not reuse it after this call.
    pub fn from_std_listener(
        mut self,
        listener: std::net::TcpListener,
    ) -> Result<Self, ServerError> {
        let tokio_listener = crate::server::listener::normalize_std_tcp_listener(listener)
            .map_err(ServerError::Bind)?;
        self.tcp_source = Some(TcpListenerSource::Listener(tokio_listener));
        Ok(self)
    }

    /// Use a pre-bound Unix-domain listener (Plan 201 Track C, Unix only).
    ///
    /// The listener must already be bound. Ownership transfers to the
    /// runtime after a successful `start()`; filesystem socket-path creation
    /// and removal stay with the caller — EggServe never unlinks. Abstract
    /// namespace sockets need no cleanup. TLS configured for TCP is not
    /// implicitly enabled over Unix streams (Unix is plaintext); H3 is not
    /// available over Unix streams (QUIC/UDP only).
    #[cfg(unix)]
    pub fn from_unix_listener(mut self, listener: tokio::net::UnixListener) -> Self {
        self.unix_source = Some(UnixListenerSource::Listener(listener));
        self
    }

    /// Use a caller-bound standard-library Unix listener (Plan 201 Track C).
    ///
    /// Nonblocking mode is normalized; path ownership stays with the caller.
    /// Failed conversion drops (closes) the passed socket.
    #[cfg(unix)]
    pub fn from_std_unix_listener(
        mut self,
        listener: std::os::unix::net::UnixListener,
    ) -> Result<Self, ServerError> {
        let tokio_listener = crate::server::listener::normalize_std_unix_listener(listener)
            .map_err(ServerError::Bind)?;
        self.unix_source = Some(UnixListenerSource::Listener(tokio_listener));
        Ok(self)
    }

    /// Adopt the `index`-th systemd/socket-activation descriptor (Unix only).
    ///
    /// Reads `LISTEN_PID`/`LISTEN_FDS`, requires an explicit `index` (never
    /// silently takes fd 3), validates `SOCK_STREAM` type, listening state,
    /// and family (`AF_INET`/`AF_INET6` → TCP, `AF_UNIX` → Unix), and rejects
    /// datagram descriptors and connected sockets adopted as listeners.
    /// Ownership transfers on success; validation failure never closes the
    /// descriptor. No process supervision, notification, or unit management
    /// is added to core. Call
    /// [`crate::server::listener::clear_systemd_activation_env`] after
    /// adoption when spawning children that must not inherit activation
    /// state.
    #[cfg(unix)]
    pub fn from_systemd_index(mut self, index: usize) -> Result<Self, ServerError> {
        match crate::server::listener::adopt_systemd_listener(index)? {
            crate::server::listener::SystemdListener::Tcp(l) => {
                self.tcp_source = Some(TcpListenerSource::Listener(l));
            }
            crate::server::listener::SystemdListener::Unix(l) => {
                self.unix_source = Some(UnixListenerSource::Listener(l));
            }
        }
        Ok(self)
    }

    /// Adopt a systemd descriptor by `LISTEN_FDNAMES` entry (Unix only).
    ///
    /// Requires `LISTEN_FDNAMES` to map `name` explicitly; never guesses.
    /// Validation and ownership match [`ServerBuilder::from_systemd_index`].
    #[cfg(unix)]
    pub fn from_systemd_name(mut self, name: &str) -> Result<Self, ServerError> {
        match crate::server::listener::adopt_systemd_listener_by_name(name)? {
            crate::server::listener::SystemdListener::Tcp(l) => {
                self.tcp_source = Some(TcpListenerSource::Listener(l));
            }
            crate::server::listener::SystemdListener::Unix(l) => {
                self.unix_source = Some(UnixListenerSource::Listener(l));
            }
        }
        Ok(self)
    }

    /// Supply a caller-owned bound UDP socket for the experimental H3/QUIC
    /// endpoint (Plan 201 Track E).
    ///
    /// The socket must already be bound. At startup the runtime wraps it in
    /// Quinn (`TokioRuntime`) without exposing Quinn types here; no duplicate
    /// bind is performed. When both a prebound TCP listener and a prebound
    /// UDP socket are supplied, their ports must match (same-port
    /// TCP+UDP semantics where requested); a mismatch fails startup with a
    /// `Config` error. Port-zero callers must discover the TCP port first
    /// and bind UDP to the same explicit port.
    #[cfg(feature = "http3")]
    pub fn http3_socket(mut self, socket: std::net::UdpSocket) -> Self {
        self.http3_socket = Some(socket);
        self
    }

    /// Build the server, eagerly constructing the built-in static file service
    /// when a serve configuration was supplied.
    ///
    /// Invalid static roots therefore fail during `build()`, before listener
    /// preparation or startup. The serve config must have been set via
    /// [`ServerBuilder::serve_config`] for [`Server::start`] to be available.
    ///
    /// Hand-constructed [`RuntimeConfig`] values are validated here (Plan 179
    /// Track C) so invalid concurrency/timeouts/parser ceilings fail before
    /// semaphore/Hyper construction.
    pub fn build(self) -> Result<Server, ServerError> {
        let serve_config = self.serve_config;
        let config = match self.runtime_config {
            Some(c) => {
                c.validate()?;
                c
            }
            None => match &serve_config {
                Some(sc) => config::try_from_serve_config(sc)?,
                None => {
                    return Err(ServerError::Config(
                        "runtime configuration or serve configuration required".into(),
                    ))
                }
            },
        };
        let ops = self
            .ops_context
            .unwrap_or_else(|| crate::ops::OpsContext::global().clone());
        let builtin_static_service = serve_config
            .map(|sc| StaticService::from_serve_config_with_ops(sc, ops.clone()))
            .transpose()
            .map_err(|e| ServerError::Config(e.to_string()))?;
        // Unix plaintext rule is enforced at startup (needs TLS presence),
        // but fail fast here when the combination is already known: a
        // Unix-only server with no TCP source cannot meaningfully carry a
        // TCP TLS identity. TCP+Unix with TLS is allowed (TCP uses TLS,
        // Unix stays plaintext; see start_with_service).
        #[cfg(all(unix, feature = "tls"))]
        if self.unix_source.is_some() && self.tcp_source.is_none() && config.tls_config.is_some() {
            return Err(ServerError::Config(
                "TLS requires a TCP listener; Unix-domain sockets are plaintext and H3 is unavailable over them".into(),
            ));
        }
        Ok(Server {
            config,
            builtin_static_service,
            lifecycle: Arc::new(Lifecycle::new()),
            tcp_source: self.tcp_source,
            #[cfg(unix)]
            unix_source: self.unix_source,
            ops,
            #[cfg(feature = "http3")]
            http3_identity: self.http3_identity,
            #[cfg(feature = "http3")]
            http3_socket: self.http3_socket,
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
            Some(c) => {
                c.validate()?;
                c
            }
            None => config::try_from_serve_config(&serve_config)?,
        };
        let ops = self
            .ops_context
            .unwrap_or_else(|| crate::ops::OpsContext::global().clone());
        let builtin_static_service =
            StaticService::from_serve_config_with_ops(serve_config, ops.clone())
                .map_err(|e| ServerError::Config(e.to_string()))?;
        Ok(Server {
            config,
            builtin_static_service: Some(builtin_static_service),
            lifecycle: Arc::new(Lifecycle::new()),
            tcp_source: self.tcp_source,
            #[cfg(unix)]
            unix_source: self.unix_source,
            ops,
            #[cfg(feature = "http3")]
            http3_identity: self.http3_identity,
            #[cfg(feature = "http3")]
            http3_socket: self.http3_socket,
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
            tcp_source,
            #[cfg(unix)]
            unix_source,
            ops,
            #[cfg(feature = "http3")]
            http3_identity,
            #[cfg(feature = "http3")]
            http3_socket,
        } = self;
        let service = builtin_static_service.ok_or_else(|| {
            ServerError::Config("serve configuration required for static service".into())
        })?;

        Server {
            config,
            builtin_static_service: None,
            lifecycle,
            tcp_source,
            #[cfg(unix)]
            unix_source,
            ops,
            #[cfg(feature = "http3")]
            http3_identity,
            #[cfg(feature = "http3")]
            http3_socket,
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
            tcp_source,
            #[cfg(unix)]
            unix_source,
            ops,
            #[cfg(feature = "http3")]
            http3_identity,
            #[cfg(feature = "http3")]
            http3_socket,
        } = self;
        // Defense-in-depth: `ServerBuilder::build` already validated, but a
        // future constructor must not silently admit an invalid hand-built
        // config into semaphore/Hyper construction.
        runtime_config.validate()?;
        lifecycle.start()?;

        // Resolve TCP listener: explicit source wins; an explicit Unix-only
        // server (unix set, tcp unset) serves Unix alone; otherwise bind the
        // runtime address (no duplicate bind for prebound sockets).
        #[cfg(unix)]
        let unix_only = unix_source.is_some() && tcp_source.is_none();
        let (tcp_listener, tcp_addr): (Option<TcpListener>, Option<std::net::SocketAddr>) =
            match tcp_source {
                Some(TcpListenerSource::Listener(l)) => {
                    let addr = l.local_addr().map_err(ServerError::Bind)?;
                    (Some(l), Some(addr))
                }
                Some(TcpListenerSource::Bind(addr)) => {
                    let l = TcpListener::bind(addr).await.map_err(ServerError::Bind)?;
                    let actual = l.local_addr().map_err(ServerError::Bind)?;
                    (Some(l), Some(actual))
                }
                None => {
                    #[cfg(unix)]
                    if unix_only {
                        (None, None)
                    } else {
                        let l = TcpListener::bind(runtime_config.bind)
                            .await
                            .map_err(ServerError::Bind)?;
                        let actual = l.local_addr().map_err(ServerError::Bind)?;
                        (Some(l), Some(actual))
                    }
                    #[cfg(not(unix))]
                    {
                        let l = TcpListener::bind(runtime_config.bind)
                            .await
                            .map_err(ServerError::Bind)?;
                        let actual = l.local_addr().map_err(ServerError::Bind)?;
                        (Some(l), Some(actual))
                    }
                }
            };

        // Resolve Unix listener (Unix only). Path ownership stays with the
        // caller; EggServe never unlinks.
        #[cfg(unix)]
        let (unix_listener, unix_path): (
            Option<tokio::net::UnixListener>,
            Option<std::path::PathBuf>,
        ) = match unix_source {
            Some(UnixListenerSource::Listener(l)) => {
                let path = crate::server::listener::unix_listener_path(&l);
                (Some(l), path)
            }
            None => (None, None),
        };

        #[cfg(unix)]
        if tcp_listener.is_none() && unix_listener.is_none() {
            return Err(ServerError::Config("no listener source configured".into()));
        }

        // Explicit Unix/TLS rule: TLS applies to TCP only. A Unix-only
        // server with TLS would silently ignore it, so fail closed. TCP+Unix
        // with TLS serves TCP via TLS and Unix as plaintext (documented).
        #[cfg(all(unix, feature = "tls"))]
        if tcp_listener.is_none() && runtime_config.tls_config.is_some() {
            return Err(ServerError::Config(
                "TLS requires a TCP listener; Unix-domain sockets are plaintext and H3 is unavailable over them".into(),
            ));
        }

        // Bound endpoints with stable IDs for readiness/handles/logs.
        let mut endpoints: Vec<crate::server::listener::BoundEndpoint> = Vec::new();
        if let Some(addr) = tcp_addr {
            endpoints.push(crate::server::listener::BoundEndpoint::Tcp {
                id: "tcp-0".into(),
                addr,
            });
        }
        #[cfg(unix)]
        if unix_listener.is_some() {
            endpoints.push(crate::server::listener::BoundEndpoint::Unix {
                id: "unix-0".into(),
                path: unix_path.clone(),
            });
        }

        // Once port zero (or a pre-bound listener) has resolved, keep the
        // actual origin port in the runtime config so response finalization
        // can construct truthful same-port Alt-Svc metadata.
        #[cfg(feature = "http3")]
        let mut runtime_config = runtime_config;
        #[cfg(feature = "http3")]
        if let Some(addr) = tcp_addr {
            runtime_config.bind = addr;
        }

        #[cfg(feature = "http3")]
        let http3_endpoint = if config_http3_enabled(&runtime_config) {
            let tcp_bind = tcp_addr.ok_or_else(|| {
                ServerError::Config(
                    "http3 requires a TCP listener; H3 is not available over Unix streams".into(),
                )
            })?;
            let (cert_path, key_path) = http3_identity.as_ref().ok_or_else(|| {
                ServerError::Config(
                    "http3 is enabled but no QUIC certificate/key identity was supplied".into(),
                )
            })?;
            let quic_config =
                crate::tls::load_quic_server_config(cert_path, key_path, &runtime_config.http3)
                    .map_err(|e| ServerError::Config(e.to_string()))?;
            if let Some(socket) = http3_socket {
                socket.set_nonblocking(true).map_err(ServerError::Bind)?;
                let udp_addr = socket.local_addr().map_err(ServerError::Bind)?;
                if udp_addr.port() != tcp_bind.port() {
                    return Err(ServerError::Config(format!(
                        "prebound H3 UDP port {} does not match TCP port {}; same-port TCP+UDP required",
                        udp_addr.port(),
                        tcp_bind.port()
                    )));
                }
                let endpoint = quinn::Endpoint::new(
                    quinn::EndpointConfig::default(),
                    Some(quic_config),
                    socket,
                    std::sync::Arc::new(quinn::TokioRuntime),
                )
                .map_err(ServerError::Bind)?;
                Some(endpoint)
            } else {
                Some(h3_quinn::Endpoint::server(quic_config, tcp_bind).map_err(ServerError::Bind)?)
            }
        } else {
            // H3 disabled but a prebound UDP socket was supplied: fail
            // closed rather than silently ignoring a caller-owned descriptor.
            if http3_socket.is_some() {
                return Err(ServerError::Config(
                    "prebound H3 UDP socket supplied but http3 is not enabled".into(),
                ));
            }
            None
        };

        let config = Arc::new(runtime_config);
        let runtime_state = Arc::new(RuntimeState::with_ops(&config, ops.clone())?);
        let connection_semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_connections));

        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let shutdown_tx_clone = shutdown_tx.clone();
        let lifecycle = lifecycle.clone();

        let join = tokio::spawn({
            let lifecycle = lifecycle.clone();
            #[cfg(feature = "http3")]
            let http3_endpoint = http3_endpoint;
            let endpoints_for_task = endpoints.clone();
            // `tcp_addr` is Copy; move a copy into the task.
            #[cfg(feature = "http3")]
            let tcp_addr_for_h3 = tcp_addr;
            async move {
                #[cfg(feature = "http3")]
                let service = Arc::new(service);
                #[cfg(feature = "http3")]
                if let Some(endpoint) = http3_endpoint {
                    let h3_bind = tcp_addr_for_h3.expect("h3 requires TCP addr (checked)");
                    let shared_service = service.clone();
                    let tcp = accept_loop_multi(
                        tcp_listener,
                        tcp_addr_for_h3,
                        #[cfg(unix)]
                        unix_listener,
                        endpoints_for_task,
                        config.clone(),
                        runtime_state.clone(),
                        connection_semaphore.clone(),
                        ArcService(shared_service.clone()),
                        shutdown_rx.resubscribe(),
                        lifecycle.clone(),
                    );
                    let h3 = http3::accept_loop(
                        endpoint,
                        h3_bind,
                        config,
                        runtime_state,
                        connection_semaphore,
                        shutdown_rx,
                        lifecycle,
                        ArcService(shared_service),
                    );
                    let (tcp_result, h3_result) = tokio::join!(tcp, h3);
                    return if tcp_result == ShutdownResult::Clean
                        && h3_result == ShutdownResult::Clean
                    {
                        ShutdownResult::Clean
                    } else {
                        ShutdownResult::Timeout
                    };
                }
                accept_loop_multi(
                    tcp_listener,
                    tcp_addr,
                    #[cfg(unix)]
                    unix_listener,
                    endpoints_for_task,
                    config,
                    runtime_state,
                    connection_semaphore,
                    #[cfg(feature = "http3")]
                    ArcService(service),
                    #[cfg(not(feature = "http3"))]
                    service,
                    shutdown_rx,
                    lifecycle,
                )
                .await
            }
        });

        Ok(ServerHandle::new_with_endpoints(
            endpoints,
            shutdown_tx_clone,
            join,
            lifecycle,
            ops,
        ))
    }
}

#[cfg(feature = "http3")]
fn config_http3_enabled(config: &RuntimeConfig) -> bool {
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
async fn accept_loop_multi<S: Service>(
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
        let conn_shutdown = connection::ConnectionShutdown::new();
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
                if let Some(tls_config) = &config.tls_config {
                    let tls_acceptor = tokio_rustls::TlsAcceptor::from(tls_config.clone());
                    #[cfg(feature = "http2")]
                    let h2_enabled = config.http2.enabled;
                    #[cfg(not(feature = "http2"))]
                    let h2_enabled = false;
                    let prefixed = connection::driver::PrefixedIo::new(proxy_leftover, tcp_stream);
                    match accept_tls(
                        prefixed,
                        &tls_acceptor,
                        config.tls_handshake_timeout,
                        h2_enabled,
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
                            let context = connection::ConnectionContext::for_tcp(
                                local_addr_pre_tls,
                                remote_addr,
                                Some(tls_info),
                            )
                            .with_proxy_endpoints(
                                proxy_source,
                                proxy_destination,
                                proxy_kind,
                            );
                            let _ = connection::serve_http_connection_with_id_and_protocol(
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
                let prefixed = connection::driver::PrefixedIo::new(proxy_leftover, tcp_stream);
                let context =
                    connection::ConnectionContext::for_tcp(local_addr_pre_tls, remote_addr, None)
                        .with_proxy_endpoints(proxy_source, proxy_destination, proxy_kind);
                let _ = connection::serve_http_connection_with_id_and_protocol(
                    prefixed,
                    ArcService(service),
                    config.clone(),
                    context,
                    runtime_state.clone(),
                    &conn_shutdown,
                    conn_id,
                    connection::driver::WireProtocol::Auto,
                )
                .await;
                return;
            }
        }

        #[cfg(feature = "tls")]
        {
            if let Some(tls_config) = &config.tls_config {
                let tls_acceptor = tokio_rustls::TlsAcceptor::from(tls_config.clone());
                #[cfg(feature = "http2")]
                let h2_enabled = config.http2.enabled;
                #[cfg(not(feature = "http2"))]
                let h2_enabled = false;
                match accept_tls(
                    stream,
                    &tls_acceptor,
                    config.tls_handshake_timeout,
                    h2_enabled,
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
                        let context = connection::ConnectionContext::for_tcp(
                            local_addr_pre_tls,
                            remote_addr,
                            Some(tls_info),
                        );
                        let _ = connection::serve_http_connection_with_id_and_protocol(
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

        let context = connection::ConnectionContext::for_tcp(local_addr_pre_tls, remote_addr, None);
        let _ = connection::serve_http_connection_with_id_and_protocol(
            stream,
            ArcService(service),
            config.clone(),
            context,
            runtime_state.clone(),
            &conn_shutdown,
            conn_id,
            connection::driver::WireProtocol::Auto,
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
        let conn_shutdown = connection::ConnectionShutdown::new();
        let forwarder_shutdown = conn_shutdown.clone();
        let mut forwarder_rx = forwarder_rx;
        tokio::spawn(async move {
            let _ = forwarder_rx.recv().await;
            forwarder_shutdown.shutdown();
        });
        let context = connection::ConnectionContext::for_unix();
        let _ = connection::serve_http_connection_with_id_and_protocol(
            stream,
            ArcService(service),
            config.clone(),
            context,
            runtime_state.clone(),
            &conn_shutdown,
            conn_id,
            connection::driver::WireProtocol::Auto,
        )
        .await;
    });
}

/// Accept a TLS connection with timeout.
///
/// Returns the TLS stream and TLS session metadata on success, or `None` if
/// the handshake failed or timed out. Emits `TlsHandshakeFailure` or
/// `TlsHandshakeTimeout` events on failure.
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
    conn_id: u64,
    ops: &crate::ops::OpsContext,
) -> Option<(
    tokio_rustls::server::TlsStream<S>,
    crate::primitives::connection_info::TlsInfo,
    connection::driver::WireProtocol,
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
                    connection::driver::WireProtocol::Http2
                } else {
                    connection::driver::WireProtocol::Http1
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
                    connection::driver::WireProtocol::Http1
                }
            };
            let tls_info = extract_tls_info(&tls_stream);
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

/// Extract TLS session metadata from a completed TLS stream.
#[cfg(feature = "tls")]
fn extract_tls_info<S>(
    tls_stream: &tokio_rustls::server::TlsStream<S>,
) -> crate::primitives::connection_info::TlsInfo
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
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
async fn classify_accept_error(
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

fn is_fd_exhaustion(error: &std::io::Error) -> bool {
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
        let ops = crate::ops::OpsContext::default();
        assert!(
            !classify_accept_error(&error, &mut rx, &mut backoff, &mut repeats, &mut last, &ops,)
                .await
        );
        let _ = tx.send(());
    }

    #[cfg(windows)]
    #[test]
    fn windows_fd_exhaustion_includes_process_handle_limit() {
        let error = std::io::Error::from_raw_os_error(10023); // WSAENFILE
        assert!(is_fd_exhaustion(&error));
    }
}
