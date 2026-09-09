//! HTTP/1 driver: Hyper builder, graceful close, outcome classification,
//! and the deadline/select loop.
//!
//! Sole authority for total lifetime, keep-alive idle timeout, response write
//! no-progress timeout, deferred-body timeout closure, server/caller
//! shutdown, and final `ConnectionOutcome` classification. Deadline
//! computation is not duplicated in transport-specific wrappers. Hyper's
//! ordinary HTTP/1 connection is used deliberately: no latent upgrade
//! capability is enabled because the canonical service boundary has no
//! upgrade handoff (Plan 176 stays deferred).

use bytes::Bytes;
use std::convert::Infallible;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::{Request, Response};
#[cfg(feature = "http2")]
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::sync::broadcast;

use crate::primitives::request_lifecycle::RequestCancellationReason;
use crate::response::BoxBodyInner;
use crate::server::config::RuntimeConfig;

use super::activity::ConnectionActivity;
use super::context::{ConnectionOutcome, ConnectionShutdown};
use super::lifecycle::ConnectionRequests;
use super::transport::ProgressIo;

/// Wire protocol selected before the Hyper connection future is constructed.
/// `Auto` is used for cleartext caller-owned streams and performs a bounded
/// H2 prior-knowledge preface check. TLS callers use the negotiated ALPN to
/// select `Http1` or `Http2` strictly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WireProtocol {
    Auto,
    Http1,
    #[cfg(feature = "http2")]
    Http2,
}

/// Graceful-shutdown capability for the pinned Hyper connection future, so
/// the shared driver below can close idle/stalled/expired connections
/// without knowing the concrete Hyper connection type.
trait ShutdownConn {
    fn graceful_shutdown(self: std::pin::Pin<&mut Self>);
}

impl<I, S> ShutdownConn for hyper::server::conn::http1::Connection<I, S>
where
    I: hyper::rt::Read + hyper::rt::Write + Unpin,
    S: hyper::service::Service<
        Request<Incoming>,
        Response = Response<BoxBodyInner>,
        Error = Infallible,
    >,
{
    fn graceful_shutdown(self: std::pin::Pin<&mut Self>) {
        hyper::server::conn::http1::Connection::graceful_shutdown(self);
    }
}

#[cfg(feature = "http2")]
impl<I, S, E> ShutdownConn for hyper::server::conn::http2::Connection<I, S, E>
where
    I: hyper::rt::Read + hyper::rt::Write + Unpin,
    S: hyper::service::HttpService<Incoming, ResBody = BoxBodyInner, Error = Infallible>,
    S::Future: Send + 'static,
    E: hyper::rt::bounds::Http2ServerConnExec<S::Future, BoxBodyInner>,
{
    fn graceful_shutdown(self: std::pin::Pin<&mut Self>) {
        hyper::server::conn::http2::Connection::graceful_shutdown(self);
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
    let http1_config = config.http1_config();
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(config.header_read_timeout)
        .max_buf_size(
            http1_config
                .max_buf_size
                .max(crate::limits::MIN_MAX_BUF_SIZE),
        )
        .max_headers(http1_config.max_headers)
        .auto_date_header(false);
    builder
}

#[cfg(feature = "http2")]
fn hyper2_builder(config: &RuntimeConfig) -> hyper::server::conn::http2::Builder<TokioExecutor> {
    let h2 = &config.http2;
    let mut builder = hyper::server::conn::http2::Builder::new(TokioExecutor::new());
    builder
        .timer(TokioTimer::new())
        .initial_stream_window_size(h2.initial_stream_window_size)
        .initial_connection_window_size(h2.initial_connection_window_size)
        .max_frame_size(h2.max_frame_size)
        .max_header_list_size(h2.max_header_list_size)
        .max_concurrent_streams(h2.max_concurrent_streams)
        .max_send_buf_size(h2.max_send_buf_size)
        .max_local_error_reset_streams(h2.max_local_error_reset_streams)
        .max_pending_accept_reset_streams(h2.max_pending_accept_reset_streams)
        .adaptive_window(h2.adaptive_window)
        .keep_alive_interval(h2.keep_alive_interval)
        .keep_alive_timeout(h2.keep_alive_timeout)
        .auto_date_header(false);
    builder
}

const H2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// An already-read protocol prefix replayed to Hyper before the underlying
/// stream. The pre-read is necessary because Hyper's H1/H2 auto detector has
/// no header-read timeout while it waits for the H2 preface.
struct PrefixedIo<I> {
    prefix: Bytes,
    inner: I,
}

impl<I> PrefixedIo<I> {
    fn new(prefix: Vec<u8>, inner: I) -> Self {
        Self {
            prefix: Bytes::from(prefix),
            inner,
        }
    }
}

impl<I: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for PrefixedIo<I> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if !self.prefix.is_empty() {
            let count = self.prefix.len().min(buf.remaining());
            buf.put_slice(&self.prefix.split_to(count));
            return std::task::Poll::Ready(Ok(()));
        }
        std::pin::Pin::new(&mut self.inner).poll_read(_cx, buf)
    }
}

