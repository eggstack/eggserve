//! Server runtime state (Plan 206 Track G).
//!
//! Owns [`RuntimeState`] (shared admission semaphores, ops context,
//! connection tracking). Single owner for runtime permits; `Server`/
//! `ServerBuilder` orchestrate startup in the parent facade.

#![allow(unused_imports)]
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::broadcast;

use crate::config::ServeConfig;
use crate::ops::{Event, EventKind, OpsContext, Severity};
#[cfg(feature = "http2")]
use crate::server::config::Http2Config;
#[cfg(feature = "http3")]
use crate::server::config::Http3Config;
use crate::server::config::{RuntimeConfig, RuntimeConfigBuilder};
use crate::server::connection::{
    serve_http1_connection, ConnectionContext, ConnectionOutcome, ConnectionShutdown,
};
use crate::server::errors::{ServerError, ShutdownResult};
use crate::server::handle::ServerHandle;
use crate::server::lifecycle::{Lifecycle, LifecycleState};
use crate::server::listener::BoundEndpoint;
use crate::server::response_policy::{DatePolicy, ResponsePolicy};
use crate::server::service::{Service, ServiceError};

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
/// [`super::connection::serve_http1_connection`] must share one `RuntimeState`
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
    /// [`super::connection::serve_http1_connection`] invocation.
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
    /// [`super::connection::serve_http1_connection`](crate::server::connection::serve_http1_connection)
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
