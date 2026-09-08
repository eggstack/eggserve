//! Connection execution pipeline.
//!
//! This module owns the per-connection execution path from byte stream to
//! response completion. It is used by the TCP/TLS accept loop, the embedded
//! runtime, and caller-owned transports.
//!
//! # Pipeline steps
//!
//! 1. Optional TLS handshake (feature-gated, above the driver for TCP)
//! 2. HTTP/1 connection setup via Hyper
//! 3. Request conversion to canonical types
//! 4. Request-policy validation (body rejection for body-forbidden methods)
//! 5. Service invocation with panic containment
//! 6. Canonical response normalization
//! 7. Transport-body conversion
//! 8. Permit release and connection termination
//!
//! # Transport-neutral driver
//!
//! [`serve_http1_connection`] is the canonical connection driver (Plan 163).
//! The caller supplies an already-established bidirectional async byte stream,
//! a canonical [`Service`], a [`ConnectionContext`], shared [`RuntimeState`]
//! admission, and a [`ConnectionShutdown`] token. EggServe supplies HTTP
//! parsing, request conversion, body policy, service dispatch, response
//! normalization/framing, timeouts, and closure semantics. The TCP/TLS
//! `Server` is a convenience runtime that owns listener acceptance and
//! handshake above this driver and shares the same pipeline.
//!
//! # Module ownership (Plan 180)
//!
//! ```text
//! connection/
//!   mod.rs           facade + caller-owned entry points (this file)
//!   context.rs       ConnectionContext / ConnectionShutdown / ConnectionOutcome
//!   lifecycle.rs     live-request registry + abnormal-termination cancellation
//!   activity.rs      in-flight/outstanding/deferred counters, admission guard,
//!                    tracked response bodies
//!   transport.rs     ProgressIo read/write progress observation
//!   driver.rs        Hyper builder, graceful close, outcome classification,
//!                    deadline/select loop, TCP + caller-token adapters
//!   pipeline.rs      CanonicalHyperService + request/service dispatch
//!   request.rs       target/header ceilings, framing checks, body-policy
//!                    selection, Hyper body bridge
//!   response.rs      normalization, panic containment, body-error mapping,
//!                    final-boundary privacy
//!   deferred_body.rs deferred-body watchdog + terminal-state tracker
//! ```
//!
//! Dependency direction is acyclic: `pipeline` and `driver` depend on
//! `activity`/`lifecycle`/`transport`/`request`/`response`/`deferred_body`;
//! `activity` depends on `response` (final privacy); nothing depends back on
//! `pipeline`/`driver` except this facade. External code imports only this
//! facade; Hyper types never appear in public signatures added here.

// Panics raised while executing a [`Service`] are contained at the
// invocation boundary and mapped to [`ServiceError::panic`], so the client
// receives an RFC-correct 500 response instead of a dropped connection.
// The standard panic hook still runs, keeping diagnostics on stderr; panics
// outside service execution (e.g., during transport-body conversion) still
// propagate to the JoinSet task boundary.

pub(crate) mod activity;
pub(crate) mod context;
pub(crate) mod deferred_body;
pub(crate) mod driver;
pub(crate) mod lifecycle;
pub(crate) mod pipeline;
pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod transport;

pub use context::{ConnectionContext, ConnectionOutcome, ConnectionShutdown};

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use hyper_util::rt::TokioIo;
use tokio::sync::broadcast;

use crate::server::config::RuntimeConfig;
use crate::server::service::Service;
use crate::server::RuntimeState;

use self::activity::ConnectionActivity;
use self::driver::{serve_connection, serve_hyper_with_token};
use self::lifecycle::ConnectionRequests;
use self::pipeline::make_canonical_hyper_service;