impl<I: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for PrefixedIo<I> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

/// Classify a cleartext stream without losing bytes or weakening H1's header
/// timeout. A stream that diverges from the H2 preface at any byte is H1;
/// only the complete preface selects H2.
async fn classify_cleartext<I>(
    mut io: I,
    config: &RuntimeConfig,
    shutdown: &ConnectionShutdown,
    activity: &Arc<ConnectionActivity>,
    conn_id: u64,
) -> Result<(PrefixedIo<I>, WireProtocol), ConnectionOutcome>
where
    I: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut prefix = Vec::with_capacity(H2_PREFACE.len());
    let deadline = tokio::time::sleep(config.header_read_timeout);
    tokio::pin!(deadline);
    loop {
        if !H2_PREFACE.starts_with(&prefix) {
            return Ok((PrefixedIo::new(prefix, io), WireProtocol::Http1));
        }
        if prefix.len() == H2_PREFACE.len() {
            #[cfg(feature = "http2")]
            if config.http2.enabled {
                return Ok((PrefixedIo::new(prefix, io), WireProtocol::Http2));
            }
            return Ok((PrefixedIo::new(prefix, io), WireProtocol::Http1));
        }
        let mut chunk = [0u8; 24];
        let remaining = H2_PREFACE.len() - prefix.len();
        tokio::select! {
            _ = shutdown.cancelled() => return Err(ConnectionOutcome::Shutdown),
            _ = &mut deadline => {
                activity.ops().counters().header_timeouts.fetch_add(1, Ordering::Relaxed);
                activity.ops().emit(crate::ops::Event::new(
                    crate::ops::Severity::Warn,
                    crate::ops::EventKind::HeaderTimeout,
                    "protocol preface timeout",
                ).connection_id(conn_id));
                return Err(ConnectionOutcome::HeaderTimeout);
            }
            result = io.read(&mut chunk[..remaining]) => {
                match result {
                    Ok(0) => return Ok((PrefixedIo::new(prefix, io), WireProtocol::Http1)),
                    Ok(count) => prefix.extend_from_slice(&chunk[..count]),
                    Err(_) => return Err(ConnectionOutcome::ClientError),
                }
            }
        }
    }
}

