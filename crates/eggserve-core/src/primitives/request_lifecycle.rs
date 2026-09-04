//! Transport-neutral request lifecycle signaling (Plan 174).
//!
//! [`RequestLifecycle`] is a cloneable per-request observer that becomes
//! ready when the peer disconnects or the runtime cancels the request or
//! connection. It is transport-neutral: no Hyper, socket, or executor type
//! appears in the public contract.
//!
//! A long-polling or SSE-style application that is not currently polling
//! request/response IO should wait on [`RequestLifecycle::cancelled`] rather
//! than inferring cancellation from body polling or response-stream drop.
//!
//! # Ownership
//!
//! Each canonical [`Request`](super::request::Request) owns one lifecycle.
//! [`RequestBody`](super::request_body::RequestBody) shares the same internal
//! state so body completion, abandonment, and failure are visible without
//! holding the body itself. Cancellation is idempotent and race-safe: the
//! first reason wins.
//!
//! # Reason taxonomy
//!
//! Reasons are intentionally coarse. The runtime provides best-effort
//! classification; downstream adapters must only rely on "no longer usable",
//! not on precise TCP-reset vs EOF vs TLS-close distinction.
//!
//! # Send-side race
//!
//! A response producer may observe disconnect (stream poll/write failure or
//! drop) before a waiting task observes `cancelled()`. Lifecycle
//! cancellation follows promptly. Downstream adapters should treat either
//! path as cancellation.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

/// Internal body-ownership lifecycle state (Track A).
///
/// Distinguishes completion from abandonment and active delegated ownership.
/// The runtime retains an observer; the service owns/moves the actual
/// [`RequestBody`](super::request_body::RequestBody).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyLifecycleState {
    /// Body still owned (unread or streaming), possibly delegated to a
    /// downstream task past `Service::call` return.
    Active = 0,
    /// Body fully consumed with framing validation succeeding.
    Complete = 1,
    /// Incomplete body explicitly dropped without EOF.
    Abandoned = 2,
    /// Transport/body error terminated consumption.
    Failed = 3,
}

impl BodyLifecycleState {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Active,
            1 => Self::Complete,
            2 => Self::Abandoned,
            _ => Self::Failed,
        }
    }
}

/// Best-effort, transport-neutral cancellation reason (Track D1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestCancellationReason {
    /// Peer transport loss (TCP reset/EOF, TLS close, client disconnect).
    PeerDisconnected,
    /// Runtime-forced connection close via graceful shutdown.
    ServerShutdown,
    /// Hard connection lifetime, header, idle, write, or body-read timeout.
    ConnectionTimeout,
    /// Transport/body failure that makes further application IO impossible.
    TransportFailure,
}

impl std::fmt::Display for RequestCancellationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PeerDisconnected => write!(f, "peer disconnected"),
            Self::ServerShutdown => write!(f, "server shutdown"),
            Self::ConnectionTimeout => write!(f, "connection timeout"),
            Self::TransportFailure => write!(f, "transport failure"),
        }
    }
}

/// Shared per-request state backing both [`RequestBody`](super::request_body::RequestBody)
/// ownership tracking and [`RequestLifecycle`] cancellation.
///
/// One allocation per streaming request. State observers do not require
/// holding the `RequestBody` itself.
#[derive(Debug)]
pub(crate) struct RequestShared {
    body_state: AtomicU8,
    cancelled: AtomicBool,
    reason: Mutex<Option<RequestCancellationReason>>,
    /// Notified when body reaches a terminal state (Complete/Abandoned/Failed).
    body_notify: tokio::sync::Notify,
    /// Notified when cancellation fires.
    cancel_notify: tokio::sync::Notify,
}

impl RequestShared {
    pub(crate) fn new_active() -> Arc<Self> {
        Arc::new(Self {
            body_state: AtomicU8::new(BodyLifecycleState::Active as u8),
            cancelled: AtomicBool::new(false),
            reason: Mutex::new(None),
            body_notify: tokio::sync::Notify::new(),
            cancel_notify: tokio::sync::Notify::new(),
        })
    }

