//! Typed request context / capability container (Plan 197).
//!
//! [`RequestContext`] is the single deliberate attachment point for
//! transport-authenticated request metadata and optional one-shot
//! capabilities. Ordinary services keep using [`Request`](super::request::Request)
//! by value; advanced capabilities attach here rather than as ad hoc
//! top-level `Request` fields.
//!
//! # What lives here
//!
//! - [`ConnectionInfo`](super::connection_info::ConnectionInfo): trustworthy
//!   transport metadata (socket endpoints when present, scheme, TLS session
//!   metadata). Values come from the actual transport or the explicit
//!   caller-owned [`ConnectionContext`](crate::server::connection::ConnectionContext).
//!   `Forwarded` / `X-Forwarded-*` headers are ordinary untrusted headers and
//!   are never copied here.
//! - [`RequestLifecycle`](super::request_lifecycle::RequestLifecycle):
//!   cloneable disconnect/cancel observer sharing the request body's
//!   allocation. Clone before moving the body into a downstream task.
//!
//! # What does NOT live here
//!
//! - No generic type map (`Any`, extension map) is provided. Downstream
//!   application state belongs in the service wrapper / adapter, not in the
//!   canonical request. Tower/framework extension maps belong in the Plan 200
//!   adapters unless a narrowly scoped native map is justified by a concrete
//!   consumer with allocation/cost evidence.
//! - No raw socket, Hyper, H2/H3, rustls-session, or executor handles are
//!   exposed. Transport capabilities remain opaque and capability-based.
//! - No upgrade/tunnel handles exist yet (Plan 176 deferred, Plan 199 owns
//!   the tunnel design). No interim-response sender exists yet (Plan 198 owns
//!   interim/trailer design). Those attach here as opaque capabilities when
//!   their plans land, without changing `Service::call` for ordinary services.
//!
//! # Cloning
//!
//! `RequestContext` is cheaply cloneable: [`ConnectionInfo`] is a small
//! value and [`RequestLifecycle`] is an `Arc`-backed observer. Cloning never
//! clones the one-shot [`RequestBody`](super::request_body::RequestBody);
//! the body stays with the owning `Request` value.

//! Typed request context / capability container (Plans 197–198).
//!
//! [`RequestContext`] is the single deliberate attachment point for
//! transport-authenticated request metadata and optional one-shot
//! capabilities. Ordinary services keep using [`Request`](super::request::Request)
//! by value; advanced capabilities attach here rather than as ad hoc
//! top-level `Request` fields.
//!
//! # What lives here
//!
//! - [`ConnectionInfo`](super::connection_info::ConnectionInfo): trustworthy
//!   transport metadata (socket endpoints when present, scheme, TLS session
//!   metadata). Values come from the actual transport or the explicit
//!   caller-owned [`ConnectionContext`](crate::server::connection::ConnectionContext).
//!   `Forwarded` / `X-Forwarded-*` headers are ordinary untrusted headers and
//!   are never copied here.
//! - [`RequestLifecycle`](super::request_lifecycle::RequestLifecycle):
//!   cloneable disconnect/cancel observer sharing the request body's
//!   allocation. Clone before moving the body into a downstream task.
//! - [`InterimSender`](super::interim::InterimSender): bounded request-scoped
//!   interim (1xx) capability (Plan 198). `None` in hand-constructed contexts
//!   that opt out; the runtime always attaches one.
//!
//! # What does NOT live here
//!
//! - No generic type map (`Any`, extension map) is provided. Downstream
//!   application state belongs in the service wrapper / adapter, not in the
//!   canonical request. Tower/framework extension maps belong in the Plan 200
//!   adapters unless a narrowly scoped native map is justified by a concrete
//!   consumer with allocation/cost evidence.
//! - No raw socket, Hyper, H2/H3, rustls-session, or executor handles are
//!   exposed. Transport capabilities remain opaque and capability-based.
//! - No upgrade/tunnel handles exist yet (Plan 176 deferred, Plan 199 owns
//!   the tunnel design).
//!
//! # Cloning
//!
//! `RequestContext` is cheaply cloneable: [`ConnectionInfo`] is a small
//! value, [`RequestLifecycle`] and [`InterimSender`] are `Arc`-backed.
//! Cloning never clones the one-shot [`RequestBody`](super::request_body::RequestBody);
//! the body stays with the owning `Request` value. Cloning shares the same
//! interim allocation (count/commitment visible on all clones).

