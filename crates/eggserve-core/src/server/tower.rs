//! Tower service adapters (Plan 200 Track E).
//!
//! This module is gated behind the `tower` feature so native consumers never
//! depend on Tower types. It provides two integration directions with explicit
//! readiness and ownership semantics.
//!
//! # E1 — Run a Tower service on EggServe (`TowerToEggserve`)
//!
//! Accepts a Tower service and exposes an EggServe native [`Service`]. The
//! Tower service must be [`Clone`]: each request clones the service and drives
//! readiness on its own clone (`poll_ready` + `call`). No global `Mutex`
//! serializes unrelated requests; shared application state must live behind
//! the clone (typically `Arc`). Readiness is per-clone, not per-connection.
//!
//! EggServe's server-wide `max_in_flight_requests` remains an outer hard
//! admission ceiling held across native `Service::call`. Tower readiness may
//! further delay or reject application work but can never raise that ceiling.
//!
//! Request bodies arrive as canonical [`RequestBody`](crate::primitives::RequestBody)
//! (which implements `http_body::Body` with trailers, limits, and
//! cancellation). Tower responses (`http::Response<B: http_body::Body>`) are
//! converted incrementally into the canonical pipeline; EggServe remains the
//! final framing authority.
//!
//! # E2 — Expose an EggServe service as Tower (`EggserveToTower`)
//!
//! Implements `tower_service::Service<http::Request<RequestBody>>` around a
//! native [`Service`]. `poll_ready` always reports ready: it reflects
//! adapter-local readiness only and never claims transport admission has been
//! acquired (admission stays runtime-owned). Responses convert through
//! canonical normalization + `to_hyper_response` boxing, so `HEAD`/
//! body-forbidden suppression, hop-by-hop stripping, and privacy policy still
//! apply.
//!
//! This inverse adapter exists for composition/testing and is never required
//! for server operation.
//!
//! # Middleware boundary (Track F)
//!
//! Tower [`tower_layer::Layer`]s compose around either adapter on standard
//! `http` request/response objects after EggServe parsing/validation and
//! before final normalization. Middleware may add ordinary headers/content
//! but cannot bypass body hard limits, framing validation, denylist/privacy,
//! no-progress timeouts, or lifecycle/shutdown.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;

use crate::primitives::canonical::{normalize_response, NormalizeRequest};
use crate::primitives::interop::{request_head_to_http, response_from_http_body, InteropError};
use crate::primitives::request::Request;
use crate::primitives::request_body::RequestBody;
use crate::primitives::request_body_policy::RequestBodyPolicy;
use crate::server::service::{Service, ServiceError};

// ---------------------------------------------------------------------------
// E1: Tower -> EggServe
// ---------------------------------------------------------------------------

/// Adapter running a Tower service as a native EggServe [`Service`].
///
/// `S` is the Tower service type operating on
/// `http::Request<RequestBody>`; `B` is its response-body type. `S` must be
/// [`Clone`] so each request drives its own readiness without a shared
/// mutex. See the module docs for the ownership contract.
///
/// The body policy is explicit (no silent default escalation): use
/// [`TowerToEggserve::new`] for streaming with a bounded limit, or
/// [`TowerToEggserve::with_policy`] for full control. The runtime hard
/// ceiling (`max_request_body_bytes`) still caps whatever is configured here.
pub struct TowerToEggserve<S> {
    inner: S,
    policy: RequestBodyPolicy,
}

impl<S> TowerToEggserve<S> {
    /// Create an adapter with a bounded streaming body policy (1 MiB).
    ///
    /// The runtime ceiling still applies; this value only lowers it.
    pub fn new(service: S) -> Self {
        Self {
            inner: service,
            policy: RequestBodyPolicy::Stream {
                max_bytes: 1024 * 1024,
            },
        }
    }

    /// Create an adapter with an explicit body policy.
    pub fn with_policy(service: S, policy: RequestBodyPolicy) -> Self {
        Self {
            inner: service,
            policy,
        }
    }

    /// Returns the configured body policy.
    pub fn body_policy(&self) -> RequestBodyPolicy {
        self.policy
    }
}

impl<S> Clone for TowerToEggserve<S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            policy: self.policy,
        }
    }
}

impl<S, B> Service for TowerToEggserve<S>
where
    S: tower_service::Service<http::Request<RequestBody>, Response = http::Response<B>>
        + Clone
        + Send
        + Sync
        + 'static,
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
    S::Future: Send,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    fn request_body_policy(
        &self,
        _head: &crate::primitives::request_head::RequestHead,
    ) -> RequestBodyPolicy {
        self.policy
    }

    fn call(
        &self,
        request: Request,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<crate::primitives::canonical::Response, ServiceError>>
                + Send
                + '_,
        >,
    > {
        // Per-request clone: readiness is per-clone, never behind a shared
        // mutex. Shared state must be `Clone` (e.g. `Arc` inside `S`).
        let mut service = self.inner.clone();
        Box::pin(async move {
            let (head, body, context) = request.into_parts_with_context();
            let connection = context.connection().clone();
            let lifecycle = context.lifecycle_clone();
            // Canonical -> http (infallible for runtime-constructed heads;
            // hand-built heads that cannot map are an internal error).
            let http_head = request_head_to_http(&head, &connection, &lifecycle)
                .map_err(|e| ServiceError::internal(format!("tower request conversion: {e}")))?;
            let http_req = http_head.map(|()| body);
            // Readiness: per-clone, bounded by Tower semantics. Transport
            // admission (`max_in_flight_requests`) is already held by the
            // runtime across this `call`; readiness here only gates app work.
            futures_util::future::poll_fn(|cx| {
                tower_service::Service::poll_ready(&mut service, cx)
            })
            .await
            .map_err(|e| ServiceError::internal(format!("tower readiness: {e}")))?;
            let http_resp = tower_service::Service::call(&mut service, http_req)
                .await
                .map_err(|e| ServiceError::internal(format!("tower service: {e}")))?;
            // http -> canonical (streaming, framing-authoritative).
            response_from_http_body(http_resp)
                .map_err(|e| ServiceError::internal(format!("tower response conversion: {e}")))
        })
    }
}

