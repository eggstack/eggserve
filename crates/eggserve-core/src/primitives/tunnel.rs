//! Generic tunnel / upgrade compatibility layer (Plans 199, 216).
//!
//! Plan 199 semantics, Plan 216 ownership: transport-neutral intent
//! vocabulary lives in `eggserve-primitives`, transport execution lives in
//! `eggserve-server`. This module re-exports the neutral values, re-exports
//! the server-owned duplex, and provides a thin compatibility
//! [`TunnelCapability`] that preserves the `RequestContext::take_tunnel`
//! source contract while delegating validation, one-shot state, handshake
//! construction, and transport handoff to the direct authority. There is no
//! second parser, state machine, handshake builder, or bridge here.
//!
//! - Intent/validation: [`TunnelKind`], [`ProtocolName`], [`TunnelRequest`],
//!   [`TunnelError`], bounds, `classify_h1_upgrade`,
//!   `classify_extended_protocol`, `validate_handshake_headers` (all owned
//!   by `eggserve-primitives`).
//! - Duplex: [`TunnelIo`] (owned by `eggserve-server`).
//! - Acceptance: [`TunnelCapability::accept`] delegates to the server
//!   implementation (same one-shot/commitment/bound semantics) and converts
//!   the handshake to the compatibility response shape (status + headers +
//!   empty body; the staged transport acceptance is shared, not converted).
//!
//! H3 stream bridging stays in `server::http3` under the Plan 213 boundary;
//! it stages through the same capability and sidecar, then bridges its own
//! streams.

pub use eggserve_primitives::tunnel::{
    classify_extended_protocol, classify_h1_upgrade, validate_handshake_headers, ProtocolName,
    TunnelError, TunnelKind, TunnelRequest, MAX_TUNNEL_HEADER_BYTES, MAX_TUNNEL_HEADER_COUNT,
    MAX_TUNNEL_PROTOCOL_BYTES, TUNNEL_IO_BUFFER_BYTES,
};
pub use eggserve_server::tunnel::{TunnelAcceptance, TunnelIo, TunnelShared};

/// One-shot, non-cloneable, transport-backed tunnel capability
/// (compatibility wrapper).
///
/// Obtained via `RequestContext::take_tunnel()` (or inspected via
/// `tunnel_request()`). Consumed by [`accept`](Self::accept) to produce a
/// handshake [`Response`](crate::primitives::canonical::Response);
/// dropping/ignoring uses the normal HTTP denial path.
///
/// Thin delegation: validation, one-shot/commitment state, handshake
/// construction, and the staged transport acceptance are all owned by
/// `eggserve-server`. This wrapper only converts the handshake response
/// shape (direct → compatibility; status + headers + empty body) so
/// compatibility services keep their response type. Handler ergonomics are
/// identical (`FnOnce(TunnelIo)`; capture the request lifecycle in the
/// closure when cancellation is needed).
pub struct TunnelCapability {
    inner: eggserve_server::tunnel::TunnelCapability,
}

impl std::fmt::Debug for TunnelCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TunnelCapability")
            .field("request", &self.inner.request())
            .finish()
    }
}

impl TunnelCapability {
    /// Create a capability (runtime only, after validation).
    ///
    /// Crate-internal: runtimes classify intent, acquire `OnUpgrade`, and
    /// share the returned commitment state/sidecar with the post-service
    /// check. Returns the capability plus its shared state and staged
    /// sidecar (both server-owned).
    pub(crate) fn new(
        request: TunnelRequest,
        upgrade: Option<hyper::upgrade::OnUpgrade>,
    ) -> (
        Self,
        std::sync::Arc<TunnelShared>,
        std::sync::Arc<std::sync::Mutex<Option<TunnelAcceptance>>>,
    ) {
        let shared = std::sync::Arc::new(TunnelShared::new());
        let sidecar = std::sync::Arc::new(std::sync::Mutex::new(None));
        let inner = eggserve_server::tunnel::TunnelCapability::new(
            request,
            upgrade,
            shared.clone(),
            sidecar.clone(),
        );
        (Self { inner }, shared, sidecar)
    }

    /// Returns validated tunnel intent.
    pub fn request(&self) -> &TunnelRequest {
        self.inner.request()
    }

    /// Shared commitment/acceptance state (runtime pre-service snapshot).
    pub(crate) fn shared(&self) -> std::sync::Arc<TunnelShared> {
        self.inner.shared()
    }

    /// Staged-acceptance sidecar (runtime post-service check).
    ///
    /// The pipeline snapshots this before the service runs and takes the
    /// staged acceptance after it returns; ordinary denial stages nothing.
    pub(crate) fn sidecar(&self) -> std::sync::Arc<std::sync::Mutex<Option<TunnelAcceptance>>> {
        self.inner.sidecar()
    }

    /// Accept the tunnel: validate handshake headers, claim one-shot
    /// ownership, stage the transport acceptance, and return the handshake
    /// response (compatibility shape).
    ///
    /// Same contract as the direct authority: `101` for H1 `Upgrade`
    /// (runtime owns `Upgrade`/`Connection`), `200` for
    /// `Connect`/`ExtendedConnect`, bounded application headers, framing
    /// rejected, hop-by-hop stripped. See
    /// `eggserve_server::tunnel::TunnelCapability::accept`.
    pub fn accept<F, Fut>(
        self,
        headers: crate::primitives::header_block::HeaderBlock,
        handler: F,
    ) -> Result<crate::primitives::canonical::Response, TunnelError>
    where
        F: FnOnce(TunnelIo) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let direct = self.inner.accept(headers, handler)?;
        Ok(convert_handshake(direct))
    }
}

/// Convert a direct handshake response to the compatibility shape.
///
/// Handshakes are status + headers + empty body by construction; the
/// conversion moves them across (header types are already unified by the
/// `header_block` facade, so no re-validation loss). Any non-empty body
/// (unreachable) fails closed as empty with the status preserved.
fn convert_handshake(
    mut direct: eggserve_primitives::canonical::Response,
) -> crate::primitives::canonical::Response {
    use crate::primitives::canonical::{Response, ResponseBody};
    let status = crate::primitives::canonical::StatusCode::new(direct.status().as_u16())
        .unwrap_or(crate::primitives::canonical::StatusCode::INTERNAL_SERVER_ERROR);
    let mut builder = Response::builder().status(status);
    for field in direct.headers().iter() {
        builder = builder.push_header(field.name.clone(), field.value.clone());
    }
    let body = match direct.take_body() {
        Some(eggserve_primitives::canonical::ResponseBody::Empty) | None => ResponseBody::Empty,
        // Unreachable for handshakes; fail closed with the status preserved.
        Some(_) => ResponseBody::Empty,
    };
    builder.body(body).unwrap_or_else(|_| {
        Response::builder()
            .status(crate::primitives::canonical::StatusCode::INTERNAL_SERVER_ERROR)
            .body(ResponseBody::Empty)
            .expect("fallback handshake converts")
    })
}
