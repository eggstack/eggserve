//! Connection request-lifecycle registry (Plan 174 Track D).
//!
//! Each canonical request registers a weak observer of its shared lifecycle.
//! On abnormal connection termination the driver cancels all still-live
//! lifecycles with a best-effort reason so idle downstream waiters wake
//! without polling body/response IO. Completed requests prune lazily on next
//! registration; the list stays tiny because HTTP/1 processes one request at
//! a time (plus at most one deferred body).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::primitives::request_lifecycle::{RequestCancellationReason, RequestShared};

/// Per-connection registry of live request lifecycles (Plan 174 Track D).
///
/// Each canonical request registers a [`Weak`](std::sync::Weak) observer.
/// On abnormal connection termination the driver cancels all still-live
/// lifecycles with a best-effort reason so idle downstream waiters wake
/// without polling body/response IO. Completed requests prune lazily on
/// next registration; the list stays tiny because HTTP/1 processes one
/// request at a time (plus at most one deferred body).
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
    pub(crate) fn cancel_all(&self, reason: RequestCancellationReason, conn_id: u64) {
        let live: Vec<Arc<RequestShared>> = if let Ok(guard) = self.inner.lock() {
            guard.iter().filter_map(|w| w.upgrade()).collect()
        } else {
            Vec::new()
        };
        for shared in live {
            cancel_shared_with_observability(&shared, reason, conn_id);
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
) -> bool {
    if shared.is_cancelled() {
        return false;
    }
    shared.cancel(reason);
    match reason {
        RequestCancellationReason::PeerDisconnected => {
            crate::ops::global_counters()
                .lifecycle_peer_disconnects
                .fetch_add(1, Ordering::Relaxed);
            crate::ops::Logger::global().emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Debug,
                    crate::ops::EventKind::RequestLifecyclePeerDisconnect,
                    "request lifecycle peer disconnect",
                )
                .connection_id(conn_id),
            );
        }
        _ => {
            crate::ops::global_counters()
                .lifecycle_runtime_cancels
                .fetch_add(1, Ordering::Relaxed);
            crate::ops::Logger::global().emit(
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
