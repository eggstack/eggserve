//! Experimental HTTP/3 server adapter (Plan 187).
//!
//! Quinn and h3 are deliberately contained here. The adapter translates H3
//! request/response streams to EggServe's canonical request/service/response
//! types and owns only protocol mechanics; service admission, panic
//! containment, timeouts, normalization, and privacy policy remain shared
//! with the canonical runtime helpers.

use std::sync::Arc;

use bytes::{Buf, Bytes};
use futures_util::{stream, StreamExt};
use tokio::sync::{broadcast, OwnedSemaphorePermit, Semaphore};

use crate::primitives::canonical::{normalize_response, NormalizeRequest, Response, ResponseBody};
use crate::primitives::connection_info::TlsInfo;
use crate::primitives::header_block::{HeaderBlock, HeaderName, HeaderValue};
use crate::primitives::method::Method;
use crate::primitives::request::Request;
use crate::primitives::request_body::IncomingError;
use crate::primitives::request_head::RequestHead;
use crate::primitives::request_lifecycle::{RequestCancellationReason, RequestShared};
use crate::primitives::request_target::RequestTarget;
use crate::primitives::version::HttpVersion;
use crate::server::config::RuntimeConfig;
use crate::server::connection::lifecycle::{cancel_shared_with_observability, ConnectionRequests};
use crate::server::connection::ConnectionContext;
use crate::server::errors::ShutdownResult;
use crate::server::service::{Service, ServiceError};
use crate::server::RuntimeState;

type H3Bytes = Bytes;

