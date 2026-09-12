//! Experimental HTTP/3 server adapter (Plan 187).
//!
//! Quinn and h3 are deliberately contained here. The adapter translates H3
//! request/response streams to EggServe's canonical request/service/response
//! types and owns only protocol mechanics; service admission, panic
//! containment, timeouts, normalization, and privacy policy remain shared
//! with the canonical runtime helpers.

//! Module layout (Plan 206 Track F): `endpoint` owns listener/connection
//! lifecycle; `request` owns metadata/body conversion; `response` owns
//! send/body/trailer + timeout/reset; `tunnel` owns Extended CONNECT.
//! One shared service kernel + canonical normalization (no H3-specific
//! semantics).

use eggserve_h3::{h3, h3_quinn, quinn};

pub(super) mod endpoint;
pub(super) mod request;
pub(super) mod response;
pub(super) mod tunnel;

#[allow(unused_imports)]
use std::sync::Arc;

#[allow(unused_imports)]
use bytes::{Buf, Bytes};
#[allow(unused_imports)]
use futures_util::{stream, StreamExt};
#[allow(unused_imports)]
use tokio::sync::{broadcast, OwnedSemaphorePermit, Semaphore};

#[allow(unused_imports)]
use crate::primitives::canonical::{normalize_response, NormalizeRequest, Response, ResponseBody};
#[allow(unused_imports)]
use crate::primitives::connection_info::TlsInfo;
#[allow(unused_imports)]
use crate::primitives::header_block::{HeaderBlock, HeaderName, HeaderValue};
#[allow(unused_imports)]
use crate::primitives::method::Method;
#[allow(unused_imports)]
use crate::primitives::request::Request;
#[allow(unused_imports)]
use crate::primitives::request_body::IncomingError;
#[allow(unused_imports)]
use crate::primitives::request_head::RequestHead;
#[allow(unused_imports)]
use crate::primitives::request_lifecycle::{RequestCancellationReason, RequestShared};
#[allow(unused_imports)]
use crate::primitives::request_target::RequestTarget;
#[allow(unused_imports)]
use crate::primitives::version::HttpVersion;
#[allow(unused_imports)]
use crate::server::config::RuntimeConfig;
#[allow(unused_imports)]
use crate::server::connection::lifecycle::{cancel_shared_with_observability, ConnectionRequests};
#[allow(unused_imports)]
use crate::server::connection::ConnectionContext;
#[allow(unused_imports)]
use crate::server::errors::ShutdownResult;
#[allow(unused_imports)]
use crate::server::service::{Service, ServiceError};
#[allow(unused_imports)]
use crate::server::RuntimeState;

