//! Per-connection activity, admission, and response tracking.
//!
//! Answers the driver's core questions: is a service invocation in flight,
//! is a response body outstanding, is a deferred request body still active,
//! when was the last inbound/outbound progress, has the deferred-body
//! watchdog requested closure, how many requests completed. Atomic/mutex
//! ordering and exactly-once counter/permit release (RAII guards,
//! tracked-body completion) are unchanged from the monolithic module.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use http_body::{Body, Frame};

use crate::response::BoxBodyInner;
use crate::server::config::RuntimeConfig;

use super::lifecycle::LifecycleDisposition;
use super::response::finalize_runtime_response;

/// Per-connection request/response activity shared between the Hyper service
/// closure (which observes requests and responses) and the connection driver
/// (which enforces keep-alive-idle, write-progress, and total-lifetime
/// deadlines).
///
/// The driver sleeps until the next applicable deadline and recomputes on
/// every state change: all transitions that create new (earlier) deadlines
/// wake the driver via [`ConnectionActivity::notify`]. Transitions that only
/// extend deadlines (read/write progress) do not notify; the driver wakes at
/// the previously computed deadline, observes no expiry, and recomputes.
#[derive(Debug)]
pub(crate) struct ConnectionActivity {
    pub(crate) start: std::time::Instant,
    ops: crate::ops::OpsContext,
    state: std::sync::Mutex<ActivityState>,
    response_poll_progress: std::sync::Mutex<Vec<(u64, std::time::Instant)>>,
    in_flight: AtomicU64,
    outstanding: AtomicU64,
    completed: AtomicU64,
    drain_requested: AtomicBool,
    next_request_id: AtomicU64,
    /// Deferred request bodies still owned past `Service::call` return
    /// (Plan 174 Track B). While >0 the connection is not idle even when
    /// no service execution is in-flight and no response is outstanding:
    /// the prior request framing boundary is not yet complete.
    deferred: AtomicU64,
    /// Set by the deferred-body watchdog when `body_read_timeout` fires
    /// after response-start. The driver observes it on its next wake and
    /// closes the connection; the watchdog already marked the body Failed
    /// and cancelled the lifecycle.
    body_timeout_fired: AtomicBool,
    pub(crate) notify: tokio::sync::Notify,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ActivityState {
    pub(crate) last_activity: std::time::Instant,
    pub(crate) last_write: std::time::Instant,
}

impl ConnectionActivity {
    pub(crate) fn new(ops: crate::ops::OpsContext) -> Self {
        let now = std::time::Instant::now();
        Self {
            start: now,
            ops,
            state: std::sync::Mutex::new(ActivityState {
                last_activity: now,
                last_write: now,
            }),
            response_poll_progress: std::sync::Mutex::new(Vec::new()),
            in_flight: AtomicU64::new(0),
            outstanding: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            drain_requested: AtomicBool::new(false),
            next_request_id: AtomicU64::new(1),
            deferred: AtomicU64::new(0),
            body_timeout_fired: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        }
    }

    /// This connection's runtime observability context.
    pub(crate) fn ops(&self) -> &crate::ops::OpsContext {
        &self.ops
    }

    /// A request entered the Hyper service pipeline.
    pub(crate) fn request_started(&self) {
        self.in_flight.fetch_add(1, Ordering::Relaxed);
        self.ops
            .counters()
            .active_service_requests
            .fetch_add(1, Ordering::Relaxed);
        self.notify.notify_one();
    }

    /// Allocate a request/response activity identity. HTTP/1 maps one active
    /// identity to its serial request; multiplexed adapters can retain the
    /// same hook with one identity per stream.
    pub(crate) fn begin_request(self: &Arc<Self>) -> RequestActivity {
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        self.request_started();
        RequestActivity {
            connection: self.clone(),
            id,
        }
    }

    /// The service pipeline produced a response without invoking the
    /// service (parse/validation rejection). The in-flight slot is released
    /// but the request still counts toward per-connection totals.
    pub(crate) fn request_finished_without_service(&self) {
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
        self.ops
            .counters()
            .active_service_requests
            .fetch_sub(1, Ordering::Relaxed);
        self.touch();
        self.notify.notify_one();
    }

    /// Record any client activity (request bytes, new request, completed
    /// response) as keep-alive progress.
    fn touch(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.last_activity = std::time::Instant::now();
        }
    }

    /// Record forward socket-write progress.
    pub(crate) fn record_write(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        let now = std::time::Instant::now();
        if let Ok(mut state) = self.state.lock() {
            state.last_write = now;
            state.last_activity = now;
        }
    }

    /// Record inbound request bytes as connection activity. This extends
    /// (never shortens) the keep-alive idle deadline, so no driver wake is
    /// needed: the driver recomputes on its next scheduled wake.
    pub(crate) fn record_read(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        self.touch();
    }

