//! Connection execution pipeline.
//!
//! This module owns the per-connection execution path from byte stream to
//! response completion. It is used by the TCP/TLS accept loop, the embedded
//! runtime, and caller-owned transports.
//!
//! # Pipeline steps
//!
//! 1. Optional TLS handshake (feature-gated, above the driver for TCP)
//! 2. HTTP/1 connection setup via Hyper
//! 3. Request conversion to canonical types
//! 4. Request-policy validation (body rejection for body-forbidden methods)
//! 5. Service invocation with panic containment
//! 6. Canonical response normalization
//! 7. Transport-body conversion
//! 8. Permit release and connection termination
//!
//! # Transport-neutral driver
//!
//! [`serve_http1_connection`] is the canonical connection driver (Plan 163).
//! The caller supplies an already-established bidirectional async byte stream,
//! a canonical [`Service`], a [`ConnectionContext`], shared [`RuntimeState`]
//! admission, and a [`ConnectionShutdown`] token. EggServe supplies HTTP
//! parsing, request conversion, body policy, service dispatch, response
//! normalization/framing, timeouts, and closure semantics. The TCP/TLS
//! `Server` is a convenience runtime that owns listener acceptance and
//! handshake above this driver and shares the same pipeline.

// Panics raised while executing a [`Service`] are contained at the
// invocation boundary and mapped to [`ServiceError::panic`], so the client
// receives an RFC-correct 500 response instead of a dropped connection.
// The standard panic hook still runs, keeping diagnostics on stderr; panics
// outside service execution (e.g., during transport-body conversion) still
// propagate to the JoinSet task boundary.

use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use http_body::{Body, Frame};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::broadcast;

use crate::primitives::connection_info::{Scheme, SocketEndpoints, TlsInfo};
use crate::primitives::request_body_policy::RequestBodyPolicy;
use crate::primitives::request_lifecycle::{RequestCancellationReason, RequestShared};
use crate::response::BoxBodyInner;
use crate::server::config::RuntimeConfig;
use crate::server::service::{Service, ServiceError};
use crate::server::RuntimeState;

/// Concrete wrapper type for the canonical Hyper service returned by
/// [`make_canonical_hyper_service`].
///
/// Using a named type (rather than `impl Service`) preserves the `Send`
/// bound on the `Future` associated type, which is required by Hyper's
/// `serve_connection` when the task is spawned on a multi-threaded runtime.
#[allow(clippy::type_complexity)]
struct CanonicalHyperService {
    inner: std::sync::Arc<
        dyn Fn(
                hyper::Request<hyper::body::Incoming>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<hyper::Response<BoxBodyInner>, Infallible>,
                        > + Send,
                >,
            > + Send
            + Sync,
    >,
}

impl Clone for CanonicalHyperService {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl hyper::service::Service<hyper::Request<hyper::body::Incoming>> for CanonicalHyperService {
    type Response = hyper::Response<BoxBodyInner>;
    type Error = Infallible;
    type Future = std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<hyper::Response<BoxBodyInner>, Infallible>>
                + Send,
        >,
    >;

    fn call(&self, req: hyper::Request<hyper::body::Incoming>) -> Self::Future {
        (self.inner)(req)
    }
}

/// Trustworthy per-connection transport description supplied by the caller.
///
/// For real TCP/TLS connections the runtime builds this from observed
/// socket addresses and the completed handshake. For caller-owned streams
/// (for example an anonymity-network byte stream) the caller asserts the
/// semantic `scheme` explicitly: such a transport is `Scheme::Http` unless
/// HTTPS was explicitly terminated on it. `tls` is present only when
/// EggServe performed or otherwise knows the TLS session; opaque encrypted
/// transports leave it as `None`.
///
/// No I2P `Destination`, tunnel IDs, router identities, or LeaseSet types
/// enter EggServe. If downstream code needs peer identity it retains that
/// identity outside EggServe and associates it with its own service
/// wrapper/session state. Forwarded/`X-Forwarded-*` values remain ordinary
/// untrusted HTTP headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionContext {
    /// Local socket address when the transport has one.
    pub local_addr: Option<std::net::SocketAddr>,
    /// Remote socket address when the transport has one.
    pub remote_addr: Option<std::net::SocketAddr>,
    /// HTTP vs HTTPS semantic scheme.
    pub scheme: Scheme,
    /// TLS session metadata when EggServe knows the session.
    pub tls: Option<TlsInfo>,
}

impl ConnectionContext {
    /// Create a context from explicit parts.
    pub fn new(
        local_addr: Option<std::net::SocketAddr>,
        remote_addr: Option<std::net::SocketAddr>,
        scheme: Scheme,
        tls: Option<TlsInfo>,
    ) -> Self {
        Self {
            local_addr,
            remote_addr,
            scheme,
            tls,
        }
    }

    /// Context for a real TCP connection with observed endpoints.
    pub fn for_tcp(
        local_addr: std::net::SocketAddr,
        remote_addr: std::net::SocketAddr,
        tls: Option<TlsInfo>,
    ) -> Self {
        let scheme = if tls.is_some() {
            Scheme::Https
        } else {
            Scheme::Http
        };
        Self {
            local_addr: Some(local_addr),
            remote_addr: Some(remote_addr),
            scheme,
            tls,
        }
    }

    /// Context for a caller-owned non-socket byte stream.
    ///
    /// No socket endpoints are recorded and no addresses are fabricated.
    pub fn for_non_socket(scheme: Scheme, tls: Option<TlsInfo>) -> Self {
        Self {
            local_addr: None,
            remote_addr: None,
            scheme,
            tls,
        }
    }

    /// Paired socket endpoints when both addresses are present.
    pub fn socket_endpoints(&self) -> Option<SocketEndpoints> {
        match (self.local_addr, self.remote_addr) {
            (Some(local), Some(remote)) => Some(SocketEndpoints { local, remote }),
            _ => None,
        }
    }

    /// Returns `true` when both socket endpoints are present.
    pub fn has_socket_endpoints(&self) -> bool {
        self.local_addr.is_some() && self.remote_addr.is_some()
    }

    /// Convert into the per-request [`crate::primitives::connection_info::ConnectionInfo`].
    pub fn connection_info(&self) -> crate::primitives::connection_info::ConnectionInfo {
        crate::primitives::connection_info::ConnectionInfo {
            local_addr: self.local_addr,
            remote_addr: self.remote_addr,
            scheme: self.scheme,
            tls: self.tls.clone(),
        }
    }
}

/// Per-connection graceful-shutdown token for caller-owned streams.
///
/// The caller retains ownership and calls [`ConnectionShutdown::shutdown`]
/// to request graceful connection shutdown independently of the TCP
/// `ServerHandle`. Dropping the token without shutdown is equivalent to
/// never requesting shutdown; in-flight work still observes hard timeouts,
/// protocol errors, and task cancellation via drop semantics. Permits and
/// producer tasks are released on driver exit regardless of outcome.
#[derive(Debug, Clone, Default)]
pub struct ConnectionShutdown {
    inner: Arc<ConnectionShutdownInner>,
}

#[derive(Debug, Default)]
struct ConnectionShutdownInner {
    notify: tokio::sync::Notify,
    flag: std::sync::atomic::AtomicBool,
}

impl ConnectionShutdown {
    /// Create a new un-signalled shutdown token.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ConnectionShutdownInner {
                notify: tokio::sync::Notify::new(),
                flag: std::sync::atomic::AtomicBool::new(false),
            }),
        }
    }

    /// Request graceful connection shutdown.
    pub fn shutdown(&self) {
        self.inner
            .flag
            .store(true, std::sync::atomic::Ordering::Release);
        self.inner.notify.notify_waiters();
    }

    /// Returns `true` once shutdown has been requested.
    pub fn is_shutdown(&self) -> bool {
        self.inner.flag.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Wait until shutdown is requested.
    pub async fn cancelled(&self) {
        self.inner.notify.notified().await;
    }
}

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
    fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Register a new lifecycle observer, pruning dead entries.
    fn register(&self, shared: &Arc<RequestShared>) {
        let weak = Arc::downgrade(shared);
        if let Ok(mut guard) = self.inner.lock() {
            guard.retain(|w| w.upgrade().is_some());
            guard.push(weak);
        }
    }

    /// Cancel all still-live lifecycles with observability.
    ///
    /// Already-cancelled lifecycles are skipped (first reason wins).
    fn cancel_all(&self, reason: RequestCancellationReason, conn_id: u64) {
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
    start: std::time::Instant,
    state: std::sync::Mutex<ActivityState>,
    in_flight: AtomicU64,
    outstanding: AtomicU64,
    completed: AtomicU64,
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
    notify: tokio::sync::Notify,
}

#[derive(Debug, Clone, Copy)]
struct ActivityState {
    last_activity: std::time::Instant,
    last_write: std::time::Instant,
}

impl ConnectionActivity {
    fn new() -> Self {
        let now = std::time::Instant::now();
        Self {
            start: now,
            state: std::sync::Mutex::new(ActivityState {
                last_activity: now,
                last_write: now,
            }),
            in_flight: AtomicU64::new(0),
            outstanding: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            deferred: AtomicU64::new(0),
            body_timeout_fired: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        }
    }

    /// A request entered the Hyper service pipeline.
    fn request_started(&self) {
        self.in_flight.fetch_add(1, Ordering::Relaxed);
        crate::ops::global_counters()
            .active_service_requests
            .fetch_add(1, Ordering::Relaxed);
        self.notify.notify_one();
    }

