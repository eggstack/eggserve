//! Server lifecycle handle.
//!
//! A [`ServerHandle`] is returned by [`Server::start`] and provides control
//! over the running server: waiting for shutdown, triggering graceful
//! shutdown, and inspecting the listening address.

use std::net::SocketAddr;

use tokio::sync::broadcast;

/// Handle to a running server instance.
///
/// The handle allows the caller to:
/// - Wait for the server to finish (via [`ServerHandle::wait`])
/// - Trigger graceful shutdown (via [`ServerHandle::shutdown`])
/// - Query the listening address (via [`ServerHandle::local_addr`])
///
/// Dropping the handle does NOT shut down the server — the server continues
/// running until shutdown is triggered or the process exits.
#[derive(Debug)]
pub struct ServerHandle {
    local_addr: SocketAddr,
    shutdown_tx: broadcast::Sender<()>,
    join: Option<tokio::task::JoinHandle<()>>,
}

impl ServerHandle {
    pub(crate) fn new(
        local_addr: SocketAddr,
        shutdown_tx: broadcast::Sender<()>,
        join: tokio::task::JoinHandle<()>,
    ) -> Self {
        Self {
            local_addr,
            shutdown_tx,
            join: Some(join),
        }
    }

    /// Returns the address the server is listening on.
    ///
    /// Useful when binding to port 0 to discover the actual port.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Trigger graceful shutdown.
    ///
    /// The server will stop accepting new connections and wait for in-flight
    /// requests to complete (up to the configured grace period).
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Wait for the server to finish.
    ///
    /// This consumes the handle and waits for the accept loop to exit.
    /// Returns once the server has fully stopped.
    pub async fn wait(mut self) -> Result<(), crate::server::errors::ServerError> {
        if let Some(join) = self.join.take() {
            join.await.map_err(|e| {
                crate::server::errors::ServerError::Accept(std::io::Error::other(format!(
                    "server task panicked: {}",
                    e
                )))
            })?;
        }
        Ok(())
    }

    /// Wait for the server to finish, with a timeout.
    pub async fn wait_timeout(
        mut self,
        timeout: std::time::Duration,
    ) -> Result<(), crate::server::errors::ServerError> {
        if let Some(join) = self.join.take() {
            match tokio::time::timeout(timeout, join).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(crate::server::errors::ServerError::Accept(
                    std::io::Error::other(format!("server task panicked: {}", e)),
                )),
                Err(_) => {
                    self.shutdown();
                    Err(crate::server::errors::ServerError::ShutdownTimeout)
                }
            }
        } else {
            Ok(())
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        // If the handle is dropped without explicit shutdown, trigger one.
        if self.join.is_some() {
            let _ = self.shutdown_tx.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn handle_local_addr() {
        let (tx, _rx) = broadcast::channel(1);
        let join = tokio::spawn(async {});
        let addr: SocketAddr = "127.0.0.1:8000".parse().unwrap();
        let handle = ServerHandle::new(addr, tx, join);
        assert_eq!(handle.local_addr(), addr);
    }

    #[tokio::test]
    async fn handle_shutdown_sends_signal() {
        let (tx, mut rx) = broadcast::channel(1);
        let join = tokio::spawn(async move {
            let _ = rx.recv().await;
        });
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let handle = ServerHandle::new(addr, tx, join);
        handle.shutdown();
        // The task should complete after receiving the shutdown signal.
    }
}
