//! Server lifecycle handle.
//!
//! A [`ServerHandle`] is returned by [`Server::start`] and provides control
//! over the running server: readiness signaling, graceful/forced shutdown,
//! and waiting for completion.
//!
//! # Lifecycle
//!
//! After `Server::start()` returns a handle, the caller should:
//!
//! 1. Call [`ServerHandle::ready`] to wait for the listener to be bound and
//!    the accept loop to be running.
//! 2. Use the server (make requests).
//! 3. Call [`ServerHandle::shutdown`] to initiate graceful shutdown.
//! 4. Call [`ServerHandle::wait`] to wait for all connections to drain.
//!
//! Dropping the handle triggers graceful shutdown (the server will stop
//! accepting new connections and drain in-flight requests).
//!
//! # Thread safety
//!
//! All handle methods are safe to call from any thread. The handle is not
//! `Clone` — there is exactly one handle per server instance. This prevents
//! ambiguous shutdown semantics.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::sync::broadcast;

use crate::server::errors::{ServerError, ShutdownResult};
use crate::server::lifecycle::Lifecycle;

/// Handle to a running server instance.
///
/// This type is experimental and its API may change without notice.
///
/// The handle allows the caller to:
/// - Wait for readiness (via [`ServerHandle::ready`])
/// - Trigger graceful shutdown (via [`ServerHandle::shutdown`])
/// - Trigger forced shutdown (via [`ServerHandle::force_shutdown`])
/// - Query the listening address (via [`ServerHandle::local_addr`]; prefer
///   [`ServerHandle::tcp_local_addr`]/[`ServerHandle::endpoints`] for
///   Unix-only servers where `local_addr()` panics)
/// - Wait for completion (via [`ServerHandle::wait`])
///
/// Dropping the handle triggers graceful shutdown — the server stops
/// accepting new connections and drains in-flight requests.
pub struct ServerHandle {
    endpoints: Vec<crate::server::listener::BoundEndpoint>,
    shutdown_tx: broadcast::Sender<()>,
    join: Option<tokio::task::JoinHandle<ShutdownResult>>,
    lifecycle: std::sync::Arc<Lifecycle>,
    ops: crate::ops::OpsContext,
    #[cfg(feature = "tls")]
    tls_reload: Option<crate::tls::TlsReloadHandle>,
}

impl std::fmt::Debug for ServerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerHandle")
            .field("endpoints", &self.endpoints)
            .field("state", &self.lifecycle.state())
            .finish()
    }
}

impl ServerHandle {
    /// Compatibility constructor for the common one-TCP-listener path.
    ///
    /// Integration-unit tests use this; production startup uses
    /// `new_with_endpoints` so multi-listener readiness is explicit.
    #[allow(dead_code)]
    pub(crate) fn new(
        local_addr: SocketAddr,
        shutdown_tx: broadcast::Sender<()>,
        join: tokio::task::JoinHandle<ShutdownResult>,
        lifecycle: std::sync::Arc<Lifecycle>,
        ops: crate::ops::OpsContext,
    ) -> Self {
        Self::new_with_endpoints(
            vec![crate::server::listener::BoundEndpoint::Tcp {
                id: "tcp-0".into(),
                addr: local_addr,
            }],
            shutdown_tx,
            join,
            lifecycle,
            ops,
        )
    }

    pub(crate) fn new_with_endpoints(
        endpoints: Vec<crate::server::listener::BoundEndpoint>,
        shutdown_tx: broadcast::Sender<()>,
        join: tokio::task::JoinHandle<ShutdownResult>,
        lifecycle: std::sync::Arc<Lifecycle>,
        ops: crate::ops::OpsContext,
    ) -> Self {
        Self {
            endpoints,
            shutdown_tx,
            join: Some(join),
            lifecycle,
            ops,
            #[cfg(feature = "tls")]
            tls_reload: None,
        }
    }

    #[cfg(feature = "tls")]
    pub(crate) fn new_with_endpoints_and_tls(
        endpoints: Vec<crate::server::listener::BoundEndpoint>,
        shutdown_tx: broadcast::Sender<()>,
        join: tokio::task::JoinHandle<ShutdownResult>,
        lifecycle: std::sync::Arc<Lifecycle>,
        ops: crate::ops::OpsContext,
        tls_reload: Option<crate::tls::TlsReloadHandle>,
    ) -> Self {
        Self {
            endpoints,
            shutdown_tx,
            join: Some(join),
            lifecycle,
            ops,
            tls_reload,
        }
    }

