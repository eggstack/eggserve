//! Generic EggServe HTTP runtime.
//!
//! This crate owns transport and lifecycle machinery only. It deliberately
//! has no filesystem, MIME, or static-policy dependency; applications provide
//! a [`Service`] and may add `eggserve-static` separately.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use eggserve_primitives::{
    normalize_response, ConnectionInfo, HeaderBlock, HttpVersion, Method, NormalizeRequest,
    Request, RequestBody, RequestBodyPolicy, RequestHead, RequestTarget, Response, ResponseBody,
    ResponseStream, ResponseStreamError, Scheme, StatusCode,
};
use futures_util::{Stream, StreamExt};
use http_body::{Body as HttpBody, Frame, SizeHint};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn as hyper_service_fn;
use hyper_util::rt::TokioIo;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, Semaphore};

pub type ServiceFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Response, ServiceError>> + Send + 'a>>;

/// Application service invoked after transport parsing.
pub trait Service: Send + Sync + 'static {
    fn request_body_policy(&self, _head: &RequestHead) -> RequestBodyPolicy {
        RequestBodyPolicy::Reject
    }

    fn call(&self, request: Request) -> ServiceFuture<'_>;
}

impl<F, Fut> Service for F
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, ServiceError>> + Send + 'static,
{
    fn call(&self, request: Request) -> ServiceFuture<'_> {
        Box::pin((self)(request))
    }
}

pub fn service_fn<F, Fut>(f: F) -> F
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, ServiceError>> + Send + 'static,
{
    f
}

#[derive(Debug)]
pub struct ServiceError {
    kind: ServiceErrorKind,
    message: String,
}

#[derive(Debug)]
enum ServiceErrorKind {
    Internal,
    Rejected(u16),
    Timeout,
}
impl ServiceError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: ServiceErrorKind::Internal,
            message: message.into(),
        }
    }
    pub fn rejected(status: u16) -> Self {
        let status = if (200..=599).contains(&status) {
            status
        } else {
            500
        };
        Self {
            kind: ServiceErrorKind::Rejected(status),
            message: String::new(),
        }
    }
    fn timeout(message: impl Into<String>) -> Self {
        Self {
            kind: ServiceErrorKind::Timeout,
            message: message.into(),
        }
    }
    fn status_code(&self) -> StatusCode {
        let status = match self.kind {
            ServiceErrorKind::Internal => 500,
            ServiceErrorKind::Rejected(status) => status,
            ServiceErrorKind::Timeout => 504,
        };
        StatusCode::new(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            ServiceErrorKind::Internal => write!(f, "service failed: {}", self.message),
            ServiceErrorKind::Rejected(status) => {
                write!(f, "service rejected request with {status}")
            }
            ServiceErrorKind::Timeout => write!(f, "service timed out: {}", self.message),
        }
    }
}

impl std::error::Error for ServiceError {}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub bind: SocketAddr,
    pub max_connections: usize,
    pub request_timeout: Duration,
    pub limits: eggserve_primitives::Limits,
}
impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8000".parse().unwrap(),
            max_connections: 64,
            request_timeout: Duration::from_secs(30),
            limits: Default::default(),
        }
    }
}
impl RuntimeConfig {
    pub fn validate(&self) -> Result<(), ServerError> {
        if self.max_connections == 0 {
            return Err(ServerError::InvalidConfig(
                "max_connections must be non-zero".into(),
            ));
        }
        self.limits
            .validate()
            .map_err(|e| ServerError::InvalidConfig(e.to_string()))
    }
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("invalid runtime configuration: {0}")]
    InvalidConfig(String),
    #[error("listener failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP connection failed: {0}")]
    Protocol(String),
}

#[derive(Debug, Default)]
pub struct ServerBuilder {
    config: RuntimeConfig,
}
impl ServerBuilder {
    pub fn runtime(mut self, config: RuntimeConfig) -> Self {
        self.config = config;
        self
    }
    pub fn bind(mut self, bind: SocketAddr) -> Self {
        self.config.bind = bind;
        self
    }
    pub fn build(self) -> Result<Server, ServerError> {
        self.config.validate()?;
        Ok(Server {
            config: self.config,
        })
    }
}

