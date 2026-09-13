//! Generic EggServe HTTP runtime (Plan 215: direct embeddable H1 substrate).
//!
//! This crate owns the mature generic HTTP/1 connection-serving substrate:
//! transport-neutral request/response conversion, admission, timeouts,
//! lifecycle, and shutdown semantics. It deliberately has no filesystem,
//! MIME, or static-policy dependency; applications provide a [`Service`]
//! and may add `eggserve-static` separately.
//!
//! There is one authoritative H1 connection execution path:
//! [`connection::serve_http1_connection`] drives caller-owned byte streams
//! and [`Server`] drives accepted TCP connections through the same driver
//! and [`runtime::RuntimeState`].

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::{Notify, Semaphore};

/// Outbound Hyper conversion boundary.
///
/// Explicit low-level adapter for embedders that own a Hyper transport but
/// want canonical conversion. Returns an opaque `http_body::Body`;
/// downstream code must not name the concrete erasure type.
pub mod adapters;
/// H1-generic runtime configuration (single authority for direct use).
pub mod config;
/// H1 connection execution pipeline and caller-owned driver.
pub mod connection;
/// Single-authority runtime error taxonomy.
pub mod errors;
/// Single-authority per-runtime observability.
pub mod ops;
/// Runtime-owned Hyper error responses (implementation detail).
mod response;
/// Single-authority response privacy policy.
pub mod response_policy;
/// Shared admission state for one running runtime.
pub mod runtime;
/// Single-authority shared runtime defaults/validation.
pub mod runtime_limits;
/// Transport-independent service abstraction (single authority).
pub mod service;

pub use config::{RuntimeConfig, RuntimeConfigBuilder};
pub use connection::{
    serve_http1_connection, serve_http1_connection_with_id, ConnectionContext, ConnectionOutcome,
    ConnectionShutdown,
};
pub use errors::{ServerError, ShutdownResult};
pub use runtime::RuntimeState;
pub use service::{
    service_fn, service_fn_head, service_fn_with_policy, Service, ServiceError, ServiceFn,
    ServiceFuture,
};

/// Canonical request type for service implementations (mirrors the
/// compatibility facade so `service_fn` closures resolve without importing
/// the primitives crate directly).
pub use eggserve_primitives::Request;

/// Builder for [`Server`].
pub struct ServerBuilder {
    config: RuntimeConfig,
    tcp_listener: Option<TcpListener>,
    ops: Option<ops::OpsContext>,
}

impl ServerBuilder {
    /// Set the runtime configuration.
    pub fn runtime(mut self, config: RuntimeConfig) -> Self {
        self.config = config;
        self
    }

    /// Set the bind address (used when no prebound listener is supplied).
    pub fn bind(mut self, bind: SocketAddr) -> Self {
        self.config.bind = bind;
        self
    }

    /// Attach an explicit per-runtime observability context.
    ///
    /// Events, counters, sink-failure accounting, and connection correlation
    /// IDs for this server resolve through `ops` instead of the
    /// process-global default.
    pub fn ops_context(mut self, ops: ops::OpsContext) -> Self {
        self.ops = Some(ops);
        self
    }

    /// Use a pre-bound Tokio TCP listener instead of binding on start.
    ///
    /// The listener must already be bound. Ownership transfers to the
    /// server; the actual local address becomes readiness metadata. No
    /// duplicate bind is performed.
    pub fn from_listener(mut self, listener: TcpListener) -> Self {
        self.tcp_listener = Some(listener);
        self
    }

    /// Use a caller-bound standard-library TCP listener.
    ///
    /// Nonblocking mode is normalized internally and all other socket
    /// options are preserved. Failed conversion drops (closes) the passed
    /// socket; do not reuse it after this call.
    pub fn from_std_listener(
        mut self,
        listener: std::net::TcpListener,
    ) -> Result<Self, ServerError> {
        listener.set_nonblocking(true).map_err(ServerError::Bind)?;
        let listener = TcpListener::from_std(listener).map_err(ServerError::Bind)?;
        self.tcp_listener = Some(listener);
        Ok(self)
    }

