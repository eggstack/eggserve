//! Canonical request envelope.
//!
//! [`Request`] combines the request head, body, and connection metadata
//! into a single transport-independent value. This is the input type
//! for the [`Service`](crate::server::Service) trait.

use crate::primitives::connection_info::ConnectionInfo;
use crate::primitives::request_body::RequestBody;
use crate::primitives::request_context::RequestContext;
use crate::primitives::request_head::RequestHead;
use crate::primitives::request_lifecycle::RequestLifecycle;

/// A canonical, transport-independent HTTP request.
///
/// Combines the immutable request head, the bounded request body, and a
/// typed request context ([`RequestContext`]: transport-authenticated
/// connection metadata plus the cloneable disconnect/cancel observer) into
/// a single value. The runtime constructs
/// `Request` instances from incoming connections; services receive
/// them by value.
///
/// `RequestContext` is the single deliberate attachment point for future
/// transport capabilities (Plan 197 Track B): interim-response senders
/// (Plan 198) and tunnel capabilities (Plan 199) attach there as opaque
/// capabilities rather than as ad hoc top-level `Request` fields. Ordinary
/// services keep destructuring head/body and reading
/// [`Request::connection`] / [`Request::lifecycle`]; those accessors forward
/// to the context and remain source-compatible with the Plan 175 consumer.
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
    context: RequestContext,
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
    /// holding the body itself. The context wraps the supplied connection
    /// metadata with that lifecycle; connection values must come from the
    /// observed transport, never from untrusted request headers.
    pub fn new(head: RequestHead, body: RequestBody, connection: ConnectionInfo) -> Self {
        let lifecycle = body.lifecycle();
        let context = RequestContext::new(connection, lifecycle);
        Self {
            head,
            body,
            context,
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
        let context = RequestContext::new(connection, lifecycle);
        Self {
            head,
            body,
            context,
        }
    }

    /// Create a request with an explicit typed context.
    ///
    /// The caller must ensure `context.lifecycle()` shares the same
    /// internal allocation as `body` (obtained via `body.lifecycle()`).
    /// This is the forward-compatible constructor for capability-bearing
    /// contexts; `new` / `new_with_lifecycle` remain for the common path.
    pub fn new_with_context(head: RequestHead, body: RequestBody, context: RequestContext) -> Self {
        Self {
            head,
            body,
            context,
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
    /// moving the body into a downstream task. Forwards to the request
    /// context (Plan 197); prefer [`Request::context`] when threading
    /// connection + lifecycle together.
    pub fn lifecycle(&self) -> &RequestLifecycle {
        self.context.lifecycle()
    }

    /// Cloneable lifecycle observer sharing this request's allocation.
    pub fn lifecycle_clone(&self) -> RequestLifecycle {
        self.context.lifecycle_clone()
    }

    /// The typed request context for this request.
    ///
    /// Single deliberate attachment point for transport-authenticated
    /// metadata and future opaque capabilities (Plan 197 Track B).
    /// Cloning the context never clones the one-shot body.
    pub fn context(&self) -> &RequestContext {
        &self.context
    }

    /// Consume the request, returning the head and body separately.
    ///
    /// This is useful for services that need to pass the head to one
    /// code path and the body to another. Preserved without lifecycle for
    /// source compatibility; use [`Request::into_parts_with_lifecycle`]
    /// when the observer must outlive deconstruction, or
    /// [`Request::into_parts_with_context`] when the full context must
    /// outlive deconstruction.
    pub fn into_parts(self) -> (RequestHead, RequestBody, ConnectionInfo) {
        let connection = self.context.connection().clone();
        (self.head, self.body, connection)
    }

    /// Consume the request, returning head, body, connection, and lifecycle.
    ///
    /// Added for Plan 174 without changing `into_parts` tuple arity.
    /// Preserved for the Plan 175 common path; new code that needs the
    /// full capability container should prefer
    /// [`Request::into_parts_with_context`].
    pub fn into_parts_with_lifecycle(
        self,
    ) -> (RequestHead, RequestBody, ConnectionInfo, RequestLifecycle) {
        let connection = self.context.connection().clone();
        let lifecycle = self.context.lifecycle_clone();
        (self.head, self.body, connection, lifecycle)
    }

    /// Consume the request, returning head, body, and the typed context.
    ///
    /// Forward-compatible deconstruction for capability-bearing contexts
    /// (Plan 197): the context carries connection metadata, the lifecycle
    /// observer, and — once Plans 198–199 land — opaque interim/tunnel
    /// capabilities, without growing the tuple arity again.
    pub fn into_parts_with_context(self) -> (RequestHead, RequestBody, RequestContext) {
        (self.head, self.body, self.context)
    }

    /// Consume the request, returning the body.
    ///
    /// The request head and connection info are discarded.
    pub fn into_body(self) -> RequestBody {
        self.body
    }

    /// Returns a reference to the connection metadata.
    ///
    /// Forwards to the request context (Plan 197). Transport-authenticated
    /// values only; forwarding headers are never consulted here.
    pub fn connection(&self) -> &ConnectionInfo {
        self.context.connection()
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
