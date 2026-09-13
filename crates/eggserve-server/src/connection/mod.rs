//! H1 connection execution pipeline (Plan 215: moved from compatibility core).
//!
//! This module owns the per-connection execution path from byte stream to
//! response completion for strict HTTP/1: Hyper request conversion,
//! header/target/body framing validation, service body-policy selection,
//! service admission, service invocation with panic containment, canonical
//! response normalization/conversion, and deferred-body lifecycle handling.
//!
//! [`serve_http1_connection`] is the canonical caller-owned driver: the
//! caller supplies an already-established bidirectional async byte stream, a
//! canonical [`Service`](crate::service::Service), a [`ConnectionContext`],
//! shared [`RuntimeState`](crate::runtime::RuntimeState) admission, and a
//! [`ConnectionShutdown`] token. EggServe supplies HTTP/1 parsing, request
//! conversion, body policy, service dispatch, response
//! normalization/framing, timeouts, and closure semantics. The TCP `Server`
//! is a convenience runtime that owns listener acceptance above this driver
//! and shares the same pipeline.
//!
//! H2 selection, H3, and PROXY-preamble reading stay compatibility-owned
//! (Plans 217/213/202); tunnel acceptance is direct-owned (Plan 216);
//! header-derived forwarding policy moves with the pipeline because it is
//! transport-neutral request finalization over direct primitives types.
//!
//! # Module ownership
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
//! facade; Hyper types never appear in public signatures.

// Panics raised while executing a [`Service`](crate::service::Service) are
// contained at the invocation boundary and mapped to a 500 response. The
// standard panic hook still runs, keeping diagnostics on stderr; panics
// outside service execution (e.g., during transport-body conversion) still
// propagate to the task boundary.

pub(crate) mod activity;
pub mod context;
pub(crate) mod deferred_body;
pub(crate) mod driver;
pub(crate) mod lifecycle;
pub(crate) mod pipeline;
pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod transport;

pub use context::{ConnectionContext, ConnectionOutcome, ConnectionShutdown};

use std::sync::Arc;

use hyper_util::rt::TokioIo;

use crate::config::RuntimeConfig;
use crate::runtime::RuntimeState;
use crate::service::Service;

use self::activity::ConnectionActivity;
use self::driver::serve_hyper_with_token;
use self::lifecycle::ConnectionRequests;
use self::pipeline::make_canonical_hyper_service;

/// Serve one HTTP/1 connection over any suitable bidirectional async byte
/// stream.
///
/// The caller supplies an already-established bidirectional async byte stream,
/// a canonical [`Service`], a [`ConnectionContext`], shared [`RuntimeState`]
/// admission, and a [`ConnectionShutdown`] token. EggServe supplies HTTP/1
/// parsing, request conversion, body policy, service dispatch, response
/// normalization/framing, timeouts, and closure semantics. The TCP
/// `Server` is a convenience runtime that owns listener acceptance above
/// this driver and shares the same pipeline.
///
/// No Hyper types appear in the signature. The caller need not supply
/// `SocketAddr` values — non-socket transports use
/// [`ConnectionContext::for_non_socket`]. Permits and producer tasks are
/// released on driver exit regardless of outcome.
///
/// # Example
///
/// ```no_run
/// use eggserve_server::connection::{
///     serve_http1_connection, ConnectionContext, ConnectionShutdown,
/// };
/// use eggserve_server::{config::RuntimeConfig, runtime::RuntimeState, service_fn, Request};
/// use eggserve_primitives::canonical::{Response, StatusCode, ResponseBody};
/// use eggserve_primitives::connection_info::Scheme;
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
    // Correlation IDs are owned by the runtime context: each runtime numbers
    // its own connections from 1, so two runtimes in one process never share
    // an ID sequence. Explicit IDs still flow through
    // `serve_http1_connection_with_id`.
    let conn_id = runtime_state.ops().next_connection_id();
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
    // Reject hand-constructed invalid configs before Hyper/semaphore use.
    // Caller-owned drivers bypass `ServerBuilder`, so this is the ownership
    // boundary. `hyper_builder` still clamps `max_buf_size` as last-resort
    // panic protection.
    if let Err(e) = config.validate() {
        runtime_state.ops().emit(
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
    let tunnel_semaphore = runtime_state.tunnel_semaphore().clone();
    let ops = runtime_state.ops().clone();
    let activity = Arc::new(ConnectionActivity::new(ops.clone()));
    let requests = Arc::new(ConnectionRequests::new());
    let hyper_service = make_canonical_hyper_service(
        service,
        config.clone(),
        file_stream_semaphore,
        service_semaphore,
        tunnel_semaphore,
        activity.clone(),
        requests.clone(),
        config.stream_chunk_size,
        config.handler_timeout,
        config.body_read_timeout,
        config.max_request_body_bytes,
        context,
        conn_id,
        ops,
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
    use eggserve_primitives::connection_info::Scheme;

    #[tokio::test]
    async fn presignaled_token_terminates_caller_owned_driver() {
        use eggserve_primitives::canonical::{Response, ResponseBody, StatusCode};

        let config = Arc::new(RuntimeConfig::default());
        let runtime_state = Arc::new(RuntimeState::new(&config));
        let shutdown = ConnectionShutdown::new();
        shutdown.shutdown();
        let context = ConnectionContext::for_non_socket(Scheme::Http, None);
        let service = crate::service::service_fn(|_req| async {
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