/// Serve a single connection with a custom [`Service`] implementation.
///
/// This is a compatibility wrapper that builds a [`ConnectionContext`] from
/// the TCP socket addresses and delegates to [`make_canonical_hyper_service`]
/// and [`serve_connection`]. New callers should use
/// [`serve_http1_connection`] with an explicit [`ConnectionContext`].
///
/// Panics raised while polling the service future are contained and mapped
/// to [`ServiceError::panic`], producing a 500 response. Panics outside
/// service execution propagate to the tokio task boundary, are caught by
/// the `JoinSet` in the accept loop, and drop the connection with a
/// `ConnectionPanic` event.
#[allow(clippy::too_many_arguments)]
pub async fn serve_connection_with_runtime_state<I, S>(
    io: TokioIo<I>,
    service: S,
    config: Arc<RuntimeConfig>,
    runtime_state: Arc<RuntimeState>,
    shutdown_rx: &mut broadcast::Receiver<()>,
    conn_id: u64,
    local_addr: std::net::SocketAddr,
    remote_addr: std::net::SocketAddr,
    tls: bool,
    tls_info: Option<crate::primitives::connection_info::TlsInfo>,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: Service,
{
    let context = if tls {
        ConnectionContext::for_tcp(local_addr, remote_addr, tls_info)
    } else {
        ConnectionContext::for_tcp(local_addr, remote_addr, None)
    };
    let service = Arc::new(service);
    let file_stream_semaphore = runtime_state.file_stream_semaphore().clone();
    let service_semaphore = runtime_state.service_semaphore().clone();
    let activity = Arc::new(ConnectionActivity::new());
    let requests = Arc::new(ConnectionRequests::new());
    let hyper_service = make_canonical_hyper_service(
        service,
        config.clone(),
        file_stream_semaphore,
        service_semaphore,
        activity.clone(),
        requests.clone(),
        config.stream_chunk_size,
        config.handler_timeout,
        config.body_read_timeout,
        config.max_request_body_bytes,
        context,
        conn_id,
    );
    let _ = serve_connection(
        io,
        hyper_service,
        &config,
        &activity,
        &requests,
        shutdown_rx,
        conn_id,
    )
    .await;
}

/// Serve one HTTP/1 connection over any suitable bidirectional async byte
/// stream.
///
/// The caller supplies an already-established bidirectional async byte stream,
/// a canonical [`Service`], a [`ConnectionContext`], shared [`RuntimeState`]
/// admission, and a [`ConnectionShutdown`] token. EggServe supplies HTTP/1
/// parsing, request conversion, body policy, service dispatch, response
/// normalization/framing, timeouts, and closure semantics. The TCP/TLS
/// `Server` is a convenience runtime that owns listener acceptance and
/// handshake above this driver and shares the same pipeline.
///
/// No Hyper types appear in the signature. The caller need not supply
/// `SocketAddr` values — non-socket transports use
/// [`ConnectionContext::for_non_socket`]. Permits and producer tasks are
/// released on driver exit regardless of outcome.
///
/// # Example
///
/// ```no_run
/// use eggserve_core::server::connection::{
///     serve_http1_connection, ConnectionContext, ConnectionShutdown,
/// };
/// use eggserve_core::server::{RuntimeConfig, RuntimeState, service_fn, Request};
/// use eggserve_core::primitives::canonical::{Response, StatusCode, ResponseBody};
/// use eggserve_core::primitives::connection_info::Scheme;
/// use std::sync::Arc;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = Arc::new(RuntimeConfig::default());
/// let runtime_state = Arc::new(RuntimeState::new(&config));
/// let shutdown = ConnectionShutdown::new();
/// let context = ConnectionContext::for_non_socket(Scheme::Http, None);
///
/// let (client, server) = tokio::io::duplex(1024);
///
/// let outcome = serve_http1_connection(
///     server,
///     service_fn(|_req: Request| async {
///         Ok(Response::builder()
///             .status(StatusCode::OK)
///             .body(ResponseBody::Bytes(b"hello".to_vec()))
///             .unwrap())
///     }),
///     config,
///     context,
///     runtime_state,
///     &shutdown,
/// ).await;
/// # Ok(())
/// # }
/// ```
pub async fn serve_http1_connection<I, S>(
    io: I,
    service: S,
    config: Arc<RuntimeConfig>,
    context: ConnectionContext,
    runtime_state: Arc<RuntimeState>,
    shutdown: &ConnectionShutdown,
) -> ConnectionOutcome
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: Service,
{
    static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);
    let conn_id = NEXT_CONN_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    serve_http1_connection_with_id(
        io,
        service,
        config,
        context,
        runtime_state,
        shutdown,
        conn_id,
    )
    .await
}

