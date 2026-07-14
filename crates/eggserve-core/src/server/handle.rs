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
    pub async fn ready(&self) -> Result<(), ServerError> {
        // Check if already in a terminal failure state.
        let state = self.lifecycle.state();
        if state == crate::server::lifecycle::LifecycleState::Failed {
            return Err(ServerError::Startup("server failed during startup".into()));
        }

        self.lifecycle.wait_ready().await;

        // Re-check after waiting.
        let state = self.lifecycle.state();
        if state == crate::server::lifecycle::LifecycleState::Failed {
            return Err(ServerError::Startup("server failed during startup".into()));
        }
        if state == crate::server::lifecycle::LifecycleState::Running {
            Ok(())
        } else {
            Err(ServerError::Config(format!(
                "unexpected state after ready: {}",
                state
            )))
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
    /// server does not stop within `deadline`, the remaining tasks are
    /// abandoned (they will be cancelled when the runtime shuts down).
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
            Err(_deadline_exceeded) => Ok(ShutdownResult::Forced),
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
}