    /// Atomically replace TLS identity/trust for new handshakes (Plan 203 F).
    ///
    /// All-or-nothing: build the replacement first (validation happens in
    /// `TlsServerConfigBuilder::build`); this swap never fails and never
    /// touches established sessions. Returns an error when the server has no
    /// reload handle (plaintext or legacy `tls_config`-only servers).
    #[cfg(feature = "tls")]
    pub fn replace_tls_config(
        &self,
        next: std::sync::Arc<rustls::ServerConfig>,
    ) -> Result<(), crate::server::errors::ServerError> {
        self.tls_reload
            .as_ref()
            .map(|h| {
                h.replace(next);
            })
            .ok_or_else(|| {
                crate::server::errors::ServerError::Config(
                    "TLS reload not configured for this server".into(),
                )
            })
    }

    /// Replace from a validated [`crate::tls::TlsServerConfig`].
    #[cfg(feature = "tls")]
    pub fn replace_tls_server_config(
        &self,
        next: &crate::tls::TlsServerConfig,
    ) -> Result<(), crate::server::errors::ServerError> {
        self.replace_tls_config(next.into_server_config())
    }

    /// Current TLS snapshot for observability/tests, if TLS is configured.
    #[cfg(feature = "tls")]
    pub fn tls_reload_handle(&self) -> Option<crate::tls::TlsReloadHandle> {
        self.tls_reload.clone()
    }

    /// All successfully adopted listener endpoints (Plan 201 Track G).
    ///
    /// Entries carry stable string IDs (`tcp-0`, `unix-0`, ...) rather than
    /// positional meaning. Readiness implies every entry here was adopted
    /// and protocol configuration validated, not merely that a task spawned.
    pub fn endpoints(&self) -> &[crate::server::listener::BoundEndpoint] {
        &self.endpoints
    }

    /// TCP local address of the first TCP endpoint, when one exists.
    ///
    /// Returns `None` for Unix-only servers (which have no IP endpoint and
    /// never fabricate one); use [`ServerHandle::endpoints`] there.
    pub fn tcp_local_addr(&self) -> Option<SocketAddr> {
        self.endpoints.iter().find_map(|ep| ep.tcp_addr())
    }

    /// Returns the address the server is listening on.
    ///
    /// Useful when binding to port 0 to discover the actual port.
    ///
    /// This preserves the common one-TCP-listener path. Unix-only servers
    /// have no TCP address: this panics with an actionable message directing
    /// to [`ServerHandle::endpoints`]/[`ServerHandle::tcp_local_addr`].
    ///
    /// # Panics
    ///
    /// Panics on Unix-only servers (no TCP endpoint). Prefer
    /// [`ServerHandle::tcp_local_addr`] (returns `None` there) or
    /// [`ServerHandle::endpoints`] for generic callers that must handle
    /// both TCP and Unix-only servers without panicking.
    pub fn local_addr(&self) -> SocketAddr {
        self.tcp_local_addr()
            .expect("unix-only server has no TCP local address; use ServerHandle::endpoints()")
    }

    /// This server's observability context.
    ///
    /// Cloning is cheap (shared inner); useful for wiring related components
    /// to the same sink/counters without going through the process global.
    pub fn ops_context(&self) -> &crate::ops::OpsContext {
        &self.ops
    }

    /// Non-blocking, bounded snapshot of this server's counters (Plan 181
    /// Track E). Reads never reset; no exporter or endpoint is involved.
    pub fn ops_snapshot(&self) -> crate::ops::OpsSnapshot {
        self.ops.snapshot()
    }

    /// Returns the current lifecycle state.
    pub fn state(&self) -> crate::server::lifecycle::LifecycleState {
        self.lifecycle.state()
    }

