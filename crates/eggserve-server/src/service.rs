//! Transport-independent service abstraction (Plan 215: converged with the
//! mature compatibility contract; Plan 216: additive tunnel entry point).
//!
//! A [`Service`] receives a canonical [`Request`](eggserve_primitives::Request)
//! and produces a canonical [`Response`](eggserve_primitives::Response). The
//! runtime owns transport, parsing, normalization, and timeout enforcement.
//! Services never see raw sockets or Hyper types.
//!
//! Tunnel-aware services implement [`Service::call_with_tunnel`] (or use
//! [`service_fn_with_tunnel`]) to receive the server-owned one-shot
//! [`TunnelCapability`](crate::tunnel::TunnelCapability) alongside the
//! request. Ordinary services keep implementing [`Service::call`]: the
//! default `call_with_tunnel` drops the capability and runs `call`, so
//! denial stays ordinary HTTP and existing services are source-compatible.
//!
//! # Example
//!
//! ```no_run
//! use eggserve_primitives::{Response, ResponseBody, StatusCode};
//! use eggserve_server::{service_fn, Request};
//! # fn main() {
//!
//! let service = service_fn(|_req: Request| async {
//!     Ok(Response::builder()
//!         .status(StatusCode::OK)
//!         .body(ResponseBody::Bytes(b"hello".to_vec()))
//!         .unwrap())
//! });
//! # }
//! ```
//!
//! `eggserve_core::server` re-exports this module's trait, error, and helpers
//! during the 0.x line; this crate is the single implementation authority for
//! the service contract (tunnel acceptance included). Hyper response
//! conversion for compatibility paths lives outside this definition (in the
//! runtime's response-finalization path), never as a second error
//! implementation.

use std::future::Future;
use std::pin::Pin;

use eggserve_primitives::canonical::Response;
use eggserve_primitives::request::Request;
use eggserve_primitives::request_body_error::RequestBodyError;
use eggserve_primitives::request_body_policy::RequestBodyPolicy;
use eggserve_primitives::StatusCode;

pub type ServiceFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Response, ServiceError>> + Send + 'a>>;

/// Errors produced by a service implementation.
///
/// The runtime converts these into appropriate HTTP responses without leaking
/// internal details. Services should use [`ServiceError::internal`] for
/// unexpected failures and [`ServiceError::rejected`] for intentional rejections
/// that should produce a specific status code.
///
/// Client-facing bodies stay sanitized (fixed `<status> <reason>` or empty);
/// committed-stream failures never synthesize a second HTTP error after
/// commitment.
#[derive(Debug)]
pub struct ServiceError {
    kind: ServiceErrorKind,
    message: String,
}

#[derive(Debug)]
enum ServiceErrorKind {
    /// An unexpected internal failure. Maps to 500.
    Internal,
    /// A deliberate rejection with a specific status code.
    Rejected(u16),
    /// The handler panicked. Maps to 500.
    #[allow(dead_code)]
    Panic,
    /// The handler timed out. Maps to 504.
    Timeout,
}

impl ServiceError {
    /// Create an internal error (500).
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: ServiceErrorKind::Internal,
            message: message.into(),
        }
    }

    /// Create a rejection with a specific status code.
    ///
    /// Status codes in `200..=599` are preserved. Anything else maps to 500:
    /// out-of-range codes would bypass the response status invariant, and
    /// `100..=199` interim statuses cannot be final error responses.
    /// Body-forbidden survivors (`204`/`205`/`304`) keep their status with an
    /// empty representation at response finalization.
    pub fn rejected(status: u16, message: impl Into<String>) -> Self {
        let status = if (200..=599).contains(&status) {
            status
        } else {
            500
        };
        Self {
            kind: ServiceErrorKind::Rejected(status),
            message: message.into(),
        }
    }

    #[allow(dead_code)]
    #[doc(hidden)]
    /// Handler-panic constructor (runtime-internal).
    ///
    /// Hidden: only runtimes containing a panicking service construct this;
    /// services never construct errors from panics.
    pub fn panic(message: impl Into<String>) -> Self {
        Self {
            kind: ServiceErrorKind::Panic,
            message: message.into(),
        }
    }

    #[doc(hidden)]
    /// Handler/body timeout constructor (runtime-internal).
    ///
    /// Hidden: only runtimes enforcing handler/body deadlines construct
    /// this; services use `internal`/`rejected`.
    pub fn timeout(message: impl Into<String>) -> Self {
        Self {
            kind: ServiceErrorKind::Timeout,
            message: message.into(),
        }
    }

    /// Returns the error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns `true` if this error was caused by a handler panic.
    pub fn is_panic(&self) -> bool {
        matches!(self.kind, ServiceErrorKind::Panic)
    }

    /// Returns `true` if this error was caused by a handler timeout.
    pub fn is_timeout(&self) -> bool {
        matches!(self.kind, ServiceErrorKind::Timeout)
    }

    /// Return the sanitized final status used by protocol adapters.
    pub fn status_code(&self) -> StatusCode {
        let status = match self.kind {
            ServiceErrorKind::Internal | ServiceErrorKind::Panic => 500,
            ServiceErrorKind::Rejected(status) => status,
            ServiceErrorKind::Timeout => 504,
        };
        StatusCode::new(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            ServiceErrorKind::Internal => write!(f, "internal error: {}", self.message),
            ServiceErrorKind::Rejected(code) => {
                write!(f, "rejected ({}): {}", code, self.message)
            }
            ServiceErrorKind::Panic => write!(f, "handler panicked: {}", self.message),
            ServiceErrorKind::Timeout => write!(f, "handler timeout: {}", self.message),
        }
    }
}