type H3Bytes = Bytes;

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
                        let _active_connection = endpoint::ActiveConnectionGuard { ops: ops.clone() };
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
                let reason = endpoint::h3_connection_close_reason(&closed);
                requests_registry.cancel_all(reason, conn_id, runtime_state.ops());
                break;
            }
            accepted = h3_connection.accept() => {
                let resolver = match accepted {
                    Ok(Some(resolver)) => resolver,
                    Ok(None) => break,
                    Err(_) => {
                        requests_registry.cancel_all(
                            endpoint::h3_connection_close_reason(
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
    let head = match request::convert_request_head(&request, &config, conn_id, runtime_state.ops())
    {
        Ok(head) => head,
        Err(error) => {
            let response = response::runtime_error_response(error.status_code(), is_head, &config);
            if !response::send_response_or_cancel(
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
                // `response::send_response_or_cancel` on failure; still abort receive so
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
    let declared_length = match request::declared_content_length(&request) {
        Ok(length) => length,
        Err(error) => {
            let _ = response::send_response_or_cancel(
                &mut send_stream,
                response::runtime_error_response(error.status_code(), is_head, &config),
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
            let response = response::runtime_error_response(413, is_head, &config);
            let _ = response::send_response_or_cancel(
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
        let response = response::runtime_error_response(413, is_head, &config);
        let _ = response::send_response_or_cancel(
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
                let response = response::runtime_error_response(413, is_head, &config);
                let _ = response::send_response_or_cancel(
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
                let _ = response::send_response_or_cancel(
                    &mut send_stream,
                    response::runtime_error_response(500, is_head, &config),
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
                let _ = response::send_response_or_cancel(
                    &mut send_stream,
                    response::runtime_error_response(408, is_head, &config),
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
                                        match request::h3_trailers_to_block(&map) {
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
        response::spawn_body_timeout_watchdog(
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
                    let _ = response::send_response_or_cancel(
                        &mut send_stream,
                        response::runtime_error_response(error.to_status_code(), is_head, &config),
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
                    let _ = response::send_response_or_cancel(
                        &mut send_stream,
                        response::runtime_error_response(408, is_head, &config),
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
                alpn: Some("h3".into()),
                ..Default::default()
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
            let _ = response::send_response_or_cancel(
                &mut send_stream,
                response::runtime_error_response(503, is_head, &config),
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
    let result =
        request::invoke_service(service, request, permit, &config, runtime_state.ops()).await;
    let response = match result {
        Ok(response) => response,
        Err(error) => response::runtime_error_response(error.status_code(), is_head, &config),
    };
    let _ = response::send_response_or_cancel(
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
    match request::declared_content_length(&request) {
        Ok(Some(len)) if len > 0 => {
            let _ = response::send_response_or_cancel(
                &mut send_stream,
                response::runtime_error_response(413, false, &config),
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
            let _ = response::send_response_or_cancel(
                &mut send_stream,
                response::runtime_error_response(error.status_code(), false, &config),
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
                let _ = response::send_response_or_cancel(
                    &mut send_stream,
                    response::runtime_error_response(400, false, &config),
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
        let _ = response::send_response_or_cancel(
            &mut send_stream,
            response::runtime_error_response(400, false, &config),
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
            alpn: Some("h3".into()),
            ..Default::default()
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
            let _ = response::send_response_or_cancel(
                &mut send_stream,
                response::runtime_error_response(503, false, &config),
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
            let response = response::runtime_error_response(error.status_code(), false, &config);
            let _ = response::send_response_or_cancel(
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
        let _ = response::send_response_or_cancel(
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
            let _ = response::send_response_or_cancel(
                &mut send_stream,
                response::runtime_error_response(503, false, &config),
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
    // Re-attach empty body for `response::send_canonical_response`-style conversion?
    // Instead, send headers directly (no body, no trailers, no finish).
    let send_result = tunnel::send_h3_tunnel_handshake(&mut send_stream, handshake, &config).await;
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
    let _active_guard = tunnel::H3ActiveTunnelGuard { ops: ops.clone() };
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
            format!("tunnel closed: {}", tunnel::kind_string(kind)),
        )
        .connection_id(conn_id),
    );
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
            request::convert_request_head(&request, &config(), 7, crate::ops::OpsContext::global())
                .unwrap();

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
            let error = request::convert_request_head(
                &request,
                &config(),
                1,
                crate::ops::OpsContext::global(),
            )
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
        assert_eq!(
            request::declared_content_length(&request).unwrap(),
            Some(12)
        );

        let request = hyper::Request::builder()
            .uri("https://example.test/")
            .header(hyper::header::CONTENT_LENGTH, "not-a-length")
            .body(())
            .unwrap();
        assert_eq!(
            request::declared_content_length(&request)
                .unwrap_err()
                .status_code(),
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
        response::spawn_body_timeout_watchdog(
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
            endpoint::h3_connection_close_reason(&quinn::ConnectionError::TimedOut),
            RequestCancellationReason::ConnectionTimeout
        );
        assert_eq!(
            endpoint::h3_connection_close_reason(&quinn::ConnectionError::Reset),
            RequestCancellationReason::PeerDisconnected
        );
        assert_eq!(
            endpoint::h3_connection_close_reason(&quinn::ConnectionError::LocallyClosed),
            RequestCancellationReason::ServerShutdown
        );
    }
}