    /// Wait for the server to be ready to accept connections.
    ///
    /// This returns once the listener is bound and the accept loop has been
    /// polled. After this returns, the server will accept new connections.
    ///
    /// If the server fails during startup, this returns an error.
    ///
    /// # State behavior
    ///
    /// - `Running`: immediate success (already ready)
    /// - `Starting`: waits for transition to `Running` or `Failed`
    /// - `Failed`: returns startup error
    /// - `Created`: returns not-started error
    /// - `Draining`/`Stopped`: returns not-running error
    pub async fn ready(&self) -> Result<(), ServerError> {
        let state = self.lifecycle.state();
        match state {
            crate::server::lifecycle::LifecycleState::Running => Ok(()),
            crate::server::lifecycle::LifecycleState::Starting => {
                self.lifecycle.wait_ready().await;

                // Re-check after waiting.
                let state = self.lifecycle.state();
                match state {
                    crate::server::lifecycle::LifecycleState::Running => Ok(()),
                    crate::server::lifecycle::LifecycleState::Failed => {
                        Err(ServerError::Startup("server failed during startup".into()))
                    }
                    crate::server::lifecycle::LifecycleState::Stopped => {
                        // `Lifecycle::drain` moves Created/Starting directly to Stopped so
                        // ready waiters are not left hanging. This is a shutdown that
                        // raced startup, not a generic config misuse.
                        Err(ServerError::Startup("shutdown raced with startup".into()))
                    }
                    other => Err(ServerError::Config(format!(
                        "unexpected state after ready: {other}"
                    ))),
                }
            }
            crate::server::lifecycle::LifecycleState::Failed => {
                Err(ServerError::Startup("server failed during startup".into()))
            }
            other => Err(ServerError::Config(format!(
                "server not ready: in {other} state"
            ))),
        }
    }

    /// Trigger graceful shutdown.
    ///
    /// The server will stop accepting new connections and wait for in-flight
    /// requests to complete (up to the configured grace period).
    ///
    /// Multiple calls are idempotent — only the first call has an effect.
    pub fn shutdown(&self) {
        // Transition to draining (idempotent — returns Ok for already-draining/stopped/created).
        let _ = self.lifecycle.drain_with_ops(&self.ops);
        // Send broadcast signal to break accept loop.
        let _ = self.shutdown_tx.send(());
    }

    /// Trigger forced shutdown with a deadline.
    ///
    /// Sends the shutdown signal and waits for the server to stop. If the
    /// server does not stop within `deadline`, the accept task is aborted and
    /// the server is marked stopped.
    ///
    /// Returns the [`ShutdownResult`] indicating how the shutdown completed.
    pub async fn force_shutdown(
        mut self,
        deadline: Duration,
    ) -> Result<ShutdownResult, ServerError> {
        self.shutdown();
        match tokio::time::timeout(deadline, self.wait_internal()).await {
            Ok(()) => {
                // Terminal state reached — await the join handle.
                if let Some(join) = self.join.take() {
                    match join.await {
                        Ok(result) => Ok(result),
                        Err(e) => Err(ServerError::Accept(std::io::Error::other(format!(
                            "server task panicked: {e}"
                        )))),
                    }
                } else {
                    Ok(ShutdownResult::Clean)
                }
            }
            Err(_deadline_exceeded) => {
                if let Some(join) = self.join.take() {
                    join.abort();
                    let _ = join.await;
                }
                let _ = self.lifecycle.mark_stopped();
                Ok(ShutdownResult::Forced)
            }
        }
    }

    /// Wait for the server to finish.
    ///
    /// This consumes the handle. If the server is still running, triggers
    /// graceful shutdown first, then waits for all connections to drain.
    /// Returns the [`ShutdownResult`] indicating how the shutdown completed.
    pub async fn wait(mut self) -> Result<ShutdownResult, ServerError> {
        // Trigger shutdown if still running.
        let state = self.lifecycle.state();
        if !state.is_terminal() {
            self.shutdown();
        }

        // Wait for terminal state.
        self.wait_internal().await;

        // Await the join handle.
        if let Some(join) = self.join.take() {
            match join.await {
                Ok(result) => Ok(result),
                Err(e) => Err(ServerError::Accept(std::io::Error::other(format!(
                    "server task panicked: {e}"
                )))),
            }
        } else {
            Ok(ShutdownResult::Clean)
        }
    }