    /// The service pipeline produced a response without invoking the
    /// service (parse/validation rejection). The in-flight slot is released
    /// but the request still counts toward per-connection totals.
    fn request_finished_without_service(&self) {
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
        crate::ops::global_counters()
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
    fn record_write(&self, bytes: usize) {
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
    fn record_read(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        self.touch();
    }

    /// A response was handed to Hyper for transmission. Starts the
    /// write-progress budget and marks the connection as busy so the
    /// keep-alive idle timer cannot fire mid-response.
    fn response_started(&self) {
        self.outstanding.fetch_add(1, Ordering::Relaxed);
        let now = std::time::Instant::now();
        if let Ok(mut state) = self.state.lock() {
            state.last_write = now;
            state.last_activity = now;
        }
        self.notify.notify_one();
    }

    /// A response body reached end-of-stream, failed, or was dropped
    /// (cancellation/disconnect/shutdown). Exactly-once per response via
    /// [`TrackedBody`]'s done flag.
    fn response_finished(&self) {
        if self.outstanding.fetch_sub(1, Ordering::Relaxed) == 0 {
            // Unreachable in correct operation (every finish pairs with one
            // start); restore the counter instead of wrapping to zero.
            self.outstanding.fetch_add(1, Ordering::Relaxed);
        }
        self.touch();
        self.notify.notify_one();
    }

    /// A deferred body started (service returned with Active body).
    fn deferred_started(&self) {
        self.deferred.fetch_add(1, Ordering::Relaxed);
        self.notify.notify_one();
    }

    /// A deferred body reached a terminal state.
    fn deferred_finished(&self) {
        if self.deferred.fetch_sub(1, Ordering::Relaxed) == 0 {
            self.deferred.fetch_add(1, Ordering::Relaxed);
        }
        self.touch();
        self.notify.notify_one();
    }

    /// Arm the deferred-body timeout close. Called by the watchdog after it
    /// marked the body Failed and cancelled the lifecycle.
    fn fire_body_timeout(&self) {
        self.body_timeout_fired.store(true, Ordering::Release);
        self.notify.notify_one();
    }

    fn take_body_timeout(&self) -> bool {
        self.body_timeout_fired.swap(false, Ordering::AcqRel)
    }

    fn snapshot(&self) -> (u64, u64, u64, u64, ActivityState) {
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
struct InFlightGuard {
    activity: Arc<ConnectionActivity>,
    service_permit: Option<tokio::sync::OwnedSemaphorePermit>,
    finished: bool,
}

impl InFlightGuard {
    fn new(activity: Arc<ConnectionActivity>) -> Self {
        activity.request_started();
        Self {
            activity,
            service_permit: None,
            finished: false,
        }
    }

    /// Try to admit one service execution under the server-wide in-flight
    /// budget. Returns `None` when admitted; on exhaustion returns the
    /// deterministic generic 503 and the caller must return it via
    /// [`InFlightGuard::finish`] without invoking the service. The permit
    /// is held until the guard is finished or dropped, so timeout,
    /// cancellation, panic, disconnect, and shutdown paths all recover it.
    fn admit(
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
                crate::ops::global_counters()
                    .service_admission_rejected
                    .fetch_add(1, Ordering::Relaxed);
                crate::ops::Logger::global().emit(
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
    fn finish(
        mut self,
        mut response: hyper::Response<BoxBodyInner>,
        config: &RuntimeConfig,
        conn_id: u64,
    ) -> hyper::Response<BoxBodyInner> {
        self.finished = true;
        self.service_permit.take();
        self.activity.request_finished_without_service();
        let completed = self.activity.completed.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(max) = config.max_requests_per_connection {
            if completed >= max {
                crate::ops::global_counters()
                    .max_requests_closes
                    .fetch_add(1, Ordering::Relaxed);
                crate::ops::Logger::global().emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Debug,
                        crate::ops::EventKind::MaxRequestsClose,
                        "max requests per connection reached; closing after response",
                    )
                    .connection_id(conn_id),
                );
                response.headers_mut().insert(
                    hyper::header::CONNECTION,
                    hyper::header::HeaderValue::from_static("close"),
                );
            }
        }
        self.activity.response_started();
        let response = finalize_runtime_response(response, config);
        let activity = self.activity.clone();
        response.map(move |body| BoxBodyInner::new(TrackedBody::new(body, activity)))
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.service_permit.take();
            self.activity.request_finished_without_service();
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
    done: AtomicBool,
}

impl TrackedBody {
    fn new(inner: BoxBodyInner, activity: Arc<ConnectionActivity>) -> Self {
        Self {
            inner,
            activity,
            done: AtomicBool::new(false),
        }
    }

    fn finish(&self) {
        if !self.done.swap(true, Ordering::AcqRel) {
            self.activity.response_finished();
        }
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
            other => other,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

/// Transport wrapper at the Plan 163 transport boundary that records
/// forward socket-write progress (and inbound activity) for the
/// write-no-progress timeout. Transparent to Hyper's HTTP/1 framing; works
/// for TCP, TLS, and caller-owned transports because all of them flow
/// through this point before [`TokioIo`].
struct ProgressIo<I> {
    inner: I,
    activity: Arc<ConnectionActivity>,
}

impl<I> ProgressIo<I> {
    fn new(inner: I, activity: Arc<ConnectionActivity>) -> Self {
        Self { inner, activity }
    }
}

impl<I: AsyncRead + Unpin> AsyncRead for ProgressIo<I> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let this = self.get_mut();
        match std::pin::Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                this.activity
                    .record_read(buf.filled().len().saturating_sub(before));
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

impl<I: AsyncWrite + Unpin> AsyncWrite for ProgressIo<I> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        let this = self.get_mut();
        match std::pin::Pin::new(&mut this.inner).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => {
                this.activity.record_write(n);
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<Result<usize, std::io::Error>> {
        let this = self.get_mut();
        match std::pin::Pin::new(&mut this.inner).poll_write_vectored(cx, bufs) {
            Poll::Ready(Ok(n)) => {
                this.activity.record_write(n);
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

/// Graceful-shutdown capability for the pinned Hyper connection future, so
/// the shared driver below can close idle/stalled/expired connections
/// without knowing the concrete Hyper connection type.
trait ShutdownConn {
    fn graceful_shutdown(self: std::pin::Pin<&mut Self>);
}

impl<I, S> ShutdownConn for hyper::server::conn::http1::UpgradeableConnection<I, S>
where
    I: hyper::rt::Read + hyper::rt::Write + Unpin,
    S: hyper::service::Service<
        Request<Incoming>,
        Response = Response<BoxBodyInner>,
        Error = Infallible,
    >,
{
    fn graceful_shutdown(self: std::pin::Pin<&mut Self>) {
        hyper::server::conn::http1::UpgradeableConnection::graceful_shutdown(self);
    }
}

/// Outcome of driving one HTTP/1 connection to completion.
///
/// Returned by [`serve_http1_connection`] for internal observability.
/// Every exit releases all permits and producer tasks; no outcome leaks
/// admission state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionOutcome {
    /// Clean EOF / keep-alive close with no error.
    Normal,
    /// Protocol or client error (malformed request, framing rejection,
    /// connection error, client disconnect).
    ClientError,
    /// Header-read timeout fired.
    HeaderTimeout,
    /// Keep-alive idle timeout fired: no in-flight request and no
    /// outstanding response body for the configured interval.
    IdleTimeout,
    /// Response write no-progress timeout fired: a response body was
    /// outstanding but no forward socket progress was made in time.
    WriteTimeout,
    /// Total connection lifetime expired.
    TotalTimeout,
    /// Graceful shutdown was requested (caller token or server signal).
    Shutdown,
    /// Unexpected internal failure.
    Internal,
}

impl ConnectionOutcome {
    /// Returns `true` for a clean close with no error or timeout.
    ///
    /// Keep-alive idle expiry counts as clean: the connection completed
    /// every response and turned over routinely. Write-stall expiry does
    /// not: a response was abandoned mid-transmission.
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::Normal | Self::Shutdown | Self::IdleTimeout)
    }
}

impl std::fmt::Display for ConnectionOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::ClientError => write!(f, "client-error"),
            Self::HeaderTimeout => write!(f, "header-timeout"),
            Self::IdleTimeout => write!(f, "idle-timeout"),
            Self::WriteTimeout => write!(f, "write-timeout"),
            Self::TotalTimeout => write!(f, "total-timeout"),
            Self::Shutdown => write!(f, "shutdown"),
            Self::Internal => write!(f, "internal"),
        }
    }
}

/// Upper bound on the post-`graceful_shutdown()` drain wait.
///
/// Hyper's graceful shutdown still waits for the in-flight response to
/// finish; a client that stops reading its response body applies TCP
/// backpressure forever. Capping the drain releases the connection's
/// admission permit promptly instead of letting stalled clients pin pool
/// slots after their lifetime budget has already expired.
const MAX_POST_SHUTDOWN_DRAIN: std::time::Duration = std::time::Duration::from_secs(5);

fn post_shutdown_drain_budget(config: &RuntimeConfig) -> std::time::Duration {
    config
        .graceful_shutdown_timeout
        .min(MAX_POST_SHUTDOWN_DRAIN)
}

/// Build the Hyper HTTP/1 connection builder with EggServe-owned parser
/// policy applied explicitly.
///
/// `max_buf_size` and `max_headers` are set on every connection so release
/// upgrades cannot silently widen parser memory. Hyper documents both
/// defaults as unstable; the EggServe-owned values in [`RuntimeConfig`] are
/// the policy of record. `max_buf_size` below Hyper's 8192 minimum is
/// clamped (builder validation rejects it first; the clamp only protects
/// hand-constructed configs from panicking a connection task).
///
/// Hyper automatic `Date` generation is explicitly disabled: the EggServe
/// [`crate::server::response_policy::ResponsePolicy`] is the sole `Date`
/// authority (system clock by default, caller-supplied provider or explicit
/// suppression for privacy profiles). Tests prove exactly zero or one `Date`
/// according to policy.
fn hyper_builder(config: &RuntimeConfig) -> http1::Builder {
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(config.header_read_timeout)
        .max_buf_size(config.max_buf_size.max(crate::limits::MIN_MAX_BUF_SIZE))
        .max_headers(config.max_headers)
        .auto_date_header(false);
    builder
}

/// Far-future deadline used when a timeout is effectively disabled by a huge
/// configured duration. `Instant + Duration` panics on overflow, so
/// unrepresentable deadlines saturate here instead.
fn far_future() -> std::time::Instant {
    std::time::Instant::now() + std::time::Duration::from_secs(365 * 24 * 3600)
}

/// Gracefully close a connection with a bounded post-shutdown drain.
///
/// Hyper's graceful shutdown still waits for the in-flight response to
/// finish; a client that stops reading applies TCP backpressure forever.
/// The bounded drain releases the connection's admission permit promptly
/// instead of letting stalled clients pin pool slots.
async fn graceful_close<C>(mut conn: std::pin::Pin<&mut C>, config: &RuntimeConfig, conn_id: u64)
where
    C: std::future::Future<Output = Result<(), hyper::Error>> + ShutdownConn,
{
    conn.as_mut().graceful_shutdown();
    if tokio::time::timeout(post_shutdown_drain_budget(config), conn.as_mut())
        .await
        .is_err()
    {
        crate::ops::Logger::global().emit(
            crate::ops::Event::new(
                crate::ops::Severity::Debug,
                crate::ops::EventKind::ClientDisconnect,
                "post-shutdown drain budget expired; closing connection",
            )
            .connection_id(conn_id),
        );
    }
}

/// Classify a completed Hyper connection future into an outcome with
/// observability.
///
/// Hyper reports an expired header-read timeout as a timeout-class
/// connection error, and parser rejections (including `max_buf_size` /
/// `max_headers` excess, which Hyper answers with 431 itself) as
/// parse-class errors; each increments the counter named for it. Anything
/// else is a client disconnect. Hostile bytes never reach the logs: parse
/// errors are sanitized before emission.
fn finish_conn_result(result: Result<(), hyper::Error>, conn_id: u64) -> ConnectionOutcome {
    match result {
        Ok(()) => {
            crate::ops::Logger::global().emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Debug,
                    crate::ops::EventKind::KeepAliveClosed,
                    "connection closed",
                )
                .connection_id(conn_id),
            );
            ConnectionOutcome::Normal
        }
        Err(e) => {
            let header_timeout = e.is_timeout();
            let parse_error = !header_timeout && e.is_parse();
            if header_timeout {
                crate::ops::global_counters()
                    .header_timeouts
                    .fetch_add(1, Ordering::Relaxed);
            } else if parse_error {
                crate::ops::global_counters()
                    .parser_rejects
                    .fetch_add(1, Ordering::Relaxed);
            }
            crate::ops::Logger::global().emit(
                crate::ops::Event::new(
                    if header_timeout {
                        crate::ops::Severity::Warn
                    } else {
                        crate::ops::Severity::Debug
                    },
                    if header_timeout {
                        crate::ops::EventKind::HeaderTimeout
                    } else if parse_error {
                        crate::ops::EventKind::ParserRejection
                    } else {
                        crate::ops::EventKind::ClientDisconnect
                    },
                    if header_timeout {
                        "header read timeout".to_string()
                    } else if parse_error {
                        crate::ops::sanitize_text_field(&format!("parser rejection: {e}"))
                    } else {
                        format!("connection error: {e}")
                    },
                )
                .connection_id(conn_id),
            );
            if header_timeout {
                ConnectionOutcome::HeaderTimeout
            } else {
                ConnectionOutcome::ClientError
            }
        }
    }
}

/// Shared connection driver: polls one Hyper connection while enforcing
/// independent deadlines.
///
/// - `connection_total_timeout` — hard maximum connection lifetime, never
///   reset (defense in depth);
/// - `keep_alive_idle_timeout` — graceful close after inactivity, reset on
///   every request/transport activity; only applies with no in-flight
///   request, no outstanding response body, and no deferred request body
///   still owned past `Service::call` return (Plan 174);
/// - `response_write_timeout` — close after no forward socket progress
///   while a response body is outstanding; steady progress, however slow,
///   never triggers it;
/// - deferred-body timeout — close after `body_read_timeout` with an Active
///   deferred body (armed by the per-request watchdog);
/// - shutdown signal — graceful close with bounded post-shutdown drain.
///
/// The driver sleeps until the next applicable deadline and recomputes on
/// every [`ConnectionActivity`] state change, so expiry precision does not
/// depend on polling. Total lifetime is the hard ceiling: when it expires
/// first, the request dies mid-flight regardless of the other budgets.
async fn drive_connection<C, F>(
    mut conn: std::pin::Pin<&mut C>,
    config: &RuntimeConfig,
    activity: &Arc<ConnectionActivity>,
    requests: &Arc<ConnectionRequests>,
    conn_id: u64,
    shutdown: F,
) -> ConnectionOutcome
where
    C: std::future::Future<Output = Result<(), hyper::Error>> + ShutdownConn,
    F: std::future::Future<Output = ()>,
{
    let total_deadline = activity.start.checked_add(config.connection_total_timeout);
    let mut shutdown = std::pin::pin!(shutdown);
    loop {
        let now = std::time::Instant::now();
        if total_deadline.is_some_and(|deadline| now >= deadline) {
            crate::ops::global_counters()
                .connection_total_timeouts
                .fetch_add(1, Ordering::Relaxed);
            crate::ops::Logger::global().emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Warn,
                    crate::ops::EventKind::ConnectionTotalTimeout,
                    "connection total timeout",
                )
                .connection_id(conn_id),
            );
            requests.cancel_all(RequestCancellationReason::ConnectionTimeout, conn_id);
            graceful_close(conn.as_mut(), config, conn_id).await;
            return ConnectionOutcome::TotalTimeout;
        }
        // Deferred-body timeout fired by the per-request watchdog: the body
        // was already marked Failed and its lifecycle cancelled with
        // ConnectionTimeout. Close the transport so pending body/response
        // polls wake via transport failure.
        if activity.take_body_timeout() {
            graceful_close(conn.as_mut(), config, conn_id).await;
            requests.cancel_all(RequestCancellationReason::ConnectionTimeout, conn_id);
            return ConnectionOutcome::ClientError;
        }
        let (in_flight, outstanding, _completed, deferred, state) = activity.snapshot();
        let idle = in_flight == 0 && outstanding == 0 && deferred == 0;
        if idle && now.duration_since(state.last_activity) >= config.keep_alive_idle_timeout {
            crate::ops::global_counters()
                .keepalive_idle_timeouts
                .fetch_add(1, Ordering::Relaxed);
            crate::ops::Logger::global().emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Debug,
                    crate::ops::EventKind::KeepAliveIdleTimeout,
                    "keep-alive idle timeout",
                )
                .connection_id(conn_id),
            );
            graceful_close(conn.as_mut(), config, conn_id).await;
            return ConnectionOutcome::IdleTimeout;
        }
        if outstanding > 0 && now.duration_since(state.last_write) >= config.response_write_timeout
        {
            crate::ops::global_counters()
                .write_stall_timeouts
                .fetch_add(1, Ordering::Relaxed);
            crate::ops::Logger::global().emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Warn,
                    crate::ops::EventKind::WriteStallTimeout,
                    "response write stall timeout",
                )
                .connection_id(conn_id),
            );
            requests.cancel_all(RequestCancellationReason::ConnectionTimeout, conn_id);
            graceful_close(conn.as_mut(), config, conn_id).await;
            return ConnectionOutcome::WriteTimeout;
        }
        let mut wake = total_deadline.unwrap_or_else(far_future);
        if idle {
            wake = wake.min(
                state
                    .last_activity
                    .checked_add(config.keep_alive_idle_timeout)
                    .unwrap_or_else(far_future),
            );
        }
        if outstanding > 0 {
            wake = wake.min(
                state
                    .last_write
                    .checked_add(config.response_write_timeout)
                    .unwrap_or_else(far_future),
            );
        }
        let sleep = tokio::time::sleep_until(tokio::time::Instant::from_std(wake));
        tokio::select! {
            result = &mut conn => {
                let outcome = finish_conn_result(result, conn_id);
                // Peer disconnect / transport failure must wake idle
                // downstream waiters even if they are not polling body/response IO.
                match outcome {
                    ConnectionOutcome::ClientError => {
                        requests.cancel_all(RequestCancellationReason::PeerDisconnected, conn_id);
                    }
                    ConnectionOutcome::HeaderTimeout => {
                        requests.cancel_all(RequestCancellationReason::ConnectionTimeout, conn_id);
                    }
                    _ => {}
                }
                return outcome;
            }
            _ = &mut shutdown => {
                requests.cancel_all(RequestCancellationReason::ServerShutdown, conn_id);
                graceful_close(conn.as_mut(), config, conn_id).await;
                return ConnectionOutcome::Shutdown;
            }
            // A state change may have created an earlier deadline (new
            // response arms the write timer; deferred completion clears
            // idle; body-timeout flag requests close); recompute immediately.
            _ = activity.notify.notified() => continue,
            _ = sleep => continue,
        }
    }
}