    /// A response was handed to Hyper for transmission. Starts the
    /// write-progress budget and marks the connection as busy so the
    /// keep-alive idle timer cannot fire mid-response.
    pub(crate) fn response_started(&self, request_id: u64) {
        self.outstanding.fetch_add(1, Ordering::Relaxed);
        let now = std::time::Instant::now();
        if let Ok(mut state) = self.state.lock() {
            state.last_write = now;
            state.last_activity = now;
        }
        if let Ok(mut progress) = self.response_poll_progress.lock() {
            progress.push((request_id, now));
        }
        self.notify.notify_one();
    }

    /// A response body reached end-of-stream, failed, or was dropped
    /// (cancellation/disconnect/shutdown). Exactly-once per response via
    /// [`TrackedBody`]'s done flag.
    pub(crate) fn response_finished(&self, request_id: u64) {
        if self.outstanding.fetch_sub(1, Ordering::Relaxed) == 0 {
            // Unreachable in correct operation (every finish pairs with one
            // start); restore the counter instead of wrapping to zero.
            self.outstanding.fetch_add(1, Ordering::Relaxed);
        }
        if let Ok(mut progress) = self.response_poll_progress.lock() {
            progress.retain(|(id, _)| *id != request_id);
        }
        self.touch();
        self.notify.notify_one();
    }

    /// Record application-body poll progress attributable to one response.
    /// H2 uses this stream-local producer signal; a socket write by a sibling
    /// stream never updates it. It does not prove that Hyper has advanced
    /// stream-level flow control or put bytes on the wire.
    pub(crate) fn response_poll_progress(&self, request_id: u64) {
        let now = std::time::Instant::now();
        if let Ok(mut progress) = self.response_poll_progress.lock() {
            if let Some((_, last)) = progress.iter_mut().find(|(id, _)| *id == request_id) {
                *last = now;
            }
        }
        self.notify.notify_one();
    }

    pub(crate) fn h2_response_producer_stalled(
        &self,
        now: std::time::Instant,
        timeout: std::time::Duration,
    ) -> bool {
        self.response_poll_progress
            .lock()
            .map(|progress| {
                progress
                    .iter()
                    .any(|(_, last)| now.duration_since(*last) >= timeout)
            })
            .unwrap_or(false)
    }

    pub(crate) fn h2_response_producer_deadline(
        &self,
        timeout: std::time::Duration,
    ) -> Option<std::time::Instant> {
        self.response_poll_progress
            .lock()
            .ok()
            .and_then(|progress| {
                progress
                    .iter()
                    .map(|(_, last)| last.checked_add(timeout).unwrap_or(*last))
                    .min()
            })
    }

    /// A deferred body started (service returned with Active body).
    pub(crate) fn deferred_started(&self) {
        self.deferred.fetch_add(1, Ordering::Relaxed);
        self.notify.notify_one();
    }

    /// A deferred body reached a terminal state.
    pub(crate) fn deferred_finished(&self) {
        if self.deferred.fetch_sub(1, Ordering::Relaxed) == 0 {
            self.deferred.fetch_add(1, Ordering::Relaxed);
        }
        self.touch();
        self.notify.notify_one();
    }

    /// Arm the deferred-body timeout close. Called by the watchdog after it
    /// marked the body Failed and cancelled the lifecycle.
    pub(crate) fn fire_body_timeout(&self) {
        self.body_timeout_fired.store(true, Ordering::Release);
        self.notify.notify_one();
    }

    pub(crate) fn take_body_timeout(&self) -> bool {
        self.body_timeout_fired.swap(false, Ordering::AcqRel)
    }

    /// Request a protocol-level graceful drain (GOAWAY for HTTP/2). The
    /// HTTP/1 adapter still uses its existing `Connection: close` mapping.
    pub(crate) fn request_drain(&self) {
        self.drain_requested.store(true, Ordering::Release);
        self.notify.notify_one();
    }

    pub(crate) fn take_drain_request(&self) -> bool {
        self.drain_requested.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn snapshot(&self) -> (u64, u64, u64, u64, ActivityState) {
        let state = self
            .state
            .lock()
            .map(|guard| *guard)
            .unwrap_or(ActivityState {
                last_activity: self.start,
                last_write: self.start,
            });
        (
            self.in_flight.load(Ordering::Relaxed),
            self.outstanding.load(Ordering::Relaxed),
            self.completed.load(Ordering::Relaxed),
            self.deferred.load(Ordering::Relaxed),
            state,
        )
    }
}

/// RAII guard for one in-flight `Service::call()` execution.
///
/// Created when the Hyper service closure starts; [`InFlightGuard::finish`]
/// consumes it once the service pipeline has produced a Hyper response. If
/// the closure exits without producing a response (implementation bug —
/// all known paths go through `finish`), the slot is still released on drop
/// so permits and gauges cannot leak.
pub(crate) struct InFlightGuard {
    request_activity: RequestActivity,
    service_permit: Option<tokio::sync::OwnedSemaphorePermit>,
    finished: bool,
}

impl InFlightGuard {
    pub(crate) fn new(activity: Arc<ConnectionActivity>) -> Self {
        Self {
            request_activity: activity.begin_request(),
            service_permit: None,
            finished: false,
        }
    }

