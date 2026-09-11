//! Ops operational counters (Plan 206 Track H).
//!
//! Owns [`OpsCounters`] and its bounded [`OpsSnapshot`]. Snapshots never
//! reset on read and involve no exporter; they are the explicit per-runtime
//! observability signal consumed via `OpsContext::snapshot()`.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
pub struct OpsCounters {
    pub connections_accepted: AtomicU64,
    pub connections_rejected: AtomicU64,
    pub active_connections: AtomicU64,
    pub active_file_streams: AtomicU64,
    pub active_service_requests: AtomicU64,
    pub connection_panics: AtomicU64,
    pub parser_rejects: AtomicU64,
    pub header_bytes_rejected: AtomicU64,
    pub request_target_rejected: AtomicU64,
    pub service_admission_rejected: AtomicU64,
    pub body_rejections: AtomicU64,
    pub header_timeouts: AtomicU64,
    pub body_read_timeouts: AtomicU64,
    pub keepalive_idle_timeouts: AtomicU64,
    pub max_requests_closes: AtomicU64,
    pub write_stall_timeouts: AtomicU64,
    pub connection_total_timeouts: AtomicU64,
    pub graceful_shutdowns: AtomicU64,
    pub forced_shutdowns: AtomicU64,
    pub listener_errors: AtomicU64,
    pub dropped_log_events: AtomicU64,
    pub streaming_started: AtomicU64,
    pub streaming_completed: AtomicU64,
    pub stream_length_mismatches: AtomicU64,
    pub stream_producer_errors: AtomicU64,
    pub stream_producer_panics: AtomicU64,
    pub stream_cancelled: AtomicU64,
    pub deferred_bodies_delegated: AtomicU64,
    pub deferred_bodies_completed: AtomicU64,
    pub deferred_bodies_abandoned: AtomicU64,
    pub deferred_body_timeouts: AtomicU64,
    pub lifecycle_peer_disconnects: AtomicU64,
    pub lifecycle_runtime_cancels: AtomicU64,
    pub tunnels_accepted: AtomicU64,
    pub tunnels_rejected: AtomicU64,
    pub active_tunnels: AtomicU64,
    pub tunnel_upgrade_failures: AtomicU64,
    pub proxy_accepted: AtomicU64,
    pub proxy_rejected: AtomicU64,
    pub forwarded_accepted: AtomicU64,
    pub forwarded_rejected: AtomicU64,
}

impl Default for OpsCounters {
    fn default() -> Self {
        Self::new()
    }
}