/// Low-level Hyper connection executor for the TCP accept loop.
///
/// Crate-private: downstream callers must use
/// [`serve_http1_connection`], which takes a canonical [`Service`] and a
/// [`ConnectionContext`] instead of Hyper service types. This helper retains
/// the TCP wire behavior (header-read timeout, explicit parser limits,
/// idle/write/total lifetimes, graceful shutdown with bounded
/// post-shutdown drain) and reports a [`ConnectionOutcome`] for
/// observability.
///
/// The caller supplies the [`ConnectionActivity`] shared with the Hyper
/// service so request/response observations drive the idle and
/// write-progress deadlines.
pub(crate) async fn serve_connection<I, S>(
    io: TokioIo<I>,
    service: S,
    config: &RuntimeConfig,
    activity: &Arc<ConnectionActivity>,
    requests: &Arc<ConnectionRequests>,
    shutdown_rx: &mut broadcast::Receiver<()>,
    conn_id: u64,
) -> ConnectionOutcome
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: hyper::service::Service<
            Request<Incoming>,
            Response = Response<BoxBodyInner>,
            Error = Infallible,
        > + 'static,
{
    let io = TokioIo::new(ProgressIo::new(io.into_inner(), activity.clone()));
    let conn = hyper_builder(config)
        .serve_connection(io, service)
        .with_upgrades();
    let mut conn = std::pin::pin!(conn);
    let shutdown = async move {
        let _ = shutdown_rx.recv().await;
    };
    drive_connection(conn.as_mut(), config, activity, requests, conn_id, shutdown).await
}