fn record_protocol(protocol: WireProtocol, conn_id: u64, ops: &crate::ops::OpsContext) {
    let name = match protocol {
        WireProtocol::Auto => "auto",
        WireProtocol::Http1 => "http/1.1",
        #[cfg(feature = "http2")]
        WireProtocol::Http2 => "h2",
    };
    ops.emit(
        crate::ops::Event::new(
            crate::ops::Severity::Debug,
            crate::ops::EventKind::ProtocolNegotiated,
            "application protocol selected",
        )
        .connection_id(conn_id)
        .field(crate::ops::Field::Str("protocol".into(), name.into())),
    );
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
async fn graceful_close<C>(
    mut conn: std::pin::Pin<&mut C>,
    config: &RuntimeConfig,
    conn_id: u64,
    ops: &crate::ops::OpsContext,
) where
    C: std::future::Future<Output = Result<(), hyper::Error>> + ShutdownConn,
{
    conn.as_mut().graceful_shutdown();
    if tokio::time::timeout(post_shutdown_drain_budget(config), conn.as_mut())
        .await
        .is_err()
    {
        ops.emit(
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
fn finish_conn_result(
    result: Result<(), hyper::Error>,
    conn_id: u64,
    ops: &crate::ops::OpsContext,
) -> ConnectionOutcome {
    match result {
        Ok(()) => {
            ops.emit(
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
                ops.counters()
                    .header_timeouts
                    .fetch_add(1, Ordering::Relaxed);
            } else if parse_error {
                ops.counters()
                    .parser_rejects
                    .fetch_add(1, Ordering::Relaxed);
            }
            ops.emit(
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
    multiplexed: bool,
    shutdown: F,
) -> ConnectionOutcome
where
    C: std::future::Future<Output = Result<(), hyper::Error>> + ShutdownConn,
    F: std::future::Future<Output = ()>,
{
    let total_deadline = activity.start.checked_add(config.connection_total_timeout);
    let ops = activity.ops().clone();
    let mut shutdown = std::pin::pin!(shutdown);
    loop {
        let now = std::time::Instant::now();
        if total_deadline.is_some_and(|deadline| now >= deadline) {
            ops.counters()
                .connection_total_timeouts
                .fetch_add(1, Ordering::Relaxed);
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Warn,
                    crate::ops::EventKind::ConnectionTotalTimeout,
                    "connection total timeout",
                )
                .connection_id(conn_id),
            );
            requests.cancel_all(RequestCancellationReason::ConnectionTimeout, conn_id, &ops);
            graceful_close(conn.as_mut(), config, conn_id, &ops).await;
            return ConnectionOutcome::TotalTimeout;
        }
        // Deferred-body timeout fired by the per-request watchdog: the body
        // was already marked Failed and its lifecycle cancelled with
        // ConnectionTimeout. Close the transport so pending body/response
        // polls wake via transport failure.
        if activity.take_body_timeout() {
            graceful_close(conn.as_mut(), config, conn_id, &ops).await;
            requests.cancel_all(RequestCancellationReason::ConnectionTimeout, conn_id, &ops);
            return ConnectionOutcome::ClientError;
        }
        if multiplexed && activity.take_drain_request() {
            graceful_close(conn.as_mut(), config, conn_id, &ops).await;
            requests.cancel_all(RequestCancellationReason::ServerShutdown, conn_id, &ops);
            return ConnectionOutcome::Shutdown;
        }
        let (in_flight, outstanding, _completed, deferred, state) = activity.snapshot();
        let idle = in_flight == 0 && outstanding == 0 && deferred == 0;
        if idle && now.duration_since(state.last_activity) >= config.keep_alive_idle_timeout {
            ops.counters()
                .keepalive_idle_timeouts
                .fetch_add(1, Ordering::Relaxed);
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Debug,
                    crate::ops::EventKind::KeepAliveIdleTimeout,
                    "keep-alive idle timeout",
                )
                .connection_id(conn_id),
            );
            graceful_close(conn.as_mut(), config, conn_id, &ops).await;
            return ConnectionOutcome::IdleTimeout;
        }
        let write_stalled = if multiplexed {
            activity.h2_response_stalled(now, config.response_write_timeout)
        } else {
            outstanding > 0 && now.duration_since(state.last_write) >= config.response_write_timeout
        };
        if write_stalled {
            ops.counters()
                .write_stall_timeouts
                .fetch_add(1, Ordering::Relaxed);
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Warn,
                    crate::ops::EventKind::WriteStallTimeout,
                    "response write stall timeout",
                )
                .connection_id(conn_id),
            );
            requests.cancel_all(RequestCancellationReason::ConnectionTimeout, conn_id, &ops);
            graceful_close(conn.as_mut(), config, conn_id, &ops).await;
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
        if multiplexed {
            if let Some(deadline) = activity.h2_response_deadline(config.response_write_timeout) {
                wake = wake.min(deadline);
            }
        } else if outstanding > 0 {
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
                let outcome = finish_conn_result(result, conn_id, &ops);
                // Peer disconnect / transport failure must wake idle
                // downstream waiters even if they are not polling body/response IO.
                match outcome {
                    ConnectionOutcome::ClientError => {
                        requests.cancel_all(RequestCancellationReason::PeerDisconnected, conn_id, &ops);
                    }
                    ConnectionOutcome::HeaderTimeout => {
                        requests.cancel_all(RequestCancellationReason::ConnectionTimeout, conn_id, &ops);
                    }
                    _ => {}
                }
                return outcome;
            }
            _ = &mut shutdown => {
                requests.cancel_all(RequestCancellationReason::ServerShutdown, conn_id, &ops);
                graceful_close(conn.as_mut(), config, conn_id, &ops).await;
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
    let conn = hyper_builder(config).serve_connection(io, service);
    let mut conn = std::pin::pin!(conn);
    let shutdown = async move {
        let _ = shutdown_rx.recv().await;
    };
    drive_connection(
        conn.as_mut(),
        config,
        activity,
        requests,
        conn_id,
        false,
        shutdown,
    )
    .await
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
    S::Future: Send + 'static,
{
    serve_selected_with_token(
        io.into_inner(),
        service,
        config,
        activity,
        requests,
        shutdown,
        conn_id,
        WireProtocol::Http1,
    )
    .await
}

/// Drive a connection after the protocol has been selected by TLS ALPN or a
/// cleartext preface check. This is also the shared implementation used by
/// the public multi-protocol caller-owned entry point.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn serve_selected_with_token<I, S>(
    io: I,
    service: S,
    config: &RuntimeConfig,
    activity: &Arc<ConnectionActivity>,
    requests: &Arc<ConnectionRequests>,
    shutdown: &ConnectionShutdown,
    conn_id: u64,
    protocol: WireProtocol,
) -> ConnectionOutcome
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: hyper::service::Service<
            Request<Incoming>,
            Response = Response<BoxBodyInner>,
            Error = Infallible,
        > + 'static,
    S::Future: Send + 'static,
{
    if protocol == WireProtocol::Auto {
        return match classify_cleartext(io, config, shutdown, activity, conn_id).await {
            Ok((io, selected)) => {
                serve_selected_resolved_with_token(
                    io, service, config, activity, requests, shutdown, conn_id, selected,
                )
                .await
            }
            Err(outcome) => outcome,
        };
    }
    serve_selected_resolved_with_token(
        io, service, config, activity, requests, shutdown, conn_id, protocol,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn serve_selected_resolved_with_token<I, S>(
    io: I,
    service: S,
    config: &RuntimeConfig,
    activity: &Arc<ConnectionActivity>,
    requests: &Arc<ConnectionRequests>,
    shutdown: &ConnectionShutdown,
    conn_id: u64,
    protocol: WireProtocol,
) -> ConnectionOutcome
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: hyper::service::Service<
            Request<Incoming>,
            Response = Response<BoxBodyInner>,
            Error = Infallible,
        > + 'static,
    S::Future: Send + 'static,
{
    let ops = activity.ops().clone();

    record_protocol(protocol, conn_id, &ops);
    let io = TokioIo::new(ProgressIo::new(io, activity.clone()));
    let shutdown = async move {
        shutdown.cancelled().await;
    };
    match protocol {
        WireProtocol::Http1 => {
            let conn = hyper_builder(config).serve_connection(io, service);
            let mut conn = std::pin::pin!(conn);
            drive_connection(
                conn.as_mut(),
                config,
                activity,
                requests,
                conn_id,
                false,
                shutdown,
            )
            .await
        }
        #[cfg(feature = "http2")]
        WireProtocol::Http2 => {
            let conn = hyper2_builder(config).serve_connection(io, service);
            let mut conn = std::pin::pin!(conn);
            drive_connection(
                conn.as_mut(),
                config,
                activity,
                requests,
                conn_id,
                true,
                shutdown,
            )
            .await
        }
        WireProtocol::Auto => unreachable!("auto was resolved above"),
    }
}

/// Drive a cleartext caller-owned stream, accepting H1 or H2 prior knowledge.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn serve_hyper_with_token_auto<I, S>(
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
    S::Future: Send + 'static,
{
    serve_selected_with_token(
        io.into_inner(),
        service,
        config,
        activity,
        requests,
        shutdown,
        conn_id,
        WireProtocol::Auto,
    )
    .await
}
