//! H3 response send/body/trailer adapter (Plan 206 Track F).
//!
//! Owns response error construction (`runtime_error_response`) and stream
//! activity/timeout mapping (`spawn_body_timeout_watchdog`,
//! `response_write_timeout` no-progress semantics, per-stream reset).
//! Siblings survive stalls; normalization stays canonical.

#![allow(unused_imports)]
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

pub(super) type H3Bytes = Bytes;

pub(super) fn spawn_body_timeout_watchdog(
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

pub(super) fn runtime_error_response(
    status: u16,
    is_head: bool,
    config: &RuntimeConfig,
) -> Response {
    let status = crate::primitives::canonical::StatusCode::new(status)
        .unwrap_or(crate::primitives::canonical::StatusCode::INTERNAL_SERVER_ERROR);
    crate::primitives::canonical::runtime_error_with_policy(
        status,
        is_head,
        config.response_policy.error_policy,
    )
}

pub(super) async fn send_canonical_response<S>(
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
pub(super) async fn send_response_or_cancel<S>(
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

pub(super) async fn send_bytes<S>(
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