impl OpsCounters {
    pub fn new() -> Self {
        Self {
            connections_accepted: AtomicU64::new(0),
            connections_rejected: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
            active_file_streams: AtomicU64::new(0),
            active_service_requests: AtomicU64::new(0),
            connection_panics: AtomicU64::new(0),
            parser_rejects: AtomicU64::new(0),
            header_bytes_rejected: AtomicU64::new(0),
            request_target_rejected: AtomicU64::new(0),
            service_admission_rejected: AtomicU64::new(0),
            body_rejections: AtomicU64::new(0),
            header_timeouts: AtomicU64::new(0),
            body_read_timeouts: AtomicU64::new(0),
            keepalive_idle_timeouts: AtomicU64::new(0),
            max_requests_closes: AtomicU64::new(0),
            write_stall_timeouts: AtomicU64::new(0),
            connection_total_timeouts: AtomicU64::new(0),
            graceful_shutdowns: AtomicU64::new(0),
            forced_shutdowns: AtomicU64::new(0),
            listener_errors: AtomicU64::new(0),
            dropped_log_events: AtomicU64::new(0),
            streaming_started: AtomicU64::new(0),
            streaming_completed: AtomicU64::new(0),
            stream_length_mismatches: AtomicU64::new(0),
            stream_producer_errors: AtomicU64::new(0),
            stream_producer_panics: AtomicU64::new(0),
            stream_cancelled: AtomicU64::new(0),
            deferred_bodies_delegated: AtomicU64::new(0),
            deferred_bodies_completed: AtomicU64::new(0),
            deferred_bodies_abandoned: AtomicU64::new(0),
            deferred_body_timeouts: AtomicU64::new(0),
            lifecycle_peer_disconnects: AtomicU64::new(0),
            lifecycle_runtime_cancels: AtomicU64::new(0),
            tunnels_accepted: AtomicU64::new(0),
            tunnels_rejected: AtomicU64::new(0),
            active_tunnels: AtomicU64::new(0),
            tunnel_upgrade_failures: AtomicU64::new(0),
            proxy_accepted: AtomicU64::new(0),
            proxy_rejected: AtomicU64::new(0),
            forwarded_accepted: AtomicU64::new(0),
            forwarded_rejected: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> OpsSnapshot {
        OpsSnapshot {
            connections_accepted: self.connections_accepted.load(Ordering::Relaxed),
            connections_rejected: self.connections_rejected.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            active_file_streams: self.active_file_streams.load(Ordering::Relaxed),
            active_service_requests: self.active_service_requests.load(Ordering::Relaxed),
            connection_panics: self.connection_panics.load(Ordering::Relaxed),
            parser_rejects: self.parser_rejects.load(Ordering::Relaxed),
            header_bytes_rejected: self.header_bytes_rejected.load(Ordering::Relaxed),
            request_target_rejected: self.request_target_rejected.load(Ordering::Relaxed),
            service_admission_rejected: self.service_admission_rejected.load(Ordering::Relaxed),
            body_rejections: self.body_rejections.load(Ordering::Relaxed),
            header_timeouts: self.header_timeouts.load(Ordering::Relaxed),
            body_read_timeouts: self.body_read_timeouts.load(Ordering::Relaxed),
            keepalive_idle_timeouts: self.keepalive_idle_timeouts.load(Ordering::Relaxed),
            max_requests_closes: self.max_requests_closes.load(Ordering::Relaxed),
            write_stall_timeouts: self.write_stall_timeouts.load(Ordering::Relaxed),
            connection_total_timeouts: self.connection_total_timeouts.load(Ordering::Relaxed),
            graceful_shutdowns: self.graceful_shutdowns.load(Ordering::Relaxed),
            forced_shutdowns: self.forced_shutdowns.load(Ordering::Relaxed),
            listener_errors: self.listener_errors.load(Ordering::Relaxed),
            dropped_log_events: self.dropped_log_events.load(Ordering::Relaxed),
            streaming_started: self.streaming_started.load(Ordering::Relaxed),
            streaming_completed: self.streaming_completed.load(Ordering::Relaxed),
            stream_length_mismatches: self.stream_length_mismatches.load(Ordering::Relaxed),
            stream_producer_errors: self.stream_producer_errors.load(Ordering::Relaxed),
            stream_producer_panics: self.stream_producer_panics.load(Ordering::Relaxed),
            stream_cancelled: self.stream_cancelled.load(Ordering::Relaxed),
            deferred_bodies_delegated: self.deferred_bodies_delegated.load(Ordering::Relaxed),
            deferred_bodies_completed: self.deferred_bodies_completed.load(Ordering::Relaxed),
            deferred_bodies_abandoned: self.deferred_bodies_abandoned.load(Ordering::Relaxed),
            deferred_body_timeouts: self.deferred_body_timeouts.load(Ordering::Relaxed),
            lifecycle_peer_disconnects: self.lifecycle_peer_disconnects.load(Ordering::Relaxed),
            lifecycle_runtime_cancels: self.lifecycle_runtime_cancels.load(Ordering::Relaxed),
            tunnels_accepted: self.tunnels_accepted.load(Ordering::Relaxed),
            tunnels_rejected: self.tunnels_rejected.load(Ordering::Relaxed),
            active_tunnels: self.active_tunnels.load(Ordering::Relaxed),
            tunnel_upgrade_failures: self.tunnel_upgrade_failures.load(Ordering::Relaxed),
            proxy_accepted: self.proxy_accepted.load(Ordering::Relaxed),
            proxy_rejected: self.proxy_rejected.load(Ordering::Relaxed),
            forwarded_accepted: self.forwarded_accepted.load(Ordering::Relaxed),
            forwarded_rejected: self.forwarded_rejected.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpsSnapshot {
    pub connections_accepted: u64,
    pub connections_rejected: u64,
    pub active_connections: u64,
    pub active_file_streams: u64,
    pub active_service_requests: u64,
    pub connection_panics: u64,
    pub parser_rejects: u64,
    pub header_bytes_rejected: u64,
    pub request_target_rejected: u64,
    pub service_admission_rejected: u64,
    pub body_rejections: u64,
    pub header_timeouts: u64,
    pub body_read_timeouts: u64,
    pub keepalive_idle_timeouts: u64,
    pub max_requests_closes: u64,
    pub write_stall_timeouts: u64,
    pub connection_total_timeouts: u64,
    pub graceful_shutdowns: u64,
    pub forced_shutdowns: u64,
    pub listener_errors: u64,
    pub dropped_log_events: u64,
    pub streaming_started: u64,
    pub streaming_completed: u64,
    pub stream_length_mismatches: u64,
    pub stream_producer_errors: u64,
    pub stream_producer_panics: u64,
    pub stream_cancelled: u64,
    pub deferred_bodies_delegated: u64,
    pub deferred_bodies_completed: u64,
    pub deferred_bodies_abandoned: u64,
    pub deferred_body_timeouts: u64,
    pub lifecycle_peer_disconnects: u64,
    pub lifecycle_runtime_cancels: u64,
    pub tunnels_accepted: u64,
    pub tunnels_rejected: u64,
    pub active_tunnels: u64,
    pub tunnel_upgrade_failures: u64,
    pub proxy_accepted: u64,
    pub proxy_rejected: u64,
    pub forwarded_accepted: u64,
    pub forwarded_rejected: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn ops_counters_snapshot() {
        let counters = OpsCounters::new();
        counters
            .connections_accepted
            .fetch_add(5, Ordering::Relaxed);
        counters.header_timeouts.fetch_add(1, Ordering::Relaxed);
        counters.listener_errors.fetch_add(1, Ordering::Relaxed);

        let snap = counters.snapshot();
        assert_eq!(snap.connections_accepted, 5);
        assert_eq!(snap.header_timeouts, 1);
        assert_eq!(snap.listener_errors, 1);
        assert_eq!(snap.connections_rejected, 0);
        assert_eq!(snap.active_connections, 0);
    }
}