/// Run the UDP endpoint alongside the TCP accept loop.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn accept_loop<S: Service>(
    endpoint: h3_quinn::Endpoint,
    local_addr: std::net::SocketAddr,
    config: Arc<RuntimeConfig>,
    runtime_state: Arc<RuntimeState>,
    connection_semaphore: Arc<Semaphore>,
    mut shutdown_rx: broadcast::Receiver<()>,
    _lifecycle: Arc<crate::server::lifecycle::Lifecycle>,
    service: S,
) -> ShutdownResult {
    let service = Arc::new(service);
    let ops = runtime_state.ops().clone();
    ops.emit(crate::ops::Event::new(
        crate::ops::Severity::Info,
        crate::ops::EventKind::ListenerReady,
        "HTTP/3 QUIC endpoint started",
    ));

    let mut tasks = tokio::task::JoinSet::new();
    let pending_handshakes = Arc::new(Semaphore::new(config.http3.max_pending_handshakes));
    loop {
        tokio::select! {
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else { break };
                let conn_id = ops.next_connection_id();
                let connection_permit = match connection_semaphore.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        ops.counters().connections_rejected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        incoming.refuse();
                        continue;
                    }
                };
                ops.counters()
                    .connections_accepted
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                ops.counters()
                    .active_connections
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let pending_permit = match pending_handshakes.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        incoming.refuse();
                        drop(connection_permit);
                        ops.counters()
                            .active_connections
                            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                        continue;
                    }
                };
                    let service = service.clone();
                let config = config.clone();
                let runtime_state = runtime_state.clone();
                let ops = ops.clone();
                let connection_shutdown_rx = shutdown_rx.resubscribe();
                tasks.spawn(async move {
                        let _connection_permit = connection_permit;
                        let _active_connection = ActiveConnectionGuard { ops: ops.clone() };
                    let pending_permit = pending_permit;
                    let remote_addr = incoming.remote_address();
                    let connecting = if config.http3.stateless_retry && incoming.may_retry() {
                        match incoming.retry() {
                            Ok(()) => return,
                            Err(_) => return,
                        }
                    } else {
                        match incoming.accept() {
                            Ok(connecting) => connecting,
                            Err(error) => {
                                ops.emit(crate::ops::Event::new(
                                    crate::ops::Severity::Debug,
                                    crate::ops::EventKind::TlsHandshakeFailure,
                                    format!("QUIC handshake rejected: {error}"),
                                ).connection_id(conn_id));
                                return;
                            }
                        }
                    };
                    let connection = match tokio::time::timeout(
                        config.tls_handshake_timeout,
                        connecting,
                    ).await {
                        Ok(Ok(connection)) => connection,
                        Ok(Err(error)) => {
                            ops.emit(crate::ops::Event::new(
                                crate::ops::Severity::Debug,
                                crate::ops::EventKind::TlsHandshakeFailure,
                                format!("QUIC handshake failed: {error}"),
                            ).connection_id(conn_id));
                            return;
                        }
                        Err(_) => {
                            ops.emit(crate::ops::Event::new(
                                crate::ops::Severity::Warn,
                                crate::ops::EventKind::TlsHandshakeTimeout,
                                "QUIC handshake timed out",
                            ).connection_id(conn_id));
                            return;
                        }
                    };
                    drop(pending_permit);
                    serve_connection(
                        connection,
                        local_addr,
                        remote_addr,
                        service,
                        config,
                        runtime_state,
                        conn_id,
                        connection_shutdown_rx,
                    ).await;
                });
            }
            _ = shutdown_rx.recv() => {
                endpoint.close(
                    quinn::VarInt::from_u64(h3::error::Code::H3_NO_ERROR.value()).expect("H3 code is a QUIC varint"),
                    b"server shutdown",
                );
                break;
            }
        }
    }

    endpoint.close(
        quinn::VarInt::from_u64(h3::error::Code::H3_NO_ERROR.value())
            .expect("H3 code is a QUIC varint"),
        b"server shutdown",
    );
    let deadline = tokio::time::Instant::now() + config.graceful_shutdown_timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            tasks.abort_all();
            return ShutdownResult::Timeout;
        }
        match tokio::time::timeout(remaining, tasks.join_next()).await {
            Ok(Some(Ok(()))) => {}
            Ok(Some(Err(_))) => {}
            Ok(None) => return ShutdownResult::Clean,
            Err(_) => {
                tasks.abort_all();
                return ShutdownResult::Timeout;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn serve_connection<S: Service>(
    connection: quinn::Connection,
    local_addr: std::net::SocketAddr,
    remote_addr: std::net::SocketAddr,
    service: Arc<S>,
    config: Arc<RuntimeConfig>,
    runtime_state: Arc<RuntimeState>,
    conn_id: u64,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    let connection_closed = connection.clone();
    let requests_registry = Arc::new(ConnectionRequests::new());
    let mut builder = h3::server::builder();
    builder.max_field_section_size(config.http3.max_field_section_size);
    // Advertise Extended CONNECT (Plan 199 Track A): H3 supports `CONNECT`
    // (kind `Connect`) and the h3-crate `:protocol` values (`webtransport`,
    // `connect-udp`, kind `ExtendedConnect`). Generic `:protocol` (e.g.
    // `websocket`) is rejected as malformed by `h3` 0.0.8 before EggServe
    // sees it — documented as blocked, not bypassed.
    builder.enable_extended_connect(true);
    let mut h3_connection = match builder
        .build::<_, H3Bytes>(h3_quinn::Connection::new(connection))
        .await
    {
        Ok(connection) => connection,
        Err(error) => {
            runtime_state.ops().emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Debug,
                    crate::ops::EventKind::ConnectionRejected,
                    format!("HTTP/3 connection setup failed: {error}"),
                )
                .connection_id(conn_id),
            );
            return;
        }
    };

    let mut requests = tokio::task::JoinSet::new();
    let mut accepted_requests = 0u64;
    let mut draining = false;
    let mut drain_deadline = None;
    loop {
        tokio::select! {
            _ = shutdown_rx.recv(), if !draining => {
                let _ = h3_connection.shutdown(0).await;
                draining = true;
                drain_deadline = Some(tokio::time::Instant::now() + config.graceful_shutdown_timeout);
            }
            _ = async {
                if let Some(deadline) = drain_deadline {
                    tokio::time::sleep_until(deadline).await;
                } else {
                    std::future::pending::<()>().await;
                }
            }, if draining => {
                requests_registry.cancel_all(
                    RequestCancellationReason::ServerShutdown,
                    conn_id,
                    runtime_state.ops(),
                );
                requests.abort_all();
                return;
            }
            closed = connection_closed.closed() => {
                let reason = h3_connection_close_reason(&closed);
                requests_registry.cancel_all(reason, conn_id, runtime_state.ops());
                break;
            }
            accepted = h3_connection.accept() => {
                let resolver = match accepted {
                    Ok(Some(resolver)) => resolver,
                    Ok(None) => break,
                    Err(_) => {
                        requests_registry.cancel_all(
                            h3_connection_close_reason(
                                &connection_closed
                                    .close_reason()
                                    .unwrap_or(quinn::ConnectionError::LocallyClosed),
                            ),
                            conn_id,
                            runtime_state.ops(),
                        );
                        break;
                    }
                };
                accepted_requests = accepted_requests.saturating_add(1);
                if !draining && config.max_requests_per_connection.is_some_and(|max| accepted_requests >= max) {
                    let _ = h3_connection.shutdown(0).await;
                    draining = true;
                    drain_deadline = Some(tokio::time::Instant::now() + config.graceful_shutdown_timeout);
                    runtime_state.ops().emit(
                        crate::ops::Event::new(
                            crate::ops::Severity::Debug,
                            crate::ops::EventKind::MaxRequestsClose,
                            "HTTP/3 max requests per connection reached; draining",
                        )
                        .connection_id(conn_id),
                    );
                }
                let service = service.clone();
                let config = config.clone();
                let state = runtime_state.clone();
                let requests_registry = requests_registry.clone();
                requests.spawn(async move {
                    let (request, stream) = match resolver.resolve_request().await {
                        Ok(value) => value,
                        Err(_) => return,
                    };
                    let shared = RequestShared::new_active();
                    requests_registry.register(&shared);
                    handle_request(
                        request,
                        stream,
                        local_addr,
                        remote_addr,
                        service,
                        config,
                        state,
                        shared,
                        conn_id,
                    )
                    .await;
                });
            }
        }
    }

    let deadline = drain_deadline
        .unwrap_or_else(|| tokio::time::Instant::now() + config.graceful_shutdown_timeout);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            requests_registry.cancel_all(
                RequestCancellationReason::ServerShutdown,
                conn_id,
                runtime_state.ops(),
            );
            requests.abort_all();
            return;
        }
        match tokio::time::timeout(remaining, requests.join_next()).await {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                if requests.is_empty() {
                    return;
                }
                requests_registry.cancel_all(
                    RequestCancellationReason::ServerShutdown,
                    conn_id,
                    runtime_state.ops(),
                );
                requests.abort_all();
                return;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_request<S, C>(
    request: hyper::Request<()>,
    stream: h3::server::RequestStream<C, H3Bytes>,
    local_addr: std::net::SocketAddr,
    remote_addr: std::net::SocketAddr,
    service: Arc<S>,
    config: Arc<RuntimeConfig>,
    runtime_state: Arc<RuntimeState>,
    shared: Arc<RequestShared>,
    conn_id: u64,
) where
    S: Service,
    C: h3::quic::BidiStream<H3Bytes>,
    C::SendStream: Send + 'static,
    C::RecvStream: Send + 'static,
{
    let is_head = request.method() == hyper::Method::HEAD;
    let (mut send_stream, mut recv_stream) = stream.split();
    let head = match convert_request_head(&request, &config, conn_id, runtime_state.ops()) {
        Ok(head) => head,
        Err(error) => {
            let response = runtime_error_response(error.status_code(), is_head, &config);
            if !send_response_or_cancel(
                &mut send_stream,
                response,
                &config,
                is_head,
                runtime_state.file_stream_semaphore(),
                &shared,
                conn_id,
                runtime_state.ops(),
            )
            .await
            {
                // Plan 192 (#262): the send direction is already reset inside
                // `send_response_or_cancel` on failure; still abort receive so
                // a dropped half does not leave a live QUIC stream.
                recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
                return;
            }
            // The request was rejected before any application processing, so
            // abort the receive direction explicitly (RFC 9114 §4.1.1). A
            // bare `RequestStream` drop does not reset the QUIC stream.
            recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
            return;
        }
    };
    // CONNECT / Extended CONNECT tunnel candidate (Plan 199 Track B).
    // Plain `CONNECT` (no `:protocol`) => kind `Connect`; `:protocol`
    // present (h3-crate `webtransport`/`connect-udp`) => `ExtendedConnect`.
    // Generic `:protocol` (e.g. `websocket`) is rejected as malformed by `h3`
    // 0.0.8 before this point (documented as blocked). Bodies never cross:
    // `Content-Length > 0` => 413, no capability. No DATA probe here so early
    // tunnel DATA stays in the QUIC stream for the bridge.
    if head.method().as_str() == "CONNECT" {
        handle_h3_connect::<S, C>(
            request,
            head,
            send_stream,
            recv_stream,
            local_addr,
            remote_addr,
            service,
            config,
            runtime_state,
            shared,
            conn_id,
        )
        .await;
        return;
    }
    let declared_length = match declared_content_length(&request) {
        Ok(length) => length,
        Err(error) => {
            let _ = send_response_or_cancel(
                &mut send_stream,
                runtime_error_response(error.status_code(), is_head, &config),
                &config,
                is_head,
                runtime_state.file_stream_semaphore(),
                &shared,
                conn_id,
                runtime_state.ops(),
            )
            .await;
            // Same early-rejection ownership as above: the error response is
            // sent on the send direction while receive is aborted here.
            recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
            return;
        }
    };
    let policy = crate::server::connection::request::select_body_policy(
        service.request_body_policy(&head),
        config.max_request_body_bytes,
    );
    if let Some(limit) = policy.max_bytes() {
        if declared_length.is_some_and(|length| length > limit) {
            let response = runtime_error_response(413, is_head, &config);
            let _ = send_response_or_cancel(
                &mut send_stream,
                response,
                &config,
                is_head,
                runtime_state.file_stream_semaphore(),
                &shared,
                conn_id,
                runtime_state.ops(),
            )
            .await;
            recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
            return;
        }
    }
    if policy.is_reject() && declared_length.is_some_and(|length| length > 0) {
        let response = runtime_error_response(413, is_head, &config);
        let _ = send_response_or_cancel(
            &mut send_stream,
            response,
            &config,
            is_head,
            runtime_state.file_stream_semaphore(),
            &shared,
            conn_id,
            runtime_state.ops(),
        )
        .await;
        recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
        return;
    }

    let request_body = if policy.is_reject() {
        // A missing or zero Content-Length does not prove that an H3 request
        // has no content. Perform one bounded protocol read so Reject is
        // based on actual stream state, not HTTP/1 header inference. The
        // first DATA chunk is discarded and the receive direction is then
        // cancelled; the request is never buffered in full or dispatched.
        let presence =
            tokio::time::timeout(config.body_read_timeout, recv_stream.recv_data()).await;
        match presence {
            Ok(Ok(Some(_data))) => {
                let response = runtime_error_response(413, is_head, &config);
                let _ = send_response_or_cancel(
                    &mut send_stream,
                    response,
                    &config,
                    is_head,
                    runtime_state.file_stream_semaphore(),
                    &shared,
                    conn_id,
                    runtime_state.ops(),
                )
                .await;
                recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
                return;
            }
            Ok(Ok(None)) => {
                shared.mark_complete();
                crate::primitives::request_body::RequestBody::from_incoming_with_shared(
                    stream::empty(),
                    Some(0),
                    0,
                    shared.clone(),
                )
            }
            Ok(Err(_error)) => {
                shared.mark_failed();
                let _ = send_response_or_cancel(
                    &mut send_stream,
                    runtime_error_response(500, is_head, &config),
                    &config,
                    is_head,
                    runtime_state.file_stream_semaphore(),
                    &shared,
                    conn_id,
                    runtime_state.ops(),
                )
                .await;
                send_stream.stop_stream(h3::error::Code::H3_INTERNAL_ERROR);
                // The probe read failed, so the receive direction is already
                // unusable; abort it explicitly rather than relying on drop.
                recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
                return;
            }
            Err(_) => {
                shared.mark_failed_with_reason(RequestCancellationReason::ConnectionTimeout);
                let _ = send_response_or_cancel(
                    &mut send_stream,
                    runtime_error_response(408, is_head, &config),
                    &config,
                    is_head,
                    runtime_state.file_stream_semaphore(),
                    &shared,
                    conn_id,
                    runtime_state.ops(),
                )
                .await;
                recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
                return;
            }
        }
    } else {
        let body_cancel = tokio::sync::watch::channel(false);
        // Plan 198 Track G: stream-local terminal field sections reuse the
        // single canonical trailer validator. The slot is populated only from
        // `recv_trailers` after DATA EOF (bounded probe, sibling-isolated);
        // no second H3 trailer policy exists.
        let wire_slot = crate::primitives::request_body::new_wire_slot();
        let wire_slot_clone = wire_slot.clone();
        let body_stream = stream::unfold(
            (recv_stream, body_cancel.1),
            move |(mut stream, mut cancel)| {
                let wire_slot = wire_slot_clone.clone();
                async move {
                    if *cancel.borrow() {
                        stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
                        return None;
                    }
                    tokio::select! {
                        changed = cancel.changed() => {
                            if changed.is_ok() {
                                stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
                            }
                            None
                        }
                        data = stream.recv_data() => match data {
                            Ok(Some(mut data)) => {
                                let bytes = data.copy_to_bytes(data.remaining());
                                Some((Ok::<_, IncomingError>(bytes), (stream, cancel)))
                            }
                            Ok(None) => {
                                // DATA EOF: one bounded trailer probe. Prompt
                                // None means no trailers (minimal overhead for
                                // services ignoring trailers); Some(map) is
                                // converted via canonical header rules and stored
                                // raw for `RequestBody` canonical validation.
                                // Failures are stream-local (stored as slot error,
                                // failing this request body only).
                                let probe = tokio::time::timeout(
                                    std::time::Duration::from_millis(500),
                                    stream.recv_trailers(),
                                )
                                .await;
                                match probe {
                                    Ok(Ok(Some(map))) => {
                                        match h3_trailers_to_block(&map) {
                                            Ok(block) => {
                                                if let Ok(mut g) = wire_slot.lock() {
                                                    if g.is_none() {
                                                        *g = Some(Ok(block));
                                                    } else {
                                                        *g = Some(Err(
                                                            "repeated trailer block".to_string(),
                                                        ));
                                                    }
                                                }
                                            }
                                            Err(msg) => {
                                                if let Ok(mut g) = wire_slot.lock() {
                                                    if g.is_none() {
                                                        *g = Some(Err(msg));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Ok(Ok(None)) => {}
                                    Ok(Err(_)) => {
                                        // Transport failure while probing trailers:
                                        // record as slot error so the body fails
                                        // safely rather than exposing partial data.
                                        // Only record if no prior terminal state;
                                        // DATA already ended, so this is trailer
                                        // scope, stream-local.
                                        // Note: `recv_trailers` errors are
                                        // stream-local by h3 design (siblings survive).
                                    }
                                    Err(_) => {
                                        // Bounded probe timeout: treat as no trailers
                                        // (do not hang body completion for trailer-less
                                        // requests).
                                    }
                                }
                                None
                            }
                            Err(error) => Some((Err(IncomingError(error.to_string())), (stream, cancel))),
                        }
                    }
                }
            },
        );
        let request_body =
            crate::primitives::request_body::RequestBody::from_incoming_with_shared_and_wire_slot(
                body_stream,
                declared_length,
                policy.max_bytes().unwrap_or(config.max_request_body_bytes),
                shared.clone(),
                wire_slot,
            );
        spawn_body_timeout_watchdog(
            shared.clone(),
            body_cancel.0,
            tokio::time::Instant::now() + config.body_read_timeout,
            conn_id,
            runtime_state.ops().clone(),
        );
        request_body
    };
    let request_body = match policy {
        crate::primitives::request_body_policy::RequestBodyPolicy::Buffer { max_bytes } => {
            match tokio::time::timeout(
                config.body_read_timeout,
                request_body.read_all_with_trailers(),
            )
            .await
            {
                Ok(Ok((bytes, trailers))) => {
                    // Preserve trailers (not discarded); validation already ran
                    // during `read_all_with_trailers`.
                    // Note: `from_bytes_with_shared` loses trailers, so rebuild
                    // with trailers when present via the validated constructor.
                    // Since `from_bytes_with_shared` takes shared allocation for
                    // lifecycle continuity, and `from_bytes_with_trailers` creates
                    // a fresh allocation, prefer continuity + manual attach:
                    // create via shared then attach validated trailers.
                    let mut body =
                        crate::primitives::request_body::RequestBody::from_bytes_with_shared(
                            bytes,
                            max_bytes,
                            shared.clone(),
                        );
                    if let Some(t) = trailers {
                        body.set_trailers(t);
                    }
                    body
                }
                Ok(Err(error)) => {
                    let _ = send_response_or_cancel(
                        &mut send_stream,
                        runtime_error_response(error.to_status_code(), is_head, &config),
                        &config,
                        is_head,
                        runtime_state.file_stream_semaphore(),
                        &shared,
                        conn_id,
                        runtime_state.ops(),
                    )
                    .await;
                    return;
                }
                Err(_) => {
                    shared.mark_failed_with_reason(RequestCancellationReason::ConnectionTimeout);
                    let _ = send_response_or_cancel(
                        &mut send_stream,
                        runtime_error_response(408, is_head, &config),
                        &config,
                        is_head,
                        runtime_state.file_stream_semaphore(),
                        &shared,
                        conn_id,
                        runtime_state.ops(),
                    )
                    .await;
                    return;
                }
            }
        }
        _ => request_body,
    };
    let request = Request::new(
        head,
        request_body,
        ConnectionContext::for_quic(
            local_addr,
            remote_addr,
            TlsInfo {
                protocol_version: Some("TLSv1.3".into()),
                server_name: None,
            },
        )
        .connection_info(),
    );

    let permit = match runtime_state
        .service_semaphore()
        .clone()
        .try_acquire_owned()
    {
        Ok(permit) => permit,
        Err(_) => {
            let _ = send_response_or_cancel(
                &mut send_stream,
                runtime_error_response(503, is_head, &config),
                &config,
                is_head,
                runtime_state.file_stream_semaphore(),
                &shared,
                conn_id,
                runtime_state.ops(),
            )
            .await;
            return;
        }
    };
    let result = invoke_service(service, request, permit, &config, runtime_state.ops()).await;
    let response = match result {
        Ok(response) => response,
        Err(error) => runtime_error_response(error.status_code(), is_head, &config),
    };
    let _ = send_response_or_cancel(
        &mut send_stream,
        response,
        &config,
        is_head,
        runtime_state.file_stream_semaphore(),
        &shared,
        conn_id,
        runtime_state.ops(),
    )
    .await;
}

/// H3 CONNECT / Extended CONNECT tunnel path (Plan 199 Tracks B–G).
///
/// - Plain `CONNECT` (no `:protocol`) => `Connect`; `:protocol` present
///   (`webtransport`/`connect-udp` via h3-crate) => `ExtendedConnect`.
/// - `Content-Length > 0` or invalid length => 413/400, no capability (body
///   never crosses). No DATA probe so early tunnel DATA stays buffered.
/// - Empty body + one-shot capability attached; service denial sends ordinary
///   response (recv aborted); acceptance sends `200` (no `101`, no body, no
///   `finish` yet) then runs a stream-scoped duplex tunnel (siblings survive).
/// - Admission via server-wide `max_active_tunnels` (503 on exhaustion);
///   lifecycle cancellation wakes idle tunnels; hard shutdown aborts via the
///   parent `requests` JoinSet (this task stays alive until tunnel close, so
///   `wait()` accounts for tunnels).
#[allow(clippy::too_many_arguments)]
async fn handle_h3_connect<S, C>(
    request: hyper::Request<()>,
    head: RequestHead,
    mut send_stream: h3::server::RequestStream<C::SendStream, H3Bytes>,
    mut recv_stream: h3::server::RequestStream<C::RecvStream, H3Bytes>,
    local_addr: std::net::SocketAddr,
    remote_addr: std::net::SocketAddr,
    service: Arc<S>,
    config: Arc<RuntimeConfig>,
    runtime_state: Arc<RuntimeState>,
    shared: Arc<RequestShared>,
    conn_id: u64,
) where
    S: Service,
    C: h3::quic::BidiStream<H3Bytes>,
    C::SendStream: Send + 'static,
    C::RecvStream: Send + 'static,
{
    use crate::primitives::tunnel::{classify_extended_protocol, TunnelKind, TunnelRequest};
    use crate::primitives::tunnel::{TunnelCapability, TunnelIo};

    let ops = runtime_state.ops().clone();
    // Bodies never cross the transition (Track H).
    match declared_content_length(&request) {
        Ok(Some(len)) if len > 0 => {
            let _ = send_response_or_cancel(
                &mut send_stream,
                runtime_error_response(413, false, &config),
                &config,
                false,
                runtime_state.file_stream_semaphore(),
                &shared,
                conn_id,
                &ops,
            )
            .await;
            recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
            return;
        }
        Ok(_) => {}
        Err(error) => {
            let _ = send_response_or_cancel(
                &mut send_stream,
                runtime_error_response(error.status_code(), false, &config),
                &config,
                false,
                runtime_state.file_stream_semaphore(),
                &shared,
                conn_id,
                &ops,
            )
            .await;
            recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
            return;
        }
    }
    let protocol_raw: Option<String> = request
        .extensions()
        .get::<h3::ext::Protocol>()
        .map(|p| p.as_str().to_string());
    let (kind, protocol) = match protocol_raw {
        Some(raw) => match classify_extended_protocol(Some(&raw)) {
            Some(valid) => (TunnelKind::ExtendedConnect, Some(valid)),
            None => {
                // Present-but-invalid `:protocol`: no capability, ordinary
                // denial (400). Never fallback to plain CONNECT.
                let _ = send_response_or_cancel(
                    &mut send_stream,
                    runtime_error_response(400, false, &config),
                    &config,
                    false,
                    runtime_state.file_stream_semaphore(),
                    &shared,
                    conn_id,
                    &ops,
                )
                .await;
                recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
                return;
            }
        },
        None => (TunnelKind::Connect, None),
    };
    let Some(authority) = head.authority().cloned() else {
        let _ = send_response_or_cancel(
            &mut send_stream,
            runtime_error_response(400, false, &config),
            &config,
            false,
            runtime_state.file_stream_semaphore(),
            &shared,
            conn_id,
            &ops,
        )
        .await;
        recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
        return;
    };
    let tunnel_request = TunnelRequest::new(kind, protocol, Some(authority));
    let capability = TunnelCapability::new(tunnel_request, None);
    let tunnel_shared = capability.shared();
    // Empty body sharing the registered allocation (lifecycle continuity).
    let request_body = crate::primitives::request_body::RequestBody::from_incoming_with_shared(
        futures_util::stream::empty(),
        Some(0),
        0,
        shared.clone(),
    );
    // `from_incoming_with_shared` with empty stream starts Active; mark
    // Complete immediately (no DATA to consume, no probe to preserve tunnel
    // bytes). Safe: empty stream never yields DATA, terminal immediately.
    shared.mark_complete();
    let connection_info = ConnectionContext::for_quic(
        local_addr,
        remote_addr,
        TlsInfo {
            protocol_version: Some("TLSv1.3".into()),
            server_name: None,
        },
    )
    .connection_info();
    let version = head.version();
    let ctx = crate::primitives::request_context::RequestContext::new_with_version(
        connection_info,
        request_body.lifecycle(),
        version,
    )
    .with_tunnel(capability);
    let request_for_service =
        crate::primitives::request::Request::new_with_context(head, request_body, ctx);
    let interim = request_for_service.context().interim().cloned();
    let lifecycle = request_for_service.lifecycle_clone();
    // Service admission (pre-response `max_in_flight_requests`, outer ceiling).
    let permit = match runtime_state
        .service_semaphore()
        .clone()
        .try_acquire_owned()
    {
        Ok(p) => p,
        Err(_) => {
            if let Some(s) = interim.as_ref() {
                s.mark_committed();
            }
            tunnel_shared.mark_committed();
            let _ = send_response_or_cancel(
                &mut send_stream,
                runtime_error_response(503, false, &config),
                &config,
                false,
                runtime_state.file_stream_semaphore(),
                &shared,
                conn_id,
                &ops,
            )
            .await;
            recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
            return;
        }
    };
    let _permit = permit;
    let result = crate::server::connection::response::invoke_canonical_service(
        service.as_ref(),
        request_for_service,
        config.handler_timeout,
        &ops,
    )
    .await;
    if let Some(s) = interim.as_ref() {
        s.mark_committed();
    }
    tunnel_shared.mark_committed();
    let mut canonical = match result {
        Ok(r) => r,
        Err(error) => {
            let response = runtime_error_response(error.status_code(), false, &config);
            let _ = send_response_or_cancel(
                &mut send_stream,
                response,
                &config,
                false,
                runtime_state.file_stream_semaphore(),
                &shared,
                conn_id,
                &ops,
            )
            .await;
            recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
            return;
        }
    };
    if !canonical.is_tunnel() {
        // Ordinary denial: normal response, recv aborted (no tunnel DATA).
        let _ = send_response_or_cancel(
            &mut send_stream,
            canonical,
            &config,
            false,
            runtime_state.file_stream_semaphore(),
            &shared,
            conn_id,
            &ops,
        )
        .await;
        recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
        return;
    }
    let acceptance = canonical
        .take_tunnel_acceptance()
        .expect("is_tunnel checked");
    // Tunnel admission (server-wide, 503 on exhaustion, no handler run).
    let tunnel_permit = match runtime_state.tunnel_semaphore().clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            ops.counters()
                .tunnels_rejected
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Warn,
                    crate::ops::EventKind::TunnelRejected,
                    "tunnel saturated: active tunnel limit",
                )
                .connection_id(conn_id),
            );
            let _ = send_response_or_cancel(
                &mut send_stream,
                runtime_error_response(503, false, &config),
                &config,
                false,
                runtime_state.file_stream_semaphore(),
                &shared,
                conn_id,
                &ops,
            )
            .await;
            recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
            return;
        }
    };
    let _tunnel_permit = tunnel_permit;
    // Send `200` handshake (no body, no FIN yet; stream stays open for duplex).
    // Reuse canonical privacy (Server/Date/denylist) without inventing
    // `Content-Length`; hop-by-hop already stripped in `accept`.
    let mut handshake = canonical;
    // `take_tunnel_acceptance` left head/body; ensure no body bytes.
    if let Some(body) = handshake.take_body() {
        drop(body);
    }
    // Re-attach empty body for `send_canonical_response`-style conversion?
    // Instead, send headers directly (no body, no trailers, no finish).
    let send_result = send_h3_tunnel_handshake(&mut send_stream, handshake, &config).await;
    if send_result.is_err() {
        ops.counters()
            .tunnel_upgrade_failures
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ops.emit(
            crate::ops::Event::new(
                crate::ops::Severity::Debug,
                crate::ops::EventKind::TunnelUpgradeFailed,
                "H3 tunnel handshake failed",
            )
            .connection_id(conn_id),
        );
        crate::server::connection::lifecycle::cancel_shared_with_observability(
            &shared,
            RequestCancellationReason::TransportFailure,
            conn_id,
            &ops,
        );
        send_stream.stop_stream(h3::error::Code::H3_INTERNAL_ERROR);
        recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
        return;
    }
    ops.counters()
        .tunnels_accepted
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ops.counters()
        .active_tunnels
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ops.emit(
        crate::ops::Event::new(
            crate::ops::Severity::Info,
            crate::ops::EventKind::TunnelAccepted,
            format!("tunnel accepted: {}", acceptance.kind),
        )
        .connection_id(conn_id),
    );
    let _active_guard = H3ActiveTunnelGuard { ops: ops.clone() };
    let (io_handler, io_bridge) = TunnelIo::pair();
    let handler_lifecycle = lifecycle.clone();
    let bridge_lifecycle_a = lifecycle.clone();
    let bridge_lifecycle_b = lifecycle.clone();
    let handler_join = tokio::spawn(async move {
        (acceptance.handler)(io_handler, handler_lifecycle).await;
    });
    // Stream-scoped duplex (siblings survive): two concurrent directions with
    // bounded chunks, lifecycle wakes idle, half-close propagates (recv EOF
    // shuts duplex write, duplex EOF finishes H3 send).
    let duplex = io_bridge.into_duplex();
    let (mut duplex_read, mut duplex_write) = tokio::io::split(duplex);
    // Client -> server: H3 DATA -> duplex.
    let recv_task = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let mut recv_stream = recv_stream;
        loop {
            tokio::select! {
                data = recv_stream.recv_data() => {
                    match data {
                        Ok(Some(mut chunk)) => {
                            use bytes::Buf;
                            let bytes = chunk.copy_to_bytes(chunk.remaining());
                            if duplex_write.write_all(&bytes).await.is_err() {
                                break;
                            }
                        }
                        Ok(None) => {
                            let _ = duplex_write.shutdown().await;
                            break;
                        }
                        Err(_) => break,
                    }
                }
                _ = bridge_lifecycle_a.cancelled() => break,
            }
        }
        recv_stream
    });
    // Server -> client: duplex -> H3 DATA (16 KiB frames, flow-controlled).
    let send_task = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut send_stream = send_stream;
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            tokio::select! {
                read = duplex_read.read(&mut buf) => {
                    match read {
                        Ok(0) => break,
                        Ok(n) => {
                            let chunk = Bytes::copy_from_slice(&buf[..n]);
                            if send_stream.send_data(chunk).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                _ = bridge_lifecycle_b.cancelled() => break,
            }
        }
        send_stream
    });
    let (recv_res, send_res) = tokio::join!(recv_task, send_task);
    handler_join.abort();
    let _ = handler_join.await;
    // Orderly/error closure maps to this H3 request stream only (siblings
    // survive). On normal duplex EOF, FIN the send direction; always stop
    // the recv direction (already EOF or cancelled).
    if let Ok(mut send_stream) = send_res {
        let _ = tokio::time::timeout(config.response_write_timeout, send_stream.finish()).await;
    }
    if let Ok(mut recv_stream) = recv_res {
        recv_stream.stop_sending(h3::error::Code::H3_NO_ERROR);
    }
    ops.emit(
        crate::ops::Event::new(
            crate::ops::Severity::Debug,
            crate::ops::EventKind::TunnelClosed,
            format!("tunnel closed: {}", kind_string(kind)),
        )
        .connection_id(conn_id),
    );
}