    /// Internal wait implementation.
    async fn wait_internal(&self) {
        // Subscribe to terminal state.
        let mut terminal_rx = self.lifecycle.subscribe_terminal();
        let state = self.lifecycle.state();
        if state.is_terminal() {
            return;
        }
        let _ = terminal_rx.recv().await;
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        // If the handle is dropped without explicit shutdown, trigger graceful shutdown.
        if self.join.is_some() {
            let _ = self.lifecycle.drain_with_ops(&self.ops);
            let _ = self.shutdown_tx.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::lifecycle::Lifecycle;
    use std::sync::Arc;

    async fn make_test_handle() -> ServerHandle {
        let lifecycle = Arc::new(Lifecycle::new());
        let (tx, _rx) = broadcast::channel(1);
        let join = tokio::spawn(async { ShutdownResult::Clean });
        ServerHandle::new(
            "127.0.0.1:8000".parse().unwrap(),
            tx,
            join,
            lifecycle,
            crate::ops::OpsContext::default(),
        )
    }

    fn make_handle_with_state(state: crate::server::lifecycle::LifecycleState) -> ServerHandle {
        let lifecycle = Arc::new(Lifecycle::new());
        match state {
            crate::server::lifecycle::LifecycleState::Created => {}
            crate::server::lifecycle::LifecycleState::Starting => {
                lifecycle.start().unwrap();
            }
            crate::server::lifecycle::LifecycleState::Running => {
                lifecycle.start().unwrap();
                lifecycle.mark_running().unwrap();
            }
            crate::server::lifecycle::LifecycleState::Failed => {
                lifecycle.mark_failed().unwrap();
            }
            crate::server::lifecycle::LifecycleState::Draining => {
                lifecycle.start().unwrap();
                lifecycle.mark_running().unwrap();
                lifecycle.drain().unwrap();
            }
            crate::server::lifecycle::LifecycleState::Stopped => {
                lifecycle.start().unwrap();
                lifecycle.mark_running().unwrap();
                lifecycle.drain().unwrap();
                lifecycle.mark_stopped().unwrap();
            }
        }
        let (shutdown_tx, _) = broadcast::channel(1);
        let join = tokio::spawn(async { ShutdownResult::Clean });
        ServerHandle::new(
            "127.0.0.1:0".parse().unwrap(),
            shutdown_tx,
            join,
            lifecycle,
            crate::ops::OpsContext::default(),
        )
    }

    #[tokio::test]
    async fn handle_local_addr() {
        let handle = make_test_handle().await;
        assert_eq!(
            handle.local_addr(),
            "127.0.0.1:8000".parse::<SocketAddr>().unwrap()
        );
    }

    #[tokio::test]
    async fn handle_state_initial() {
        let handle = make_test_handle().await;
        assert_eq!(
            handle.state(),
            crate::server::lifecycle::LifecycleState::Created
        );
    }

    #[tokio::test]
    async fn handle_shutdown_sends_signal() {
        let lifecycle = Arc::new(Lifecycle::new());
        // Transition to Running so drain works.
        lifecycle.start().unwrap();
        lifecycle.mark_running().unwrap();

        let (tx, mut rx) = broadcast::channel(1);
        let join = tokio::spawn(async move {
            let _ = rx.recv().await;
            ShutdownResult::Clean
        });
        let handle = ServerHandle::new(
            "127.0.0.1:0".parse().unwrap(),
            tx,
            join,
            lifecycle,
            crate::ops::OpsContext::default(),
        );
        handle.shutdown();
        // The task should complete after receiving the shutdown signal.
    }

    #[tokio::test]
    async fn handle_ready_returns_error_for_failed() {
        let lifecycle = Arc::new(Lifecycle::new());
        lifecycle.mark_failed().unwrap();

        let (tx, _rx) = broadcast::channel(1);
        let join = tokio::spawn(async { ShutdownResult::Clean });
        let handle = ServerHandle::new(
            "127.0.0.1:0".parse().unwrap(),
            tx,
            join,
            lifecycle,
            crate::ops::OpsContext::default(),
        );

        let result = handle.ready().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn handle_debug_format() {
        let handle = make_test_handle().await;
        let debug = format!("{handle:?}");
        assert!(debug.contains("ServerHandle"));
        assert!(debug.contains("127.0.0.1:8000"));
    }

    // --- Readiness correctness regression tests (Plan 121, Track C) ---

    #[tokio::test]
    async fn ready_already_running_returns_ok() {
        let lifecycle = Arc::new(Lifecycle::new());
        lifecycle.start().unwrap();
        lifecycle.mark_running().unwrap();
        assert_eq!(
            lifecycle.state(),
            crate::server::lifecycle::LifecycleState::Running
        );

        let (tx, _rx) = broadcast::channel(1);
        let join = tokio::spawn(async { ShutdownResult::Clean });
        let handle = ServerHandle::new(
            "127.0.0.1:0".parse().unwrap(),
            tx,
            join,
            lifecycle,
            crate::ops::OpsContext::default(),
        );

        let result = handle.ready().await;
        assert!(
            result.is_ok(),
            "ready() on already-Running server: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn ready_failed_returns_error() {
        let handle = make_handle_with_state(crate::server::lifecycle::LifecycleState::Failed);
        let result = handle.ready().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn ready_starting_then_running_succeeds() {
        let lifecycle = Arc::new(Lifecycle::new());
        lifecycle.start().unwrap();
        let (tx, _) = broadcast::channel(1);
        let join = tokio::spawn(async { ShutdownResult::Clean });
        let handle = ServerHandle::new(
            "127.0.0.1:0".parse().unwrap(),
            tx,
            join,
            lifecycle.clone(),
            crate::ops::OpsContext::default(),
        );

        // Transition to Running after a short delay.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            lifecycle.mark_running().unwrap();
        });

        let result = tokio::time::timeout(Duration::from_secs(5), handle.ready()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());
    }

    #[tokio::test]
    async fn ready_starting_then_failed_returns_error() {
        let lifecycle = Arc::new(Lifecycle::new());
        lifecycle.start().unwrap();
        let (tx, _) = broadcast::channel(1);
        let join = tokio::spawn(async { ShutdownResult::Clean });
        let handle = ServerHandle::new(
            "127.0.0.1:0".parse().unwrap(),
            tx,
            join,
            lifecycle.clone(),
            crate::ops::OpsContext::default(),
        );

        // Transition to Failed after a short delay.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            lifecycle.mark_failed().unwrap();
        });

        let result = handle.ready().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn ready_stuck_starting_times_out() {
        let handle = make_handle_with_state(crate::server::lifecycle::LifecycleState::Starting);
        let result = tokio::time::timeout(Duration::from_millis(50), handle.ready()).await;
        // Timeout fires; ready() was still awaiting.
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn ready_starting_then_drain_returns_error() {
        // Shutdown landing mid-startup must not leave ready() blocked
        // forever; the waiter observes Stopped and errors out.
        let lifecycle = Arc::new(Lifecycle::new());
        lifecycle.start().unwrap();
        let (tx, _) = broadcast::channel(1);
        let join = tokio::spawn(async { ShutdownResult::Clean });
        let handle = ServerHandle::new(
            "127.0.0.1:0".parse().unwrap(),
            tx,
            join,
            lifecycle.clone(),
            crate::ops::OpsContext::default(),
        );

        let drainer_lc = Arc::clone(&lifecycle);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = drainer_lc.drain();
        });

        let result = tokio::time::timeout(Duration::from_secs(5), handle.ready()).await;
        assert!(
            result.is_ok(),
            "ready() must not hang when shutdown lands mid-startup"
        );
        assert!(result.unwrap().is_err());
    }

    #[tokio::test]
    async fn ready_draining_is_error() {
        let handle = make_handle_with_state(crate::server::lifecycle::LifecycleState::Draining);
        let result = handle.ready().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn ready_stopped_is_error() {
        let handle = make_handle_with_state(crate::server::lifecycle::LifecycleState::Stopped);
        let result = handle.ready().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn ready_idempotent_on_running() {
        let handle = make_handle_with_state(crate::server::lifecycle::LifecycleState::Running);
        // Call ready() twice — both should succeed immediately.
        let r1 = tokio::time::timeout(Duration::from_millis(50), handle.ready()).await;
        assert!(r1.is_ok() && r1.unwrap().is_ok());
        // Re-use requires a new handle (ready takes &self, but we can call again).
        let r2 = tokio::time::timeout(Duration::from_millis(50), handle.ready()).await;
        assert!(r2.is_ok() && r2.unwrap().is_ok());
    }
}
