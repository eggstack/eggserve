//! Connection request-lifecycle registry (Plan 174 Track D).
//!
//! Each canonical request registers a weak observer of its shared lifecycle.
//! On abnormal connection termination the driver cancels all still-live
//! lifecycles with a best-effort reason so idle downstream waiters wake
//! without polling body/response IO. Completed requests prune lazily on next
//! registration; multiplexed H2/H3 connections may retain several weak
//! observers, so registration also removes entries whose request ownership
//! has ended.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::primitives::request_lifecycle::{RequestCancellationReason, RequestShared};

/// Protocol-neutral result of a request lifecycle decision.
///
/// The shared request/service policy records what must happen to ownership or
/// transport state. A protocol adapter maps this value to wire behavior. The
/// HTTP/1 adapter currently expresses `close_after_response` as
/// `Connection: close`; HTTP/2/3 adapters can instead reset a stream or begin
/// connection drain without changing service code.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LifecycleDisposition {
    close_after_response: bool,
    cancel_request_body: bool,
    graceful_drain: bool,
    terminate_transport: bool,
}

impl LifecycleDisposition {
    pub(crate) const KEEP_ALIVE: Self = Self {
        close_after_response: false,
        cancel_request_body: false,
        graceful_drain: false,
        terminate_transport: false,
    };

    pub(crate) const fn close_after_response() -> Self {
        Self {
            close_after_response: true,
            ..Self::KEEP_ALIVE
        }
    }

    pub(crate) const fn close_and_cancel_body() -> Self {
        Self {
            close_after_response: true,
            cancel_request_body: true,
            ..Self::KEEP_ALIVE
        }
    }

    pub(crate) const fn with_graceful_drain(mut self) -> Self {
        self.graceful_drain = true;
        self
    }

    pub(crate) const fn with_transport_termination(mut self) -> Self {
        self.terminate_transport = true;
        self
    }

    pub(crate) const fn close_after_response_required(self) -> bool {
        self.close_after_response
    }

    #[allow(dead_code)]
    pub(crate) const fn request_body_cancellation_required(self) -> bool {
        self.cancel_request_body
    }

    #[allow(dead_code)]
    pub(crate) const fn graceful_drain_required(self) -> bool {
        self.graceful_drain
    }

    #[allow(dead_code)]
    pub(crate) const fn transport_termination_required(self) -> bool {
        self.terminate_transport
    }
}

/// Per-connection registry of live request lifecycles (Plan 174 Track D).
///
/// Each canonical request registers a [`Weak`](std::sync::Weak) observer.
/// On abnormal connection termination the driver cancels all still-live
/// lifecycles with a best-effort reason so idle downstream waiters wake
/// without polling body/response IO. Completed requests prune lazily on
/// next registration; multiplexed H2/H3 connections may retain several
/// weak observers concurrently, while dead entries are removed on each
/// registration.
#[derive(Debug, Default)]
pub(crate) struct ConnectionRequests {
    inner: std::sync::Mutex<Vec<std::sync::Weak<RequestShared>>>,
}

impl ConnectionRequests {
    pub(crate) fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Register a new lifecycle observer, pruning dead entries.
    pub(crate) fn register(&self, shared: &Arc<RequestShared>) {
        let weak = Arc::downgrade(shared);
        if let Ok(mut guard) = self.inner.lock() {
            guard.retain(|w| w.upgrade().is_some());
            guard.push(weak);
        }
    }

    /// Cancel all still-live lifecycles with observability.
    ///
    /// Already-cancelled lifecycles are skipped (first reason wins).
    pub(crate) fn cancel_all(
        &self,
        reason: RequestCancellationReason,
        conn_id: u64,
        ops: &crate::ops::OpsContext,
    ) {
        let live: Vec<Arc<RequestShared>> = if let Ok(guard) = self.inner.lock() {
            guard.iter().filter_map(|w| w.upgrade()).collect()
        } else {
            Vec::new()
        };
        for shared in live {
            cancel_shared_with_observability(&shared, reason, conn_id, ops);
        }
    }
}

/// Cancel one lifecycle with narrow observability (Plan 174).
///
/// Idempotent: already-cancelled lifecycles are skipped so the first reason
/// wins under cancellation races.
pub(crate) fn cancel_shared_with_observability(
    shared: &Arc<RequestShared>,
    reason: RequestCancellationReason,
    conn_id: u64,
    ops: &crate::ops::OpsContext,
) -> bool {
    if shared.is_cancelled() {
        return false;
    }
    shared.cancel(reason);
    match reason {
        RequestCancellationReason::PeerDisconnected => {
            ops.counters()
                .lifecycle_peer_disconnects
                .fetch_add(1, Ordering::Relaxed);
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Debug,
                    crate::ops::EventKind::RequestLifecyclePeerDisconnect,
                    "request lifecycle peer disconnect",
                )
                .connection_id(conn_id),
            );
        }
        _ => {
            ops.counters()
                .lifecycle_runtime_cancels
                .fetch_add(1, Ordering::Relaxed);
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Debug,
                    crate::ops::EventKind::RequestLifecycleRuntimeCancel,
                    format!("request lifecycle cancelled: {reason}"),
                )
                .connection_id(conn_id),
            );
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::request_lifecycle::RequestLifecycle;

    #[tokio::test]
    async fn registry_cancels_multiple_live_request_observers() {
        let registry = ConnectionRequests::new();
        let first = RequestShared::new_active();
        let second = RequestShared::new_active();
        let first_lifecycle = RequestLifecycle::from_shared(first.clone());
        let second_lifecycle = RequestLifecycle::from_shared(second.clone());
        registry.register(&first);
        registry.register(&second);

        registry.cancel_all(
            RequestCancellationReason::PeerDisconnected,
            17,
            crate::ops::OpsContext::global(),
        );

        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            first_lifecycle.cancelled(),
        )
        .await
        .expect("first H2/H3 lifecycle should wake");
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            second_lifecycle.cancelled(),
        )
        .await
        .expect("second H2/H3 lifecycle should wake");
        assert_eq!(
            first_lifecycle.cancellation_reason(),
            Some(RequestCancellationReason::PeerDisconnected)
        );
        assert_eq!(
            second_lifecycle.cancellation_reason(),
            Some(RequestCancellationReason::PeerDisconnected)
        );
    }
}
