//! Compatibility connection composition (Plan 249).
//!
//! Core owns H2-specific Hyper execution and the bounded cleartext
//! H2-prior-knowledge classifier. All HTTP/1 execution is owned by
//! `eggserve-server` (single H1 authority); core never constructs or drives a
//! Hyper HTTP/1 connection. This module therefore provides:
//!
//! - protocol selection/replay composition (`WireProtocol`, `PrefixedIo`,
//!   `classify_cleartext`);
//! - H2-specific execution (`hyper2_builder`, H2 `ShutdownConn`,
//!   `drive_connection`, `serve_h2_with_token`);
//!
//! H1 callers delegate to
//! `eggserve_server::connection::serve_http1_connection_with_id` with the
//! replayable stream. H2 execution remains core-owned and feature-gated.

use bytes::Bytes;

#[cfg(feature = "http2")]
use std::convert::Infallible;
#[cfg(feature = "http2")]
use std::sync::atomic::Ordering;
#[cfg(feature = "http2")]
use std::sync::Arc;

#[cfg(feature = "http2")]
use hyper::body::Incoming;
#[cfg(feature = "http2")]
use hyper::{Request, Response};
#[cfg(feature = "http2")]
use hyper_util::rt::TokioExecutor;
#[cfg(feature = "http2")]
use hyper_util::rt::{TokioIo, TokioTimer};

#[cfg(feature = "http2")]
use crate::primitives::request_lifecycle::RequestCancellationReason;
#[cfg(feature = "http2")]
use crate::response::BoxBodyInner;
#[cfg(feature = "http2")]
use crate::server::config::RuntimeConfig;

#[cfg(feature = "http2")]
use super::activity::ConnectionActivity;
#[cfg(feature = "http2")]
use super::context::{ConnectionOutcome, ConnectionShutdown};
#[cfg(feature = "http2")]
use super::lifecycle::ConnectionRequests;
#[cfg(feature = "http2")]
use super::transport::ProgressIo;

/// Wire protocol selected before the Hyper connection future is constructed.
/// `Auto` is used for cleartext caller-owned streams and performs a bounded
/// H2 prior-knowledge preface check. TLS callers use the negotiated ALPN to
/// select `Http1` or `Http2` strictly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WireProtocol {
    Auto,
    // Explicit H1 selection exists when TLS ALPN can negotiate it or when
    // H2 classification can resolve to H1. Minimal builds (neither) only
    // ever see `Auto` cleartext H1.
    #[cfg(any(feature = "tls", feature = "http2"))]
    Http1,
    #[cfg(feature = "http2")]
    Http2,
}

/// Graceful-shutdown capability for the pinned H2 Hyper connection future,
/// so the shared H2 driver below can close idle/stalled/expired connections
/// without knowing the concrete Hyper connection type. H1 has no core driver
/// (Plan 249: single H1 authority in `eggserve-server`).
#[cfg(feature = "http2")]
trait ShutdownConn {
    fn graceful_shutdown(self: std::pin::Pin<&mut Self>);
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
#[cfg(feature = "http2")]
const MAX_POST_SHUTDOWN_DRAIN: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(feature = "http2")]
fn post_shutdown_drain_budget(config: &RuntimeConfig) -> std::time::Duration {
    config
        .graceful_shutdown_timeout
        .min(MAX_POST_SHUTDOWN_DRAIN)
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
        .enable_connect_protocol()
        .auto_date_header(false);
    builder
}

#[cfg(feature = "http2")]
const H2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// An already-read protocol prefix replayed to Hyper before the underlying
/// stream. The pre-read is necessary because Hyper's H1/H2 auto detector has
/// no header-read timeout while it waits for the H2 preface.
///
/// Also reused by the Plan 202 PROXY preamble path to replay bytes read
/// beyond the preamble (e.g., the start of a TLS ClientHello) without loss.
pub(crate) struct PrefixedIo<I> {
    prefix: Bytes,
    inner: I,
}