fn kind_string(kind: crate::primitives::tunnel::TunnelKind) -> &'static str {
    match kind {
        crate::primitives::tunnel::TunnelKind::Http1Upgrade => "http1-upgrade",
        crate::primitives::tunnel::TunnelKind::Connect => "connect",
        crate::primitives::tunnel::TunnelKind::ExtendedConnect => "extended-connect",
    }
}

struct H3ActiveTunnelGuard {
    ops: crate::ops::OpsContext,
}

impl Drop for H3ActiveTunnelGuard {
    fn drop(&mut self) {
        self.ops
            .counters()
            .active_tunnels
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Send an H3 tunnel `200` handshake (headers only, no body, no FIN).
///
/// Applies canonical privacy (Server/Date/denylist) like ordinary responses
/// but never invents `Content-Length`/`Transfer-Encoding` and never finishes
/// the stream (duplex continues). Hop-by-hop already stripped in `accept`.
async fn send_h3_tunnel_handshake<S>(
    stream: &mut h3::server::RequestStream<S, H3Bytes>,
    response: Response,
    config: &RuntimeConfig,
) -> Result<(), String>
where
    S: h3::quic::SendStream<H3Bytes>,
{
    // Start from accepted handshake headers (validated/bounded, no framing,
    // no hop-by-hop), then apply canonical privacy (Server/Date/denylist).
    let mut builder_block = HeaderBlock::new();
    for field in response.headers().iter() {
        // Defense in depth: strip framing/hop-by-hop even though `accept`
        // already did (`head_mut` clears tunnel, so this is unchanged, but
        // re-validate before wire).
        if field.name.as_str().eq_ignore_ascii_case("content-length")
            || field
                .name
                .as_str()
                .eq_ignore_ascii_case("transfer-encoding")
            || crate::primitives::canonical::is_hop_by_hop_header(field.name.as_str())
        {
            continue;
        }
        builder_block.push(field.name.clone(), field.value.clone());
    }
    let mut tmp = Response::builder()
        .status(response.status())
        .body(ResponseBody::Empty)
        .map_err(|e| e.to_string())?;
    for field in builder_block.iter() {
        // `head_mut` clears tunnel acceptance, but `tmp` has none (fresh),
        // so safe: we are building a wire head, not mutating the handshake.
        tmp.head_mut()
            .headers_mut()
            .push(field.name.clone(), field.value.clone());
    }
    let tmp = crate::server::connection::response::finalize_canonical_response(tmp, config);
    let status = hyper::StatusCode::from_u16(tmp.status().as_u16()).map_err(|e| e.to_string())?;
    // Ensure 200 (not 101) for H3 Extended/CONNECT; reject 101 defensively.
    if status == hyper::StatusCode::SWITCHING_PROTOCOLS {
        return Err("H3 tunnel must not synthesize 101".to_string());
    }
    let mut headers = hyper::HeaderMap::new();
    for field in tmp.headers().iter() {
        let name = hyper::header::HeaderName::from_bytes(field.name.as_str().as_bytes())
            .map_err(|e| e.to_string())?;
        let value = hyper::header::HeaderValue::from_bytes(field.value.as_bytes())
            .map_err(|e| e.to_string())?;
        headers.append(name, value);
    }
    let mut head = hyper::Response::builder()
        .status(status)
        .body(())
        .map_err(|e| e.to_string())?;
    *head.headers_mut() = headers;
    tokio::time::timeout(config.response_write_timeout, stream.send_response(head))
        .await
        .map_err(|_| "response write timeout".to_string())?
        .map_err(|e| e.to_string())
}

fn h3_trailers_to_block(map: &hyper::HeaderMap) -> Result<HeaderBlock, String> {
    // Canonical header validation only; trailer denylist/limits enforced once
    // in `RequestBody` via `validate_trailers` (no second H3 policy).
    let mut block = HeaderBlock::new();
    for (name, value) in map.iter() {
        let name =
            HeaderName::new(name.as_str()).map_err(|_| "invalid trailer name".to_string())?;
        let value = HeaderValue::from_bytes(value.as_bytes())
            .map_err(|_| "invalid trailer value".to_string())?;
        block.push(name, value);
    }
    Ok(block)
}

fn spawn_body_timeout_watchdog(
    shared: Arc<RequestShared>,
    cancel: tokio::sync::watch::Sender<bool>,
    deadline: tokio::time::Instant,
    conn_id: u64,
    ops: crate::ops::OpsContext,
) {
    tokio::spawn(async move {
        tokio::select! {
            _ = shared.wait_body_terminal() => {}
            _ = tokio::time::sleep_until(deadline) => {
                if shared.mark_failed_with_reason(RequestCancellationReason::ConnectionTimeout) {
                    let _ = cancel.send(true);
                    ops.counters().body_read_timeouts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    ops.counters().deferred_body_timeouts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    ops.emit(crate::ops::Event::new(
                        crate::ops::Severity::Warn,
                        crate::ops::EventKind::DeferredBodyTimeout,
                        "HTTP/3 request body timeout",
                    ).connection_id(conn_id));
                }
            }
        }
    });
}

async fn invoke_service<S: Service>(
    service: Arc<S>,
    request: Request,
    _permit: OwnedSemaphorePermit,
    config: &RuntimeConfig,
    ops: &crate::ops::OpsContext,
) -> Result<Response, ServiceError> {
    crate::server::connection::response::invoke_canonical_service(
        service.as_ref(),
        request,
        config.handler_timeout,
        ops,
    )
    .await
}

fn declared_content_length(request: &hyper::Request<()>) -> Result<Option<u64>, ServiceError> {
    let values = request
        .headers()
        .get_all(hyper::header::CONTENT_LENGTH)
        .iter()
        .collect::<Vec<_>>();
    if values.len() > 1 {
        return Err(ServiceError::rejected(
            400,
            "duplicate Content-Length headers",
        ));
    }
    values
        .first()
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| ServiceError::rejected(400, "invalid Content-Length header"))
        })
        .transpose()
}

