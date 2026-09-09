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
    let mut builder = h3::server::builder();
    builder.max_field_section_size(config.http3.max_field_section_size);
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
    loop {
        tokio::select! {
            _ = shutdown_rx.recv(), if !draining => {
                let _ = h3_connection.shutdown(0).await;
                draining = true;
            }
            accepted = h3_connection.accept() => {
                let resolver = match accepted {
                    Ok(Some(resolver)) => resolver,
                    Ok(None) | Err(_) => break,
                };
                accepted_requests = accepted_requests.saturating_add(1);
                if !draining && config.max_requests_per_connection.is_some_and(|max| accepted_requests >= max) {
                    let _ = h3_connection.shutdown(0).await;
                    draining = true;
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
                requests.spawn(async move {
                    let (request, stream) = match resolver.resolve_request().await {
                        Ok(value) => value,
                        Err(_) => return,
                    };
                    handle_request(request, stream, local_addr, remote_addr, service, config, state, conn_id).await;
                });
            }
        }
    }

    let deadline = tokio::time::Instant::now() + config.graceful_shutdown_timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            requests.abort_all();
            return;
        }
        match tokio::time::timeout(remaining, requests.join_next()).await {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                if requests.is_empty() {
                    return;
                }
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
    conn_id: u64,
) where
    S: Service,
    C: h3::quic::BidiStream<H3Bytes>,
    C::RecvStream: Send + 'static,
{
    let is_head = request.method() == hyper::Method::HEAD;
    let (mut send_stream, mut recv_stream) = stream.split();
    let head = match convert_request_head(&request, &config, conn_id, runtime_state.ops()) {
        Ok(head) => head,
        Err(error) => {
            let response = error_response(error.status_code(), &config);
            if send_canonical_response(
                &mut send_stream,
                response,
                &config,
                is_head,
                runtime_state.file_stream_semaphore(),
            )
            .await
            .is_err()
            {
                send_stream.stop_stream(h3::error::Code::H3_INTERNAL_ERROR);
            }
            return;
        }
    };
    let declared_length = match declared_content_length(&request) {
        Ok(length) => length,
        Err(error) => {
            if send_canonical_response(
                &mut send_stream,
                error_response(error.status_code(), &config),
                &config,
                is_head,
                runtime_state.file_stream_semaphore(),
            )
            .await
            .is_err()
            {
                send_stream.stop_stream(h3::error::Code::H3_INTERNAL_ERROR);
            }
            return;
        }
    };
    let policy = crate::server::connection::request::select_body_policy(
        service.request_body_policy(&head),
        config.max_request_body_bytes,
    );
    if let Some(limit) = policy.max_bytes() {
        if declared_length.is_some_and(|length| length > limit) {
            let response = error_response(413, &config);
            if send_canonical_response(
                &mut send_stream,
                response,
                &config,
                is_head,
                runtime_state.file_stream_semaphore(),
            )
            .await
            .is_err()
            {
                send_stream.stop_stream(h3::error::Code::H3_INTERNAL_ERROR);
            }
            recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
            return;
        }
    }
    if policy.is_reject() && declared_length.is_some_and(|length| length > 0) {
        let response = error_response(413, &config);
        if send_canonical_response(
            &mut send_stream,
            response,
            &config,
            is_head,
            runtime_state.file_stream_semaphore(),
        )
        .await
        .is_err()
        {
            send_stream.stop_stream(h3::error::Code::H3_INTERNAL_ERROR);
        }
        recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
        return;
    }

    let request_body = if policy.is_reject() {
        // A missing Content-Length does not prove that an H3 request has no
        // content. Stop the receive direction for every rejected body policy
        // so a peer cannot continue sending data after the response starts.
        recv_stream.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
        crate::primitives::request_body::RequestBody::empty()
    } else {
        let body_cancel = tokio::sync::watch::channel(false);
        let body_stream = stream::unfold(
            (recv_stream, body_cancel.1),
            |(mut stream, mut cancel)| async move {
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
                        Ok(None) => None,
                        Err(error) => Some((Err(IncomingError(error.to_string())), (stream, cancel))),
                    }
                }
            },
        );
        let shared = RequestShared::new_active();
        let request_body = crate::primitives::request_body::RequestBody::from_incoming_with_shared(
            body_stream,
            declared_length,
            policy.max_bytes().unwrap_or(config.max_request_body_bytes),
            shared.clone(),
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
            match tokio::time::timeout(config.body_read_timeout, request_body.read_all()).await {
                Ok(Ok(bytes)) => {
                    crate::primitives::request_body::RequestBody::from_bytes(bytes, max_bytes)
                }
                Ok(Err(error)) => {
                    if send_canonical_response(
                        &mut send_stream,
                        error_response(error.to_status_code(), &config),
                        &config,
                        is_head,
                        runtime_state.file_stream_semaphore(),
                    )
                    .await
                    .is_err()
                    {
                        send_stream.stop_stream(h3::error::Code::H3_INTERNAL_ERROR);
                    }
                    return;
                }
                Err(_) => {
                    if send_canonical_response(
                        &mut send_stream,
                        error_response(408, &config),
                        &config,
                        is_head,
                        runtime_state.file_stream_semaphore(),
                    )
                    .await
                    .is_err()
                    {
                        send_stream.stop_stream(h3::error::Code::H3_INTERNAL_ERROR);
                    }
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
            let _ = send_canonical_response(
                &mut send_stream,
                error_response(503, &config),
                &config,
                is_head,
                runtime_state.file_stream_semaphore(),
            )
            .await;
            return;
        }
    };
    let result = invoke_service(service, request, permit, &config, runtime_state.ops()).await;
    let response = match result {
        Ok(response) => response,
        Err(error) => error_response(error.status_code(), &config),
    };
    if send_canonical_response(
        &mut send_stream,
        response,
        &config,
        is_head,
        runtime_state.file_stream_semaphore(),
    )
    .await
    .is_err()
    {
        send_stream.stop_stream(h3::error::Code::H3_INTERNAL_ERROR);
    }
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
        Err(_) => error_response(500, config),
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
            while let Some(chunk) = response_stream.next().await {
                let chunk = chunk.map_err(|_| "response producer failed".to_string())?;
                if !chunk.is_empty() {
                    emitted = emitted.saturating_add(chunk.len() as u64);
                    send_bytes(stream, chunk, config).await?;
                }
            }
            if let Some(declared) = declared {
                if emitted != declared {
                    return Err(format!(
                        "response stream length mismatch: declared {declared}, emitted {emitted}"
                    ));
                }
            }
        }
    }
    tokio::time::timeout(config.response_write_timeout, stream.finish())
        .await
        .map_err(|_| "response write timeout".to_string())?
        .map_err(|e| e.to_string())
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

fn error_response(status: u16, config: &RuntimeConfig) -> Response {
    let status = crate::primitives::canonical::StatusCode::new(status)
        .unwrap_or(crate::primitives::canonical::StatusCode::INTERNAL_SERVER_ERROR);
    let body =
        if config.response_policy.error_policy == crate::policy::ErrorRepresentationPolicy::Empty {
            ResponseBody::Empty
        } else {
            ResponseBody::Bytes(match status.as_u16() {
                400 => b"bad request".to_vec(),
                413 => b"payload too large".to_vec(),
                414 => b"request-target too long".to_vec(),
                431 => b"request headers too large".to_vec(),
                503 => b"service unavailable".to_vec(),
                _ => b"internal server error".to_vec(),
            })
        };
    Response::builder()
        .status(status)
        .body(body)
        .expect("valid runtime error response")
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
}