/// Serve one HTTP/1 connection with an explicit connection ID.
///
/// Same as [`serve_http1_connection`] but uses the caller-supplied `conn_id`
/// for structured log correlation instead of generating one. The TCP accept
/// loop uses this to preserve its accept-time correlation IDs while sharing
/// the canonical driver pipeline.
pub async fn serve_http1_connection_with_id<I, S>(
    io: I,
    service: S,
    config: Arc<RuntimeConfig>,
    context: ConnectionContext,
    runtime_state: Arc<RuntimeState>,
    shutdown: &ConnectionShutdown,
    conn_id: u64,
) -> ConnectionOutcome
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: Service,
{
    // Plan 179 Track C: reject hand-constructed invalid configs before
    // Hyper/semaphore use. Caller-owned drivers bypass `ServerBuilder`, so
    // this is the ownership boundary. `hyper_builder` still clamps
    // `max_buf_size` as last-resort panic protection.
    if let Err(e) = config.validate() {
        crate::ops::Logger::global().emit(
            crate::ops::Event::new(
                crate::ops::Severity::Error,
                crate::ops::EventKind::ConnectionRejected,
                format!("caller-owned connection rejected invalid RuntimeConfig: {e}"),
            )
            .connection_id(conn_id),
        );
        return ConnectionOutcome::Internal;
    }
    let io = TokioIo::new(io);
    let service = Arc::new(service);
    let file_stream_semaphore = runtime_state.file_stream_semaphore().clone();
    let service_semaphore = runtime_state.service_semaphore().clone();
    let activity = Arc::new(ConnectionActivity::new());
    let requests = Arc::new(ConnectionRequests::new());
    let hyper_service = make_canonical_hyper_service(
        service,
        config.clone(),
        file_stream_semaphore,
        service_semaphore,
        activity.clone(),
        requests.clone(),
        config.stream_chunk_size,
        config.handler_timeout,
        config.body_read_timeout,
        config.max_request_body_bytes,
        context,
        conn_id,
    );
    serve_hyper_with_token(
        io,
        hyper_service,
        &config,
        &activity,
        &requests,
        shutdown,
        conn_id,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ServeConfig, ServeState};
    use crate::server::static_service::StaticService;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::broadcast;

    fn build_state(tmp: &TempDir) -> Arc<ServeState> {
        let config = Arc::new(ServeConfig {
            root: tmp.path().to_path_buf(),
            ..ServeConfig::default()
        });
        Arc::new(ServeState::new(config).unwrap())
    }

    #[tokio::test]
    async fn serve_connection_handles_get() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
        let state = build_state(&tmp);
        let config = Arc::new(RuntimeConfig::default());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, _rx) = broadcast::channel::<()>(1);

        let state_clone = state.clone();
        let server = tokio::spawn(async move {
            let (stream, remote_addr) = listener.accept().await.unwrap();
            let mut shutdown_rx = tx.subscribe();
            let runtime_state = Arc::new(RuntimeState::new(&config));
            serve_connection_with_runtime_state(
                TokioIo::new(stream),
                StaticService::from_state(state_clone),
                config,
                runtime_state,
                &mut shutdown_rx,
                1,
                addr,
                remote_addr,
                false,
                None,
            )
            .await;
        });

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();

        let _ = server.await;

        let response = String::from_utf8_lossy(&buf);
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "unexpected response: {}",
            response
        );
    }

    #[tokio::test]
    async fn presignaled_token_terminates_caller_owned_driver() {
        use crate::primitives::canonical::{Response, ResponseBody, StatusCode};
        use crate::primitives::connection_info::Scheme;

        let config = Arc::new(RuntimeConfig::default());
        let runtime_state = Arc::new(RuntimeState::new(&config));
        let shutdown = ConnectionShutdown::new();
        shutdown.shutdown();
        let context = ConnectionContext::for_non_socket(Scheme::Http, None);
        let service = crate::server::service_fn(|_req| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"hello".to_vec()))
                .unwrap())
        });
        let (client, server) = tokio::io::duplex(1024);
        // Hold the client half so the server side stays open until shutdown
        // drives termination; the pre-signaled token must still win promptly.
        let _client = client;
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            serve_http1_connection(server, service, config, context, runtime_state, &shutdown),
        )
        .await
        .expect("pre-signaled driver must terminate promptly");
        assert_eq!(
            outcome,
            ConnectionOutcome::Shutdown,
            "pre-signaled token must yield Shutdown outcome"
        );
    }
}
