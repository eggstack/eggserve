//! Ops operational counters (Plan 215: moved from compatibility core).
//!
//! Owns [`OpsCounters`] and its bounded [`OpsSnapshot`]. Snapshots never
//! reset on read and involve no exporter; they are the explicit per-runtime
//! observability signal consumed via `OpsContext::snapshot()`.

use std::sync::atomic::{AtomicU64, Ordering};

/// Live, monotonically increasing runtime counters.
///
/// Every counter is a relaxed atomic; reads and writes never block and the
/// values are advisory, not exact under concurrency. Counters belong to one
/// runtime: an explicit [`super::OpsContext`] owns them, and the process-global
/// set is only the compatibility default.
#[derive(Debug)]
pub struct OpsCounters {
    /// Connections accepted by the listener and dispatched to a driver.
    pub connections_accepted: AtomicU64,
    /// Connections dropped before dispatch because the connection limit was
    /// saturated.
    pub connections_rejected: AtomicU64,
    /// Connections currently being served.
    pub active_connections: AtomicU64,
    /// File-stream responses currently in flight.
    pub active_file_streams: AtomicU64,
    /// Service invocations currently in flight.
    pub active_service_requests: AtomicU64,
    /// Connection tasks that ended because the connection panicked.
    pub connection_panics: AtomicU64,
    /// Requests refused by the HTTP/1 parser.
    pub parser_rejects: AtomicU64,
    /// Requests refused because the aggregate post-parse request-header byte
    /// ceiling was exceeded.
    pub header_bytes_rejected: AtomicU64,
    /// Requests refused because the request target exceeded its ceiling or
    /// used an unsupported form.
    pub request_target_rejected: AtomicU64,
    /// Service invocations refused because the in-flight request limit was
    /// saturated.
    pub service_admission_rejected: AtomicU64,
    /// Request bodies refused by the body policy or the body-size ceiling.
    pub body_rejections: AtomicU64,
    /// Connections closed because the header-read deadline expired.
    pub header_timeouts: AtomicU64,
    /// Request bodies that were not fully consumed before the body-read
    /// deadline.
    pub body_read_timeouts: AtomicU64,
    /// Keep-alive connections closed after the idle timeout elapsed.
    pub keepalive_idle_timeouts: AtomicU64,
    /// Connections closed after reaching the per-connection request limit.
    pub max_requests_closes: AtomicU64,
    /// Responses closed because no bytes were written within the
    /// response-write no-progress deadline.
    pub write_stall_timeouts: AtomicU64,
    /// Connections closed because the total connection lifetime expired.
    pub connection_total_timeouts: AtomicU64,
    /// Shutdowns that drained every connection within the grace budget.
    pub graceful_shutdowns: AtomicU64,
    /// Shutdowns that exceeded the drain budget and aborted the remaining
    /// connections.
    pub forced_shutdowns: AtomicU64,
    /// Accept-loop errors classified by the accept-error authority.
    pub listener_errors: AtomicU64,
    /// Events that could not be delivered because the sink panicked,
    /// including a panicking `enabled()` probe.
    ///
    /// A sink that filters an event out by severity has not *dropped* it:
    /// the event was never built and is not counted here.
    pub dropped_log_events: AtomicU64,
    /// Streaming responses that started producing body chunks.
    pub streaming_started: AtomicU64,
    /// Streaming responses that produced their full body and finished
    /// normally.
    pub streaming_completed: AtomicU64,
    /// Streaming responses whose declared length did not match the number of
    /// bytes emitted.
    pub stream_length_mismatches: AtomicU64,
    /// Streaming producers that failed with an error.
    pub stream_producer_errors: AtomicU64,
    /// Streaming producers that panicked; the panic was contained at the
    /// stream boundary.
    pub stream_producer_panics: AtomicU64,
    /// Streaming responses dropped before completion, for example after a
    /// client disconnect or a shutdown.
    pub stream_cancelled: AtomicU64,
    /// Request bodies handed to the service after the response had already
    /// started (deferred body ownership).
    pub deferred_bodies_delegated: AtomicU64,
    /// Deferred bodies that reached a complete terminal state.
    pub deferred_bodies_completed: AtomicU64,
    /// Deferred bodies that were abandoned or failed after the response
    /// started.
    pub deferred_bodies_abandoned: AtomicU64,
    /// Deferred bodies that hit the body-read deadline.
    pub deferred_body_timeouts: AtomicU64,
    /// Requests cancelled because the peer disconnected.
    pub lifecycle_peer_disconnects: AtomicU64,
    /// Requests cancelled by the runtime, for example by a shutdown or a
    /// connection timeout.
    pub lifecycle_runtime_cancels: AtomicU64,
    /// Tunnel handshakes that were admitted and upgraded.
    pub tunnels_accepted: AtomicU64,
    /// Tunnel handshakes refused because the active-tunnel limit was
    /// saturated.
    pub tunnels_rejected: AtomicU64,
    /// Tunnels currently active.
    pub active_tunnels: AtomicU64,
    /// Tunnel transport upgrades that were unavailable or failed.
    pub tunnel_upgrade_failures: AtomicU64,
    /// Connections whose PROXY protocol preamble was accepted.
    pub proxy_accepted: AtomicU64,
    /// Connections whose PROXY protocol preamble was rejected.
    pub proxy_rejected: AtomicU64,
    /// Requests whose forwarded metadata was accepted from a trusted peer.
    pub forwarded_accepted: AtomicU64,
    /// Requests whose forwarded metadata was rejected.
    pub forwarded_rejected: AtomicU64,
}

