//! Connection execution pipeline.
//!
//! This module owns the per-connection execution path from TCP accept to
//! response completion. It is used by both the CLI accept loop and the
//! embedded runtime.
//!
//! # Pipeline steps
//!
//! 1. Optional TLS handshake (feature-gated)
//! 2. HTTP/1 connection setup via Hyper
//! 3. Request conversion to canonical types
//! 4. Request-policy validation (body rejection for body-forbidden methods)
//! 5. Service invocation with panic containment
//! 6. Canonical response normalization
//! 7. Transport-body conversion
//! 8. Permit release and connection termination

// Panics raised while executing a [`Service`] are contained at the
// invocation boundary and mapped to [`ServiceError::panic`], so the client
// receives an RFC-correct 500 response instead of a dropped connection.
// The standard panic hook still runs, keeping diagnostics on stderr; panics
// outside service execution (e.g., during transport-body conversion) still
// propagate to the JoinSet task boundary.

use std::convert::Infallible;
use std::sync::Arc;

use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::sync::broadcast;

use crate::primitives::request_body_policy::RequestBodyPolicy;
use crate::response::BoxBodyInner;
use crate::server::config::RuntimeConfig;
use crate::server::service::{Service, ServiceError};
use crate::server::RuntimeState;

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

/// Serve a single HTTP/1.1 connection.
///
/// This is the core connection executor used by both the CLI and embedded
/// runtime. It handles:
///
/// - HTTP/1 connection setup with Hyper
/// - Header-read timeout enforcement
/// - Connection-total-timeout enforcement (maximum connection lifetime)
/// - Graceful shutdown propagation
///
/// The `service` parameter provides the request handler. The built-in static
/// path supplies [`crate::server::StaticService`]; custom services supply their own
/// [`Service`] implementation.
pub async fn serve_connection<I, S>(
    io: TokioIo<I>,
    service: S,
    config: &RuntimeConfig,
    shutdown_rx: &mut broadcast::Receiver<()>,
    conn_id: u64,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: hyper::service::Service<
            Request<Incoming>,
            Response = Response<BoxBodyInner>,
            Error = Infallible,
        > + 'static,
{
    let conn = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(config.header_read_timeout)
        .serve_connection(io, service)
        .with_upgrades();
    let mut conn = std::pin::pin!(conn);
    tokio::select! {
        result = tokio::time::timeout(config.connection_total_timeout, &mut conn) => {
            match result {
                Ok(Ok(())) => {
                    crate::ops::Logger::global().emit(
                        crate::ops::Event::new(
                            crate::ops::Severity::Debug,
                            crate::ops::EventKind::KeepAliveClosed,
                            "connection closed",
                        )
                        .connection_id(conn_id),
                    );
                }
                Ok(Err(e)) => {
                    // Hyper reports an expired header-read timeout as a
                    // timeout-class connection error; classify it so the
                    // ops counter tracks the event it is named for.
                    let header_timeout = e.is_timeout();
                    if header_timeout {
                        crate::ops::global_counters()
                            .header_timeouts
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
                            } else {
                                crate::ops::EventKind::ClientDisconnect
                            },
                            if header_timeout {
                                "header read timeout".to_string()
                            } else {
                                format!("connection error: {}", e)
                            },
                        )
                        .connection_id(conn_id),
                    );
                }
                Err(_elapsed) => {
                    crate::ops::global_counters().connection_total_timeouts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    crate::ops::Logger::global().emit(
                        crate::ops::Event::new(
                            crate::ops::Severity::Warn,
                            crate::ops::EventKind::ConnectionTotalTimeout,
                            "connection total timeout",
                        )
                        .connection_id(conn_id),
                    );
                    conn.as_mut().graceful_shutdown();
                    if tokio::time::timeout(
                        post_shutdown_drain_budget(config),
                        conn.as_mut(),
                    )
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
            }
        }
        _ = shutdown_rx.recv() => {
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
    }
}

/// Serve a single connection with a custom [`Service`] implementation.
///
/// This wraps the raw Hyper service with:
/// - Request conversion from Hyper to canonical types
/// - Handler timeout enforcement
/// - Service error to response conversion
/// - Canonical response normalization
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
    let service = std::sync::Arc::new(service);
    let handler_timeout = config.handler_timeout;
    let body_read_timeout = config.body_read_timeout;
    let max_body_bytes = config.max_request_body_bytes;
    let tls_info = std::sync::Arc::new(tls_info);
    let file_stream_semaphore = runtime_state.file_stream_semaphore().clone();
    let stream_chunk_size = config.stream_chunk_size;
    let response_config = config.clone();

    let hyper_service = service_fn(move |req: Request<Incoming>| {
        let service = service.clone();
        let tls_info = tls_info.clone();
        let file_stream_semaphore = file_stream_semaphore.clone();
        let config = response_config.clone();
        async move {
            // Convert Hyper request to canonical RequestHead.
            let head = match convert_request_head(&req) {
                Ok(h) => h,
                Err(e) => {
                    return Ok::<_, Infallible>(finalize_runtime_response(
                        e.to_response(),
                        &config,
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
                let mut response = crate::response::bad_request(false);
                response.headers_mut().insert(
                    hyper::header::CONNECTION,
                    hyper::header::HeaderValue::from_static("close"),
                );
                return Ok::<_, Infallible>(finalize_runtime_response(response, &config));
            }

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
                return Ok::<_, Infallible>(finalize_runtime_response(e.to_response(), &config));
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
                        return Ok::<_, Infallible>(finalize_runtime_response(
                            body_error_to_response(err, &head),
                            &config,
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
                        let mut response = crate::response::payload_too_large(false);
                        response.headers_mut().insert(
                            hyper::header::CONNECTION,
                            hyper::header::HeaderValue::from_static("close"),
                        );
                        return Ok::<_, Infallible>(finalize_runtime_response(response, &config));
                    }
                }
            }

            // Handle Reject policy — reject without invoking the service,
            // but only if the request actually carries a body.
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
                let mut response = crate::response::payload_too_large(false);
                // Do not drain the body — drop it and close the connection to
                // prevent unread bytes from being interpreted as a subsequent
                // request. Hyper handles cleanup of the unconsumed body when
                // the connection is dropped.
                response.headers_mut().insert(
                    hyper::header::CONNECTION,
                    hyper::header::HeaderValue::from_static("close"),
                );
                return Ok::<_, Infallible>(finalize_runtime_response(response, &config));
            }

            // For Buffer/Stream policies, create RequestBody with proper limits.
            // For Reject with no body, create an empty body (nothing to reject).
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
                    let connection =
                        build_connection_info(local_addr, remote_addr, tls, (*tls_info).clone());
                    let request =
                        crate::primitives::request::Request::new(head, request_body, connection);

                    let result = tokio::time::timeout(
                        handler_timeout,
                        contain_service_panic(service.call(request)),
                    )
                    .await;

                    let response = match result {
                        Ok(Ok(canonical)) => {
                            match crate::primitives::canonical::to_hyper_response_with_file_stream_semaphore_and_chunk_size(canonical, &file_stream_semaphore, stream_chunk_size) {
                                Ok(r) => r,
                                Err(crate::primitives::canonical::ResponseConstructionError::FileStreamLimit) => crate::response::service_unavailable(),
                                Err(_) => crate::response::internal_error(),
                            }
                        }
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
                            service_err.to_response()
                        }
                        Err(_elapsed) => {
                            crate::ops::Logger::global().emit(crate::ops::Event::new(
                                crate::ops::Severity::Warn,
                                crate::ops::EventKind::ServiceTimeout,
                                "handler timed out",
                            ));
                            ServiceError::timeout("handler timed out".to_string()).to_response()
                        }
                    };

                    Ok::<_, Infallible>(finalize_runtime_response(response, &config))
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
                            return Ok::<_, Infallible>(finalize_runtime_response(
                                body_error_to_response(err, &head),
                                &config,
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
                            return Ok::<_, Infallible>(finalize_runtime_response(
                                body_error_to_response(err, &head),
                                &config,
                            ));
                        }
                    };

                    let connection =
                        build_connection_info(local_addr, remote_addr, tls, (*tls_info).clone());
                    let request =
                        crate::primitives::request::Request::new(head, request_body, connection);

                    let result = tokio::time::timeout(
                        handler_timeout,
                        contain_service_panic(service.call(request)),
                    )
                    .await;

                    let response = match result {
                        Ok(Ok(canonical)) => {
                            match crate::primitives::canonical::to_hyper_response_with_file_stream_semaphore_and_chunk_size(canonical, &file_stream_semaphore, stream_chunk_size) {
                                Ok(r) => r,
                                Err(crate::primitives::canonical::ResponseConstructionError::FileStreamLimit) => crate::response::service_unavailable(),
                                Err(_) => crate::response::internal_error(),
                            }
                        }
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
                            service_err.to_response()
                        }
                        Err(_elapsed) => {
                            crate::ops::Logger::global().emit(crate::ops::Event::new(
                                crate::ops::Severity::Warn,
                                crate::ops::EventKind::ServiceTimeout,
                                "handler timed out",
                            ));
                            ServiceError::timeout("handler timed out".to_string()).to_response()
                        }
                    };

                    Ok::<_, Infallible>(finalize_runtime_response(response, &config))
                }
                RequestBodyPolicy::Stream { .. } => {
                    // For Stream mode the service call (including body
                    // consumption) runs under a total deadline of
                    // min(body_read_timeout, handler_timeout).
                    let effective_timeout = body_read_timeout.min(handler_timeout);
                    let connection =
                        build_connection_info(local_addr, remote_addr, tls, (*tls_info).clone());
                    // Clone the consumption flag before the body is moved into
                    // Request; Stream mode is the only consumer.
                    let consumed_flag = request_body.consumed_flag();
                    let request =
                        crate::primitives::request::Request::new(head, request_body, connection);

                    let result = tokio::time::timeout(
                        effective_timeout,
                        contain_service_panic(service.call(request)),
                    )
                    .await;

                    let response = match result {
                        Ok(Ok(canonical)) => {
                            match crate::primitives::canonical::to_hyper_response_with_file_stream_semaphore_and_chunk_size(canonical, &file_stream_semaphore, stream_chunk_size) {
                                Ok(r) => r,
                                Err(crate::primitives::canonical::ResponseConstructionError::FileStreamLimit) => crate::response::service_unavailable(),
                                Err(_) => crate::response::internal_error(),
                            }
                        }
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
                            service_err.to_response()
                        }
                        Err(_elapsed) => {
                            crate::ops::Logger::global().emit(crate::ops::Event::new(
                                crate::ops::Severity::Warn,
                                crate::ops::EventKind::ServiceTimeout,
                                "handler timed out",
                            ));
                            ServiceError::timeout("handler timed out".to_string()).to_response()
                        }
                    };

                    // A stream that is not consumed to EOF cannot safely leave
                    // unread bytes on an HTTP/1.1 connection. Close only in
                    // that case; fully consumed streams remain reusable.
                    let incomplete = !consumed_flag.load(std::sync::atomic::Ordering::Acquire);
                    if incomplete {
                        crate::ops::Logger::global().emit(
                            crate::ops::Event::new(
                                crate::ops::Severity::Debug,
                                crate::ops::EventKind::IncompleteBodyClose,
                                "service returned with unconsumed body; connection will close",
                            )
                            .connection_id(conn_id),
                        );
                    }

                    let mut response = finalize_runtime_response(response, &config);
                    if incomplete {
                        response.headers_mut().insert(
                            hyper::header::CONNECTION,
                            hyper::header::HeaderValue::from_static("close"),
                        );
                    }
                    Ok::<_, Infallible>(response)
                }
            }
        }
    });

    serve_connection(io, hyper_service, &config, shutdown_rx, conn_id).await;
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

