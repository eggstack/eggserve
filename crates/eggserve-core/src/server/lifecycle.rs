//! Lifecycle state machine for the HTTP server.
//!
//! The server follows a strict state progression:
//!
//! ```text
//! Created → Starting → Running → Draining → Stopped
//!             ↓                    ↓
//!          Failed               Failed
//! ```
//!
//! # State transitions
//!
//! - **Created → Starting**: `Server::start()` is called
//! - **Starting → Running**: listener bound, accept loop polled, readiness signaled
//! - **Starting → Failed**: bind, configuration, or accept-loop startup failure
//! - **Running → Draining**: `ServerHandle::shutdown()` is called
//! - **Draining → Stopped**: all in-flight connections complete or deadline expires
//! - **Draining → Failed**: fatal runtime error during drain
//!
//! # Allowed operations per state
//!
//! | State     | build | start | ready | shutdown | force_shutdown | wait |
//! |-----------|-------|-------|-------|----------|---------------|------|
//! | Created   | yes   | yes   | -     | noop     | noop          | err  |
//! | Starting  | -     | err   | yes   | pending  | pending       | err  |
//! | Running   | -     | err   | ok    | ok       | ok            | yes  |
//! | Draining  | -     | err   | err   | idempot  | ok            | yes  |
//! | Stopped   | -     | err   | err   | noop     | noop          | ok   |
//! | Failed    | -     | err   | err   | noop     | noop          | err  |

use std::sync::atomic::{AtomicU8, Ordering};

use tokio::sync::{broadcast, watch};

/// Server lifecycle states.
///
/// Each state is represented as a `u8` for atomic storage.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifecycleState {
    /// Initial state after `ServerBuilder::build()`.
    Created = 0,
    /// `Server::start()` has been called; binding and accept-loop init in progress.
    Starting = 1,
    /// Listener bound, accept loop running, ready to accept connections.
    Running = 2,
    /// Shutdown requested; draining in-flight connections.
    Draining = 3,
    /// All connections drained; terminal state.
    Stopped = 4,
    /// Fatal error during startup or drain; terminal state.
    Failed = 5,
}

impl LifecycleState {
    /// Convert from raw atomic value.
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Created,
            1 => Self::Starting,
            2 => Self::Running,
            3 => Self::Draining,
            4 => Self::Stopped,
            5 => Self::Failed,
            _ => Self::Failed,
        }
    }

    /// Whether this is a terminal state (no further transitions expected).
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Stopped | Self::Failed)
    }
}