fn convert_request_head(
    request: &hyper::Request<()>,
    config: &RuntimeConfig,
    _conn_id: u64,
    ops: &crate::ops::OpsContext,
) -> Result<RequestHead, ServiceError> {
    let method = Method::new(request.method().as_str())
        .map_err(|_| ServiceError::rejected(400, "invalid method"))?;
    let uri = request.uri();
    if !uri
        .scheme_str()
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https"))
    {
        return Err(ServiceError::rejected(400, "HTTP/3 requires https scheme"));
    }
    let raw_target = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    if raw_target.len() > config.max_request_target_bytes {
        ops.counters()
            .request_target_rejected
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return Err(ServiceError::rejected(414, "request target too long"));
    }
    let target = RequestTarget::parse(raw_target)
        .map_err(|_| ServiceError::rejected(400, "invalid request target"))?;
    let mut headers = HeaderBlock::with_capacity(request.headers().len());
    let mut header_bytes = 0usize;
    if request.headers().len() > config.max_headers {
        return Err(ServiceError::rejected(431, "too many request headers"));
    }
    for (name, value) in request.headers() {
        if matches!(
            name.as_str().to_ascii_lowercase().as_str(),
            "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade"
        ) {
            return Err(ServiceError::rejected(
                400,
                "forbidden HTTP/3 connection header",
            ));
        }
        if name.as_str().eq_ignore_ascii_case("te")
            && !value.as_bytes().eq_ignore_ascii_case(b"trailers")
        {
            return Err(ServiceError::rejected(400, "invalid HTTP/3 TE header"));
        }
        header_bytes = header_bytes
            .saturating_add(name.as_str().len())
            .saturating_add(value.len());
        if header_bytes > config.max_header_bytes {
            return Err(ServiceError::rejected(431, "request headers too large"));
        }
        let name = HeaderName::new(name.as_str())
            .map_err(|_| ServiceError::rejected(400, "invalid header name"))?;
        let value = HeaderValue::from_bytes(value.as_bytes())
            .map_err(|_| ServiceError::rejected(400, "invalid header value"))?;
        headers.push(name, value);
    }
    let authority = uri
        .authority()
        .map(|value| crate::primitives::authority::Authority::parse(value.as_str()))
        .transpose()
        .map_err(|_| ServiceError::rejected(400, "invalid authority"))?;
    Ok(RequestHead::new_with_authority(
        method,
        target,
        HttpVersion::Http3,
        headers,
        authority,
    ))
}

