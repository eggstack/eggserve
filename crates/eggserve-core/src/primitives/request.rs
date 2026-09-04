//! Canonical request envelope.
//!
//! [`Request`] combines the request head, body, and connection metadata
//! into a single transport-independent value. This is the input type
//! for the [`Service`](crate::server::Service) trait.

use crate::primitives::connection_info::ConnectionInfo;
use crate::primitives::request_body::RequestBody;
use crate::primitives::request_head::RequestHead;
use crate::primitives::request_lifecycle::RequestLifecycle;

/// A canonical, transport-independent HTTP request.
///
/// Combines the immutable request head, the bounded request body, and
/// connection metadata into a single value. The runtime constructs
/// `Request` instances from incoming connections; services receive
/// them by value.
///
/// # Hyper independence
///
/// No Hyper type appears in this struct or its public API. The body
/// stream is opaque behind [`RequestBody`].
///
/// # One-shot body
///
/// The body can only be consumed once, either via
/// [`read_all`](RequestBody::read_all) or by streaming chunks.
/// After consumption, the body is in the `Complete` state.
#[derive(Debug)]
pub struct Request {
    head: RequestHead,
    body: RequestBody,
    connection: ConnectionInfo,
    lifecycle: RequestLifecycle,
}

impl Request {
    /// Create a new request envelope.
    ///
    /// Prefer using the runtime adapter (Hyper → canonical conversion)
    /// for production use. This constructor is for tests and downstream
    /// code that already has validated components.
    ///
    /// The lifecycle is derived from the body's shared allocation so body
    /// completion, abandonment, and cancellation are visible without
    /// holding the body itself.
    pub fn new(head: RequestHead, body: RequestBody, connection: ConnectionInfo) -> Self {
        let lifecycle = body.lifecycle();
        Self {
            head,
            body,
            connection,
            lifecycle,
        }
    }

    /// Create a request with an explicit lifecycle sharing the body's
    /// allocation.
    ///
    /// The caller must ensure `lifecycle` shares the same internal
    /// allocation as `body` (obtained via `body.lifecycle()`); otherwise
    /// body completion and cancellation observations diverge. The runtime
    /// always constructs them shared.
    pub fn new_with_lifecycle(
        head: RequestHead,
        body: RequestBody,
        connection: ConnectionInfo,
        lifecycle: RequestLifecycle,
    ) -> Self {
        Self {
            head,
            body,
            connection,
            lifecycle,
        }
    }

    /// Returns the immutable request head.
    pub fn head(&self) -> &RequestHead {
        &self.head
    }

    /// Returns a reference to the request body.
    pub fn body(&self) -> &RequestBody {
        &self.body
    }

    /// Transport-neutral disconnect/cancellation observer for this request.
    ///
    /// Becomes ready on peer disconnect or runtime cancellation even when
    /// the application is not polling request/response IO. Clone before
    /// moving the body into a downstream task.
    pub fn lifecycle(&self) -> &RequestLifecycle {
        &self.lifecycle
    }

    /// Cloneable lifecycle observer sharing this request's allocation.
    pub fn lifecycle_clone(&self) -> RequestLifecycle {
        self.lifecycle.clone()
    }

    /// Consume the request, returning the head and body separately.
    ///
    /// This is useful for services that need to pass the head to one
    /// code path and the body to another. Preserved without lifecycle for
    /// source compatibility; use [`Request::into_parts_with_lifecycle`]
    /// when the observer must outlive deconstruction.
    pub fn into_parts(self) -> (RequestHead, RequestBody, ConnectionInfo) {
        (self.head, self.body, self.connection)
    }

    /// Consume the request, returning head, body, connection, and lifecycle.
    ///
    /// Added for Plan 174 without changing `into_parts` tuple arity.
    pub fn into_parts_with_lifecycle(
        self,
    ) -> (RequestHead, RequestBody, ConnectionInfo, RequestLifecycle) {
        (self.head, self.body, self.connection, self.lifecycle)
    }

    /// Consume the request, returning the body.
    ///
    /// The request head and connection info are discarded.
    pub fn into_body(self) -> RequestBody {
        self.body
    }

    /// Returns a reference to the connection metadata.
    pub fn connection(&self) -> &ConnectionInfo {
        &self.connection
    }

    /// Deconstruct the request into head and a body-bearing tuple.
    ///
    /// Returns `(head, body)`.
    pub fn into_head_and_body(self) -> (RequestHead, RequestBody) {
        (self.head, self.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::connection_info::{ConnectionInfo, Scheme};
    use crate::primitives::header_block::HeaderBlock;
    use crate::primitives::method::Method;
    use crate::primitives::request_body::RequestBody;
    use crate::primitives::request_target::RequestTarget;
    use crate::primitives::version::HttpVersion;
    use std::net::SocketAddr;

    fn test_connection() -> ConnectionInfo {
        ConnectionInfo {
            local_addr: Some("127.0.0.1:8000".parse::<SocketAddr>().unwrap()),
            remote_addr: Some("127.0.0.1:12345".parse::<SocketAddr>().unwrap()),
            scheme: Scheme::Http,
            tls: None,
        }
    }

    fn test_head() -> RequestHead {
        RequestHead::new(
            Method::get(),
            RequestTarget::parse("/test").unwrap(),
            HttpVersion::Http11,
            HeaderBlock::new(),
        )
    }

    #[test]
    fn request_construction() {
        let req = Request::new(test_head(), RequestBody::empty(), test_connection());
        assert_eq!(req.head().method().as_str(), "GET");
        assert!(
            req.body().is_complete()
                || req.body().state() == crate::primitives::request_body::BodyState::Unread
        );
    }

    #[test]
    fn request_into_parts() {
        let req = Request::new(test_head(), RequestBody::empty(), test_connection());
        let (head, _body, conn) = req.into_parts();
        assert_eq!(head.method().as_str(), "GET");
        // body is empty
        assert_eq!(conn.scheme, Scheme::Http);
    }

    #[test]
    fn request_into_body() {
        let req = Request::new(test_head(), RequestBody::empty(), test_connection());
        let body = req.into_body();
        assert!(body.declared_length().is_none() || body.declared_length() == Some(0));
    }

    #[test]
    fn request_connection() {
        let req = Request::new(test_head(), RequestBody::empty(), test_connection());
        assert_eq!(req.connection().scheme, Scheme::Http);
    }
}
