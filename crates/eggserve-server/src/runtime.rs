//! Server runtime state (Plan 215: moved from compatibility core).
//!
//! Owns [`RuntimeState`] (shared admission semaphores, ops context).
//! Single owner for runtime permits; `Server`/`ServerBuilder` orchestrate
//! startup in the parent facade.
//!
//! `eggserve_core::server::RuntimeState` keeps its extended (TLS/H2/H3)
//! compatibility state and projects its H1 fields onto this type; this crate
//! is the single implementation authority for H1-generic admission.

use std::sync::Arc;

use crate::config::RuntimeConfig;
use crate::errors::ServerError;

/// Transport state shared by every connection in one running server.
///
/// File-stream and in-flight-service admission pools are created once here
/// and cloned into connection tasks.
///
/// The state also owns the runtime's observability context
/// ([`crate::ops::OpsContext`]): connection correlation IDs, connection and
/// request events, and counters resolve through this context rather than the
/// process-global logger. [`RuntimeState::new`]/[`RuntimeState::try_new`]
/// clone the process-global default so default construction keeps working;
/// [`RuntimeState::with_ops`] attaches an explicit per-runtime context for
/// isolated embedding.
///
/// Callers driving caller-owned byte streams with
/// [`crate::connection::serve_http1_connection`] must share one
/// `RuntimeState` across all of their connections rather than constructing
/// one per connection; otherwise file/response/service budgets become
/// per-connection instead of server-wide. It owns only transport-runtime
/// admission (file-stream permits and in-flight service permits); it never
/// owns static filesystem state or application routing state.
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
    /// [`crate::connection::serve_http1_connection`] invocation.
    ///
    /// # Panics
    ///
    /// Panics with an actionable message when `config` fails
    /// [`RuntimeConfig::validate`]. Prefer [`RuntimeState::try_new`] when the
    /// configuration is hand-constructed or otherwise untrusted so the error
    /// is returned instead of panicking. Validation happens before any
    /// semaphore construction so invalid values cannot trigger obscure
    /// downstream panics.
    pub fn new(config: &RuntimeConfig) -> Self {
        Self::try_new(config).expect("invalid RuntimeConfig for RuntimeState")
    }

    /// Validated constructor for the shared admission context.
    ///
    /// Returns [`ServerError::Config`] when a hand-constructed
    /// [`RuntimeConfig`] violates the shared runtime kernel, response
    /// policy, or semaphore bounds.
    pub fn try_new(config: &RuntimeConfig) -> Result<Self, ServerError> {
        Self::with_ops(config, crate::ops::OpsContext::global().clone())
    }

    /// Validated constructor with an explicit observability context.
    ///
    /// Same admission budgets as [`RuntimeState::try_new`], but connection
    /// correlation IDs, events, and counters resolve through `ops` instead
    /// of the process-global default.
    pub fn with_ops(
        config: &RuntimeConfig,
        ops: crate::ops::OpsContext,
    ) -> Result<Self, ServerError> {
        config.validate()?;
        Ok(Self {
            file_stream_semaphore: Arc::new(tokio::sync::Semaphore::new(config.max_file_streams)),
            service_semaphore: Arc::new(tokio::sync::Semaphore::new(config.max_in_flight_requests)),
            tunnel_semaphore: Arc::new(tokio::sync::Semaphore::new(config.max_active_tunnels)),
            ops,
        })
    }

    /// This runtime's observability context.
    ///
    /// Connection correlation IDs, events, and counters for every connection
    /// driven by this state resolve here. Cloning is cheap (shared inner).
    pub fn ops(&self) -> &crate::ops::OpsContext {
        &self.ops
    }

    /// Non-blocking, bounded snapshot of this runtime's counters.
    ///
    /// Reads never reset; no exporter or endpoint is involved.
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

    /// Return the server-wide active-tunnel admission pool.
    ///
    /// Reserved seam for Plan 216: held constant by the direct H1 driver,
    /// which performs no tunnel acceptance. Exhaustion semantics for
    /// accepted tunnels are defined by the compatibility path until then.
    pub fn tunnel_semaphore(&self) -> &Arc<tokio::sync::Semaphore> {
        &self.tunnel_semaphore
    }
}