    pub(crate) fn request_id(&self) -> u64 {
        self.request_activity.id()
    }

    /// Try to admit one service execution under the server-wide in-flight
    /// budget. Returns `None` when admitted; on exhaustion returns the
    /// deterministic generic 503 and the caller must return it via
    /// [`InFlightGuard::finish`] without invoking the service. The permit
    /// is held until the guard is finished or dropped, so timeout,
    /// cancellation, panic, disconnect, and shutdown paths all recover it.
    pub(crate) fn admit(
        &mut self,
        semaphore: &Arc<tokio::sync::Semaphore>,
        conn_id: u64,
        error_policy: crate::policy::ErrorRepresentationPolicy,
    ) -> Option<hyper::Response<BoxBodyInner>> {
        match semaphore.clone().try_acquire_owned() {
            Ok(permit) => {
                self.service_permit = Some(permit);
                None
            }
            Err(_) => {
                self.request_activity
                    .connection
                    .ops()
                    .counters()
                    .service_admission_rejected
                    .fetch_add(1, Ordering::Relaxed);
                self.request_activity.connection.ops().emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Warn,
                        crate::ops::EventKind::ServiceAdmissionRejected,
                        "service saturated: in-flight request limit",
                    )
                    .connection_id(conn_id),
                );
                Some(crate::response::service_unavailable_with_policy(
                    error_policy,
                ))
            }
        }
    }

    /// Complete the request: release the in-flight slot, count the response
    /// toward the per-connection total (every response counts, including
    /// HEAD, errors, and pre-service rejections), enforce
    /// `max_requests_per_connection` with a clean `Connection: close`, arm
    /// the write-progress budget, and wrap the body so its completion
    /// releases the outstanding slot.
    pub(crate) fn finish(
        mut self,
        response: hyper::Response<BoxBodyInner>,
        config: &RuntimeConfig,
        conn_id: u64,
        mut disposition: LifecycleDisposition,
    ) -> (hyper::Response<BoxBodyInner>, LifecycleDisposition) {
        self.finished = true;
        self.service_permit.take();
        self.request_activity.request_finished();
        let completed = self
            .request_activity
            .connection
            .completed
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        if let Some(max) = config.max_requests_per_connection {
            if completed >= max {
                self.request_activity
                    .connection
                    .ops()
                    .counters()
                    .max_requests_closes
                    .fetch_add(1, Ordering::Relaxed);
                self.request_activity.connection.ops().emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Debug,
                        crate::ops::EventKind::MaxRequestsClose,
                        "max requests per connection reached; closing after response",
                    )
                    .connection_id(conn_id),
                );
                disposition = LifecycleDisposition::close_after_response()
                    .with_graceful_drain()
                    .with_transport_termination();
                self.request_activity.connection.request_drain();
            }
        }
        self.request_activity.response_started();
        let response = finalize_runtime_response(response, config);
        let activity = self.request_activity.connection.clone();
        let request_id = self.request_id();
        (
            response
                .map(move |body| BoxBodyInner::new(TrackedBody::new(body, activity, request_id))),
            disposition,
        )
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.service_permit.take();
            self.request_activity.request_finished();
        }
    }
}

/// Response-body wrapper that releases the connection's outstanding-response
/// slot exactly once, when the body reaches end-of-stream, fails, or is
/// dropped (client disconnect, shutdown, HEAD suppression, cancellation).
/// This is what lets the driver distinguish a stalled response (writes
/// outstanding, no progress) from an idle keep-alive connection (nothing
/// outstanding).
struct TrackedBody {
    inner: BoxBodyInner,
    activity: Arc<ConnectionActivity>,
    #[allow(dead_code)]
    request_id: u64,
    done: AtomicBool,
}

impl TrackedBody {
    fn new(inner: BoxBodyInner, activity: Arc<ConnectionActivity>, request_id: u64) -> Self {
        Self {
            inner,
            activity,
            request_id,
            done: AtomicBool::new(false),
        }
    }

    pub(crate) fn finish(&self) {
        if !self.done.swap(true, Ordering::AcqRel) {
            self.activity.response_finished(self.request_id);
        }
    }
}

/// Per-request/response activity identity. The connection remains the owner
/// of aggregate deadlines and counters, while this record is the future hook
/// for stream-local progress, cancellation, and timeout state.
#[derive(Debug, Clone)]
pub(crate) struct RequestActivity {
    connection: Arc<ConnectionActivity>,
    id: u64,
}

impl RequestActivity {
    fn id(&self) -> u64 {
        self.id
    }

    fn request_finished(&self) {
        self.connection.request_finished_without_service();
    }

    fn response_started(&self) {
        self.connection.response_started(self.id);
    }
}

impl Drop for TrackedBody {
    fn drop(&mut self) {
        self.finish();
    }
}

impl Body for TrackedBody {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        match std::pin::Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(None) => {
                this.finish();
                Poll::Ready(None)
            }
            Poll::Ready(Some(Err(e))) => {
                this.finish();
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(Some(Ok(frame))) => {
                this.activity.response_poll_progress(this.request_id);
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}