/// Drive a Hyper connection with a caller-owned shutdown token.
///
/// Shared executor with [`serve_connection`] but selected on
/// [`ConnectionShutdown::cancelled`] instead of the TCP accept-loop
/// broadcast channel. Used only by [`serve_http1_connection`].
#[allow(clippy::too_many_arguments)]
async fn serve_hyper_with_token<I, S>(
    io: TokioIo<I>,
    service: S,
    config: &RuntimeConfig,
    activity: &Arc<ConnectionActivity>,
    requests: &Arc<ConnectionRequests>,
    shutdown: &ConnectionShutdown,
    conn_id: u64,
) -> ConnectionOutcome
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: hyper::service::Service<
            Request<Incoming>,
            Response = Response<BoxBodyInner>,
            Error = Infallible,
        > + 'static,
{
    let io = TokioIo::new(ProgressIo::new(io.into_inner(), activity.clone()));
    let conn = hyper_builder(config)
        .serve_connection(io, service)
        .with_upgrades();
    let mut conn = std::pin::pin!(conn);
    let shutdown = async move {
        shutdown.cancelled().await;
    };
    drive_connection(conn.as_mut(), config, activity, requests, conn_id, shutdown).await
}

/// Build the shared per-request canonical pipeline as a Hyper service.
///
/// This is the single source of truth for the request lifecycle:
/// Hyper parsing, EggServe parser ceilings (header count/size,
/// request-target length), TRACE check, body policy, service admission,
/// service invocation, normalization, framing, incomplete-body close, and
/// response finalization. Both the TCP/TLS accept loop and the
/// transport-neutral driver ([`serve_http1_connection`]) share this
/// pipeline.
///
/// Every response handed to Hyper — including parse rejections and policy
/// errors — passes through [`InFlightGuard::finish`], which counts it
/// toward `max_requests_per_connection`, arms the write-progress budget,
/// and wraps the body so completion releases the outstanding slot.
#[allow(clippy::too_many_arguments)]
fn make_canonical_hyper_service<S>(
    service: Arc<S>,
    config: Arc<RuntimeConfig>,
    file_stream_semaphore: Arc<tokio::sync::Semaphore>,
    service_semaphore: Arc<tokio::sync::Semaphore>,
    activity: Arc<ConnectionActivity>,
    requests: Arc<ConnectionRequests>,
    stream_chunk_size: usize,
    handler_timeout: std::time::Duration,
    body_read_timeout: std::time::Duration,
    max_body_bytes: u64,
    context: ConnectionContext,
    conn_id: u64,
) -> CanonicalHyperService
where
    S: Service + 'static,
{
    #[allow(clippy::type_complexity)]
    let handler: std::sync::Arc<
        dyn Fn(
                hyper::Request<hyper::body::Incoming>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<hyper::Response<BoxBodyInner>, Infallible>,
                        > + Send,
                >,
            > + Send
            + Sync,
    > = std::sync::Arc::new(move |req: Request<Incoming>| {
        let service = service.clone();
        let context = context.clone();
        let file_stream_semaphore = file_stream_semaphore.clone();
        let service_semaphore = service_semaphore.clone();
        let activity = activity.clone();
        let requests = requests.clone();
        let config = config.clone();
        Box::pin(async move {
            let mut guard = InFlightGuard::new(activity.clone());
            // Convert Hyper request to canonical RequestHead, enforcing the
            // EggServe-owned request-target and aggregate header ceilings
            // before any service work.
            let head = match convert_request_head(
                &req,
                config.max_request_target_bytes,
                config.max_header_bytes,
                conn_id,
            ) {
                Ok(h) => h,
                Err(e) => {
                    return Ok::<_, Infallible>(guard.finish(
                        e.to_response_with_head_and_policy(
                            false,
                            config.response_policy.error_policy,
                        ),
                        &config,
                        conn_id,
                    ));
                }
            };

            // TRACE content remains a transport-level rejection. Other
            // methods, including GET, HEAD, and DELETE, are governed by the
            // service-declared policy below.
            if head.method().as_str() == "TRACE"
                && (req
                    .headers()
                    .get(hyper::header::CONTENT_LENGTH)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .is_some_and(|length| length > 0)
                    || req.headers().contains_key(hyper::header::TRANSFER_ENCODING))
            {
                let mut response = crate::response::bad_request_with_policy(
                    false,
                    config.response_policy.error_policy,
                );
                response.headers_mut().insert(
                    hyper::header::CONNECTION,
                    hyper::header::HeaderValue::from_static("close"),
                );
                return Ok::<_, Infallible>(guard.finish(response, &config, conn_id));
            }

            let is_head = head.method().is_head();

            // Select effective body policy.
            let service_policy = service.request_body_policy(&head);
            let effective_policy = select_body_policy(service_policy, max_body_bytes);

            // Extract body from Hyper request.
            let (parts, body) = req.into_parts();

            // Validate body framing (TE+CL conflict, duplicate CL) for all methods.
            if let Err(e) = validate_body_framing(&parts.headers) {
                crate::ops::global_counters()
                    .parser_rejects
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                crate::ops::Logger::global().emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Debug,
                        crate::ops::EventKind::ParserRejection,
                        format!("parser rejection: {}", e),
                    )
                    .connection_id(conn_id),
                );
                let is_head = head.method().is_head();
                return Ok::<_, Infallible>(guard.finish(
                    e.to_response_with_head_and_policy(
                        is_head,
                        config.response_policy.error_policy,
                    ),
                    &config,
                    conn_id,
                ));
            }

            let declared_length = parts
                .headers
                .get(hyper::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());

            // Validate Content-Length against effective limit.
            if let Some(len) = declared_length {
                if let Some(limit) = effective_policy.max_bytes() {
                    if len > limit {
                        crate::ops::global_counters()
                            .body_rejections
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        crate::ops::Logger::global().emit(
                            crate::ops::Event::new(
                                crate::ops::Severity::Debug,
                                crate::ops::EventKind::BodyPolicyRejection,
                                "body too large",
                            )
                            .connection_id(conn_id)
                            .field(crate::ops::Field::U64("declared_bytes".into(), len))
                            .field(crate::ops::Field::U64("limit_bytes".into(), limit)),
                        );
                        let err = crate::primitives::request_body_error::RequestBodyError::DeclaredLengthTooLarge {
                            declared: len,
                            limit,
                        };
                        return Ok::<_, Infallible>(guard.finish(
                            body_error_to_response(err, &head, config.response_policy.error_policy),
                            &config,
                            conn_id,
                        ));
                    }
                }
            }

            // Reject Expect: 100-continue early — do not send an invitation
            // to send a body that will be rejected.
            if effective_policy.is_reject() {
                if let Some(expect) = parts.headers.get(hyper::header::EXPECT) {
                    if expect
                        .to_str()
                        .ok()
                        .is_some_and(|value| value.trim().eq_ignore_ascii_case("100-continue"))
                    {
                        crate::ops::global_counters()
                            .body_rejections
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        crate::ops::Logger::global().emit(
                            crate::ops::Event::new(
                                crate::ops::Severity::Debug,
                                crate::ops::EventKind::BodyPolicyRejection,
                                "100-continue rejected by body policy",
                            )
                            .connection_id(conn_id),
                        );
                        let mut response = crate::response::payload_too_large_with_policy(
                            is_head,
                            config.response_policy.error_policy,
                        );
                        response.headers_mut().insert(
                            hyper::header::CONNECTION,
                            hyper::header::HeaderValue::from_static("close"),
                        );
                        return Ok::<_, Infallible>(guard.finish(response, &config, conn_id));
                    }
                }
            }

            // Handle Reject policy — reject without invoking the service,
            // but only if the request actually carries a body.
            // A `Transfer-Encoding` header is treated as has-body even for
            // zero-length chunked input (`0\r\n\r\n`), since framing is
            // unknown until the stream is consumed. Size enforcement for
            // chunked bodies without `Content-Length` is deferred to the
            // streaming limit.
            let has_body = declared_length.is_some_and(|len| len > 0)
                || parts.headers.contains_key(hyper::header::TRANSFER_ENCODING);
            if effective_policy.is_reject() && has_body {
                crate::ops::global_counters()
                    .body_rejections
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                crate::ops::Logger::global().emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Debug,
                        crate::ops::EventKind::BodyPolicyRejection,
                        "request body rejected by policy",
                    )
                    .connection_id(conn_id),
                );
                crate::ops::Logger::global().emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Debug,
                        crate::ops::EventKind::ServiceInvocationSuppressed,
                        "service invocation suppressed: body rejected by policy",
                    )
                    .connection_id(conn_id),
                );
                let mut response = crate::response::payload_too_large_with_policy(
                    is_head,
                    config.response_policy.error_policy,
                );
                // Do not drain the body — drop it and close the connection to
                // prevent unread bytes from being interpreted as a subsequent
                // request. Hyper handles cleanup of the unconsumed body when
                // the connection is dropped.
                response.headers_mut().insert(
                    hyper::header::CONNECTION,
                    hyper::header::HeaderValue::from_static("close"),
                );
                return Ok::<_, Infallible>(guard.finish(response, &config, conn_id));
            }

            // For Buffer/Stream policies, create RequestBody with proper limits.
            // For Reject with no body, create an empty body (nothing to reject).
            // B-01: `declared_length > limit` is rejected above before `RequestBody`
            // construction. For `Transfer-Encoding: chunked` (no declared length)
            // enforcement is via `max_bytes` only. `Buffer` pre-buffers with
            // `read_all()` and fails fast; `Stream` delegates to the handler
            // under `min(body_read_timeout, handler_timeout)` and fails lazily
            // as `RequestBody` is consumed — intentional behavioral difference.
            let request_body = match &effective_policy {
                RequestBodyPolicy::Reject => crate::primitives::request_body::RequestBody::empty(),
                RequestBodyPolicy::Buffer { max_bytes }
                | RequestBodyPolicy::Stream { max_bytes } => {
                    crate::primitives::request_body::RequestBody::from_incoming(
                        wrap_incoming_body(body),
                        declared_length,
                        *max_bytes,
                    )
                }
            };

            // For Buffer policy, pre-buffer the body under timeout.
            match &effective_policy {
                RequestBodyPolicy::Reject => {
                    // Reject with no body — proceed to service with empty body.
                    // Service admission is independent of idle keep-alive
                    // connections: exhaustion fails with a deterministic
                    // generic 503 before service invocation.
                    if let Some(unavailable) = guard.admit(
                        &service_semaphore,
                        conn_id,
                        config.response_policy.error_policy,
                    ) {
                        return Ok::<_, Infallible>(guard.finish(unavailable, &config, conn_id));
                    }
                    let connection = context.connection_info();
                    requests.register(&request_body.shared());
                    let request =
                        crate::primitives::request::Request::new(head, request_body, connection);

                    let result = tokio::time::timeout(
                        handler_timeout,
                        contain_service_panic(service.call(request)),
                    )
                    .await;

                    let response = match result {
                        Ok(Ok(canonical)) => normalize_then_convert(
                            canonical,
                            is_head,
                            &file_stream_semaphore,
                            stream_chunk_size,
                            config.response_policy.error_policy,
                        ),
                        Ok(Err(service_err)) => {
                            let severity = if service_err.is_panic() || !service_err.is_timeout() {
                                crate::ops::Severity::Error
                            } else {
                                crate::ops::Severity::Warn
                            };
                            crate::ops::Logger::global().emit(
                                crate::ops::Event::new(
                                    severity,
                                    crate::ops::EventKind::ServiceError,
                                    crate::ops::sanitize_text_field(&service_err.to_string()),
                                )
                                .connection_id(conn_id),
                            );
                            service_err.to_response_with_head_and_policy(
                                is_head,
                                config.response_policy.error_policy,
                            )
                        }
                        Err(_elapsed) => {
                            crate::ops::Logger::global().emit(crate::ops::Event::new(
                                crate::ops::Severity::Warn,
                                crate::ops::EventKind::ServiceTimeout,
                                "handler timed out",
                            ));
                            ServiceError::timeout("handler timed out".to_string())
                                .to_response_with_head_and_policy(
                                    is_head,
                                    config.response_policy.error_policy,
                                )
                        }
                    };

                    Ok::<_, Infallible>(guard.finish(response, &config, conn_id))
                }
                RequestBodyPolicy::Buffer { .. } => {
                    // Buffer: body is fully consumed during pre-buffering.
                    // No incomplete body handling needed.
                    let body_limit = match effective_policy {
                        RequestBodyPolicy::Buffer { max_bytes } => max_bytes,
                        _ => unreachable!("buffer branch requires a buffer policy"),
                    };
                    let request_body = match tokio::time::timeout(
                        body_read_timeout,
                        request_body.read_all(),
                    )
                    .await
                    {
                        Ok(Ok(bytes)) => crate::primitives::request_body::RequestBody::from_bytes(
                            bytes, body_limit,
                        ),
                        Ok(Err(err)) => {
                            return Ok::<_, Infallible>(guard.finish(
                                body_error_to_response(
                                    err,
                                    &head,
                                    config.response_policy.error_policy,
                                ),
                                &config,
                                conn_id,
                            ));
                        }
                        Err(_elapsed) => {
                            crate::ops::global_counters()
                                .body_read_timeouts
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            crate::ops::Logger::global().emit(crate::ops::Event::new(
                                crate::ops::Severity::Warn,
                                crate::ops::EventKind::BodyReadTimeout,
                                "body read timeout",
                            ));
                            let err = crate::primitives::request_body_error::RequestBodyError::ReadTimeout;
                            return Ok::<_, Infallible>(guard.finish(
                                body_error_to_response(
                                    err,
                                    &head,
                                    config.response_policy.error_policy,
                                ),
                                &config,
                                conn_id,
                            ));
                        }
                    };

                    if let Some(unavailable) = guard.admit(
                        &service_semaphore,
                        conn_id,
                        config.response_policy.error_policy,
                    ) {
                        return Ok::<_, Infallible>(guard.finish(unavailable, &config, conn_id));
                    }
                    let connection = context.connection_info();
                    requests.register(&request_body.shared());
                    let request =
                        crate::primitives::request::Request::new(head, request_body, connection);

                    let result = tokio::time::timeout(
                        handler_timeout,
                        contain_service_panic(service.call(request)),
                    )
                    .await;

                    let response = match result {
                        Ok(Ok(canonical)) => normalize_then_convert(
                            canonical,
                            is_head,
                            &file_stream_semaphore,
                            stream_chunk_size,
                            config.response_policy.error_policy,
                        ),
                        Ok(Err(service_err)) => {
                            let severity = if service_err.is_panic() || !service_err.is_timeout() {
                                crate::ops::Severity::Error
                            } else {
                                crate::ops::Severity::Warn
                            };
                            crate::ops::Logger::global().emit(
                                crate::ops::Event::new(
                                    severity,
                                    crate::ops::EventKind::ServiceError,
                                    crate::ops::sanitize_text_field(&service_err.to_string()),
                                )
                                .connection_id(conn_id),
                            );
                            service_err.to_response_with_head_and_policy(
                                is_head,
                                config.response_policy.error_policy,
                            )
                        }
                        Err(_elapsed) => {
                            crate::ops::Logger::global().emit(crate::ops::Event::new(
                                crate::ops::Severity::Warn,
                                crate::ops::EventKind::ServiceTimeout,
                                "handler timed out",
                            ));
                            ServiceError::timeout("handler timed out".to_string())
                                .to_response_with_head_and_policy(
                                    is_head,
                                    config.response_policy.error_policy,
                                )
                        }
                    };

                    Ok::<_, Infallible>(guard.finish(response, &config, conn_id))
                }
                RequestBodyPolicy::Stream { .. } => {
                    // Plan 174 Track C (compatibility-preserving split):
                    // during `Service::call` the two deadlines remain
                    // collapsed as `min(body_read_timeout, handler_timeout)`
                    // with body-vs-handler disambiguation, preserving the
                    // documented pre-174 behavior for conventional handlers.
                    // Once response-start is available with an Active
                    // deferred body, `body_read_timeout` continues as a total
                    // deadline via the watchdog below while `handler_timeout`
                    // no longer applies to the downstream task.
                    let effective_timeout = body_read_timeout.min(handler_timeout);
                    // Total body deadline from ingestion start for the
                    // post-return watchdog.
                    let body_deadline = tokio::time::Instant::now() + body_read_timeout;
                    if let Some(unavailable) = guard.admit(
                        &service_semaphore,
                        conn_id,
                        config.response_policy.error_policy,
                    ) {
                        return Ok::<_, Infallible>(guard.finish(unavailable, &config, conn_id));
                    }
                    let connection = context.connection_info();
                    // Shared lifecycle observer retained by the runtime while
                    // the service owns/moves the actual body (Track A/B1).
                    let body_shared = request_body.shared();
                    requests.register(&body_shared);
                    let request =
                        crate::primitives::request::Request::new(head, request_body, connection);

                    let result = tokio::time::timeout(
                        effective_timeout,
                        contain_service_panic(service.call(request)),
                    )
                    .await;

                    let response = match result {
                        Ok(Ok(canonical)) => normalize_then_convert(
                            canonical,
                            is_head,
                            &file_stream_semaphore,
                            stream_chunk_size,
                            config.response_policy.error_policy,
                        ),
                        Ok(Err(service_err)) => {
                            let severity = if service_err.is_panic() || !service_err.is_timeout() {
                                crate::ops::Severity::Error
                            } else {
                                crate::ops::Severity::Warn
                            };
                            crate::ops::Logger::global().emit(
                                crate::ops::Event::new(
                                    severity,
                                    crate::ops::EventKind::ServiceError,
                                    crate::ops::sanitize_text_field(&service_err.to_string()),
                                )
                                .connection_id(conn_id),
                            );
                            service_err.to_response_with_head_and_policy(
                                is_head,
                                config.response_policy.error_policy,
                            )
                        }
                        Err(_elapsed) => {
                            // Preserved collapsed distinction: body still
                            // unconsumed => body stall (408 + counters),
                            // otherwise handler stall (504).
                            let body_pending = body_shared.is_body_active();
                            if body_pending {
                                crate::ops::global_counters()
                                    .body_read_timeouts
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                crate::ops::Logger::global().emit(crate::ops::Event::new(
                                    crate::ops::Severity::Warn,
                                    crate::ops::EventKind::BodyReadTimeout,
                                    "body read timeout",
                                ));
                                ServiceError::timeout("body read timeout".to_string())
                                    .to_response_with_head_and_policy(
                                        is_head,
                                        config.response_policy.error_policy,
                                    )
                            } else {
                                crate::ops::Logger::global().emit(crate::ops::Event::new(
                                    crate::ops::Severity::Warn,
                                    crate::ops::EventKind::ServiceTimeout,
                                    "handler timed out",
                                ));
                                ServiceError::timeout("handler timed out".to_string())
                                    .to_response_with_head_and_policy(
                                        is_head,
                                        config.response_policy.error_policy,
                                    )
                            }
                        }
                    };

                    // Ownership-derived reuse safety (Track B): returning the
                    // Response does not force a decision while the body is
                    // still Active (deferred to a downstream task). Hyper
                    // prevents next-request parsing until the framing
                    // boundary is complete (pinned by regression tests); EggServe
                    // forces close only on abandonment/failure.
                    use crate::primitives::request_lifecycle::BodyLifecycleState;
                    match body_shared.body_state() {
                        BodyLifecycleState::Complete => {
                            let response = guard.finish(response, &config, conn_id);
                            Ok::<_, Infallible>(response)
                        }
                        BodyLifecycleState::Active => {
                            // Deferred: response-start available while a valid
                            // downstream task still owns the body. Do NOT add
                            // `Connection: close` merely because response-start
                            // came first. Track for idle accounting and
                            // completion observability; permits remain distinct
                            // (Track F): service admission already released by
                            // `finish`, downstream owns its own budget.
                            crate::ops::global_counters()
                                .deferred_bodies_delegated
                                .fetch_add(1, Ordering::Relaxed);
                            crate::ops::Logger::global().emit(
                                crate::ops::Event::new(
                                    crate::ops::Severity::Debug,
                                    crate::ops::EventKind::DeferredBodyDelegated,
                                    "request body delegated past service return",
                                )
                                .connection_id(conn_id),
                            );
                            activity.deferred_started();
                            spawn_deferred_tracker(body_shared.clone(), activity.clone(), conn_id);
                            // Arm the remaining body deadline for deferred
                            // consumption. If already past deadline, the
                            // watchdog fires immediately.
                            {
                                let now = tokio::time::Instant::now();
                                let deadline = if body_deadline > now {
                                    body_deadline
                                } else {
                                    now
                                };
                                spawn_body_timeout_watchdog(
                                    body_shared.clone(),
                                    activity.clone(),
                                    deadline,
                                    conn_id,
                                );
                            }
                            let response = guard.finish(response, &config, conn_id);
                            Ok::<_, Infallible>(response)
                        }
                        BodyLifecycleState::Abandoned | BodyLifecycleState::Failed => {
                            crate::ops::Logger::global().emit(
                                crate::ops::Event::new(
                                    crate::ops::Severity::Debug,
                                    crate::ops::EventKind::IncompleteBodyClose,
                                    "service returned with abandoned/failed body; connection will close",
                                )
                                .connection_id(conn_id),
                            );
                            let mut response = guard.finish(response, &config, conn_id);
                            response.headers_mut().insert(
                                hyper::header::CONNECTION,
                                hyper::header::HeaderValue::from_static("close"),
                            );
                            Ok::<_, Infallible>(response)
                        }
                    }
                }
            }
        })
    });
    CanonicalHyperService { inner: handler }
}

