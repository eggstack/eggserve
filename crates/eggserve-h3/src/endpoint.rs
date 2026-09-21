//! H3 endpoint/listener lifecycle (Plan 206 Track F).
//!
//! Owns connection-guard (`ActiveConnectionGuard`) and close-reason
//! mapping (`h3_connection_close_reason`, peer/shutdown cancellation via
//! the shared lifecycle registry). Listener startup stays in the parent.

use crate::quinn;

use eggserve_primitives::request_lifecycle::RequestCancellationReason;

pub(crate) struct ActiveConnectionGuard {
    pub(crate) ops: eggserve_server::ops::OpsContext,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.ops
            .counters()
            .active_connections
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(crate) fn h3_connection_close_reason(
    error: &quinn::ConnectionError,
) -> RequestCancellationReason {
    match error {
        quinn::ConnectionError::ApplicationClosed(_)
        | quinn::ConnectionError::ConnectionClosed(_)
        | quinn::ConnectionError::Reset => RequestCancellationReason::PeerDisconnected,
        quinn::ConnectionError::TimedOut => RequestCancellationReason::ConnectionTimeout,
        quinn::ConnectionError::LocallyClosed => RequestCancellationReason::ServerShutdown,
        _ => RequestCancellationReason::TransportFailure,
    }
}
