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
/// - Query the listening address (via [`ServerHandle::local_addr`])
/// - Wait for completion (via [`ServerHandle::wait`])
///
/// Dropping the handle triggers graceful shutdown — the server stops
/// accepting new connections and drains in-flight requests.
pub struct ServerHandle {
    local_addr: SocketAddr,
    shutdown_tx: broadcast::Sender<()>,
    join: Option<tokio::task::JoinHandle<ShutdownResult>>,
    lifecycle: std::sync::Arc<Lifecycle>,
}

impl std::fmt::Debug for ServerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerHandle")
            .field("local_addr", &self.local_addr)
            .field("state", &self.lifecycle.state())
            .finish()
    }
}

impl ServerHandle {
    pub(crate) fn new(
        local_addr: SocketAddr,
        shutdown_tx: broadcast::Sender<()>,
        join: tokio::task::JoinHandle<ShutdownResult>,
        lifecycle: std::sync::Arc<Lifecycle>,
    ) -> Self {
        Self {
            local_addr,
            shutdown_tx,
            join: Some(join),
            lifecycle,
        }
    }

    /// Returns the address the server is listening on.
    ///
    /// Useful when binding to port 0 to discover the actual port.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
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
        let _ = self.lifecycle.drain();
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
                            "server task panicked: {}",
                            e
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
                    "server task panicked: {}",
                    e
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
            let _ = self.lifecycle.drain();
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
        ServerHandle::new("127.0.0.1:8000".parse().unwrap(), tx, join, lifecycle)
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
        ServerHandle::new("127.0.0.1:0".parse().unwrap(), shutdown_tx, join, lifecycle)
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
        let handle = ServerHandle::new("127.0.0.1:0".parse().unwrap(), tx, join, lifecycle);
        handle.shutdown();
        // The task should complete after receiving the shutdown signal.
    }

    #[tokio::test]
    async fn handle_ready_returns_error_for_failed() {
        let lifecycle = Arc::new(Lifecycle::new());
        lifecycle.mark_failed().unwrap();

        let (tx, _rx) = broadcast::channel(1);
        let join = tokio::spawn(async { ShutdownResult::Clean });
        let handle = ServerHandle::new("127.0.0.1:0".parse().unwrap(), tx, join, lifecycle);

        let result = handle.ready().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn handle_debug_format() {
        let handle = make_test_handle().await;
        let debug = format!("{:?}", handle);
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
        let handle = ServerHandle::new("127.0.0.1:0".parse().unwrap(), tx, join, lifecycle);

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
        let handle = ServerHandle::new("127.0.0.1:0".parse().unwrap(), tx, join, lifecycle.clone());

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
        let handle = ServerHandle::new("127.0.0.1:0".parse().unwrap(), tx, join, lifecycle.clone());

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