impl Default for OpsCounters {
    fn default() -> Self {
        Self::new()
    }
}

impl OpsCounters {
    /// Create a counter set with every counter at zero.
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

    /// Read every counter into a plain snapshot.
    ///
    /// The read never resets the counters and is not atomic across them: the
    /// snapshot may mix values observed at slightly different instants.
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

/// A point-in-time read of one runtime's counters.
///
/// Each field mirrors the identically named counter on [`OpsCounters`]. The
/// snapshot is a plain value: reading it never resets the underlying
/// counters and never blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpsSnapshot {
    /// Connections accepted by the listener and dispatched to a driver.
    pub connections_accepted: u64,
    /// Connections dropped before dispatch because the connection limit was
    /// saturated.
    pub connections_rejected: u64,
    /// Connections currently being served.
    pub active_connections: u64,
    /// File-stream responses currently in flight.
    pub active_file_streams: u64,
    /// Service invocations currently in flight.
    pub active_service_requests: u64,
    /// Connection tasks that ended because the connection panicked.
    pub connection_panics: u64,
    /// Requests refused by the HTTP/1 parser.
    pub parser_rejects: u64,
    /// Requests refused because the aggregate post-parse request-header byte
    /// ceiling was exceeded.
    pub header_bytes_rejected: u64,
    /// Requests refused because the request target exceeded its ceiling or
    /// used an unsupported form.
    pub request_target_rejected: u64,
    /// Service invocations refused because the in-flight request limit was
    /// saturated.
    pub service_admission_rejected: u64,
    /// Request bodies refused by the body policy or the body-size ceiling.
    pub body_rejections: u64,
    /// Connections closed because the header-read deadline expired.
    pub header_timeouts: u64,
    /// Request bodies that were not fully consumed before the body-read
    /// deadline.
    pub body_read_timeouts: u64,
    /// Keep-alive connections closed after the idle timeout elapsed.
    pub keepalive_idle_timeouts: u64,
    /// Connections closed after reaching the per-connection request limit.
    pub max_requests_closes: u64,
    /// Responses closed because no bytes were written within the
    /// response-write no-progress deadline.
    pub write_stall_timeouts: u64,
    /// Connections closed because the total connection lifetime expired.
    pub connection_total_timeouts: u64,
    /// Shutdowns that drained every connection within the grace budget.
    pub graceful_shutdowns: u64,
    /// Shutdowns that exceeded the drain budget and aborted the remaining
    /// connections.
    pub forced_shutdowns: u64,
    /// Accept-loop errors classified by the accept-error authority.
    pub listener_errors: u64,
    /// Events that could not be delivered because the sink panicked,
    /// including a panicking `enabled()` probe.
    ///
    /// A sink that filters an event out by severity has not *dropped* it:
    /// the event was never built and is not counted here.
    pub dropped_log_events: u64,
    /// Streaming responses that started producing body chunks.
    pub streaming_started: u64,
    /// Streaming responses that produced their full body and finished
    /// normally.
    pub streaming_completed: u64,
    /// Streaming responses whose declared length did not match the number of
    /// bytes emitted.
    pub stream_length_mismatches: u64,
    /// Streaming producers that failed with an error.
    pub stream_producer_errors: u64,
    /// Streaming producers that panicked; the panic was contained at the
    /// stream boundary.
    pub stream_producer_panics: u64,
    /// Streaming responses dropped before completion, for example after a
    /// client disconnect or a shutdown.
    pub stream_cancelled: u64,
    /// Request bodies handed to the service after the response had already
    /// started (deferred body ownership).
    pub deferred_bodies_delegated: u64,
    /// Deferred bodies that reached a complete terminal state.
    pub deferred_bodies_completed: u64,
    /// Deferred bodies that were abandoned or failed after the response
    /// started.
    pub deferred_bodies_abandoned: u64,
    /// Deferred bodies that hit the body-read deadline.
    pub deferred_body_timeouts: u64,
    /// Requests cancelled because the peer disconnected.
    pub lifecycle_peer_disconnects: u64,
    /// Requests cancelled by the runtime, for example by a shutdown or a
    /// connection timeout.
    pub lifecycle_runtime_cancels: u64,
    /// Tunnel handshakes that were admitted and upgraded.
    pub tunnels_accepted: u64,
    /// Tunnel handshakes refused because the active-tunnel limit was
    /// saturated.
    pub tunnels_rejected: u64,
    /// Tunnels currently active.
    pub active_tunnels: u64,
    /// Tunnel transport upgrades that were unavailable or failed.
    pub tunnel_upgrade_failures: u64,
    /// Connections whose PROXY protocol preamble was accepted.
    pub proxy_accepted: u64,
    /// Connections whose PROXY protocol preamble was rejected.
    pub proxy_rejected: u64,
    /// Requests whose forwarded metadata was accepted from a trusted peer.
    pub forwarded_accepted: u64,
    /// Requests whose forwarded metadata was rejected.
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
