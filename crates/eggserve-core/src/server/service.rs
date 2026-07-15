//! Transport-independent service abstraction.
//!
//! A [`Service`] receives a canonical eggserve [`Request`] and produces a
//! canonical [`Response`]. The runtime owns transport, parsing, normalization,
//! and timeout enforcement. Services never see raw sockets or Hyper types.
//!
//! # Example
//!
//! ```ignore
//! use eggserve_core::server::{service_fn, Request, Response, StatusCode, ResponseBody};
//!
//! let service = service_fn(|_req| async {
//!     Ok(Response::builder()
//!         .status(StatusCode::OK)
//!         .body(ResponseBody::Bytes(b"hello".to_vec()))
//!         .unwrap())
//! });
//! ```

use std::future::Future;
use std::pin::Pin;

use crate::primitives::canonical::Response;
use crate::primitives::request::Request;

/// Errors produced by a service implementation.
///
/// The runtime converts these into appropriate HTTP responses without leaking
/// internal details. Services should use [`ServiceError::internal`] for
/// unexpected failures and [`ServiceError::rejected`] for intentional rejections
/// that should produce a specific status code.
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
    pub fn rejected(status: u16, message: impl Into<String>) -> Self {
        Self {
            kind: ServiceErrorKind::Rejected(status),
            message: message.into(),
        }
    }

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

    /// Convert this error into an HTTP response.
    ///
    /// Internal and panic errors map to 500. Timeout errors map to 504.
    /// Rejected errors use the provided status code. No internal details
    /// are included in the response body.
    pub(crate) fn to_response(&self) -> hyper::Response<crate::response::BoxBodyInner> {
        let status = match self.kind {
            ServiceErrorKind::Internal | ServiceErrorKind::Panic => {
                hyper::StatusCode::INTERNAL_SERVER_ERROR
            }
            ServiceErrorKind::Rejected(code) => hyper::StatusCode::from_u16(code)
                .unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR),
            ServiceErrorKind::Timeout => hyper::StatusCode::GATEWAY_TIMEOUT,
        };
        let body = match self.kind {
            ServiceErrorKind::Panic => "500 Internal Server Error\n",
            ServiceErrorKind::Timeout => "504 Gateway Timeout\n",
            ServiceErrorKind::Rejected(code) => match code {
                400 => "400 Bad Request\n",
                403 => "403 Forbidden\n",
                404 => "404 Not Found\n",
                405 => "405 Method Not Allowed\n",
                503 => "503 Service Unavailable\n",
                _ => "500 Internal Server Error\n",
            },
            ServiceErrorKind::Internal => "500 Internal Server Error\n",
        };
        crate::response::canonical_error(status, body)
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

/// A transport-independent service that handles HTTP requests.
///
/// Services are invoked by the runtime after request parsing and validation.
/// They receive a canonical [`Request`] (head, body, and connection metadata)
/// and must return a canonical [`Response`].
///
/// # Contract
///
/// - The service is called once per request.
/// - The service must not write to raw sockets or access transport internals.
/// - Panics are caught and converted to 500 responses.
/// - The response goes through runtime normalization (hop-by-hop stripping,
///   content-length computation) before transport.
///
/// # Thread safety
///
/// Services must be `Send + Sync` to be shared across connection tasks.
pub trait Service: Send + Sync + 'static {
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
/// ```ignore
/// use eggserve_core::server::{service_fn, Request, Response, StatusCode, ResponseBody};
///
/// let service = service_fn(|_req: Request| async {
///     Ok(Response::builder()
///         .status(StatusCode::OK)
///         .body(ResponseBody::Bytes(b"hello".to_vec()))
///         .unwrap())
/// });
/// ```
pub fn service_fn<F, Fut>(f: F) -> ServiceFn<F>
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, ServiceError>> + Send + 'static,
{
    ServiceFn { f }
}

/// A service created from a closure via [`service_fn`].
pub struct ServiceFn<F> {
    f: F,
}

impl<F, Fut> Service for ServiceFn<F>
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, ServiceError>> + Send + 'static,
{
    fn call(
        &self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response, ServiceError>> + Send + '_>> {
        Box::pin((self.f)(request))
    }
}

/// Wrap a future with panic containment.
///
/// If the future panics, the panic is caught and converted to a
/// [`ServiceError::panic`].
pub async fn catch_unwind_service<F>(future: F) -> Result<F::Output, ServiceError>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    match tokio::task::spawn(future).await {
        Ok(result) => Ok(result),
        Err(e) => {
            let msg = e.to_string();
            Err(ServiceError::panic(msg))
        }
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
            ConnectionInfo {
                local_addr: "127.0.0.1:8000".parse::<SocketAddr>().unwrap(),
                remote_addr: "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
                scheme: Scheme::Http,
                tls: None,
            },
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

        // Rejected(999) → 999 (valid status code, used as-is)
        let err = ServiceError::rejected(999, "weird");
        assert_eq!(
            err.to_response().status(),
            hyper::StatusCode::from_u16(999).unwrap()
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
}