    /// Build the server, validating the runtime configuration.
    pub fn build(self) -> Result<Server, ServerError> {
        self.config.validate()?;
        Ok(Server {
            config: self.config,
            tcp_listener: self.tcp_listener,
            ops: self
                .ops
                .unwrap_or_else(|| ops::OpsContext::global().clone()),
        })
    }
}

/// A reusable H1 HTTP runtime server.
///
/// Binds a TCP listener (or adopts a prebound one), accepts connections,
/// and dispatches them through the canonical connection driver shared with
/// caller-owned transports. Accepted local/remote socket addresses are
/// observed from the socket and reach canonical requests unchanged.
pub struct Server {
    config: RuntimeConfig,
    tcp_listener: Option<TcpListener>,
    ops: ops::OpsContext,
}

/// Shared ownership wrapper so one service value can drive every accepted
/// connection task.
struct SharedService<S>(Arc<S>);

impl<S> Clone for SharedService<S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<S: Service> Service for SharedService<S> {
    fn request_body_policy(
        &self,
        head: &eggserve_primitives::RequestHead,
    ) -> eggserve_primitives::RequestBodyPolicy {
        self.0.request_body_policy(head)
    }

    fn call(&self, request: Request) -> ServiceFuture<'_> {
        self.0.call(request)
    }
}

impl Server {
    /// Create a new server builder with default configuration.
    pub fn builder() -> ServerBuilder {
        ServerBuilder {
            config: RuntimeConfig::default(),
            tcp_listener: None,
            ops: None,
        }
    }

    /// This server's runtime configuration.
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Start the server with a custom service.
    ///
    /// The runtime creates only transport state (semaphores, lifecycle) and
    /// drives every accepted connection through
    /// [`serve_http1_connection`] with a truthful [`ConnectionContext`]
    /// built from observed socket addresses. Accept errors are accounted
    /// (never silently dropped) with a bounded hot-spin guard; connection
    /// exhaustion drops the new connection with an event rather than
    /// queueing unbounded work. `handle.shutdown()` reaches in-flight
    /// connections through per-connection shutdown tokens sharing the
    /// canonical driver path.
    pub async fn start_with_service<S>(self, service: S) -> Result<ServerHandle, ServerError>
    where
        S: Service,
    {
        self.config.validate()?;
        let listener = match self.tcp_listener {
            Some(listener) => listener,
            None => TcpListener::bind(self.config.bind)
                .await
                .map_err(ServerError::Bind)?,
        };
        let local_addr = listener.local_addr().map_err(ServerError::Bind)?;
        let shutdown = Arc::new(Notify::new());
        let permits = Arc::new(Semaphore::new(self.config.max_connections));
        let service = SharedService(Arc::new(service));
        let config = Arc::new(self.config);
        let runtime_state = Arc::new(RuntimeState::with_ops(&config, self.ops.clone())?);
        let ops = self.ops.clone();

        let task_shutdown = shutdown.clone();
        let join = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = task_shutdown.notified() => break,
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((stream, remote_addr)) => {
                                let Ok(permit) = permits.clone().try_acquire_owned() else {
                                    ops.counters()
                                        .connections_rejected
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                    ops.emit(
                                        ops::Event::new(
                                            ops::Severity::Debug,
                                            ops::EventKind::ConnectionRejected,
                                            "connection saturated: connection limit",
                                        ),
                                    );
                                    continue;
                                };
                                let local = stream.local_addr().unwrap_or(local_addr);
                                let context =
                                    ConnectionContext::for_tcp(local, remote_addr, None);
                                let token = ConnectionShutdown::new();
                                let relay_shutdown = task_shutdown.clone();
                                let service = service.clone();
                                let config = config.clone();
                                let state = runtime_state.clone();
                                let conn_id = state.ops().next_connection_id();
                                tokio::spawn(async move {
                                    let _permit = permit;
                                    let serve = serve_http1_connection_with_id(
                                        stream,
                                        service,
                                        config,
                                        context,
                                        state,
                                        &token,
                                        conn_id,
                                    );
                                    let watch = async {
                                        relay_shutdown.notified().await;
                                        token.shutdown();
                                    };
                                    tokio::pin!(serve);
                                    tokio::pin!(watch);
                                    tokio::select! {
                                        _ = &mut serve => {}
                                        _ = &mut watch => {
                                            serve.await;
                                        }
                                    }
                                });
                            }
                            Err(error) => {
                                // Never silently drop accept errors: account
                                // them and back off briefly so a persistent
                                // failure cannot hot-spin the accept loop.
                                ops.counters()
                                    .listener_errors
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                ops.emit(
                                    ops::Event::new(
                                        ops::Severity::Debug,
                                        ops::EventKind::ListenerTransientError,
                                        ops::sanitize_text_field(&format!(
                                            "accept error: {error}"
                                        )),
                                    ),
                                );
                                tokio::time::sleep(Duration::from_millis(10)).await;
                            }
                        }
                    }
                }
            }
        });
        Ok(ServerHandle {
            local_addr,
            shutdown,
            join: Some(join),
            ops: self.ops,
        })
    }
}