/// Convert a RequestBodyError to an HTTP response.
fn body_error_to_response(
    err: crate::primitives::request_body_error::RequestBodyError,
    _head: &crate::primitives::request_head::RequestHead,
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
    let mut resp = crate::response::canonical_error(status, body_text, is_head);
    if should_close {
        resp.headers_mut().insert(
            hyper::header::CONNECTION,
            hyper::header::HeaderValue::from_static("close"),
        );
    }
    resp
}

/// Build ConnectionInfo from real socket addresses.
fn build_connection_info(
    local_addr: std::net::SocketAddr,
    remote_addr: std::net::SocketAddr,
    tls: bool,
    tls_info: Option<crate::primitives::connection_info::TlsInfo>,
) -> crate::primitives::connection_info::ConnectionInfo {
    crate::primitives::connection_info::ConnectionInfo {
        local_addr,
        remote_addr,
        scheme: if tls {
            crate::primitives::connection_info::Scheme::Https
        } else {
            crate::primitives::connection_info::Scheme::Http
        },
        tls: tls_info,
    }
}

/// Apply runtime-owned response fields at the one final Hyper boundary.
fn finalize_runtime_response(
    mut response: hyper::Response<BoxBodyInner>,
    config: &RuntimeConfig,
) -> hyper::Response<BoxBodyInner> {
    response.headers_mut().remove(hyper::header::SERVER);
    if let Some(value) = &config.server_header {
        if let Ok(value) = hyper::header::HeaderValue::from_str(value) {
            response.headers_mut().insert(hyper::header::SERVER, value);
        }
    }
    response
}

