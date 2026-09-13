//! Typed request context / capability container (Plans 197–199, 216).
//!
//! [`RequestContext`] is the single deliberate attachment point for
//! transport-authenticated request metadata and optional capabilities.
//! Ordinary services keep using [`Request`](super::request::Request)
//! by value; advanced capabilities attach here rather than as ad hoc
//! top-level `Request` fields.
//!
//! # What lives here
//!
//! - [`ConnectionInfo`](super::connection_info::ConnectionInfo): trustworthy
//!   transport metadata (socket endpoints when present, scheme, TLS session
//!   metadata, plus Plan 202 provenance-tagged effective fields when an
//!   explicit trusted-proxy policy adopted them). Values come from the actual
//!   transport or the explicit
//!   caller-owned connection context.
//!   `Forwarded` / `X-Forwarded-*` headers are ordinary untrusted headers by
//!   default and never populate trusted fields without explicit trust.
//! - [`RequestLifecycle`](super::request_lifecycle::RequestLifecycle):
//!   cloneable disconnect/cancel observer sharing the request body's
//!   allocation. Clone before moving the body into a downstream task.
//! - [`InterimSender`](super::interim::InterimSender): bounded request-scoped
//!   interim (1xx) capability (Plan 198). `None` in hand-constructed contexts
//!   that opt out; the runtime always attaches one.
//! - [`TunnelRequest`](super::tunnel::TunnelRequest): validated tunnel
//!   *intent* (Plan 199 intent, Plan 216 placement). `None` when the request
//!   is not a validated upgrade/`CONNECT`/Extended `CONNECT`; the runtime
//!   records it only after header/pseudo-header validation. Intent is
//!   cloneable routing metadata (`tunnel_request()` clones it).
//!
//! One-shot *acceptance* ownership deliberately does NOT live here. The
//! transport-backed capability
//! (`eggserve-server::tunnel::TunnelCapability`) is server-owned and reaches
//! tunnel-aware services through the additive
//! `Service::call_with_tunnel` parameter, never through this
//! transport-neutral struct. That split keeps this crate free of Hyper,
//! Tokio, H2/H3, and QUIC dependencies while giving downstream codecs a
//! typed, one-shot, commitment-safe path without an `Any` map.
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
//! - No one-shot transport capability slot: taking/accepting is owned by
//!   `eggserve-server::tunnel`. Cloning a context clones the intent
//!   (routing may inspect it on every clone); ownership never duplicates
//!   because there is no ownership here to duplicate.
//!
//! # Cloning
//!
//! `RequestContext` is cheaply cloneable: [`ConnectionInfo`] is a small
//! value, [`RequestLifecycle`] and [`InterimSender`] are `Arc`-backed, and
//! [`TunnelRequest`] is a small validated value.
//! Cloning never clones the one-shot [`RequestBody`](super::request_body::RequestBody);
//! the body stays with the owning `Request` value. Cloning shares the same
//! interim allocation (count/commitment visible on all clones).

use crate::primitives::connection_info::ConnectionInfo;
use crate::primitives::interim::InterimSender;
use crate::primitives::request_lifecycle::RequestLifecycle;
use crate::primitives::tunnel::TunnelRequest;
use crate::primitives::version::HttpVersion;

/// Stable place for transport-authenticated metadata and optional
/// one-shot capabilities (Plan 197 Track B, extended by Plans 198–199).
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
    tunnel_request: Option<TunnelRequest>,
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
    /// No tunnel intent is attached; use
    /// [`RequestContext::with_tunnel_request`] when the runtime validated one.
    pub fn new(connection: ConnectionInfo, lifecycle: RequestLifecycle) -> Self {
        Self {
            connection,
            lifecycle,
            interim: None,
            tunnel_request: None,
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
            tunnel_request: None,
        }
    }

    /// Attach an explicit interim sender (tests/downstream with owned limits).
    pub fn with_interim(mut self, sender: InterimSender) -> Self {
        self.interim = Some(sender);
        self
    }

    /// Attach validated tunnel intent (runtime only, after validation).
    ///
    /// Consuming builder for intent-bearing contexts (Plans 199/216).
    /// Intent is cloneable routing metadata; one-shot acceptance ownership
    /// stays in `eggserve-server::tunnel` and reaches services via
    /// `Service::call_with_tunnel`, never via this struct.
    pub fn with_tunnel_request(mut self, request: TunnelRequest) -> Self {
        self.tunnel_request = Some(request);
        self
    }

    /// Transport-authenticated connection metadata.
    ///
    /// Values come from the actual transport, plus Plan 202
    /// provenance-tagged effective fields when an explicit trusted-proxy
    /// policy adopted them. Untrusted forwarding headers never populate
    /// trusted fields implicitly; read them separately only under an
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

    /// Validated tunnel intent for this request, if the runtime classified
    /// one (cloneable routing metadata).
    ///
    /// `None` on ordinary requests. Inspect `kind()`/`protocol()`/
    /// `authority()` for routing; one-shot acceptance (if any) arrives via
    /// `eggserve-server`'s `Service::call_with_tunnel`, never here.
    pub fn tunnel_request(&self) -> Option<&TunnelRequest> {
        self.tunnel_request.as_ref()
    }

    /// Clone the validated tunnel intent, if any (convenience for routing).
    pub fn clone_tunnel_request(&self) -> Option<TunnelRequest> {
        self.tunnel_request.clone()
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