pub struct Server {
    config: RuntimeConfig,
}
impl Server {
    pub fn builder() -> ServerBuilder {
        ServerBuilder {
            config: RuntimeConfig::default(),
        }
    }
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }
    pub async fn start_with_service<S>(&self, service: S) -> Result<ServerHandle, ServerError>
    where
        S: Service,
    {
        self.config.validate()?;
        let listener = TcpListener::bind(self.config.bind).await?;
        let local_addr = listener.local_addr()?;
        let shutdown = Arc::new(Notify::new());
        let permits = Arc::new(Semaphore::new(self.config.max_connections));
        let service: Arc<dyn Service> = Arc::new(service);
        let task_shutdown = shutdown.clone();
        let task_config = self.config.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = task_shutdown.notified() => break,
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break };
                        let Ok(permit) = permits.clone().try_acquire_owned() else { continue };
                        let service = service.clone();
                        let config = task_config.clone();
                        tokio::spawn(async move { let _permit = permit; let _ = serve_connection(stream, service, config).await; });
                    }
                }
            }
        });
        Ok(ServerHandle {
            local_addr,
            shutdown,
        })
    }
}

pub struct ServerHandle {
    local_addr: SocketAddr,
    shutdown: Arc<Notify>,
}
impl ServerHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
    pub fn shutdown(&self) {
        self.shutdown.notify_waiters();
    }
}

async fn serve_connection(
    stream: TcpStream,
    service: Arc<dyn Service>,
    config: RuntimeConfig,
) -> Result<(), ServerError> {
    let io = TokioIo::new(stream);
    let svc = hyper_service_fn(move |request: hyper::Request<Incoming>| {
        let service = service.clone();
        let config = config.clone();
        async move { Ok::<_, std::convert::Infallible>(dispatch(request, service, config).await) }
    });
    hyper::server::conn::http1::Builder::new()
        .keep_alive(true)
        .serve_connection(io, svc)
        .await
        .map_err(|error| ServerError::Protocol(error.to_string()))
}

type HyperBody = http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, std::io::Error>;

struct ResponseStreamBody {
    stream: eggserve_primitives::ResponseStream,
}

impl HttpBody for ResponseStreamBody {
    type Data = bytes::Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match std::pin::Pin::new(&mut self.stream).poll_next(cx) {
            std::task::Poll::Ready(Some(Ok(data))) => {
                std::task::Poll::Ready(Some(Ok(Frame::data(data))))
            }
            std::task::Poll::Ready(Some(Err(_))) => {
                std::task::Poll::Ready(Some(Err(std::io::Error::other("response stream failed"))))
            }
            std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        false
    }

    fn size_hint(&self) -> SizeHint {
        let mut hint = SizeHint::new();
        if let Some(length) = self.stream.known_length() {
            hint.set_exact(length);
        }
        hint
    }
}

