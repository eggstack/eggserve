//! Experimental HTTP/3 compatibility facade (Plan 220).
//!
//! Implementation authority lives in `eggserve-h3` (endpoint, request,
//! response, tunnel, QUIC assembly, `Http3Config`). This module preserves
//! the `server::http3::accept_loop` path for the compatibility `Server`
//! and projects core runtime state into the narrow H3 adapter API with no
//! second protocol state machine.

use std::sync::Arc;

use tokio::sync::{broadcast, Semaphore};

use crate::server::config::RuntimeConfig;
use crate::server::errors::ShutdownResult;
use crate::server::service::Service;
use crate::server::RuntimeState;

/// Compatibility H3 accept loop.
///
/// Projects the core `RuntimeConfig`/`RuntimeState` into the H3-owned
/// adapter (`eggserve-h3::accept_loop`) sharing the same admission pools
/// and observability context. No QUIC/H3 types enter the public contract
/// beyond the doc-hidden endpoint passed through from `Server` startup.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn accept_loop<S: Service>(
    endpoint: eggserve_h3::h3_quinn::Endpoint,
    local_addr: std::net::SocketAddr,
    config: Arc<RuntimeConfig>,
    runtime_state: Arc<RuntimeState>,
    connection_semaphore: Arc<Semaphore>,
    shutdown_rx: broadcast::Receiver<()>,
    _lifecycle: Arc<crate::server::lifecycle::Lifecycle>,
    service: S,
) -> ShutdownResult {
    let server_config = Arc::new(project_to_server_config(&config));
    let h3_config = Arc::new(config.http3.clone());
    let ops = runtime_state.ops().clone();
    let file_sem = runtime_state.file_stream_semaphore().clone();
    let service_sem = runtime_state.service_semaphore().clone();
    let tunnel_sem = runtime_state.tunnel_semaphore().clone();

    // `Service` is the single converged contract (Plan 217): the core
    // re-export is the same trait object as the server authority, so the
    // compatibility service drives the H3 adapter directly.
    eggserve_h3::accept_loop(
        endpoint,
        local_addr,
        server_config,
        h3_config,
        ops,
        file_sem,
        service_sem,
        tunnel_sem,
        connection_semaphore,
        shutdown_rx,
        service,
    )
    .await
}

/// Project core runtime fields onto the H1-generic server config.
///
/// H3-only transport policy stays in `Http3Config` (H3-owned); TLS/H2
/// compat fields stay in core. Shared defaults/validation already ran in
/// the core builder, so this projection is infallible field copies.
fn project_to_server_config(config: &RuntimeConfig) -> eggserve_server::config::RuntimeConfig {
    eggserve_server::config::RuntimeConfig {
        bind: config.bind,
        max_connections: config.max_connections,
        max_file_streams: config.max_file_streams,
        stream_chunk_size: config.stream_chunk_size,
        header_read_timeout: config.header_read_timeout,
        tls_handshake_timeout: config.tls_handshake_timeout,
        connection_total_timeout: config.connection_total_timeout,
        handler_timeout: config.handler_timeout,
        body_read_timeout: config.body_read_timeout,
        graceful_shutdown_timeout: config.graceful_shutdown_timeout,
        response_policy: config.response_policy.clone(),
        max_request_body_bytes: config.max_request_body_bytes,
        max_buf_size: config.max_buf_size,
        max_headers: config.max_headers,
        max_header_bytes: config.max_header_bytes,
        max_request_target_bytes: config.max_request_target_bytes,
        max_in_flight_requests: config.max_in_flight_requests,
        keep_alive_idle_timeout: config.keep_alive_idle_timeout,
        max_requests_per_connection: config.max_requests_per_connection,
        response_write_timeout: config.response_write_timeout,
        max_active_tunnels: config.max_active_tunnels,
        trusted_proxy: config.trusted_proxy.clone(),
    }
}
