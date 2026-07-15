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
//! 8. Write timeout enforcement
//! 9. Permit release and connection termination

use std::convert::Infallible;

use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::sync::broadcast;

use crate::config::ServeState;
use crate::primitives::request_body_policy::RequestBodyPolicy;
use crate::response::BoxBodyInner;
use crate::server::config::RuntimeConfig;
use crate::server::service::{Service, ServiceError};

/// Serve a single HTTP/1.1 connection.
///
/// This is the core connection executor used by both the CLI and embedded
/// runtime. It handles:
///
/// - HTTP/1 connection setup with Hyper
/// - Header-read timeout enforcement
/// - Response-write timeout enforcement
/// - Graceful shutdown propagation
///
/// The `service` parameter provides the request handler. For the static
/// service, this is a closure wrapping `handle_request`. For custom services,
/// it wraps the user's [`Service`] implementation.
pub async fn serve_connection<I, S>(
    io: TokioIo<I>,
    service: S,
    config: &RuntimeConfig,
    shutdown_rx: &mut broadcast::Receiver<()>,
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
        result = tokio::time::timeout(config.response_write_timeout, &mut conn) => {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(_e)) => {}
                Err(_elapsed) => {
                    conn.as_mut().graceful_shutdown();
                    let _ = conn.await;
                }
            }
        }
        _ = shutdown_rx.recv() => {
            conn.as_mut().graceful_shutdown();
            let _ = conn.await;
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
/// Panics in the service are caught by the tokio task boundary.
pub async fn serve_connection_with_service<I, S>(
    io: TokioIo<I>,
    service: S,
    config: &RuntimeConfig,
    _state: &ServeState,
    shutdown_rx: &mut broadcast::Receiver<()>,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: Service,
{
    let service = std::sync::Arc::new(service);
    let handler_timeout = config.handler_timeout;
    let body_read_timeout = config.body_read_timeout;
    let max_body_bytes = config.max_request_body_bytes;

    let hyper_service = service_fn(move |req: Request<Incoming>| {
        let service = service.clone();
        async move {
            // Convert Hyper request to canonical RequestHead.
            let head = match convert_request_head(&req) {
                Ok(h) => h,
                Err(e) => {
                    return Ok::<_, Infallible>(e.to_response());
                }
            };

            // Runtime-level request policy validation.
            if let Err(e) = validate_request_policy(&head) {
                return Ok::<_, Infallible>(e.to_response());
            }

            // Select effective body policy.
            let service_policy = service.request_body_policy(&head);
            let effective_policy = select_body_policy(service_policy, max_body_bytes);

            // Extract body from Hyper request.
            let (parts, body) = req.into_parts();
            let declared_length = parts
                .headers
                .get(hyper::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());

            // Validate Content-Length against effective limit.
            if let Some(len) = declared_length {
                if let Some(limit) = effective_policy.max_bytes() {
                    if len > limit {
                        let err = crate::primitives::request_body_error::RequestBodyError::DeclaredLengthTooLarge {
                            declared: len,
                            limit,
                        };
                        return Ok::<_, Infallible>(body_error_to_response(err, &head));
                    }
                }
            }

            // Handle Reject policy — consume and discard body if non-empty.
            if effective_policy.is_reject() {
                let body_limit = 0u64;
                let request_body = crate::primitives::request_body::RequestBody::from_incoming(
                    wrap_incoming_body(body),
                    declared_length,
                    body_limit,
                );

                let connection = build_connection_info(&parts);
                let request = crate::primitives::request::Request::new(
                    head.clone(),
                    crate::primitives::request_body::RequestBody::empty(),
                    connection,
                );

                let result = tokio::time::timeout(handler_timeout, service.call(request)).await;
                let response = match result {
                    Ok(Ok(canonical)) => {
                        match crate::primitives::canonical::to_hyper_response(canonical) {
                            Ok(r) => r,
                            Err(_) => crate::response::internal_error(),
                        }
                    }
                    Ok(Err(service_err)) => service_err.to_response(),
                    Err(_elapsed) => {
                        ServiceError::timeout("handler timed out".to_string()).to_response()
                    }
                };

                // Drain the rejected body to clean up the connection.
                let _ = tokio::time::timeout(
                    std::time::Duration::from_millis(100),
                    drain_body(request_body),
                )
                .await;

                return Ok::<_, Infallible>(response);
            }

            // For Buffer/Stream policies, create RequestBody with proper limits.
            let body_limit = effective_policy.max_bytes().unwrap_or(u64::MAX);
            let request_body = crate::primitives::request_body::RequestBody::from_incoming(
                wrap_incoming_body(body),
                declared_length,
                body_limit,
            );

            // For Buffer policy, pre-buffer the body under timeout.
            let request_body = match &effective_policy {
                RequestBodyPolicy::Buffer { .. } => {
                    match tokio::time::timeout(body_read_timeout, request_body.read_all()).await {
                        Ok(Ok(bytes)) => crate::primitives::request_body::RequestBody::from_bytes(
                            bytes.to_vec(),
                            body_limit,
                        ),
                        Ok(Err(err)) => {
                            return Ok::<_, Infallible>(body_error_to_response(err, &head));
                        }
                        Err(_elapsed) => {
                            let err = crate::primitives::request_body_error::RequestBodyError::ReadTimeout;
                            return Ok::<_, Infallible>(body_error_to_response(err, &head));
                        }
                    }
                }
                RequestBodyPolicy::Stream { .. } => request_body,
                _ => unreachable!(),
            };

            // Build connection info and Request envelope.
            let connection = build_connection_info(&parts);
            let request = crate::primitives::request::Request::new(head, request_body, connection);

            // Invoke the service with handler timeout.
            let result = tokio::time::timeout(handler_timeout, service.call(request)).await;

            let response = match result {
                Ok(Ok(canonical)) => {
                    match crate::primitives::canonical::to_hyper_response(canonical) {
                        Ok(r) => r,
                        Err(_) => crate::response::internal_error(),
                    }
                }
                Ok(Err(service_err)) => service_err.to_response(),
                Err(_elapsed) => {
                    ServiceError::timeout("handler timed out".to_string()).to_response()
                }
            };

            Ok::<_, Infallible>(response)
        }
    });

    serve_connection(io, hyper_service, config, shutdown_rx).await;
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
    let status = err.to_status_code();
    let status =
        hyper::StatusCode::from_u16(status).unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR);
    let should_close = matches!(
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
        501 => "501 Not Implemented\n",
        _ => "500 Internal Server Error\n",
    };
    let mut resp = crate::response::canonical_error(status, body_text);
    if should_close {
        resp.headers_mut().insert(
            hyper::header::CONNECTION,
            hyper::header::HeaderValue::from_static("close"),
        );
    }
    resp
}

/// Build ConnectionInfo from Hyper request parts.
fn build_connection_info(
    _parts: &hyper::http::request::Parts,
) -> crate::primitives::connection_info::ConnectionInfo {
    crate::primitives::connection_info::ConnectionInfo {
        local_addr: "127.0.0.1:0".parse().unwrap(),
        remote_addr: "127.0.0.1:0".parse().unwrap(),
        scheme: crate::primitives::connection_info::Scheme::Http,
        tls: None,
    }
}

/// Drain a request body, discarding all bytes.
async fn drain_body(mut body: crate::primitives::request_body::RequestBody) {
    while let Some(chunk) = body.next_chunk().await.transpose() {
        if chunk.is_err() {
            break;
        }
    }
}

/// Validate request policy at the runtime level.
///
/// This checks for transport-level correctness that the service should
/// never be responsible for:
/// - Methods that must not have a request body (GET, HEAD, OPTIONS, TRACE, DELETE)
///   must not carry Content-Length > 0 or Transfer-Encoding headers.
///
/// Returns `Ok(())` if the request passes validation, or `Err(ServiceError)`
/// with an appropriate HTTP status code.
fn validate_request_policy(
    head: &crate::primitives::request_head::RequestHead,
) -> Result<(), ServiceError> {
    let method = head.method().as_str();

    // These methods must not have a request body per RFC 9110 section 6.4.
    let body_forbidden = matches!(method, "GET" | "HEAD" | "OPTIONS" | "TRACE" | "DELETE");

    if body_forbidden {
        // Reject Transfer-Encoding — chunked is not supported.
        if head.headers().contains("transfer-encoding") {
            return Err(ServiceError::rejected(
                400,
                "transfer-encoding not allowed for this method",
            ));
        }

        // Reject Content-Length > 0.
        if let Some(content_length) = head.headers().get_first("content-length") {
            let len_str = content_length.as_str().trim();
            if let Ok(len) = len_str.parse::<u64>() {
                if len > 0 {
                    return Err(ServiceError::rejected(
                        400,
                        "request body not allowed for this method",
                    ));
                }
            }
        }
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
        other => Method::new(other).unwrap_or_else(|_| Method::get()),
    };

    let version = match req.version() {
        hyper::Version::HTTP_10 => HttpVersion::Http10,
        hyper::Version::HTTP_11 => HttpVersion::Http11,
        _ => HttpVersion::Http11,
    };

    let target = RequestTarget::parse(req.uri().path())
        .map_err(|e| ServiceError::rejected(400, format!("invalid request target: {}", e)))?;

    let mut headers = HeaderBlock::new();
    for (name, value) in req.headers().iter() {
        if let (Ok(n), Ok(v)) = (
            crate::primitives::header_block::HeaderName::new(name.as_str()),
            crate::primitives::header_block::HeaderValue::new(value.to_str().unwrap_or("")),
        ) {
            headers.push(n, v);
        }
    }

    Ok(crate::primitives::request_head::RequestHead::new(
        method, target, version, headers,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ServeConfig, ServeState};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn build_state(tmp: &TempDir) -> Arc<ServeState> {
        let config = Arc::new(ServeConfig {
            root: tmp.path().to_path_buf(),
            ..ServeConfig::default()
        });
        Arc::new(ServeState::new(config))
    }

    #[tokio::test]
    async fn serve_connection_handles_get() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
        let state = build_state(&tmp);
        let config = RuntimeConfig::default();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, _rx) = broadcast::channel::<()>(1);

        let state_clone = state.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            let mut shutdown_rx = tx.subscribe();
            let svc = service_fn(move |req: Request<Incoming>| {
                let state = state_clone.clone();
                async move { Ok::<_, Infallible>(crate::service::handle_request(req, &state).await) }
            });
            serve_connection(io, svc, &config, &mut shutdown_rx).await;
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
}