/// Serve a single connection with a custom [`Service`] implementation.
///
/// This is a compatibility wrapper that builds a [`ConnectionContext`] from
/// the TCP socket addresses and delegates to [`make_canonical_hyper_service`]
/// and [`serve_connection`]. New callers should use
/// [`serve_http1_connection`] with an explicit [`ConnectionContext`].
///
/// Panics raised while polling the service future are contained and mapped
/// to [`ServiceError::panic`], producing a 500 response. Panics outside
/// service execution propagate to the tokio task boundary, are caught by
/// the `JoinSet` in the accept loop, and drop the connection with a
/// `ConnectionPanic` event.
#[allow(clippy::too_many_arguments)]
pub async fn serve_connection_with_runtime_state<I, S>(
    io: TokioIo<I>,
    service: S,
    config: Arc<RuntimeConfig>,
    runtime_state: Arc<RuntimeState>,
    shutdown_rx: &mut broadcast::Receiver<()>,
    conn_id: u64,
    local_addr: std::net::SocketAddr,
    remote_addr: std::net::SocketAddr,
    tls: bool,
    tls_info: Option<crate::primitives::connection_info::TlsInfo>,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: Service,
{
    let context = if tls {
        ConnectionContext::for_tcp(local_addr, remote_addr, tls_info)
    } else {
        ConnectionContext::for_tcp(local_addr, remote_addr, None)
    };
    let service = Arc::new(service);
    let file_stream_semaphore = runtime_state.file_stream_semaphore().clone();
    let service_semaphore = runtime_state.service_semaphore().clone();
    let activity = Arc::new(ConnectionActivity::new());
    let requests = Arc::new(ConnectionRequests::new());
    let hyper_service = make_canonical_hyper_service(
        service,
        config.clone(),
        file_stream_semaphore,
        service_semaphore,
        activity.clone(),
        requests.clone(),
        config.stream_chunk_size,
        config.handler_timeout,
        config.body_read_timeout,
        config.max_request_body_bytes,
        context,
        conn_id,
    );
    let _ = serve_connection(
        io,
        hyper_service,
        &config,
        &activity,
        &requests,
        shutdown_rx,
        conn_id,
    )
    .await;
}

/// Serve one HTTP/1 connection over any suitable bidirectional async byte
/// stream.
///
/// The caller supplies an already-established bidirectional async byte stream,
/// a canonical [`Service`], a [`ConnectionContext`], shared [`RuntimeState`]
/// admission, and a [`ConnectionShutdown`] token. EggServe supplies HTTP/1
/// parsing, request conversion, body policy, service dispatch, response
/// normalization/framing, timeouts, and closure semantics. The TCP/TLS
/// `Server` is a convenience runtime that owns listener acceptance and
/// handshake above this driver and shares the same pipeline.
///
/// No Hyper types appear in the signature. The caller need not supply
/// `SocketAddr` values — non-socket transports use
/// [`ConnectionContext::for_non_socket`]. Permits and producer tasks are
/// released on driver exit regardless of outcome.
///
/// # Example
///
/// ```no_run
/// use eggserve_core::server::connection::{
///     serve_http1_connection, ConnectionContext, ConnectionShutdown,
/// };
/// use eggserve_core::server::{RuntimeConfig, RuntimeState, service_fn, Request};
/// use eggserve_core::primitives::canonical::{Response, StatusCode, ResponseBody};
/// use eggserve_core::primitives::connection_info::Scheme;
/// use std::sync::Arc;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = Arc::new(RuntimeConfig::default());
/// let runtime_state = Arc::new(RuntimeState::new(&config));
/// let shutdown = ConnectionShutdown::new();
/// let context = ConnectionContext::for_non_socket(Scheme::Http, None);
///
/// let (client, server) = tokio::io::duplex(1024);
///
/// let outcome = serve_http1_connection(
///     server,
///     service_fn(|_req: Request| async {
///         Ok(Response::builder()
///             .status(StatusCode::OK)
///             .body(ResponseBody::Bytes(b"hello".to_vec()))
///             .unwrap())
///     }),
///     config,
///     context,
///     runtime_state,
///     &shutdown,
/// ).await;
/// # Ok(())
/// # }
/// ```
pub async fn serve_http1_connection<I, S>(
    io: I,
    service: S,
    config: Arc<RuntimeConfig>,
    context: ConnectionContext,
    runtime_state: Arc<RuntimeState>,
    shutdown: &ConnectionShutdown,
) -> ConnectionOutcome
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: Service,
{
    static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);
    let conn_id = NEXT_CONN_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    serve_http1_connection_with_id(
        io,
        service,
        config,
        context,
        runtime_state,
        shutdown,
        conn_id,
    )
    .await
}