use crate::primitives::connection_info::ConnectionInfo;
use crate::primitives::interim::InterimSender;
use crate::primitives::request_lifecycle::RequestLifecycle;
use crate::primitives::version::HttpVersion;

/// Stable place for transport-authenticated metadata and optional
/// one-shot capabilities (Plan 197 Track B, extended by Plan 198).
///
/// Ordinary metadata access remains cheap (borrowed accessors). Typed values
/// cannot be forged via untrusted request headers: the runtime constructs
/// this from observed transport state, and direct construction is reserved
/// for tests and downstream code that already owns validated components.
#[derive(Debug, Clone)]
pub struct RequestContext {
    connection: ConnectionInfo,
    lifecycle: RequestLifecycle,
    interim: Option<InterimSender>,
}

impl RequestContext {
    /// Create a context from validated components.
    ///
    /// The caller must ensure `lifecycle` shares the same internal
    /// allocation as the request body it will accompany (obtained via
    /// `body.lifecycle()`); otherwise body completion and cancellation
    /// observations diverge. The runtime always constructs them shared.
    ///
    /// No interim capability is attached; use [`RequestContext::with_interim`]
    /// or [`RequestContext::new_with_version`] when interim handling is needed.
    pub fn new(connection: ConnectionInfo, lifecycle: RequestLifecycle) -> Self {
        Self {
            connection,
            lifecycle,
            interim: None,
        }
    }

    /// Create a context with a bounded interim sender for `version`.
    ///
    /// The runtime uses this so every request has a request-scoped interim
    /// capability with version-aware suppression (HTTP/1.0 never emits wire
    /// bytes). Direct constructors without version remain for the common path.
    pub fn new_with_version(
        connection: ConnectionInfo,
        lifecycle: RequestLifecycle,
        version: HttpVersion,
    ) -> Self {
        Self {
            connection,
            lifecycle,
            interim: Some(InterimSender::new(version)),
        }
    }

    /// Attach an explicit interim sender (tests/downstream with owned limits).
    pub fn with_interim(mut self, sender: InterimSender) -> Self {
        self.interim = Some(sender);
        self
    }

    /// Transport-authenticated connection metadata.
    ///
    /// Values come from the actual transport. Proxy-derived values are never
    /// trusted implicitly; read forwarding headers separately under an
    /// explicit trust policy (Plans 201–202 own listener/proxy metadata).
    pub fn connection(&self) -> &ConnectionInfo {
        &self.connection
    }

    /// Transport-neutral disconnect/cancellation observer.
    ///
    /// Becomes ready on peer disconnect or runtime cancellation even when
    /// the application is not polling request/response IO. Clone before
    /// moving the body into a downstream task.
    pub fn lifecycle(&self) -> &RequestLifecycle {
        &self.lifecycle
    }

    /// Cloneable lifecycle observer sharing this context's allocation.
    pub fn lifecycle_clone(&self) -> RequestLifecycle {
        self.lifecycle.clone()
    }

    /// Bounded request-scoped interim (1xx) sender, if attached.
    ///
    /// The runtime always attaches one; hand-constructed contexts may have
    /// none. Clones share the same allocation (count/commitment shared).
    /// See [`InterimSender`] for the bounded contract: only 1xx (no 101),
    /// no body/trailers, no interim after final commitment, bounded
    /// count/bytes, HTTP/1.0 suppressed.
    pub fn interim(&self) -> Option<&InterimSender> {
        self.interim.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::connection_info::Scheme;
    use crate::primitives::request_body::RequestBody;

    fn test_context() -> RequestContext {
        let body = RequestBody::empty();
        let lifecycle = body.lifecycle();
        let connection = ConnectionInfo::without_socket_addrs(Scheme::Http, None);
        RequestContext::new(connection, lifecycle)
    }

    #[test]
    fn context_exposes_connection_and_lifecycle() {
        let ctx = test_context();
        assert_eq!(ctx.connection().scheme, Scheme::Http);
        assert!(!ctx.lifecycle().is_cancelled());
        assert!(!ctx.lifecycle_clone().is_cancelled());
    }

    #[test]
    fn context_clone_is_cheap_and_shares_lifecycle() {
        let ctx = test_context();
        let cloned = ctx.clone();
        assert_eq!(cloned.connection(), ctx.connection());
        // Same allocation: cancellation would be visible on both.
        assert_eq!(
            cloned.lifecycle().is_cancelled(),
            ctx.lifecycle().is_cancelled()
        );
    }
}