/// Validate body framing for ALL methods.
///
/// Rejects requests with duplicate Content-Length fields and TE+CL
/// conflicts where both headers are visible. This is a hardened
/// framing policy applied before body construction.
///
/// Note: Hyper 1.x strips the Content-Length header when
/// Transfer-Encoding is present and rejects conflicting duplicate
/// Content-Length fields while decoding the request. Consequently, these
/// branches are defense-in-depth for a future or alternate parser and are
/// normally unreachable behind Hyper; keeping them makes the framing policy
/// explicit at this boundary.
fn validate_body_framing(headers: &hyper::HeaderMap) -> Result<(), ServiceError> {
    let has_te = headers.contains_key(hyper::header::TRANSFER_ENCODING);
    let mut cl_values = headers.get_all(hyper::header::CONTENT_LENGTH).iter();
    let has_cl = cl_values.next().is_some();
    let duplicate_cl = cl_values.next().is_some();

    if has_te && has_cl {
        return Err(ServiceError::rejected(
            400,
            "conflicting Transfer-Encoding and Content-Length",
        ));
    }

    if duplicate_cl {
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

/// Convert a Hyper request to a canonical [`RequestHead`].
///
/// This extracts method, URI, version, and headers from the Hyper request
/// and constructs a canonical [`RequestHead`]. The body is not included —
/// the runtime handles body rejection before service invocation.
fn convert_request_head(
    req: &Request<Incoming>,
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
    for (name, value) in req.headers().iter() {
        let header_name = crate::primitives::header_block::HeaderName::new(name.as_str())
            .map_err(|_| ServiceError::rejected(400, format!("invalid header name: {}", name)))?;
        let header_value = match value.to_str() {
            Ok(v) => crate::primitives::header_block::HeaderValue::new(v).map_err(|_| {
                ServiceError::rejected(400, format!("invalid header value for {}", name))
            })?,
            Err(_) => {
                return Err(ServiceError::rejected(
                    400,
                    format!("non-UTF-8 header value for {}", name),
                ))
            }
        };
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
        );
        assert_eq!(consumed.status(), hyper::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(consumed.headers().get(hyper::header::CONNECTION).is_none());

        // 499-collapsed disconnects still force close.
        let disconnected = body_error_to_response(
            crate::primitives::request_body_error::RequestBodyError::Disconnected,
            &head(),
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
