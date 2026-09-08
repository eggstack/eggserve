//! HTTP/1 driver: Hyper builder, graceful close, outcome classification,
//! and the deadline/select loop.
//!
//! Sole authority for total lifetime, keep-alive idle timeout, response write
//! no-progress timeout, deferred-body timeout closure, server/caller
//! shutdown, and final `ConnectionOutcome` classification. Deadline
//! computation is not duplicated in transport-specific wrappers. The internal
//! Hyper connection keeps `.with_upgrades()` as an implementation detail;
//! no public upgrade vocabulary or escape hatch is exposed (Plan 176 stays
//! deferred).

use std::convert::Infallible;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::sync::broadcast;

use crate::primitives::request_lifecycle::RequestCancellationReason;
use crate::response::BoxBodyInner;
use crate::server::config::RuntimeConfig;

use super::activity::ConnectionActivity;
use super::context::{ConnectionOutcome, ConnectionShutdown};
use super::lifecycle::ConnectionRequests;
use super::transport::ProgressIo;

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
pub(crate) async fn serve_hyper_with_token<I, S>(
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
