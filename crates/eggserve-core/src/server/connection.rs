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
//! 4. Service invocation with panic containment
//! 5. Canonical response normalization
//! 6. Transport-body conversion
//! 7. Write timeout enforcement
//! 8. Permit release and connection termination

use std::convert::Infallible;

use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::sync::broadcast;

use crate::config::ServeState;
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
                }
            }
        }
        _ = shutdown_rx.recv() => {
            conn.as_mut().graceful_shutdown();
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

            // Invoke the service with timeout.
            let result = tokio::time::timeout(handler_timeout, service.call(head)).await;

            let response = match result {
                Ok(Ok(canonical)) => {
                    // Normal response — convert to hyper.
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
