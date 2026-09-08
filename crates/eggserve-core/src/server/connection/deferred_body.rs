//! Deferred request-body supervision (Plan 174 Tracks B2/C).
//!
//! The watchdog enforces `body_read_timeout` as a total deadline from request
//! start, including consumption that continues after `Service::call` returns
//! response-start. The completion tracker observes terminal state for idle
//! accounting and observability. Neither holds the service admission permit:
//! `max_in_flight_requests` bounds pre-response `Service::call` only. Active
//! bodies delegate without forced close; Abandoned/Failed forces close via
//! Hyper pinning. No task-supervisor framework.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::primitives::request_lifecycle::{RequestCancellationReason, RequestShared};

use super::activity::ConnectionActivity;

/// Spawn the deferred-body read-timeout watchdog (Plan 174 Track C).
///
/// The watchdog enforces `body_read_timeout` as a total deadline from
/// request start, including consumption that continues after
/// `Service::call` returns response-start. It exits early when the body
/// reaches any terminal state. On expiry while Active it marks Failed with
/// `ConnectionTimeout`, cancels the lifecycle, increments both the legacy
/// `body_read_timeouts` (operator consistency) and the new
/// `deferred_body_timeouts`, emits narrow events, and arms the driver close
/// via [`ConnectionActivity::fire_body_timeout`] so pending body/response
/// polls wake via transport failure.
pub(crate) fn spawn_body_timeout_watchdog(
    shared: Arc<RequestShared>,
    activity: Arc<ConnectionActivity>,
    deadline: tokio::time::Instant,
    conn_id: u64,
    ops: crate::ops::OpsContext,
) {
    tokio::spawn(async move {
        tokio::select! {
            _ = shared.wait_body_terminal() => {},
            _ = tokio::time::sleep_until(deadline) => {
                if shared.is_body_active() {
                    shared.mark_failed_with_reason(
                        RequestCancellationReason::ConnectionTimeout,
                    );
                    ops.counters()
                        .body_read_timeouts
                        .fetch_add(1, Ordering::Relaxed);
                    ops.counters()
                        .deferred_body_timeouts
                        .fetch_add(1, Ordering::Relaxed);
                    ops.emit(
                        crate::ops::Event::new(
                            crate::ops::Severity::Warn,
                            crate::ops::EventKind::BodyReadTimeout,
                            "deferred body read timeout",
                        )
                        .connection_id(conn_id),
                    );
                    ops.emit(
                        crate::ops::Event::new(
                            crate::ops::Severity::Warn,
                            crate::ops::EventKind::DeferredBodyTimeout,
                            "deferred body read timeout; closing connection",
                        )
                        .connection_id(conn_id),
                    );
                    activity.fire_body_timeout();
                }
            }
        }
    });
}

/// Spawn the deferred-body completion tracker (Plan 174 Track B2/E).
///
/// Caller must have already incremented the activity deferred count via
/// [`ConnectionActivity::deferred_started`]. The tracker waits for body
/// terminal, decrements, touches activity for idle accounting, and emits
/// narrow completion/abandonment observability. It never holds a service
/// permit: EggServe `max_in_flight_requests` bounds pre-response
/// `Service::call` execution only; downstream application-task admission is
/// downstream-owned (Track F).
pub(crate) fn spawn_deferred_tracker(
    shared: Arc<RequestShared>,
    activity: Arc<ConnectionActivity>,
    conn_id: u64,
    ops: crate::ops::OpsContext,
) {
    tokio::spawn(async move {
        shared.wait_body_terminal().await;
        activity.deferred_finished();
        match shared.body_state() {
            crate::primitives::request_lifecycle::BodyLifecycleState::Complete => {
                ops.counters()
                    .deferred_bodies_completed
                    .fetch_add(1, Ordering::Relaxed);
                ops.emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Debug,
                        crate::ops::EventKind::DeferredBodyCompleted,
                        "deferred body completed after response-start",
                    )
                    .connection_id(conn_id),
                );
            }
            crate::primitives::request_lifecycle::BodyLifecycleState::Abandoned
            | crate::primitives::request_lifecycle::BodyLifecycleState::Failed => {
                ops.counters()
                    .deferred_bodies_abandoned
                    .fetch_add(1, Ordering::Relaxed);
                ops.emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Debug,
                        crate::ops::EventKind::DeferredBodyAbandoned,
                        "deferred body abandoned/failed after response-start; connection will close",
                    )
                    .connection_id(conn_id),
                );
            }
            _ => {}
        }
    });
}
