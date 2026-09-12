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
    HeaderBlock, HttpVersion, Method, Request, RequestBody, RequestHead, RequestTarget, Response,
    ResponseBody, StatusCode,
};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn as hyper_service_fn;
use hyper_util::rt::TokioIo;
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, Semaphore};

pub type ServiceFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Response, ServiceError>> + Send + 'a>>;

/// Application service invoked after transport parsing and bounded body read.
pub trait Service: Send + Sync + 'static {
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

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("service failed: {0}")]
    Internal(String),
    #[error("service rejected request with {0}")]
    Rejected(u16),
}
impl ServiceError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
    pub fn rejected(status: u16) -> Self {
        Self::Rejected(status)
    }
}

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
            .map_err(|e| ServerError::InvalidConfig(e.into()))
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

async fn dispatch(
    request: hyper::Request<Incoming>,
    service: Arc<dyn Service>,
    config: RuntimeConfig,
) -> hyper::Response<Full<bytes::Bytes>> {
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
        if headers.push_str(name.as_str(), value.as_bytes()).is_err() {
            return error_response(StatusCode::BAD_REQUEST);
        }
    }
    let collected =
        match tokio::time::timeout(config.request_timeout, request.into_body().collect()).await {
            Ok(Ok(body)) => body.to_bytes(),
            _ => return error_response(StatusCode::REQUEST_TIMEOUT),
        };
    if collected.len() as u64 > config.limits.max_request_body_bytes {
        return error_response(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let request = Request {
        head: RequestHead {
            method,
            target,
            version: HttpVersion::Http11,
            headers,
        },
        body: RequestBody::new(collected.to_vec()),
    };
    let response = match service.call(request).await {
        Ok(mut response) => {
            response.normalize();
            response
        }
        Err(ServiceError::Rejected(code)) => StatusCode::new(code).map_or_else(
            |_| error_canonical(StatusCode::INTERNAL_SERVER_ERROR),
            error_canonical,
        ),
        Err(ServiceError::Internal(_)) => error_canonical(StatusCode::INTERNAL_SERVER_ERROR),
    };
    into_hyper_response(response)
}

fn error_response(status: StatusCode) -> hyper::Response<Full<bytes::Bytes>> {
    into_hyper_response(Response::new(status, ResponseBody::Empty))
}
fn error_canonical(status: StatusCode) -> Response {
    Response::new(status, ResponseBody::Empty)
}
fn into_hyper_response(mut response: Response) -> hyper::Response<Full<bytes::Bytes>> {
    response.normalize();
    let mut builder = hyper::Response::builder().status(response.status.as_u16());
    for header in response.headers.iter() {
        builder = builder.header(header.name(), header.value());
    }
    let body = match response.body {
        ResponseBody::Empty => bytes::Bytes::new(),
        ResponseBody::Bytes(bytes) => bytes::Bytes::from(bytes),
    };
    builder
        .body(Full::new(body))
        .unwrap_or_else(|_| hyper::Response::new(Full::new(bytes::Bytes::new())))
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
