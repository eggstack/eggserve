//! Transport-independent service abstraction.
//!
//! A [`Service`] receives a canonical eggserve [`Request`] and produces a
//! canonical [`Response`]. The runtime owns transport, parsing, normalization,
//! and timeout enforcement. Services never see raw sockets or Hyper types.
//!
//! # Example
//!
//! ```no_run
//! use eggserve_core::primitives::{Response, ResponseBody, StatusCode};
//! use eggserve_core::server::{service_fn, Request};
//! # fn main() {
//!
//! let service = service_fn(|_req| async {
//!     Ok(Response::builder()
//!         .status(StatusCode::OK)
//!         .body(ResponseBody::Bytes(b"hello".to_vec()))
//!         .unwrap())
//! });
//! # }
//! ```

use std::future::Future;
use std::pin::Pin;

use crate::primitives::canonical::Response;
use crate::primitives::request::Request;
use crate::primitives::request_body_error::RequestBodyError;
use crate::primitives::request_body_policy::RequestBodyPolicy;

/// Errors produced by a service implementation.
///
/// The runtime converts these into appropriate HTTP responses without leaking
/// internal details. Services should use [`ServiceError::internal`] for
/// unexpected failures and [`ServiceError::rejected`] for intentional rejections
/// that should produce a specific status code.
///
/// This is a struct with a private kind (Plan 197 Track F): callers
/// distinguish rejection / internal / panic / timeout via
/// [`ServiceError::message`], [`ServiceError::is_panic`], and
/// [`ServiceError::is_timeout`], plus the transport's status/body mapping.
/// Future categories can be added without breaking construction; matching on
/// the kind is intentionally impossible. Client-facing bodies stay sanitized
/// (fixed `<status> <reason>` or empty); committed-stream failures never
/// synthesize a second HTTP error after commitment.
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
    /// `100..=199` interim statuses cannot be final error responses
    /// (upgrade/`101` remains unsupported per the deferred upgrade plan, and
    /// interim responses have no final representation). Body-forbidden
    /// survivors (`204`/`205`/`304`) keep their status with an empty
    /// representation; see [`ServiceError::to_response_with_head_and_policy`].
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
    pub(crate) fn panic(message: impl Into<String>) -> Self {
        Self {
            kind: ServiceErrorKind::Panic,
            message: message.into(),
        }
    }

    pub(crate) fn timeout(message: impl Into<String>) -> Self {
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
    #[cfg(feature = "http3")]
    pub(crate) fn status_code(&self) -> u16 {
        match self.kind {
            ServiceErrorKind::Internal | ServiceErrorKind::Panic => 500,
            ServiceErrorKind::Rejected(status) => status,
            ServiceErrorKind::Timeout => 504,
        }
    }

    /// Convert this error into an HTTP response.
    ///
    /// Internal and panic errors map to 500. Timeout errors map to 504.
    /// Rejected errors use the provided status code. No internal details
    /// are included in the response body.
    #[allow(dead_code)]
    pub(crate) fn to_response(&self) -> hyper::Response<crate::response::BoxBodyInner> {
        self.to_response_with_head(false)
    }

    /// Convert this error into an HTTP response, suppressing the body for `HEAD`.
    #[allow(dead_code)]
    pub(crate) fn to_response_with_head(
        &self,
        is_head: bool,
    ) -> hyper::Response<crate::response::BoxBodyInner> {
        self.to_response_with_head_and_policy(
            is_head,
            crate::policy::ErrorRepresentationPolicy::Minimal,
        )
    }

    /// Convert this error with an explicit representation policy.
    ///
    /// Status selection lives here; representation is owned by
    /// [`crate::response::runtime_error_with_policy`] so wire status and body
    /// can never disagree. `Empty` emits no body bytes for runtime-generated
    /// errors; application `Ok` bodies are never routed here. `HEAD`
    /// suppression remains correct (no body bytes). Body-forbidden statuses
    /// emit no bytes. No application message is reflected to clients.
    pub(crate) fn to_response_with_head_and_policy(
        &self,
        is_head: bool,
        policy: crate::policy::ErrorRepresentationPolicy,
    ) -> hyper::Response<crate::response::BoxBodyInner> {
        let status = match self.kind {
            ServiceErrorKind::Internal | ServiceErrorKind::Panic => {
                hyper::StatusCode::INTERNAL_SERVER_ERROR
            }
            ServiceErrorKind::Rejected(code) => hyper::StatusCode::from_u16(code)
                .unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR),
            ServiceErrorKind::Timeout => hyper::StatusCode::GATEWAY_TIMEOUT,
        };
        crate::response::runtime_error_with_policy(status, is_head, policy)
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
        };
        ServiceError::rejected(status, message)
    }
}