impl std::error::Error for ServiceError {}

impl From<RequestBodyError> for ServiceError {
    fn from(err: RequestBodyError) -> Self {
        let status = err.to_status_code();
        let message = match err {
            RequestBodyError::RejectedByPolicy => "request body rejected by policy",
            RequestBodyError::DeclaredLengthTooLarge { .. }
            | RequestBodyError::LimitExceeded { .. } => "request body exceeds configured limit",
            RequestBodyError::ReadTimeout => "request body read timed out",
            RequestBodyError::PrematureEof { .. } => {
                "request body ended before its declared length"
            }
            RequestBodyError::LengthMismatch { .. } => "request body length mismatch",
            RequestBodyError::InvalidChunkFraming(_) => "invalid request body framing",
            RequestBodyError::Cancelled => "request body consumption cancelled",
            RequestBodyError::Disconnected => "client disconnected while sending request body",
            RequestBodyError::AlreadyConsumed
            | RequestBodyError::MixedConsumptionMode
            | RequestBodyError::TrailersNotReady => "request body was already consumed",
            RequestBodyError::Transport(_) => "request body transport failure",
            RequestBodyError::InvalidTrailers(_) => "invalid request trailers",
            // Non-exhaustive upstream: future body-failure categories fail
            // closed as internal errors without leaking detail.
            _ => "request body failed",
        };
        ServiceError::rejected(status, message)
    }
}

/// A transport-independent service that handles HTTP requests.
///
/// Services are invoked by the runtime after request parsing and validation.
/// They receive a canonical [`Request`] (head, body, and typed
/// [`RequestContext`](eggserve_primitives::RequestContext)) and must return a
/// canonical [`Response`].
///
/// # Contract
///
/// - The service is called once per request.
/// - The service must not write to raw sockets or access transport internals.
/// - Panics raised during service execution are contained by the runtime and
///   produce a 500 response. Panics outside service execution are caught at
///   the task boundary and drop the connection.
/// - The response goes through runtime normalization before transport.
/// - Cancellation: peer disconnect, forced close, hard timeouts, shutdown
///   past drain, and body/transport failure cancel the request's lifecycle;
///   normal `Service::call` return, body EOF, or normal response completion
///   on keep-alive never cancel by themselves.
/// - Tunnel transitions: the runtime classifies validated H1 `Upgrade` /
///   `CONNECT` intent, records cloneable intent on the request context
///   (`RequestContext::tunnel_request`), and passes the one-shot
///   `TunnelCapability` via [`Service::call_with_tunnel`]. Ignoring the
///   capability is ordinary HTTP denial; accepting performs the handshake
///   and transfers transport ownership exactly once.
///
/// # Thread safety
///
/// Services must be `Send + Sync` to be shared across connection tasks.
/// Server-wide service admission (`max_in_flight_requests`, held across
/// `Service::call`) and downstream application-task admission are separate
/// concepts. There is no `poll_ready` on the native trait.
pub trait Service: Send + Sync + 'static {
    /// Returns the service's preferred request body policy.
    ///
    /// The runtime uses this to select the effective body policy before
    /// service invocation. The runtime enforces a hard global ceiling
    /// (`max_request_body_bytes`) that no service can exceed. Services
    /// may only lower the ceiling, not raise it.
    ///
    /// The default implementation returns `Reject` (no body accepted),
    /// which is the safe default for static file services.
    fn request_body_policy(
        &self,
        _head: &eggserve_primitives::request_head::RequestHead,
    ) -> RequestBodyPolicy {
        RequestBodyPolicy::Reject
    }

    /// Handle an HTTP request.
    ///
    /// Returns a future that resolves to a response or a service error.
    /// Tunnel-unaware path: the runtime calls
    /// [`call_with_tunnel`](Self::call_with_tunnel) with `None` (or drops
    /// the capability for services that only implement this method).
    fn call(&self, request: Request) -> ServiceFuture<'_>;

    /// Handle an HTTP request with an optional one-shot tunnel capability.
    ///
    /// The runtime always calls this entry point. `tunnel` is `Some` only
    /// when the request carried validated H1 `Upgrade`/`CONNECT` intent,
    /// carried no body, and the transport offered a handoff. The default
    /// implementation drops the capability and runs [`call`](Self::call),
    /// so ordinary services deny with ordinary HTTP and stay
    /// source-compatible. Tunnel-aware services override this method,
    /// inspect `capability.request()`, and either drop it (denial) or
    /// consume it via `TunnelCapability::accept`.
    fn call_with_tunnel(
        &self,
        request: Request,
        tunnel: Option<crate::tunnel::TunnelCapability>,
    ) -> ServiceFuture<'_> {
        let _ = tunnel;
        self.call(request)
    }
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