impl<I> PrefixedIo<I> {
    pub(crate) fn new(prefix: Vec<u8>, inner: I) -> Self {
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
///
/// H2-gated (Plan 249): without `http2`, `Auto` delegates directly to the
/// direct H1 authority with no preface sniffing, so this helper is only
/// compiled when H2 selection exists.
#[cfg(feature = "http2")]
pub(crate) async fn classify_cleartext<I>(
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

#[cfg(feature = "http2")]
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
#[cfg(feature = "http2")]
fn min_deadline(
    current: Option<std::time::Instant>,
    candidate: Option<std::time::Instant>,
) -> Option<std::time::Instant> {
    match (current, candidate) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

/// Gracefully close a connection with a bounded post-shutdown drain.
///
/// Hyper's graceful shutdown still waits for the in-flight response to
/// finish; a client that stops reading applies TCP backpressure forever.
/// The bounded drain releases the connection's admission permit promptly
/// instead of letting stalled clients pin pool slots.
#[cfg(feature = "http2")]
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
#[cfg(feature = "http2")]
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

/// H2 connection driver: polls one Hyper H2 connection while enforcing
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
///
/// H2-only (Plan 249): H1 execution lives in `eggserve-server`.
#[cfg(feature = "http2")]
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
    let total_deadline = (!config.connection_total_timeout.is_zero())
        .then(|| activity.start.checked_add(config.connection_total_timeout))
        .flatten();
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
            // Outer bound for tunnels (Plan 199 Track F): abort remaining.
            let _ = activity
                .drain_tunnels(Some(tokio::time::Instant::now()))
                .await;
            return ConnectionOutcome::TotalTimeout;
        }
        // Deferred-body timeout fired by the per-request watchdog: the body
        // was already marked Failed and its lifecycle cancelled with
        // ConnectionTimeout. Close the transport so pending body/response
        // polls wake via transport failure.
        if activity.take_body_timeout() {
            graceful_close(conn.as_mut(), config, conn_id, &ops).await;
            requests.cancel_all(RequestCancellationReason::ConnectionTimeout, conn_id, &ops);
            let _ = activity
                .drain_tunnels(Some(tokio::time::Instant::now()))
                .await;
            return ConnectionOutcome::ClientError;
        }
        if multiplexed && activity.take_drain_request() {
            graceful_close(conn.as_mut(), config, conn_id, &ops).await;
            requests.cancel_all(RequestCancellationReason::ServerShutdown, conn_id, &ops);
            let _ = activity
                .drain_tunnels(Some(tokio::time::Instant::now()))
                .await;
            return ConnectionOutcome::Shutdown;
        }
        let (in_flight, outstanding, _completed, deferred, state) = activity.snapshot();
        // Tunnels keep the connection busy (Plan 199): H1 owns the connection,
        // H2/H3 stream tunnels prevent idle close while active.
        let tunnels_active = activity.tunnel_count().await > 0;
        let idle = in_flight == 0 && outstanding == 0 && deferred == 0 && !tunnels_active;
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
            let _ = activity
                .drain_tunnels(Some(tokio::time::Instant::now()))
                .await;
            return ConnectionOutcome::IdleTimeout;
        }
        let write_stalled = if multiplexed {
            activity.h2_response_producer_stalled(now, config.response_write_timeout)
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
            let _ = activity
                .drain_tunnels(Some(tokio::time::Instant::now()))
                .await;
            return ConnectionOutcome::WriteTimeout;
        }
        let mut wake = total_deadline;
        if idle {
            wake = min_deadline(
                wake,
                state
                    .last_activity
                    .checked_add(config.keep_alive_idle_timeout),
            );
        }
        if multiplexed {
            if let Some(deadline) =
                activity.h2_response_producer_deadline(config.response_write_timeout)
            {
                wake = min_deadline(wake, Some(deadline));
            }
        } else if outstanding > 0 {
            wake = min_deadline(
                wake,
                state.last_write.checked_add(config.response_write_timeout),
            );
        }
        let sleep = async move {
            match wake {
                Some(deadline) => {
                    tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await
                }
                None => std::future::pending::<()>().await,
            }
        };
        tokio::pin!(sleep);
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
                // Tunnels keep the task alive (Plan 199 Track F): wait within
                // the hard total lifetime, then abort remainders. Ordinary
                // connections have an empty set (immediate). Shutdown during
                // drain cancels lifecycles and uses the post-shutdown budget.
                if activity.tunnel_count().await > 0 {
                    tokio::select! {
                        drained = activity.drain_tunnels(total_deadline.map(tokio::time::Instant::from_std)) => {
                            if !drained {
                                requests.cancel_all(
                                    RequestCancellationReason::ConnectionTimeout,
                                    conn_id,
                                    &ops,
                                );
                                return ConnectionOutcome::TotalTimeout;
                            }
                        }
                        _ = &mut shutdown => {
                            requests.cancel_all(
                                RequestCancellationReason::ServerShutdown,
                                conn_id,
                                &ops,
                            );
                            let deadline = tokio::time::Instant::now()
                                + post_shutdown_drain_budget(config);
                            let _ = activity.drain_tunnels(Some(deadline)).await;
                            return ConnectionOutcome::Shutdown;
                        }
                    }
                }
                return outcome;
            }
            _ = &mut shutdown => {
                requests.cancel_all(RequestCancellationReason::ServerShutdown, conn_id, &ops);
                graceful_close(conn.as_mut(), config, conn_id, &ops).await;
                // Graceful shutdown waits for tunnels within the drain budget,
                // then aborts remainders (no detached task survives `wait()`).
                let deadline =
                    tokio::time::Instant::now() + post_shutdown_drain_budget(config);
                let _ = activity.drain_tunnels(Some(deadline)).await;
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

/// Drive one H2 connection with a caller-owned shutdown token (Plan 249).
///
/// H2-specific execution: the caller supplies an already-classified replayable
/// byte stream plus the canonical Hyper service. H1 never enters this helper;
/// `Auto` classification in the facade delegates H1 to `eggserve-server`
/// before any Hyper service is constructed.
#[cfg(feature = "http2")]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn serve_h2_with_token<I, S>(
    io: I,
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
    let ops = activity.ops().clone();
    record_protocol(WireProtocol::Http2, conn_id, &ops);
    let io = TokioIo::new(ProgressIo::new(io, activity.clone()));
    let shutdown = async move {
        shutdown.cancelled().await;
    };
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

#[cfg(all(test, feature = "http2"))]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn test_activity() -> Arc<ConnectionActivity> {
        Arc::new(ConnectionActivity::new(
            crate::ops::OpsContext::global().clone(),
        ))
    }