async fn dispatch(
    request: hyper::Request<Incoming>,
    service: Arc<dyn Service>,
    config: RuntimeConfig,
) -> hyper::Response<HyperBody> {
    let target = request
        .uri()
        .path_and_query()
        .map_or_else(|| "/".to_owned(), |v| v.as_str().to_owned());
    let method = match Method::new(request.method().as_str()) {
        Ok(v) => v,
        Err(_) => return error_response(StatusCode::BAD_REQUEST),
    };
    let target = match RequestTarget::parse(target) {
        Ok(v) => v,
        Err(_) => return error_response(StatusCode::BAD_REQUEST),
    };
    let mut headers = HeaderBlock::new();
    for (name, value) in request.headers() {
        let name = match eggserve_primitives::HeaderName::new(name.as_str()) {
            Ok(name) => name,
            Err(_) => return error_response(StatusCode::BAD_REQUEST),
        };
        let value = match eggserve_primitives::HeaderValue::from_bytes(value.as_bytes()) {
            Ok(value) => value,
            Err(_) => return error_response(StatusCode::BAD_REQUEST),
        };
        headers.push(name, value);
    }

    let declared_length = request
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let has_transfer_coded_body = request.headers().contains_key("transfer-encoding");
    let incoming = request.into_body().into_data_stream().map(|item| {
        item.map_err(|error| eggserve_primitives::request_body::IncomingError(error.to_string()))
    });
    let body = RequestBody::from_incoming(
        incoming,
        declared_length,
        config.limits.max_request_body_bytes,
    );
    let request = Request::new(
        RequestHead::new(method, target, HttpVersion::Http11, headers),
        body,
        ConnectionInfo::without_socket_addrs(Scheme::Http, None),
    );
    let is_head = request.head().is_head();
    if service.request_body_policy(request.head()).is_reject()
        && (declared_length.unwrap_or(0) != 0 || has_transfer_coded_body)
    {
        return error_response(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let response = match tokio::time::timeout(config.request_timeout, service.call(request)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => error_canonical(error.status_code()),
        Err(_) => error_canonical(ServiceError::timeout("request timeout").status_code()),
    };
    into_hyper_response(response, is_head)
}

fn error_response(status: StatusCode) -> hyper::Response<HyperBody> {
    into_hyper_response(error_canonical(status), false)
}
fn error_canonical(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .empty()
        .expect("valid status")
}
fn into_hyper_response(mut response: Response, is_head: bool) -> hyper::Response<HyperBody> {
    response = normalize_response(response, &NormalizeRequest::new(is_head))
        .unwrap_or_else(|_| error_canonical(StatusCode::INTERNAL_SERVER_ERROR));
    let status = response.status().as_u16();
    let mut builder = hyper::Response::builder().status(status);
    for header in response.headers().iter() {
        if let Ok(value) = hyper::header::HeaderValue::from_bytes(header.value.as_bytes()) {
            builder = builder.header(header.name.as_str(), value);
        }
    }
    let body: HyperBody = match response.take_body().unwrap_or(ResponseBody::Empty) {
        ResponseBody::Empty | ResponseBody::EmptyWithLength(_) => Full::new(bytes::Bytes::new())
            .map_err(|never| match never {})
            .boxed_unsync(),
        ResponseBody::Bytes(bytes) => Full::new(bytes::Bytes::from(bytes))
            .map_err(|never| match never {})
            .boxed_unsync(),
        ResponseBody::Stream(stream) => ResponseStreamBody { stream }.boxed_unsync(),
        ResponseBody::File(source) => match file_body(source) {
            Ok(stream) => ResponseStreamBody { stream }.boxed_unsync(),
            Err(_) => Full::new(bytes::Bytes::new())
                .map_err(|never| match never {})
                .boxed_unsync(),
        },
    };
    builder.body(body).unwrap_or_else(|_| {
        hyper::Response::new(
            Full::new(bytes::Bytes::new())
                .map_err(|never| match never {})
                .boxed_unsync(),
        )
    })
}

/// Convert an already-opened static file capability into a bounded async
/// response stream. The path is never retained or reopened by the runtime.
fn file_body(
    source: eggserve_primitives::BodySource,
) -> Result<ResponseStream, ResponseStreamError> {
    let (file, start, remaining) = match source {
        eggserve_primitives::BodySource::FileFull { file, len, .. } => (file, 0, len),
        eggserve_primitives::BodySource::FileRange { file, range, .. } => {
            (file, range.start(), range.len())
        }
        _ => return Err(ResponseStreamError::new("response body is not file-backed")),
    };
    let stream = futures_util::stream::unfold(
        (tokio::fs::File::from_std(file), start, remaining, false),
        |(mut file, start, remaining, failed)| async move {
            if failed || remaining == 0 {
                return None;
            }
            if start != 0 {
                if let Err(error) = file.seek(std::io::SeekFrom::Start(start)).await {
                    return Some((Err(ResponseStreamError::from(error)), (file, 0, 0, true)));
                }
            }
            let capacity = remaining.min(64 * 1024) as usize;
            let mut buffer = vec![0; capacity];
            match file.read(&mut buffer).await {
                Ok(0) => Some((
                    Err(ResponseStreamError::new("file ended before content length")),
                    (file, 0, 0, true),
                )),
                Ok(read) => {
                    buffer.truncate(read);
                    Some((
                        Ok(bytes::Bytes::from(buffer)),
                        (file, 0, remaining - read as u64, false),
                    ))
                }
                Err(error) => Some((Err(ResponseStreamError::from(error)), (file, 0, 0, true))),
            }
        },
    );
    Ok(ResponseStream::with_known_length(stream, remaining))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generic_runtime_has_no_static_configuration() {
        let config = RuntimeConfig::default();
        assert!(config.bind.ip().is_loopback());
    }
}