/// Create a service from a closure or async function.
///
/// # Example
///
/// ```no_run
/// use eggserve_primitives::{Response, ResponseBody, StatusCode};
/// use eggserve_server::{service_fn, Request};
/// # fn main() {
///
/// let service = service_fn(|_req: Request| async {
///     Ok(Response::builder()
///         .status(StatusCode::OK)
///         .body(ResponseBody::Bytes(b"hello".to_vec()))
///         .unwrap())
/// });
/// # }
/// ```
pub fn service_fn<F, Fut>(f: F) -> ServiceFn<F>
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, ServiceError>> + Send + 'static,
{
    ServiceFn {
        f,
        body_policy: None,
    }
}

/// Create a service from a closure that only receives the request head,
/// discarding the body. The service uses `Reject` body policy.
pub fn service_fn_head<F, Fut>(f: F) -> ServiceFn<impl Fn(Request) -> Fut + Send + Sync + 'static>
where
    F: Fn(eggserve_primitives::request_head::RequestHead) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, ServiceError>> + Send + 'static,
{
    ServiceFn {
        f: move |req: Request| {
            let (head, _body) = req.into_head_and_body();
            f(head)
        },
        body_policy: Some(RequestBodyPolicy::Reject),
    }
}

/// Create a service from a closure with an explicit body policy.
pub fn service_fn_with_policy<F, Fut>(f: F, policy: RequestBodyPolicy) -> ServiceFn<F>
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, ServiceError>> + Send + 'static,
{
    ServiceFn {
        f,
        body_policy: Some(policy),
    }
}

/// Create a tunnel-aware service from a closure.
///
/// The closure receives the canonical [`Request`] plus the optional one-shot
/// [`TunnelCapability`](crate::tunnel::TunnelCapability). `None` means an
/// ordinary request (or a transition the runtime could not back with a
/// transport handoff); dropping a `Some` capability denies with ordinary
/// HTTP. Consuming it via `accept` performs the handshake.
pub fn service_fn_with_tunnel<F, Fut>(f: F) -> TunnelServiceFn<F>
where
    F: Fn(Request, Option<crate::tunnel::TunnelCapability>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, ServiceError>> + Send + 'static,
{
    TunnelServiceFn { f }
}

/// A tunnel-aware service created via [`service_fn_with_tunnel`].
pub struct TunnelServiceFn<F> {
    f: F,
}

impl<F, Fut> Service for TunnelServiceFn<F>
where
    F: Fn(Request, Option<crate::tunnel::TunnelCapability>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, ServiceError>> + Send + 'static,
{
    fn call(&self, request: Request) -> ServiceFuture<'_> {
        Box::pin((self.f)(request, None))
    }

    fn call_with_tunnel(
        &self,
        request: Request,
        tunnel: Option<crate::tunnel::TunnelCapability>,
    ) -> ServiceFuture<'_> {
        Box::pin((self.f)(request, tunnel))
    }
}

/// A service created from a closure via [`service_fn`].
pub struct ServiceFn<F> {
    f: F,
    body_policy: Option<RequestBodyPolicy>,
}