/// Serve one HTTP/1 connection with an explicit connection ID.
///
/// Same as [`serve_http1_connection`] but uses the caller-supplied `conn_id`
/// for structured log correlation instead of generating one. The TCP accept
/// loop uses this to preserve its accept-time correlation IDs while sharing
/// the canonical driver pipeline.
pub async fn serve_http1_connection_with_id<I, S>(
    io: I,
    service: S,
    config: Arc<RuntimeConfig>,
    context: ConnectionContext,
    runtime_state: Arc<RuntimeState>,
    shutdown: &ConnectionShutdown,
    conn_id: u64,
) -> ConnectionOutcome
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: Service,
{
    let io = TokioIo::new(io);
    let service = Arc::new(service);
    let file_stream_semaphore = runtime_state.file_stream_semaphore().clone();
    let service_semaphore = runtime_state.service_semaphore().clone();
    let activity = Arc::new(ConnectionActivity::new());
    let requests = Arc::new(ConnectionRequests::new());
    let hyper_service = make_canonical_hyper_service(
        service,
        config.clone(),
        file_stream_semaphore,
        service_semaphore,
        activity.clone(),
        requests.clone(),
        config.stream_chunk_size,
        config.handler_timeout,
        config.body_read_timeout,
        config.max_request_body_bytes,
        context,
        conn_id,
    );
    serve_hyper_with_token(
        io,
        hyper_service,
        &config,
        &activity,
        &requests,
        shutdown,
        conn_id,
    )
    .await
}

/// Normalize a service response then convert to Hyper.
///
/// The runtime is the only framing authority: every service response
/// (static or custom, buffered or streaming) converges here. Normalization
/// is idempotent so eagerly normalized static responses are preserved
/// (HEAD equivalent-GET lengths, unknown-length omission). Conversion
/// failures become generic 500/503 without leaking details.
fn normalize_then_convert(
    canonical: crate::primitives::canonical::Response,
    is_head: bool,
    file_stream_semaphore: &std::sync::Arc<tokio::sync::Semaphore>,
    stream_chunk_size: usize,
    error_policy: crate::policy::ErrorRepresentationPolicy,
) -> hyper::Response<BoxBodyInner> {
    let normalized = match crate::primitives::canonical::normalize_response(
        canonical,
        &crate::primitives::canonical::NormalizeRequest::new(is_head),
    ) {
        Ok(r) => r,
        Err(_) => return crate::response::internal_error_with_policy(error_policy),
    };
    match crate::primitives::canonical::to_hyper_response_with_file_stream_semaphore_and_chunk_size(
        normalized,
        file_stream_semaphore,
        stream_chunk_size,
    ) {
        Ok(r) => r,
        Err(crate::primitives::canonical::ResponseConstructionError::FileStreamLimit) => {
            crate::response::service_unavailable_with_policy(error_policy)
        }
        Err(_) => crate::response::internal_error_with_policy(error_policy),
    }
}

/// Contain panics raised while polling a service future.
///
/// On panic, the payload is converted into [`ServiceError::panic`] so the
/// connection produces a 500 response instead of being dropped.
async fn contain_service_panic<F>(
    future: F,
) -> Result<crate::primitives::canonical::Response, ServiceError>
where
    F: std::future::Future<Output = Result<crate::primitives::canonical::Response, ServiceError>>,
{
    match futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(future)).await {
        Ok(result) => result,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "service panicked".to_string());
            Err(ServiceError::panic(message))
        }
    }
}