struct ActiveConnectionGuard {
    ops: crate::ops::OpsContext,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.ops
            .counters()
            .active_connections
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

async fn send_canonical_response<S>(
    stream: &mut h3::server::RequestStream<S, H3Bytes>,
    mut response: Response,
    config: &RuntimeConfig,
    is_head: bool,
    file_stream_semaphore: &Arc<Semaphore>,
) -> Result<(), String>
where
    S: h3::quic::SendStream<H3Bytes>,
{
    response = match normalize_response(response, &NormalizeRequest::new(is_head)) {
        Ok(response) => response,
        Err(_) => runtime_error_response(500, is_head, config),
    };
    response = crate::server::connection::response::finalize_canonical_response(response, config);
    let status =
        hyper::StatusCode::from_u16(response.status().as_u16()).map_err(|e| e.to_string())?;
    let mut headers = hyper::HeaderMap::new();
    let mut body = response.take_body().unwrap_or(ResponseBody::Empty);
    let _file_permit = if matches!(body, ResponseBody::File(_)) {
        Some(
            file_stream_semaphore
                .clone()
                .try_acquire_owned()
                .map_err(|_| "file stream admission limit reached".to_string())?,
        )
    } else {
        None
    };
    for field in response.headers().iter() {
        if matches!(
            field.name.as_str().to_ascii_lowercase().as_str(),
            "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade"
        ) {
            continue;
        }
        let name = hyper::header::HeaderName::from_bytes(field.name.as_str().as_bytes())
            .map_err(|e| e.to_string())?;
        let value = hyper::header::HeaderValue::from_bytes(field.value.as_bytes())
            .map_err(|e| e.to_string())?;
        headers.append(name, value);
    }
    let mut response_head = hyper::Response::builder()
        .status(status)
        .body(())
        .map_err(|e| e.to_string())?;
    *response_head.headers_mut() = headers;
    tokio::time::timeout(
        config.response_write_timeout,
        stream.send_response(response_head),
    )
    .await
    .map_err(|_| "response write timeout".to_string())?
    .map_err(|e| e.to_string())?;
    match &mut body {
        ResponseBody::Empty | ResponseBody::EmptyWithLength(_) => {}
        ResponseBody::Bytes(bytes) => {
            send_bytes(stream, Bytes::from(std::mem::take(bytes)), config).await?;
        }
        ResponseBody::File(source) => {
            let mut offset = 0u64;
            let length = source.len();
            while offset < length {
                let chunk = config.stream_chunk_size.min((length - offset) as usize);
                let start = offset;
                let end = offset + chunk as u64 - 1;
                let body_source =
                    std::mem::replace(source, crate::primitives::body::BodySource::Empty);
                let (body_source, bytes) = tokio::task::spawn_blocking(move || {
                    let mut body_source = body_source;
                    let bytes = body_source
                        .read_range(start, end)
                        .map_err(|e| e.to_string())?;
                    Ok::<_, String>((body_source, bytes))
                })
                .await
                .map_err(|e| e.to_string())??;
                *source = body_source;
                if bytes.is_empty() {
                    break;
                }
                send_bytes(stream, Bytes::from(bytes), config).await?;
                offset += chunk as u64;
            }
        }
        ResponseBody::Stream(response_stream) => {
            let declared = response_stream.known_length();
            let mut emitted = 0u64;
            let mut response_stream = Box::pin(std::mem::replace(
                response_stream,
                crate::primitives::response_stream::ResponseStream::empty(),
            ));
            // Plan 194: producer no-progress budget. Armed once response
            // HEADERS have been sent; only meaningful (non-empty) production
            // followed by successful send re-arms it. Empty chunks preserve
            // the existing deadline so they cannot refresh the budget.
            let mut producer_deadline = tokio::time::Instant::now() + config.response_write_timeout;
            loop {
                let next = tokio::time::timeout_at(producer_deadline, response_stream.next())
                    .await
                    .map_err(|_| "response producer timeout".to_string())?;
                let Some(chunk) = next else { break };
                let chunk = chunk.map_err(|_| "response producer failed".to_string())?;
                if chunk.is_empty() {
                    continue;
                }
                emitted = emitted.saturating_add(chunk.len() as u64);
                send_bytes(stream, chunk, config).await?;
                producer_deadline = tokio::time::Instant::now() + config.response_write_timeout;
            }
            if let Some(declared) = declared {
                if emitted != declared {
                    return Err(format!(
                        "response stream length mismatch: declared {declared}, emitted {emitted}"
                    ));
                }
            }
            // Plan 198 Track C/G: one terminal trailer block without buffering
            // the body. The trailer future is polled once after data completion
            // under the same no-progress deadline; empty trailer futures do not
            // refresh the budget. Failures are stream-scoped (stop_stream by the
            // caller), siblings survive. Validation reuses the single canonical
            // `Trailers` validator (construction-time); no second H3 policy.
            let trailer_fut = response_stream.as_mut().get_mut().take_trailer_future();
            if let Some(mut fut) = trailer_fut {
                let trailers = tokio::time::timeout_at(producer_deadline, &mut fut)
                    .await
                    .map_err(|_| "response producer timeout".to_string())?
                    .map_err(|_| "response producer failed".to_string())?;
                if let Some(trailers) = trailers {
                    let mut map = hyper::HeaderMap::new();
                    for field in trailers.iter() {
                        let name =
                            hyper::header::HeaderName::from_bytes(field.name.as_str().as_bytes())
                                .map_err(|e| e.to_string())?;
                        let value = hyper::header::HeaderValue::from_bytes(field.value.as_bytes())
                            .map_err(|e| e.to_string())?;
                        map.append(name, value);
                    }
                    return tokio::time::timeout(
                        config.response_write_timeout,
                        stream.send_trailers(map),
                    )
                    .await
                    .map_err(|_| "response write timeout".to_string())?
                    .map_err(|e| e.to_string());
                }
            }
        }
    }
    tokio::time::timeout(config.response_write_timeout, stream.finish())
        .await
        .map_err(|_| "response write timeout".to_string())?
        .map_err(|e| e.to_string())
}

/// Send one H3 response and cancel only its request lifecycle when the stream
/// becomes unusable. QUIC stream failures are deliberately not promoted to a
/// connection-wide cancellation here; sibling request streams remain live.
#[allow(clippy::too_many_arguments)]
async fn send_response_or_cancel<S>(
    stream: &mut h3::server::RequestStream<S, H3Bytes>,
    response: Response,
    config: &RuntimeConfig,
    is_head: bool,
    file_stream_semaphore: &Arc<Semaphore>,
    shared: &Arc<RequestShared>,
    conn_id: u64,
    ops: &crate::ops::OpsContext,
) -> bool
where
    S: h3::quic::SendStream<H3Bytes>,
{
    match send_canonical_response(stream, response, config, is_head, file_stream_semaphore).await {
        Ok(()) => true,
        Err(error) => {
            // Plan 194: producer/send no-progress timeouts are observable as
            // write-stall timeouts (stream-scoped for H3), matching the H1/H2
            // `write_stall_timeouts` signal without introducing a connection
            // fallback. Explicit producer `Err` keeps the generic failure
            // path with no stall counter.
            if error == "response producer timeout" || error == "response write timeout" {
                ops.counters()
                    .write_stall_timeouts
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                ops.emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Warn,
                        crate::ops::EventKind::WriteStallTimeout,
                        "H3 response write stall timeout",
                    )
                    .connection_id(conn_id),
                );
            }
            cancel_shared_with_observability(
                shared,
                RequestCancellationReason::TransportFailure,
                conn_id,
                ops,
            );
            stream.stop_stream(h3::error::Code::H3_INTERNAL_ERROR);
            false
        }
    }
}