    /// Divergent first bytes resolve to H1 with all bytes preserved
    /// (Plan 249 Track D: H2 preface is never handed to the H1 parser).
    #[tokio::test]
    async fn cleartext_get_resolves_h1_with_replay() {
        use tokio::io::AsyncReadExt;
        let raw = b"GET / HTTP/1.1\r\nHost: h\r\n\r\n";
        let io = tokio::io::BufReader::new(&raw[..]);
        let config = RuntimeConfig::default();
        let shutdown = ConnectionShutdown::new();
        let activity = test_activity();
        let (mut prefixed, protocol) = classify_cleartext(io, &config, &shutdown, &activity, 1)
            .await
            .expect("plain H1 must classify");
        assert_eq!(protocol, WireProtocol::Http1);
        let mut replayed = Vec::new();
        prefixed.read_to_end(&mut replayed).await.unwrap();
        assert_eq!(replayed, raw);
    }

    /// The complete H2 preface resolves to H2 with all bytes preserved
    /// (Plan 249 Track D: Auto still selects H2).
    #[tokio::test]
    async fn complete_preface_resolves_h2_with_replay() {
        use tokio::io::AsyncReadExt;
        let config = crate::server::config::RuntimeConfig::builder()
            .http2(crate::server::config::Http2Config::default())
            .build()
            .unwrap();
        let io = tokio::io::BufReader::new(H2_PREFACE);
        let shutdown = ConnectionShutdown::new();
        let activity = test_activity();
        let (mut prefixed, protocol) = classify_cleartext(io, &config, &shutdown, &activity, 1)
            .await
            .expect("H2 preface must classify");
        assert_eq!(protocol, WireProtocol::Http2);
        let mut replayed = Vec::new();
        prefixed.read_to_end(&mut replayed).await.unwrap();
        assert_eq!(replayed, H2_PREFACE);
    }

    /// An empty stream at EOF resolves to H1 (existing edge preserved).
    #[tokio::test]
    async fn empty_stream_resolves_h1() {
        let io = tokio::io::BufReader::new(&[][..]);
        let config = RuntimeConfig::default();
        let shutdown = ConnectionShutdown::new();
        let activity = test_activity();
        let (_, protocol) = classify_cleartext(io, &config, &shutdown, &activity, 1)
            .await
            .expect("EOF must classify");
        assert_eq!(protocol, WireProtocol::Http1);
    }
}