/// A transport-independent service that handles HTTP requests.
///
/// Services are invoked by the runtime after request parsing and validation.
/// They receive a canonical [`Request`] (head, body, and typed
/// [`RequestContext`](crate::primitives::RequestContext)) and must return a
/// canonical [`Response`]. Plan 197 deliberately keeps this shape:
/// no `ServiceOutcome` exists — trailers belong to the message-body
/// abstraction (Plan 198), interim responses use a request-scoped
/// capability (Plan 198), and any accepted-tunnel outcome is deferred to
/// Plan 199 only if pairing a continuation with a final response cannot be
/// made type-safe otherwise.
///
/// # Contract
///
/// - The service is called once per request.
/// - The service must not write to raw sockets or access transport internals.
/// - Panics raised during service execution are contained by the runtime and
///   produce a 500 response (see `ServiceError::panic`). Panics outside
///   service execution are caught at the task boundary and drop the
///   connection.
/// - The response goes through runtime normalization (hop-by-hop stripping,
///   content-length computation) before transport.
/// - Commitment: the final response head commits once the service returns
///   `Ok(Response)` and the runtime normalizes it. There is never a second
///   HTTP error response after commitment; producer/body failures after
///   commitment close the transport (H1) or reset the stream (H3) with
///   sanitized diagnostics only. See `docs/downstream-app-server.md` for the
///   normative 7-stage commitment/cancellation contract.
/// - Cancellation: peer disconnect, forced close, hard timeouts, shutdown
///   past drain, and body/transport failure cancel the request's
///   [`RequestLifecycle`](crate::primitives::request_lifecycle::RequestLifecycle);
///   normal `Service::call` return, body EOF, or normal response completion
///   on keep-alive never cancel by themselves.
///
/// # Thread safety
///
/// Services must be `Send + Sync` to be shared across connection tasks
/// (Plan 197 Track E). Server-wide service admission
/// (`max_in_flight_requests`, held across `Service::call`) and downstream
/// application-task admission are separate concepts; downstream tasks own a
/// separate bounded budget. There is no `poll_ready` on the native trait —
/// Tower readiness belongs in the Plan 200 adapters; native admission stays
/// runtime-owned and deterministic.
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
        _head: &crate::primitives::request_head::RequestHead,
    ) -> RequestBodyPolicy {
        RequestBodyPolicy::Reject
    }

    /// Handle an HTTP request.
    ///
    /// Returns a future that resolves to a response or a service error.
    fn call(
        &self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response, ServiceError>> + Send + '_>>;
}