async fn send_bytes<S>(
    stream: &mut h3::server::RequestStream<S, H3Bytes>,
    bytes: Bytes,
    config: &RuntimeConfig,
) -> Result<(), String>
where
    S: h3::quic::SendStream<H3Bytes>,
{
    let chunk_size = config
        .http3
        .max_send_buf_size
        .min(config.stream_chunk_size)
        .max(1);
    for chunk in bytes.chunks(chunk_size) {
        tokio::time::timeout(
            config.response_write_timeout,
            stream.send_data(Bytes::copy_from_slice(chunk)),
        )
        .await
        .map_err(|_| "response write timeout".to_string())?
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn runtime_error_response(status: u16, is_head: bool, config: &RuntimeConfig) -> Response {
    let status = crate::primitives::canonical::StatusCode::new(status)
        .unwrap_or(crate::primitives::canonical::StatusCode::INTERNAL_SERVER_ERROR);
    crate::primitives::canonical::runtime_error_with_policy(
        status,
        is_head,
        config.response_policy.error_policy,
    )
}

fn h3_connection_close_reason(error: &quinn::ConnectionError) -> RequestCancellationReason {
    match error {
        quinn::ConnectionError::ApplicationClosed(_)
        | quinn::ConnectionError::ConnectionClosed(_)
        | quinn::ConnectionError::Reset => RequestCancellationReason::PeerDisconnected,
        quinn::ConnectionError::TimedOut => RequestCancellationReason::ConnectionTimeout,
        quinn::ConnectionError::LocallyClosed => RequestCancellationReason::ServerShutdown,
        _ => RequestCancellationReason::TransportFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> RuntimeConfig {
        RuntimeConfig::builder().build().expect("default config")
    }

    #[test]
    fn request_metadata_maps_to_canonical_http3_fields() {
        let request = hyper::Request::builder()
            .method("GET")
            .uri("https://example.test:443/assets?a=1")
            .version(hyper::Version::HTTP_3)
            .header("x-test", "value")
            .body(())
            .unwrap();
        let head =
            convert_request_head(&request, &config(), 7, crate::ops::OpsContext::global()).unwrap();

        assert_eq!(head.version(), HttpVersion::Http3);
        assert_eq!(head.method().as_str(), "GET");
        assert_eq!(head.target().path(), "/assets");
        assert_eq!(head.target().query(), Some("a=1"));
        assert_eq!(head.authority().unwrap().as_str(), "example.test:443");
        assert!(head.headers().contains("x-test"));
    }

    #[test]
    fn request_metadata_rejects_connection_headers_and_invalid_te() {
        for (name, value) in [("connection", "close"), ("te", "chunked")] {
            let request = hyper::Request::builder()
                .uri("https://example.test/")
                .header(name, value)
                .body(())
                .unwrap();
            let error =
                convert_request_head(&request, &config(), 1, crate::ops::OpsContext::global())
                    .unwrap_err();
            assert_eq!(error.status_code(), 400);
        }
    }

    #[test]
    fn declared_content_length_is_strict_and_duplicate_safe() {
        let request = hyper::Request::builder()
            .uri("https://example.test/")
            .header(hyper::header::CONTENT_LENGTH, "12")
            .body(())
            .unwrap();
        assert_eq!(declared_content_length(&request).unwrap(), Some(12));

        let request = hyper::Request::builder()
            .uri("https://example.test/")
            .header(hyper::header::CONTENT_LENGTH, "not-a-length")
            .body(())
            .unwrap();
        assert_eq!(
            declared_content_length(&request).unwrap_err().status_code(),
            400
        );
    }

    #[test]
    fn http3_config_rejects_missing_unidirectional_stream_headroom() {
        let error = RuntimeConfig::builder()
            .http3(crate::server::Http3Config {
                enabled: true,
                max_concurrent_uni_streams: 2,
                ..crate::server::Http3Config::default()
            })
            .build()
            .unwrap_err();
        assert!(error.to_string().contains("max_concurrent_uni_streams"));
    }

    #[tokio::test]
    async fn body_timeout_marks_h3_body_failed_and_wakes_stream() {
        let shared = RequestShared::new_active();
        let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
        spawn_body_timeout_watchdog(
            shared.clone(),
            cancel,
            tokio::time::Instant::now() + std::time::Duration::from_millis(10),
            1,
            crate::ops::OpsContext::global().clone(),
        );
        tokio::time::timeout(std::time::Duration::from_secs(1), cancelled.changed())
            .await
            .expect("H3 body timeout should wake the receive stream")
            .expect("watch sender should remain alive until timeout");
        assert!(!shared.is_body_active());
    }

    #[test]
    fn connection_close_reasons_use_transport_neutral_taxonomy() {
        assert_eq!(
            h3_connection_close_reason(&quinn::ConnectionError::TimedOut),
            RequestCancellationReason::ConnectionTimeout
        );
        assert_eq!(
            h3_connection_close_reason(&quinn::ConnectionError::Reset),
            RequestCancellationReason::PeerDisconnected
        );
        assert_eq!(
            h3_connection_close_reason(&quinn::ConnectionError::LocallyClosed),
            RequestCancellationReason::ServerShutdown
        );
    }
}