/// Control handle for a running [`Server`].
pub struct ServerHandle {
    local_addr: SocketAddr,
    shutdown: Arc<Notify>,
    join: Option<tokio::task::JoinHandle<()>>,
    ops: ops::OpsContext,
}

impl ServerHandle {
    /// The bound address the server is accepting on.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Request graceful shutdown: the accept loop stops and in-flight
    /// connections observe shutdown through their per-connection tokens.
    pub fn shutdown(&self) {
        self.shutdown.notify_waiters();
    }

    /// This runtime's observability context.
    pub fn ops_context(&self) -> &ops::OpsContext {
        &self.ops
    }

    /// Non-blocking, bounded snapshot of this runtime's counters.
    pub fn ops_snapshot(&self) -> ops::OpsSnapshot {
        self.ops.snapshot()
    }

    /// Wait for the accept loop to terminate after [`ServerHandle::shutdown`].
    pub async fn wait(mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generic_runtime_has_no_static_configuration() {
        let config = RuntimeConfig::default();
        assert!(config.bind.ip().is_loopback());
    }

    #[tokio::test]
    async fn tcp_server_reports_real_socket_metadata() {
        use eggserve_primitives::{Response, ResponseBody, StatusCode};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        type ObservedAddrs = Option<(Option<SocketAddr>, Option<SocketAddr>)>;
        let seen: Arc<std::sync::Mutex<ObservedAddrs>> = Arc::new(std::sync::Mutex::new(None));
        let seen_clone = seen.clone();
        let server = Server::builder()
            .bind("127.0.0.1:0".parse().unwrap())
            .build()
            .unwrap();
        let handle = server
            .start_with_service(service_fn(move |req: Request| {
                let seen_clone = seen_clone.clone();
                async move {
                    let info = req.connection().clone();
                    *seen_clone.lock().unwrap() = Some((info.remote_addr, info.local_addr));
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(ResponseBody::Bytes(b"ok".to_vec()))
                        .unwrap())
                }
            }))
            .await
            .unwrap();
        let addr = handle.local_addr();
        assert_ne!(addr.port(), 0, "port zero must resolve to a bound port");

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let client_port = client.local_addr().unwrap().port();
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        let text = String::from_utf8_lossy(&buf);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");

        let (remote, local) = seen.lock().unwrap().expect("service must run");
        assert_eq!(
            remote.map(|a| a.port()),
            Some(client_port),
            "service must observe the real client port"
        );
        assert_eq!(local, Some(addr), "service must observe the bound address");

        handle.shutdown();
        handle.wait().await;
    }

    #[tokio::test]
    async fn prebound_std_listener_serves_without_rebind() {
        use eggserve_primitives::{Response, ResponseBody, StatusCode};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let bound = std_listener.local_addr().unwrap();
        let server = Server::builder()
            .from_std_listener(std_listener)
            .unwrap()
            .build()
            .unwrap();
        let handle = server
            .start_with_service(service_fn(|_req: Request| async {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(ResponseBody::Bytes(b"prebound".to_vec()))
                    .unwrap())
            }))
            .await
            .unwrap();
        assert_eq!(handle.local_addr(), bound);

        let mut client = tokio::net::TcpStream::connect(bound).await.unwrap();
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        let text = String::from_utf8_lossy(&buf);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
        assert!(text.ends_with("prebound"));

        handle.shutdown();
        handle.wait().await;
    }
}