/// Create a service from a closure or async function.
///
/// # Example
///
/// ```no_run
/// use eggserve_core::primitives::{Response, ResponseBody, StatusCode};
/// use eggserve_core::server::{service_fn, Request};
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
    F: Fn(crate::primitives::request_head::RequestHead) -> Fut + Send + Sync + 'static,
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
        _head: &crate::primitives::request_head::RequestHead,
    ) -> RequestBodyPolicy {
        self.body_policy.unwrap_or(RequestBodyPolicy::Reject)
    }

    fn call(
        &self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response, ServiceError>> + Send + '_>> {
        Box::pin((self.f)(request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::canonical::ResponseBody as CanonicalResponseBody;
    use crate::primitives::canonical::StatusCode;
    use crate::primitives::connection_info::{ConnectionInfo, Scheme};
    use crate::primitives::header_block::HeaderBlock;
    use crate::primitives::request_body::RequestBody;
    use std::net::SocketAddr;

    fn make_test_request(path: &str) -> Request {
        Request::new(
            crate::primitives::request_head::RequestHead::new(
                crate::primitives::method::Method::get(),
                crate::primitives::request_target::RequestTarget::parse(path).unwrap(),
                crate::primitives::version::HttpVersion::Http11,
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
                .status(StatusCode::OK)
                .body(CanonicalResponseBody::Bytes(b"ok".to_vec()))
                .unwrap())
        });
        let req = make_test_request("/test");
        let resp = svc.call(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn custom_service_returns_bytes() {
        struct ByteService;
        impl Service for ByteService {
            fn call(
                &self,
                _req: Request,
            ) -> Pin<Box<dyn Future<Output = Result<Response, ServiceError>> + Send + '_>>
            {
                Box::pin(async {
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(CanonicalResponseBody::Bytes(b"custom bytes".to_vec()))
                        .unwrap())
                })
            }
        }
        let svc = ByteService;
        let req = make_test_request("/test");
        let resp = svc.call(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn service_error_response_codes() {
        // Internal → 500
        let err = ServiceError::internal("oops");
        assert_eq!(
            err.to_response().status(),
            hyper::StatusCode::INTERNAL_SERVER_ERROR
        );

        // Panic → 500
        let err = ServiceError::panic("crashed");
        assert_eq!(
            err.to_response().status(),
            hyper::StatusCode::INTERNAL_SERVER_ERROR
        );

        // Timeout → 504
        let err = ServiceError::timeout("slow");
        assert_eq!(
            err.to_response().status(),
            hyper::StatusCode::GATEWAY_TIMEOUT
        );

        // Rejected(400) → 400
        let err = ServiceError::rejected(400, "bad");
        assert_eq!(err.to_response().status(), hyper::StatusCode::BAD_REQUEST);

        // Rejected(403) → 403
        let err = ServiceError::rejected(403, "no");
        assert_eq!(err.to_response().status(), hyper::StatusCode::FORBIDDEN);

        // Rejected(404) → 404
        let err = ServiceError::rejected(404, "miss");
        assert_eq!(err.to_response().status(), hyper::StatusCode::NOT_FOUND);

        // Rejected(405) → 405
        let err = ServiceError::rejected(405, "nope");
        assert_eq!(
            err.to_response().status(),
            hyper::StatusCode::METHOD_NOT_ALLOWED
        );

        // Rejected(503) → 503
        let err = ServiceError::rejected(503, "busy");
        assert_eq!(
            err.to_response().status(),
            hyper::StatusCode::SERVICE_UNAVAILABLE
        );

        // Invalid rejection statuses collapse to the canonical internal error.
        let err = ServiceError::rejected(999, "weird");
        assert_eq!(
            err.to_response().status(),
            hyper::StatusCode::INTERNAL_SERVER_ERROR
        );

        let err = ServiceError::rejected(600, "too high");
        assert_eq!(
            err.to_response().status(),
            hyper::StatusCode::INTERNAL_SERVER_ERROR
        );

        let err = ServiceError::rejected(99, "too low");
        assert_eq!(
            err.to_response().status(),
            hyper::StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[tokio::test]
    async fn service_fn_with_captured_state() {
        let greeting = "hello";
        let svc = service_fn(move |_req: Request| {
            let greeting = greeting.to_string();
            async move {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(CanonicalResponseBody::Bytes(greeting.into_bytes()))
                    .unwrap())
            }
        });
        let req = make_test_request("/test");
        let resp = svc.call(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn service_fn_implements_service() {
        let svc = service_fn(|_req: Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(CanonicalResponseBody::Empty)
                .unwrap())
        });
        fn assert_service<S: Service>(_svc: &S) {}
        assert_service(&svc);
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
    fn service_error_to_response() {
        let err = ServiceError::panic("oops");
        let resp = err.to_response();
        assert_eq!(resp.status(), hyper::StatusCode::INTERNAL_SERVER_ERROR);

        let err = ServiceError::timeout("slow");
        let resp = err.to_response();
        assert_eq!(resp.status(), hyper::StatusCode::GATEWAY_TIMEOUT);

        let err = ServiceError::rejected(404, "nope");
        let resp = err.to_response();
        assert_eq!(resp.status(), hyper::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn rejected_uncommon_status_has_truthful_body() {
        use http_body_util::BodyExt;

        // 429 was previously unlisted and fell back to a 500 body while
        // keeping wire status 429. It must now be truthful.
        let err = ServiceError::rejected(429, "secret app detail");
        let resp = err.to_response_with_head_and_policy(
            false,
            crate::policy::ErrorRepresentationPolicy::Minimal,
        );
        assert_eq!(resp.status(), hyper::StatusCode::TOO_MANY_REQUESTS);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            text.contains("429"),
            "uncommon status body must name its own status, got: {text:?}"
        );
        assert!(
            !text.contains("500"),
            "uncommon status body must not claim 500, got: {text:?}"
        );
        assert!(
            !text.contains("secret app detail"),
            "application detail must stay private, got: {text:?}"
        );

        // Another valid but previously unlisted status (418) must also agree.
        let err = ServiceError::rejected(418, "secret");
        let resp = err.to_response_with_head_and_policy(
            false,
            crate::policy::ErrorRepresentationPolicy::Minimal,
        );
        assert_eq!(resp.status().as_u16(), 418);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("418"), "got: {text:?}");
        assert!(!text.contains("500"), "got: {text:?}");
        assert!(!text.contains("secret"), "got: {text:?}");
    }

    #[tokio::test]
    async fn rejected_unknown_reason_is_neutral_but_preserves_status() {
        use http_body_util::BodyExt;

        // 299 has no standard reason phrase: preserve the wire status with a
        // neutral empty representation rather than lying about 500.
        let err = ServiceError::rejected(299, "secret");
        let resp = err.to_response_with_head_and_policy(
            false,
            crate::policy::ErrorRepresentationPolicy::Minimal,
        );
        assert_eq!(resp.status().as_u16(), 299);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            !text.contains("500"),
            "unknown status must not claim 500, got: {text:?}"
        );
        assert!(!text.contains("secret"), "got: {text:?}");
    }

    #[tokio::test]
    async fn rejected_invalid_and_informational_fallback_and_head_empty() {
        use http_body_util::BodyExt;

        // Out-of-range collapses to 500.
        for bad in [99u16, 600, 999] {
            let err = ServiceError::rejected(bad, "secret");
            let resp = err.to_response_with_head_and_policy(
                false,
                crate::policy::ErrorRepresentationPolicy::Minimal,
            );
            assert_eq!(
                resp.status(),
                hyper::StatusCode::INTERNAL_SERVER_ERROR,
                "status {bad} must collapse to 500"
            );
        }

        // Interim 1xx cannot be a final error response; narrowly collapse.
        for interim in [100u16, 101, 103, 199] {
            let err = ServiceError::rejected(interim, "secret");
            let resp = err.to_response_with_head_and_policy(
                false,
                crate::policy::ErrorRepresentationPolicy::Minimal,
            );
            assert_eq!(
                resp.status(),
                hyper::StatusCode::INTERNAL_SERVER_ERROR,
                "interim {interim} must collapse to 500"
            );
        }

        // HEAD emits no bytes but keeps the wire status.
        let err = ServiceError::rejected(429, "secret");
        let resp = err.to_response_with_head_and_policy(
            true,
            crate::policy::ErrorRepresentationPolicy::Minimal,
        );
        assert_eq!(resp.status(), hyper::StatusCode::TOO_MANY_REQUESTS);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty(), "HEAD must suppress error body");

        // Empty policy emits no bytes for Minimal-equivalent GET.
        let err = ServiceError::rejected(429, "secret");
        let resp = err.to_response_with_head_and_policy(
            false,
            crate::policy::ErrorRepresentationPolicy::Empty,
        );
        assert_eq!(resp.status(), hyper::StatusCode::TOO_MANY_REQUESTS);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty(), "Empty policy must emit no bytes");

        // Body-forbidden survivor keeps status with no bytes.
        let err = ServiceError::rejected(204, "secret");
        let resp = err.to_response_with_head_and_policy(
            false,
            crate::policy::ErrorRepresentationPolicy::Minimal,
        );
        assert_eq!(resp.status().as_u16(), 204);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty(), "204 must not emit a payload");
    }
}