// ---------------------------------------------------------------------------
// E2: EggServe -> Tower
// ---------------------------------------------------------------------------

/// Adapter exposing a native EggServe [`Service`] as a Tower service.
///
/// The Tower request type is `http::Request<RequestBody>` (the canonical body
/// already implements `http_body::Body`, so middleware can consume it
/// incrementally with trailers). Generic foreign body types must be buffered
/// into [`RequestBody`] by downstream code with an explicit limit first;
/// no unbounded buffering path is provided here.
///
/// Responses are `http::Response<UnsyncBoxBody<Bytes, io::Error>>` produced
/// via canonical normalization + transport conversion, so privacy, framing,
/// and `HEAD`/body-forbidden rules still apply.
pub struct EggserveToTower<S> {
    inner: S,
}

impl<S> EggserveToTower<S> {
    /// Wrap a native service for Tower composition/testing.
    pub fn new(service: S) -> Self {
        Self { inner: service }
    }
}

impl<S> Clone for EggserveToTower<S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<S> tower_service::Service<http::Request<RequestBody>> for EggserveToTower<S>
where
    S: Service + Clone + Send + 'static,
{
    type Response =
        http::Response<http_body_util::combinators::UnsyncBoxBody<Bytes, std::io::Error>>;
    type Error = TowerAdapterError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Adapter-local readiness only: the native trait has no `poll_ready`
        // and runtime admission is acquired per-request inside the driver.
        // Never claim transport admission here.
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<RequestBody>) -> Self::Future {
        // Clone the inner service so the returned future owns everything
        // (`tower_service::Service::Future` is not tied to `&mut self`).
        // Adapter-local readiness stays `Ready`; transport admission is never
        // claimed here.
        let service = self.inner.clone();
        // Reconstruct canonical parts from the http request. Exact raw-target
        // fidelity prefers `RawTargetExt` when a previous adapter preserved
        // it; otherwise origin-form rendering is validated (absolute-form
        // rejected, never normalized silently).
        let (mut parts, body) = req.into_parts();
        let extensions = std::mem::take(&mut parts.extensions);
        let head_req = http::Request::from_parts(parts, ());
        let mut head_req = head_req;
        *head_req.extensions_mut() = extensions;

        let fut = async move {
            let (method, target, version, headers, authority) =
                crate::primitives::interop::request_head_from_http(&head_req)
                    .map_err(TowerAdapterError::Interop)?;
            let connection = head_req
                .extensions()
                .get::<crate::primitives::interop::ConnectionInfoExt>()
                .map(|e| e.0.clone())
                .unwrap_or_else(|| {
                    crate::primitives::ConnectionInfo::without_socket_addrs(
                        crate::primitives::Scheme::Http,
                        None,
                    )
                });
            // Lifecycle: prefer the extension observer when the http request
            // came from `request_head_to_http`; otherwise derive from the
            // body so cancellation still observes the same allocation.
            let lifecycle = head_req
                .extensions()
                .get::<crate::primitives::interop::LifecycleExt>()
                .map(|e| e.0.clone())
                .unwrap_or_else(|| body.lifecycle());
            let is_head = method.is_head();
            let head = crate::primitives::RequestHead::new_with_authority(
                method, target, version, headers, authority,
            );
            let request = Request::new_with_context(
                head,
                body,
                crate::primitives::RequestContext::new(connection, lifecycle),
            );
            // Native invocation (panics/timeouts contained by the caller when
            // driven inside the runtime; here map errors to Tower errors).
            let response = service
                .call(request)
                .await
                .map_err(TowerAdapterError::Service)?;
            // Normalize (HEAD/body-forbidden suppression, framing, privacy)
            // before boxing for Tower.
            let normalized = normalize_response(response, &NormalizeRequest::new(is_head))
                .map_err(TowerAdapterError::Construction)?;
            let hyper_resp = crate::primitives::canonical::to_hyper_response(normalized)
                .map_err(TowerAdapterError::Construction)?;
            let (parts, body) = hyper_resp.into_parts();
            let boxed = http_body_util::BodyExt::boxed_unsync(body);
            Ok(http::Response::from_parts(parts, boxed))
        };
        Box::pin(fut)
    }
}

/// Errors from the [`EggserveToTower`] adapter.
///
/// Conversion failures (malformed http requests that cannot map losslessly)
/// are [`Interop`](TowerAdapterError::Interop); handler failures preserve the
/// native [`ServiceError`]; response-construction failures are
/// [`Construction`](TowerAdapterError::Construction). No internal paths or
/// file contents are reflected; client bodies stay sanitized by the runtime.
#[derive(Debug)]
pub enum TowerAdapterError {
    /// The http request could not be represented canonically.
    Interop(InteropError),
    /// The native service failed.
    Service(ServiceError),
    /// Response construction/normalization failed.
    Construction(crate::primitives::canonical::ResponseConstructionError),
}

impl std::fmt::Display for TowerAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Interop(e) => write!(f, "tower adapter conversion: {e}"),
            Self::Service(e) => write!(f, "tower adapter service: {e}"),
            Self::Construction(e) => write!(f, "tower adapter response: {e}"),
        }
    }
}

impl std::error::Error for TowerAdapterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Interop(e) => Some(e),
            Self::Service(e) => Some(e),
            Self::Construction(e) => Some(e),
        }
    }
}