    pub(crate) fn new_complete() -> Arc<Self> {
        let shared = Self::new_active();
        shared.mark_complete();
        shared
    }

    pub(crate) fn body_state(&self) -> BodyLifecycleState {
        BodyLifecycleState::from_u8(self.body_state.load(Ordering::Acquire))
    }

    pub(crate) fn is_body_active(&self) -> bool {
        self.body_state() == BodyLifecycleState::Active
    }

    pub(crate) fn is_body_complete(&self) -> bool {
        self.body_state() == BodyLifecycleState::Complete
    }

    pub(crate) fn is_body_terminal(&self) -> bool {
        self.body_state() != BodyLifecycleState::Active
    }

    /// Mark body Complete after declared-length/framing validation succeeds.
    /// Only transitions from Active; idempotent otherwise.
    pub(crate) fn mark_complete(&self) -> bool {
        let res = self.body_state.compare_exchange(
            BodyLifecycleState::Active as u8,
            BodyLifecycleState::Complete as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        if res.is_ok() {
            self.body_notify.notify_waiters();
            true
        } else {
            false
        }
    }

    /// Mark body Abandoned when an incomplete network body is dropped.
    /// Only transitions from Active; preserves Complete/Failed.
    pub(crate) fn mark_abandoned(&self) -> bool {
        let res = self.body_state.compare_exchange(
            BodyLifecycleState::Active as u8,
            BodyLifecycleState::Abandoned as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        if res.is_ok() {
            self.body_notify.notify_waiters();
            true
        } else {
            false
        }
    }

    /// Mark body Failed on transport/body error. Also cancels lifecycle
    /// with TransportFailure if not already cancelled (first reason wins).
    pub(crate) fn mark_failed(&self) -> bool {
        let res = self.body_state.compare_exchange(
            BodyLifecycleState::Active as u8,
            BodyLifecycleState::Failed as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        if res.is_ok() {
            self.body_notify.notify_waiters();
            self.cancel(RequestCancellationReason::TransportFailure);
            true
        } else {
            false
        }
    }

    /// Mark body Failed with an explicit cancellation reason (e.g. body-read
    /// timeout maps to ConnectionTimeout). First cancellation reason wins.
    pub(crate) fn mark_failed_with_reason(&self, reason: RequestCancellationReason) -> bool {
        let res = self.body_state.compare_exchange(
            BodyLifecycleState::Active as u8,
            BodyLifecycleState::Failed as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        if res.is_ok() {
            self.body_notify.notify_waiters();
            self.cancel(reason);
            true
        } else {
            false
        }
    }

    /// Cancel lifecycle with a reason. Idempotent; first reason wins.
    pub(crate) fn cancel(&self, reason: RequestCancellationReason) {
        if !self.cancelled.swap(true, Ordering::AcqRel) {
            if let Ok(mut guard) = self.reason.lock() {
                *guard = Some(reason);
            }
            self.cancel_notify.notify_waiters();
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(crate) fn cancellation_reason(&self) -> Option<RequestCancellationReason> {
        self.reason.lock().ok().and_then(|g| *g)
    }

    /// Wait until body reaches a terminal state.
    pub(crate) async fn wait_body_terminal(&self) {
        loop {
            if self.is_body_terminal() {
                return;
            }
            self.body_notify.notified().await;
        }
    }

    /// Wait until cancellation fires.
    pub(crate) async fn wait_cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            self.cancel_notify.notified().await;
        }
    }
}

/// Transport-neutral per-request disconnect/cancellation observer (Track D).
///
/// Cloneable token associated with each canonical `Request`. Becomes ready on
/// peer transport loss, runtime-forced close, hard timeout, shutdown
/// cancellation after drain policy, or body/transport failure. Does NOT fire
/// merely because `Service::call` returned a streaming response, the request
/// body reached EOF normally, or the response completed normally while the
/// keep-alive connection remains valid.
///
/// A response producer may discover disconnect before a waiter observes
/// `cancelled()`; preserve the race rather than imposing synchronization.
/// Downstream adapters should fail send operations once either path
/// establishes cancellation.
#[derive(Debug, Clone)]
pub struct RequestLifecycle {
    shared: Arc<RequestShared>,
}

impl RequestLifecycle {
    pub(crate) fn from_shared(shared: Arc<RequestShared>) -> Self {
        Self { shared }
    }

    /// Wait until the request/connection is no longer usable.
    pub async fn cancelled(&self) {
        self.shared.wait_cancelled().await;
    }

    /// Returns `true` once cancellation has fired.
    pub fn is_cancelled(&self) -> bool {
        self.shared.is_cancelled()
    }

    /// Best-effort cancellation reason, if cancelled.
    pub fn cancellation_reason(&self) -> Option<RequestCancellationReason> {
        self.shared.cancellation_reason()
    }

    /// Returns `true` when the associated request body reached EOF normally.
    pub fn is_body_complete(&self) -> bool {
        self.shared.is_body_complete()
    }

    /// Returns `true` while the body is still owned (unread/streaming),
    /// including delegated ownership past `Service::call` return.
    pub fn is_body_active(&self) -> bool {
        self.shared.is_body_active()
    }

    #[allow(dead_code)]
    pub(crate) fn shared(&self) -> &Arc<RequestShared> {
        &self.shared
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_active_uncancelled() {
        let shared = RequestShared::new_active();
        assert_eq!(shared.body_state(), BodyLifecycleState::Active);
        assert!(shared.is_body_active());
        assert!(!shared.is_body_complete());
        assert!(!shared.is_body_terminal());
        assert!(!shared.is_cancelled());
        assert_eq!(shared.cancellation_reason(), None);
    }

    #[test]
    fn complete_transition() {
        let shared = RequestShared::new_active();
        assert!(shared.mark_complete());
        assert_eq!(shared.body_state(), BodyLifecycleState::Complete);
        assert!(shared.is_body_complete());
        assert!(shared.is_body_terminal());
        // Idempotent: second call fails, state preserved.
        assert!(!shared.mark_complete());
        assert!(!shared.mark_abandoned());
    }

    #[test]
    fn abandon_transition() {
        let shared = RequestShared::new_active();
        assert!(shared.mark_abandoned());
        assert_eq!(shared.body_state(), BodyLifecycleState::Abandoned);
        assert!(shared.is_body_terminal());
        assert!(!shared.is_body_complete());
    }

    #[test]
    fn fail_cancels_with_transport_failure() {
        let shared = RequestShared::new_active();
        assert!(shared.mark_failed());
        assert_eq!(shared.body_state(), BodyLifecycleState::Failed);
        assert!(shared.is_cancelled());
        assert_eq!(
            shared.cancellation_reason(),
            Some(RequestCancellationReason::TransportFailure)
        );
    }

    #[test]
    fn cancel_first_reason_wins() {
        let shared = RequestShared::new_active();
        shared.cancel(RequestCancellationReason::PeerDisconnected);
        shared.cancel(RequestCancellationReason::ServerShutdown);
        assert_eq!(
            shared.cancellation_reason(),
            Some(RequestCancellationReason::PeerDisconnected)
        );
    }

    #[tokio::test]
    async fn lifecycle_cancelled_future_resolves() {
        let shared = RequestShared::new_active();
        let lc = RequestLifecycle::from_shared(shared.clone());
        assert!(!lc.is_cancelled());
        shared.cancel(RequestCancellationReason::ServerShutdown);
        tokio::time::timeout(std::time::Duration::from_secs(1), lc.cancelled())
            .await
            .expect("cancelled() must resolve promptly");
        assert_eq!(
            lc.cancellation_reason(),
            Some(RequestCancellationReason::ServerShutdown)
        );
    }

    #[tokio::test]
    async fn wait_body_terminal_resolves_on_complete() {
        let shared = RequestShared::new_active();
        let waiter = {
            let shared = shared.clone();
            tokio::spawn(async move { shared.wait_body_terminal().await })
        };
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        shared.mark_complete();
        tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("body terminal waiter must resolve")
            .unwrap();
    }
}
