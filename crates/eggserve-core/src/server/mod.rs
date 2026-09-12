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
use eggserve_h3::{h3_quinn, quinn};
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

mod accept;
pub mod runtime;

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
pub use runtime::RuntimeState;
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
    tcp_source: Option<accept::TcpListenerSource>,
    #[cfg(unix)]
    unix_source: Option<accept::UnixListenerSource>,
    ops: crate::ops::OpsContext,
    #[cfg(feature = "http3")]
    http3_identity: Option<(PathBuf, PathBuf)>,
    #[cfg(feature = "http3")]
    http3_socket: Option<std::net::UdpSocket>,
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
    tcp_source: Option<accept::TcpListenerSource>,
    #[cfg(unix)]
    unix_source: Option<accept::UnixListenerSource>,
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
        self.tcp_source = Some(accept::TcpListenerSource::Bind(addr));
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
        self.tcp_source = Some(accept::TcpListenerSource::Listener(listener));
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
        self.tcp_source = Some(accept::TcpListenerSource::Listener(tokio_listener));
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
        self.unix_source = Some(accept::UnixListenerSource::Listener(listener));
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
        self.unix_source = Some(accept::UnixListenerSource::Listener(tokio_listener));
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
                self.tcp_source = Some(accept::TcpListenerSource::Listener(l));
            }
            crate::server::listener::SystemdListener::Unix(l) => {
                self.unix_source = Some(accept::UnixListenerSource::Listener(l));
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
                self.tcp_source = Some(accept::TcpListenerSource::Listener(l));
            }
            crate::server::listener::SystemdListener::Unix(l) => {
                self.unix_source = Some(accept::UnixListenerSource::Listener(l));
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
        if self.unix_source.is_some()
            && self.tcp_source.is_none()
            && (config.tls_config.is_some() || config.tls_reload_handle.is_some())
        {
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
                Some(accept::TcpListenerSource::Listener(l)) => {
                    let addr = l.local_addr().map_err(ServerError::Bind)?;
                    (Some(l), Some(addr))
                }
                Some(accept::TcpListenerSource::Bind(addr)) => {
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
            Some(accept::UnixListenerSource::Listener(l)) => {
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
        if tcp_listener.is_none()
            && (runtime_config.tls_config.is_some() || runtime_config.tls_reload_handle.is_some())
        {
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
        let http3_endpoint = if accept::config_http3_enabled(&runtime_config) {
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

        #[cfg(feature = "tls")]
        let tls_reload_for_handle = runtime_config.tls_reload_handle.clone();
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
                    let tcp = accept::accept_loop_multi(
                        tcp_listener,
                        tcp_addr_for_h3,
                        #[cfg(unix)]
                        unix_listener,
                        endpoints_for_task,
                        config.clone(),
                        runtime_state.clone(),
                        connection_semaphore.clone(),
                        accept::ArcService(shared_service.clone()),
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
                        accept::ArcService(shared_service),
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
                accept::accept_loop_multi(
                    tcp_listener,
                    tcp_addr,
                    #[cfg(unix)]
                    unix_listener,
                    endpoints_for_task,
                    config,
                    runtime_state,
                    connection_semaphore,
                    #[cfg(feature = "http3")]
                    accept::ArcService(service),
                    #[cfg(not(feature = "http3"))]
                    service,
                    shutdown_rx,
                    lifecycle,
                )
                .await
            }
        });

        #[cfg(feature = "tls")]
        let handle = crate::server::handle::ServerHandle::new_with_endpoints_and_tls(
            endpoints,
            shutdown_tx_clone,
            join,
            lifecycle,
            ops,
            tls_reload_for_handle,
        );
        #[cfg(not(feature = "tls"))]
        let handle =
            ServerHandle::new_with_endpoints(endpoints, shutdown_tx_clone, join, lifecycle, ops);
        Ok(handle)
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
            !super::accept::classify_accept_error(
                &error,
                &mut rx,
                &mut backoff,
                &mut repeats,
                &mut last,
                &ops,
            )
            .await
        );
        let _ = tx.send(());
    }

    #[cfg(windows)]
    #[test]
    fn windows_fd_exhaustion_includes_process_handle_limit() {
        let error = std::io::Error::from_raw_os_error(10023); // WSAENFILE
        assert!(super::accept::is_fd_exhaustion(&error));
    }
}