impl std::fmt::Display for LifecycleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created => write!(f, "created"),
            Self::Starting => write!(f, "starting"),
            Self::Running => write!(f, "running"),
            Self::Draining => write!(f, "draining"),
            Self::Stopped => write!(f, "stopped"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// Shared lifecycle state with atomic transitions and channel notifications.
#[derive(Debug)]
pub(crate) struct Lifecycle {
    state: AtomicU8,
    ready_tx: watch::Sender<bool>,
    /// Notified when a terminal state (Stopped/Failed) is reached.
    terminal_tx: broadcast::Sender<()>,
}

impl Lifecycle {
    /// Create a new lifecycle in the `Created` state.
    pub fn new() -> Self {
        let (ready_tx, _) = watch::channel(false);
        let (terminal_tx, _) = broadcast::channel(1);
        Self {
            state: AtomicU8::new(LifecycleState::Created as u8),
            ready_tx,
            terminal_tx,
        }
    }

    /// Get the current state.
    pub fn state(&self) -> LifecycleState {
        LifecycleState::from_u8(self.state.load(Ordering::Acquire))
    }

    /// Transition to `Starting`. Fails if not in `Created`.
    pub fn start(&self) -> Result<(), crate::server::errors::ServerError> {
        let prev = self.state.compare_exchange(
            LifecycleState::Created as u8,
            LifecycleState::Starting as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        match prev {
            Ok(_) => Ok(()),
            Err(actual) => {
                let state = LifecycleState::from_u8(actual);
                if matches!(state, LifecycleState::Running | LifecycleState::Starting) {
                    Err(crate::server::errors::ServerError::AlreadyStarted)
                } else {
                    Err(crate::server::errors::ServerError::Config(format!(
                        "cannot start: server is in {} state",
                        state
                    )))
                }
            }
        }
    }

    /// Transition to `Running`. Fails if not in `Starting`.
    pub fn mark_running(&self) -> Result<(), crate::server::errors::ServerError> {
        let prev = self.state.compare_exchange(
            LifecycleState::Starting as u8,
            LifecycleState::Running as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        match prev {
            Ok(_) => {
                let _ = self.ready_tx.send(true);
                Ok(())
            }
            Err(actual) => Err(crate::server::errors::ServerError::Config(format!(
                "cannot mark running: server is in {} state",
                LifecycleState::from_u8(actual)
            ))),
        }
    }

    /// Transition to `Draining`. Fails if not in `Running`.
    pub fn drain(&self) -> Result<(), crate::server::errors::ServerError> {
        let prev = self.state.compare_exchange(
            LifecycleState::Running as u8,
            LifecycleState::Draining as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        match prev {
            Ok(_) => Ok(()),
            Err(actual) => {
                let state = LifecycleState::from_u8(actual);
                if state == LifecycleState::Created || state == LifecycleState::Starting {
                    // Shutdown before start: no-op
                    Ok(())
                } else if state.is_terminal() {
                    Ok(())
                } else {
                    Err(crate::server::errors::ServerError::Config(format!(
                        "cannot drain: server is in {} state",
                        state
                    )))
                }
            }
        }
    }

    /// Transition to `Stopped`. Fails if not in `Draining`.
    pub fn mark_stopped(&self) -> Result<(), crate::server::errors::ServerError> {
        let prev = self.state.compare_exchange(
            LifecycleState::Draining as u8,
            LifecycleState::Stopped as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        match prev {
            Ok(_) => {
                let _ = self.terminal_tx.send(());
                Ok(())
            }
            Err(actual) => {
                let state = LifecycleState::from_u8(actual);
                if state.is_terminal() {
                    Ok(())
                } else {
                    Err(crate::server::errors::ServerError::Config(format!(
                        "cannot stop: server is in {} state",
                        state
                    )))
                }
            }
        }
    }

    /// Transition to `Failed` from any non-terminal state.
    #[allow(dead_code)]
    pub fn mark_failed(&self) -> Result<(), crate::server::errors::ServerError> {
        let current = self.state.load(Ordering::Acquire);
        let current_state = LifecycleState::from_u8(current);
        if current_state.is_terminal() {
            return Ok(());
        }
        self.state
            .store(LifecycleState::Failed as u8, Ordering::Release);
        let _ = self.terminal_tx.send(());
        Ok(())
    }

    /// Wait for readiness (transition to `Running`).
    pub async fn wait_ready(&self) {
        let mut rx = self.ready_tx.subscribe();
        // If already ready, return immediately.
        if *rx.borrow() {
            return;
        }
        let _ = rx.changed().await;
    }

    /// Subscribe to terminal state notifications.
    pub fn subscribe_terminal(&self) -> broadcast::Receiver<()> {
        self.terminal_tx.subscribe()
    }

    /// Check if the state matches the expected state.
    #[allow(dead_code)]
    pub fn is(&self, expected: LifecycleState) -> bool {
        self.state() == expected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_created() {
        let lc = Lifecycle::new();
        assert_eq!(lc.state(), LifecycleState::Created);
        assert!(!lc.state().is_terminal());
    }

    #[test]
    fn valid_transitions() {
        let lc = Lifecycle::new();
        assert!(lc.start().is_ok());
        assert_eq!(lc.state(), LifecycleState::Starting);

        assert!(lc.mark_running().is_ok());
        assert_eq!(lc.state(), LifecycleState::Running);

        assert!(lc.drain().is_ok());
        assert_eq!(lc.state(), LifecycleState::Draining);

        assert!(lc.mark_stopped().is_ok());
        assert_eq!(lc.state(), LifecycleState::Stopped);
        assert!(lc.state().is_terminal());
    }

    #[test]
    fn double_start_fails() {
        let lc = Lifecycle::new();
        assert!(lc.start().is_ok());
        assert!(lc.mark_running().is_ok());
        let err = lc.start().unwrap_err();
        assert!(err.to_string().contains("already started"));
    }

    #[test]
    fn shutdown_before_start_is_noop() {
        let lc = Lifecycle::new();
        assert!(lc.drain().is_ok());
        assert_eq!(lc.state(), LifecycleState::Created);
    }

    #[test]
    fn mark_failed_from_any_non_terminal() {
        let lc = Lifecycle::new();
        assert!(lc.mark_failed().is_ok());
        assert_eq!(lc.state(), LifecycleState::Failed);
        assert!(lc.state().is_terminal());
    }

    #[test]
    fn mark_stopped_from_already_stopped_is_ok() {
        let lc = Lifecycle::new();
        assert!(lc.start().is_ok());
        assert!(lc.mark_running().is_ok());
        assert!(lc.drain().is_ok());
        assert!(lc.mark_stopped().is_ok());
        assert!(lc.mark_stopped().is_ok());
    }

    #[test]
    fn lifecycle_state_display() {
        assert_eq!(LifecycleState::Created.to_string(), "created");
        assert_eq!(LifecycleState::Running.to_string(), "running");
        assert_eq!(LifecycleState::Draining.to_string(), "draining");
        assert_eq!(LifecycleState::Stopped.to_string(), "stopped");
        assert_eq!(LifecycleState::Failed.to_string(), "failed");
    }
}