/// Select the effective body policy from service preference and runtime ceiling.
fn select_body_policy(service_policy: RequestBodyPolicy, max_body_bytes: u64) -> RequestBodyPolicy {
    match service_policy {
        RequestBodyPolicy::Reject => RequestBodyPolicy::Reject,
        RequestBodyPolicy::Buffer { max_bytes } => {
            let effective = max_bytes.min(max_body_bytes);
            if effective == 0 {
                RequestBodyPolicy::Reject
            } else {
                RequestBodyPolicy::Buffer {
                    max_bytes: effective,
                }
            }
        }
        RequestBodyPolicy::Stream { max_bytes } => {
            let effective = max_bytes.min(max_body_bytes);
            if effective == 0 {
                RequestBodyPolicy::Reject
            } else {
                RequestBodyPolicy::Stream {
                    max_bytes: effective,
                }
            }
        }
    }
}

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
fn spawn_body_timeout_watchdog(
    shared: Arc<RequestShared>,
    activity: Arc<ConnectionActivity>,
    deadline: tokio::time::Instant,
    conn_id: u64,
) {
    tokio::spawn(async move {
        tokio::select! {
            _ = shared.wait_body_terminal() => {},
            _ = tokio::time::sleep_until(deadline) => {
                if shared.is_body_active() {
                    shared.mark_failed_with_reason(
                        RequestCancellationReason::ConnectionTimeout,
                    );
                    crate::ops::global_counters()
                        .body_read_timeouts
                        .fetch_add(1, Ordering::Relaxed);
                    crate::ops::global_counters()
                        .deferred_body_timeouts
                        .fetch_add(1, Ordering::Relaxed);
                    crate::ops::Logger::global().emit(
                        crate::ops::Event::new(
                            crate::ops::Severity::Warn,
                            crate::ops::EventKind::BodyReadTimeout,
                            "deferred body read timeout",
                        )
                        .connection_id(conn_id),
                    );
                    crate::ops::Logger::global().emit(
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
fn spawn_deferred_tracker(
    shared: Arc<RequestShared>,
    activity: Arc<ConnectionActivity>,
    conn_id: u64,
) {
    tokio::spawn(async move {
        shared.wait_body_terminal().await;
        activity.deferred_finished();
        match shared.body_state() {
            crate::primitives::request_lifecycle::BodyLifecycleState::Complete => {
                crate::ops::global_counters()
                    .deferred_bodies_completed
                    .fetch_add(1, Ordering::Relaxed);
                crate::ops::Logger::global().emit(
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
                crate::ops::global_counters()
                    .deferred_bodies_abandoned
                    .fetch_add(1, Ordering::Relaxed);
                crate::ops::Logger::global().emit(
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

/// Convert a RequestBodyError to an HTTP response.
fn body_error_to_response(
    err: crate::primitives::request_body_error::RequestBodyError,
    _head: &crate::primitives::request_head::RequestHead,
    error_policy: crate::policy::ErrorRepresentationPolicy,
) -> hyper::Response<BoxBodyInner> {
    let raw_status = err.to_status_code();
    let status =
        hyper::StatusCode::from_u16(raw_status).unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR);
    // Cancelled/disconnected reads report the non-standard 499, which
    // Hyper refuses on the wire; the response collapses to 500 but the
    // connection must still close because the request ended mid-body.
    // Transport failures (raw_status 500) also end the request mid-body
    // with wire framing unknown, so they force close too; consumption-
    // state 500s are application bugs with no wire anomaly and stay alive.
    let should_close = raw_status == 499
        || err.is_transport()
        || matches!(
            status,
            hyper::StatusCode::BAD_REQUEST
                | hyper::StatusCode::REQUEST_TIMEOUT
                | hyper::StatusCode::PAYLOAD_TOO_LARGE
                | hyper::StatusCode::HTTP_VERSION_NOT_SUPPORTED
        );
    let body_text = match status.as_u16() {
        400 => "400 Bad Request\n",
        408 => "408 Request Timeout\n",
        413 => "413 Payload Too Large\n",
        _ => "500 Internal Server Error\n",
    };
    let is_head = _head.method().is_head();
    let mut resp =
        crate::response::canonical_error_with_policy(status, body_text, is_head, error_policy);
    if should_close {
        resp.headers_mut().insert(
            hyper::header::CONNECTION,
            hyper::header::HeaderValue::from_static("close"),
        );
    }
    resp
}

/// Apply the final-boundary response privacy policy at the one Hyper boundary.
///
/// Order: strip denylisted application headers first (so applications cannot
/// re-add stripped identifiers), then apply `Server` identification (a fixed
/// value survives a `server` denylist entry as explicit operator intent),
/// then apply `Date` policy as the sole authority (Hyper automatic `Date` is
/// disabled in [`hyper_builder`]). Duplicates are all removed. Framing and
/// hop-by-hop headers are never stripped (rejected at validation).
/// `Last-Modified` later than `Date` is dropped to preserve the RFC
/// invariant. No transport peer metadata is copied into response headers.
/// Client responses never contain log or service error text (callers pass
/// only fixed generic bodies here).
fn finalize_runtime_response(
    mut response: hyper::Response<BoxBodyInner>,
    config: &RuntimeConfig,
) -> hyper::Response<BoxBodyInner> {
    let policy = &config.response_policy;
    // 1. Denylist after service construction.
    for name in &policy.stripped_response_headers {
        // Validation guarantees these are not framing/hop-by-hop/date, so
        // removal cannot break framing invariants. `HeaderMap::remove`
        // removes all occurrences.
        response.headers_mut().remove(name.as_str());
    }
    // 2. Server identification: application values are always subordinate.
    response.headers_mut().remove(hyper::header::SERVER);
    if let Some(value) = &policy.server_identification {
        if let Ok(value) = hyper::header::HeaderValue::from_str(value) {
            response.headers_mut().insert(hyper::header::SERVER, value);
        }
    }
    // 3. Date: EggServe is the sole authority.
    response.headers_mut().remove(hyper::header::DATE);
    if let Some(now) = policy.date_policy.now() {
        // `now()` already guarantees formattability; the `from_str` guard
        // protects against a future formatting change.
        let date_str = httpdate::fmt_http_date(now);
        if let Ok(value) = hyper::header::HeaderValue::from_str(&date_str) {
            response.headers_mut().insert(hyper::header::DATE, value);
        }
    }
    // 4. Last-Modified must not be later than Date (RFC). When Date is
    // suppressed there is no reference to compare against, so retain
    // Last-Modified as-is; when both are present and Last-Modified is in
    // the future relative to Date, drop Last-Modified.
    if let (Some(date_val), Some(lm_val)) = (
        response.headers().get(hyper::header::DATE),
        response.headers().get(hyper::header::LAST_MODIFIED),
    ) {
        let date_ok = date_val
            .to_str()
            .ok()
            .and_then(|s| httpdate::parse_http_date(s).ok());
        let lm_ok = lm_val
            .to_str()
            .ok()
            .and_then(|s| httpdate::parse_http_date(s).ok());
        if let (Some(date_time), Some(lm_time)) = (date_ok, lm_ok) {
            if lm_time > date_time {
                response.headers_mut().remove(hyper::header::LAST_MODIFIED);
            }
        }
    }
    response
}

/// Validate body framing for ALL methods.
///
/// Rejects requests with duplicate Content-Length fields and TE+CL
/// conflicts where both headers are visible. Duplicate Content-Length values
/// that disagree are the request-smuggling vector (RFC 9110 §6.3.3) and are
/// rejected as conflicting; agreeing duplicates are still rejected as
/// duplicates (safe default). This is a hardened framing policy applied
/// before body construction.
///
/// Note: Hyper 1.x strips a lone Content-Length header when
/// Transfer-Encoding is present (since 1.11, regardless of header order;
/// TE wins per RFC 9112 §6.1) and rejects duplicate Content-Length fields
/// while decoding the request. Consequently, the TE+CL branch below is
/// defense-in-depth for a future or alternate parser and is unreachable
/// behind Hyper 1.11 — the lone-CL+TE corpus cases now exercise Hyper's
/// TE-wins normalization (200 with chunked framing) rather than this
/// rejection. Keeping the branch makes the framing policy explicit at this
/// boundary.
fn validate_body_framing(headers: &hyper::HeaderMap) -> Result<(), ServiceError> {
    let has_te = headers.contains_key(hyper::header::TRANSFER_ENCODING);
    let cl_values: Vec<_> = headers
        .get_all(hyper::header::CONTENT_LENGTH)
        .iter()
        .collect();
    let has_cl = !cl_values.is_empty();

    if has_te && has_cl {
        return Err(ServiceError::rejected(
            400,
            "conflicting Transfer-Encoding and Content-Length",
        ));
    }

    if cl_values.len() > 1 {
        let first = cl_values[0].as_bytes();
        if cl_values[1..].iter().any(|v| v.as_bytes() != first) {
            return Err(ServiceError::rejected(
                400,
                "conflicting Content-Length headers",
            ));
        }
        return Err(ServiceError::rejected(
            400,
            "duplicate Content-Length headers",
        ));
    }

    Ok(())
}

/// Wrap a Hyper `Incoming` body into a `Stream<Item = Result<Bytes, IncomingError>>`.
///
/// This bridges the Hyper body type to the canonical `RequestBody` type
/// without leaking Hyper into the public API.
fn wrap_incoming_body(
    body: Incoming,
) -> impl futures_util::Stream<
    Item = Result<bytes::Bytes, crate::primitives::request_body::IncomingError>,
> + Send
       + 'static {
    use futures_util::StreamExt;
    http_body_util::BodyStream::new(body).filter_map(|result| async {
        match result {
            Ok(frame) => frame.into_data().ok().map(Ok),
            Err(e) => Some(Err(crate::primitives::request_body::IncomingError(
                e.to_string(),
            ))),
        }
    })
}

/// Convert a Hyper request to a canonical [`RequestHead`], enforcing the
/// EggServe-owned request-target and aggregate header ceilings before any
/// service work.
///
/// Hyper enforces `max_buf_size` (parse buffer) and `max_headers` (field
/// count, answered with 431 by Hyper itself) during parsing; those rejections
/// never reach this function and surface as parse-class connection errors.
/// What Hyper cannot bound independently — aggregate header bytes and
/// request-target length — is bounded here:
///
/// - request targets longer than `max_target_bytes` fail with 414;
/// - aggregate post-parse header name+value bytes above `max_header_bytes`
///   fail with 431.
///
/// There is no separate request-line knob: the request line is bounded
/// jointly by the parser buffer (raw bytes) and this target-length ceiling
/// (application semantics). Neither hostile targets nor header contents are
/// logged; only lengths are recorded as fields.
fn convert_request_head(
    req: &Request<Incoming>,
    max_target_bytes: usize,
    max_header_bytes: usize,
    conn_id: u64,
) -> Result<crate::primitives::request_head::RequestHead, ServiceError> {
    use crate::primitives::header_block::HeaderBlock;
    use crate::primitives::method::Method;
    use crate::primitives::request_target::RequestTarget;
    use crate::primitives::version::HttpVersion;

    let method = match req.method().as_str() {
        "GET" => Method::get(),
        "HEAD" => Method::head(),
        "POST" => Method::post(),
        "PUT" => Method::put(),
        "DELETE" => Method::delete(),
        "PATCH" => Method::patch(),
        "OPTIONS" => Method::options(),
        "TRACE" => Method::trace(),
        other => Method::new(other)
            .map_err(|_| ServiceError::rejected(400, format!("invalid method: {}", other)))?,
    };

    let version = match req.version() {
        hyper::Version::HTTP_10 => HttpVersion::Http10,
        hyper::Version::HTTP_11 => HttpVersion::Http11,
        other => {
            return Err(ServiceError::rejected(
                505,
                format!("unsupported HTTP version: {:?}", other),
            ))
        }
    };

    let raw_target = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    if raw_target.len() > max_target_bytes {
        crate::ops::global_counters()
            .request_target_rejected
            .fetch_add(1, Ordering::Relaxed);
        crate::ops::Logger::global().emit(
            crate::ops::Event::new(
                crate::ops::Severity::Debug,
                crate::ops::EventKind::RequestTargetTooLong,
                "request target too long",
            )
            .connection_id(conn_id)
            .field(crate::ops::Field::U64(
                "target_bytes".into(),
                raw_target.len() as u64,
            ))
            .field(crate::ops::Field::U64(
                "limit_bytes".into(),
                max_target_bytes as u64,
            )),
        );
        return Err(ServiceError::rejected(414, "request target too long"));
    }

    // Reject absolute-form URIs (authority present in raw target).
    // Hyper strips scheme/authority from path_and_query, so we must check
    // the full URI string.
    if req.uri().scheme_str().is_some() {
        return Err(ServiceError::rejected(
            400,
            "absolute-form request target not allowed",
        ));
    }

    // Asterisk-form (`*`) is rejected as method-not-allowed (405) rather
    // than bad-request (400) because the method check must fire before the
    // target-form check per the release contract.
    if raw_target == "*" {
        return Err(ServiceError::rejected(
            405,
            format!("method not allowed: {}", method.as_str()),
        ));
    }

    let target = RequestTarget::parse(raw_target)
        .map_err(|e| ServiceError::rejected(400, format!("invalid request target: {}", e)))?;

    let mut headers = HeaderBlock::new();
    let mut header_bytes: usize = 0;
    for (name, value) in req.headers().iter() {
        header_bytes = header_bytes
            .saturating_add(name.as_str().len())
            .saturating_add(value.len());
        if header_bytes > max_header_bytes {
            crate::ops::global_counters()
                .header_bytes_rejected
                .fetch_add(1, Ordering::Relaxed);
            crate::ops::Logger::global().emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Debug,
                    crate::ops::EventKind::HeaderBytesRejected,
                    "request headers too large",
                )
                .connection_id(conn_id)
                .field(crate::ops::Field::U64(
                    "limit_bytes".into(),
                    max_header_bytes as u64,
                )),
            );
            return Err(ServiceError::rejected(431, "request headers too large"));
        }
        let header_name = crate::primitives::header_block::HeaderName::new(name.as_str())
            .map_err(|_| ServiceError::rejected(400, format!("invalid header name: {}", name)))?;
        // Byte-preserving inbound conversion (Plan 173 Track C1): legal opaque
        // bytes reach the service unchanged. Aggregate limits above already
        // count bytes, not Unicode scalars.
        let header_value = crate::primitives::header_block::HeaderValue::from_bytes(
            value.as_bytes(),
        )
        .map_err(|_| ServiceError::rejected(400, format!("invalid header value for {}", name)))?;
        headers.push(header_name, header_value);
    }

    Ok(crate::primitives::request_head::RequestHead::new(
        method, target, version, headers,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ServeConfig, ServeState};
    use crate::server::static_service::StaticService;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn build_state(tmp: &TempDir) -> Arc<ServeState> {
        let config = Arc::new(ServeConfig {
            root: tmp.path().to_path_buf(),
            ..ServeConfig::default()
        });
        Arc::new(ServeState::new(config).unwrap())
    }

    #[tokio::test]
    async fn serve_connection_handles_get() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
        let state = build_state(&tmp);
        let config = Arc::new(RuntimeConfig::default());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, _rx) = broadcast::channel::<()>(1);

        let state_clone = state.clone();
        let server = tokio::spawn(async move {
            let (stream, remote_addr) = listener.accept().await.unwrap();
            let mut shutdown_rx = tx.subscribe();
            let runtime_state = Arc::new(RuntimeState::new(&config));
            serve_connection_with_runtime_state(
                TokioIo::new(stream),
                StaticService::from_state(state_clone),
                config,
                runtime_state,
                &mut shutdown_rx,
                1,
                addr,
                remote_addr,
                false,
                None,
            )
            .await;
        });

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();

        let _ = server.await;

        let response = String::from_utf8_lossy(&buf);
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "unexpected response: {}",
            response
        );
    }

    #[test]
    fn runtime_server_header_replaces_service_value() {
        let config = RuntimeConfig::builder()
            .server_header("eggserve-test".into())
            .build()
            .unwrap();
        let mut response = crate::response::not_found(false);
        response.headers_mut().insert(
            hyper::header::SERVER,
            hyper::header::HeaderValue::from_static("spoofed"),
        );
        let response = finalize_runtime_response(response, &config);
        assert_eq!(
            response.headers().get(hyper::header::SERVER).unwrap(),
            "eggserve-test"
        );
        assert_eq!(
            response
                .headers()
                .get_all(hyper::header::SERVER)
                .iter()
                .count(),
            1
        );
    }

    #[test]
    fn framing_rejects_disagreeing_duplicate_content_length() {
        let mut headers = hyper::HeaderMap::new();
        headers.append(
            hyper::header::CONTENT_LENGTH,
            hyper::header::HeaderValue::from_static("5"),
        );
        headers.append(
            hyper::header::CONTENT_LENGTH,
            hyper::header::HeaderValue::from_static("10"),
        );
        let err = validate_body_framing(&headers).unwrap_err();
        assert_eq!(err.message(), "conflicting Content-Length headers");
    }

    #[test]
    fn framing_rejects_agreeing_duplicate_content_length() {
        let mut headers = hyper::HeaderMap::new();
        headers.append(
            hyper::header::CONTENT_LENGTH,
            hyper::header::HeaderValue::from_static("5"),
        );
        headers.append(
            hyper::header::CONTENT_LENGTH,
            hyper::header::HeaderValue::from_static("5"),
        );
        let err = validate_body_framing(&headers).unwrap_err();
        assert_eq!(err.message(), "duplicate Content-Length headers");
    }

    #[test]
    fn framing_accepts_single_content_length() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert(
            hyper::header::CONTENT_LENGTH,
            hyper::header::HeaderValue::from_static("5"),
        );
        assert!(validate_body_framing(&headers).is_ok());
    }

    #[test]
    fn body_error_transport_forces_connection_close() {
        fn head() -> crate::primitives::request_head::RequestHead {
            crate::primitives::request_head::RequestHead::new(
                crate::primitives::method::Method::get(),
                crate::primitives::request_target::RequestTarget::parse("/x").unwrap(),
                crate::primitives::version::HttpVersion::Http11,
                crate::primitives::header_block::HeaderBlock::new(),
            )
        }

        // Transport failures (500) must force close: the body stream broke
        // mid-read, so wire framing state is unknown.
        let transport = body_error_to_response(
            crate::primitives::request_body_error::RequestBodyError::Transport("io".into()),
            &head(),
            crate::policy::ErrorRepresentationPolicy::Minimal,
        );
        assert_eq!(transport.status(), hyper::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            transport
                .headers()
                .get(hyper::header::CONNECTION)
                .map(|v| v.as_bytes()),
            Some(&b"close"[..])
        );

        // Application-state 500s have no wire anomaly and stay reusable.
        let consumed = body_error_to_response(
            crate::primitives::request_body_error::RequestBodyError::AlreadyConsumed,
            &head(),
            crate::policy::ErrorRepresentationPolicy::Minimal,
        );
        assert_eq!(consumed.status(), hyper::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(consumed.headers().get(hyper::header::CONNECTION).is_none());

        // 499-collapsed disconnects still force close.
        let disconnected = body_error_to_response(
            crate::primitives::request_body_error::RequestBodyError::Disconnected,
            &head(),
            crate::policy::ErrorRepresentationPolicy::Minimal,
        );
        assert_eq!(
            disconnected
                .headers()
                .get(hyper::header::CONNECTION)
                .map(|v| v.as_bytes()),
            Some(&b"close"[..])
        );
    }
}