impl<F, Fut> Service for ServiceFn<F>
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, ServiceError>> + Send + 'static,
{
    fn request_body_policy(
        &self,
        _head: &eggserve_primitives::request_head::RequestHead,
    ) -> RequestBodyPolicy {
        self.body_policy.unwrap_or(RequestBodyPolicy::Reject)
    }

    fn call(&self, request: Request) -> ServiceFuture<'_> {
        Box::pin((self.f)(request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggserve_primitives::canonical::ResponseBody as CanonicalResponseBody;
    use eggserve_primitives::canonical::StatusCode as CanonicalStatusCode;
    use eggserve_primitives::connection_info::{ConnectionInfo, Scheme};
    use eggserve_primitives::header_block::HeaderBlock;
    use eggserve_primitives::request_body::RequestBody;
    use std::net::SocketAddr;

    fn make_test_request(path: &str) -> Request {
        Request::new(
            eggserve_primitives::request_head::RequestHead::new(
                eggserve_primitives::method::Method::get(),
                eggserve_primitives::request_target::RequestTarget::parse(path).unwrap(),
                eggserve_primitives::version::HttpVersion::Http11,
                HeaderBlock::new(),
            ),
            RequestBody::empty(),
            ConnectionInfo::with_socket_addrs(
                "127.0.0.1:8000".parse::<SocketAddr>().unwrap(),
                "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
                Scheme::Http,
                None,
            ),
        )
    }

    #[tokio::test]
    async fn service_fn_calls_handler() {
        let svc = service_fn(|_req: Request| async {
            Ok(Response::builder()
                .status(CanonicalStatusCode::OK)
                .body(CanonicalResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        });
        let req = make_test_request("/test");
        let resp = svc.call(req).await.unwrap();
        assert_eq!(resp.status(), CanonicalStatusCode::OK);
    }

    #[tokio::test]
    async fn custom_service_returns_bytes() {
        struct ByteService;
        impl Service for ByteService {
            fn call(&self, _req: Request) -> ServiceFuture<'_> {
                Box::pin(async {
                    Ok(Response::builder()
                        .status(CanonicalStatusCode::OK)
                        .body(CanonicalResponseBody::Bytes(b"custom bytes".to_vec()))
                        .unwrap())
                })
            }
        }
        let svc = ByteService;
        let req = make_test_request("/test");
        let resp = svc.call(req).await.unwrap();
        assert_eq!(resp.status(), CanonicalStatusCode::OK);
    }

    #[test]
    fn service_error_status_codes() {
        assert_eq!(
            ServiceError::internal("oops").status_code(),
            CanonicalStatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            ServiceError::panic("crashed").status_code(),
            CanonicalStatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(ServiceError::timeout("slow").status_code().as_u16(), 504);
        assert_eq!(
            ServiceError::rejected(404, "miss").status_code().as_u16(),
            404
        );
        assert_eq!(
            ServiceError::rejected(429, "busy").status_code().as_u16(),
            429
        );
        // Out-of-range and interim statuses collapse to 500.
        for bad in [99u16, 100, 199, 600, 999] {
            assert_eq!(
                ServiceError::rejected(bad, "weird").status_code(),
                CanonicalStatusCode::INTERNAL_SERVER_ERROR,
                "status {bad} must collapse to 500"
            );
        }
    }

    #[test]
    fn service_error_display() {
        let err = ServiceError::internal("something broke");
        assert!(err.to_string().contains("something broke"));
        assert!(!err.is_panic());
        assert!(!err.is_timeout());

        let err = ServiceError::rejected(404, "not found");
        assert!(err.to_string().contains("404"));

        let err = ServiceError::panic("oops");
        assert!(err.is_panic());

        let err = ServiceError::timeout("too slow");
        assert!(err.is_timeout());
    }

    #[test]
    fn service_fn_implements_service() {
        let svc = service_fn(|_req: Request| async {
            Ok(Response::builder()
                .status(CanonicalStatusCode::OK)
                .body(CanonicalResponseBody::Empty)
                .unwrap())
        });
        fn assert_service<S: Service>(_svc: &S) {}
        assert_service(&svc);
    }

    #[tokio::test]
    async fn service_fn_with_captured_state() {
        let greeting = "hello";
        let svc = service_fn(move |_req: Request| {
            let greeting = greeting.to_string();
            async move {
                Ok(Response::builder()
                    .status(CanonicalStatusCode::OK)
                    .body(CanonicalResponseBody::Bytes(greeting.into_bytes()))
                    .unwrap())
            }
        });
        let req = make_test_request("/test");
        let resp = svc.call(req).await.unwrap();
        assert_eq!(resp.status(), CanonicalStatusCode::OK);
    }

    #[tokio::test]
    async fn service_fn_with_policy_keeps_body_policy() {
        use eggserve_primitives::RequestBodyPolicy;
        let svc = service_fn_with_policy(
            |_req: Request| async {
                Ok(Response::builder()
                    .status(CanonicalStatusCode::OK)
                    .body(CanonicalResponseBody::Empty)
                    .unwrap())
            },
            RequestBodyPolicy::Reject,
        );
        let req = make_test_request("/test");
        assert!(svc.request_body_policy(req.head()).is_reject());
    }
}
