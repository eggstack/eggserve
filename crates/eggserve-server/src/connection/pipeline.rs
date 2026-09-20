//! Canonical request/service pipeline.
//!
//! Single request-processing source of truth for TCP/TLS and caller-owned
//! transports: Hyper request conversion, header/target/body framing
//! validation, service body-policy selection, service admission, service
//! invocation with panic containment, canonical response
//! normalization/conversion, and deferred-body lifecycle handling. Neither
//! `Server` nor caller-owned entry points grow alternate validation paths.

use std::convert::Infallible;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use hyper::body::Incoming;
use hyper::Request;

use crate::config::RuntimeConfig;
use crate::response::BoxBodyInner;
use crate::service::{Service, ServiceError};
use eggserve_primitives::request_body_policy::RequestBodyPolicy;

use super::activity::{ConnectionActivity, InFlightGuard};
use super::context::ConnectionContext;
use super::deferred_body::{spawn_body_timeout_watchdog, spawn_deferred_tracker};
use super::lifecycle::ConnectionRequests;
use super::lifecycle::LifecycleDisposition;
use super::request::{
    convert_request_head, select_body_policy, validate_body_framing,
    wrap_incoming_body_with_trailers,
};
use super::response::{
    apply_http1_disposition, body_error_disposition, body_error_to_response, contain_service_panic,
    normalize_then_convert,
};

fn finish_response(
    guard: InFlightGuard,
    response: hyper::Response<BoxBodyInner>,
    config: &RuntimeConfig,
    conn_id: u64,
    disposition: LifecycleDisposition,
) -> hyper::Response<BoxBodyInner> {
    let (response, disposition) = guard.finish(response, config, conn_id, disposition);
    apply_http1_disposition(response, disposition)
}

/// Apply trusted header-derived forwarding policy for one request (Plan 202 Track D).
///
/// Trust uses the immediate transport peer (`context.remote_addr`) or, for
/// peer-less transports, the explicit `trust_unix` flag. Untrusted peers,
/// disabled policy, conflicts, oversized chains, and malformed values all
/// fail closed to the base connection (raw peer preserved). Accepted values
/// enrich the provenance-tagged effective layer without rewriting the
/// canonical Host/target.
fn apply_forwarded_policy(
    base: eggserve_primitives::connection_info::ConnectionInfo,
    head: &eggserve_primitives::request_head::RequestHead,
    config: &RuntimeConfig,
    context: &ConnectionContext,
    conn_id: u64,
    ops: &crate::ops::OpsContext,
) -> eggserve_primitives::connection_info::ConnectionInfo {
    use eggserve_primitives::proxy::{derive_forwarded_effective, ForwardedRejection};

    if config.trusted_proxy.forwarded.is_disabled() {
        return base;
    }
    let trusted_peer = match context.remote_addr {
        Some(peer) => config.trusted_proxy.is_trusted_peer(&peer),
        None => config.trusted_proxy.trust_unix,
    };
    match derive_forwarded_effective(
        head.headers(),
        &config.trusted_proxy.forwarded,
        trusted_peer,
    ) {
        Ok(None) => base,
        Ok(Some(effective)) => {
            ops.counters()
                .forwarded_accepted
                .fetch_add(1, Ordering::Relaxed);
            let effective_client = effective
                .client
                .map(|addr| addr.to_string())
                .unwrap_or_else(|| "none".to_owned());
            let effective_scheme = effective
                .scheme
                .map(|scheme| scheme.as_str().to_owned())
                .unwrap_or_else(|| "none".to_owned());
            let effective_authority = effective
                .authority
                .as_ref()
                .map(|authority| authority.as_str().to_owned())
                .unwrap_or_else(|| "none".to_owned());
            let peer = context
                .remote_addr
                .map(|addr| addr.to_string())
                .unwrap_or_else(|| "unix-or-opaque".to_owned());
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Debug,
                    crate::ops::EventKind::ForwardedMetadataAccepted,
                    format!("forwarded metadata accepted ({})", effective.provenance),
                )
                .connection_id(conn_id)
                .field(crate::ops::Field::Str("peer".into(), peer))
                .field(crate::ops::Field::Str(
                    "source".into(),
                    effective.provenance.as_str().to_owned(),
                ))
                .field(crate::ops::Field::Str(
                    "effective_client".into(),
                    effective_client,
                ))
                .field(crate::ops::Field::Str(
                    "effective_scheme".into(),
                    effective_scheme,
                ))
                .field(crate::ops::Field::Str(
                    "effective_authority".into(),
                    effective_authority,
                )),
            );
            base.with_forwarded_effective(&effective)
        }
        Err(rejection) => {
            ops.counters()
                .forwarded_rejected
                .fetch_add(1, Ordering::Relaxed);
            let (severity, category) = match rejection {
                ForwardedRejection::UntrustedPeer => {
                    (crate::ops::Severity::Debug, "untrusted_peer")
                }
                ForwardedRejection::Disabled => (crate::ops::Severity::Debug, "disabled"),
                ForwardedRejection::Conflict => (crate::ops::Severity::Warn, "conflict"),
                ForwardedRejection::TooLarge => (crate::ops::Severity::Warn, "too_large"),
                ForwardedRejection::TooMany => (crate::ops::Severity::Warn, "too_many"),
                ForwardedRejection::Invalid => (crate::ops::Severity::Warn, "invalid"),
            };
            let peer = context
                .remote_addr
                .map(|addr| addr.to_string())
                .unwrap_or_else(|| "unix-or-opaque".to_owned());
            ops.emit(
                crate::ops::Event::new(
                    severity,
                    crate::ops::EventKind::ForwardedMetadataRejected,
                    format!("forwarded metadata rejected: {category}"),
                )
                .connection_id(conn_id)
                .field(crate::ops::Field::Str("peer".into(), peer))
                .field(crate::ops::Field::Str(
                    "category".into(),
                    category.to_owned(),
                )),
            );
            base
        }
    }
}

/// H1 trailer negotiation policy (Plan 198 Track D).
///
/// - HTTP/1.0: trailers unavailable, always suppressed.
/// - HTTP/1.1: trailers emitted only when the request indicates willingness
///   via `TE: trailers` (case-insensitive token). Otherwise suppressed with
///   diagnostics; the runtime never emits a `Trailer` header for suppressed
///   responses.
/// - HTTP/2, HTTP/3: protocol-native terminal fields, always allowed (H1
///   negotiation artifacts omitted).
///
/// Application code never controls transfer coding: services declare trailers
/// via `ResponseStream::with_trailers`, never by setting `Transfer-Encoding`
/// or `Trailer` (both stripped as runtime-owned in normalization).
fn h1_trailers_allowed(head: &eggserve_primitives::request_head::RequestHead) -> bool {
    use eggserve_primitives::version::HttpVersion;
    match head.version() {
        HttpVersion::Http10 => false,
        HttpVersion::Http11 => {
            let Some(te) = head.headers().get_first("te") else {
                return false;
            };
            let Ok(text) = te.to_str() else {
                return false;
            };
            text.split(',')
                .map(str::trim)
                .any(|token| token.eq_ignore_ascii_case("trailers"))
        }
        HttpVersion::Http2 | HttpVersion::Http3 => true,
        // Future protocol versions cannot carry H1-style trailers safely.
        _ => false,
    }
}

/// Convert an accepted tunnel handshake without ordinary normalization.
///
/// `TunnelCapability::accept` already produced a valid handshake (101 for H1
/// Upgrade with runtime-owned `Upgrade`/`Connection`, 200 for CONNECT,
/// bounded application headers, empty body). The ordinary normalizer would
/// strip the runtime-owned handshake as hop-by-hop, so it is bypassed here
/// by construction. Privacy finalization still applies in
/// `InFlightGuard::finish` (denylist, `Server` subordination, sole `Date`
/// authority). Conversion failure (unreachable for validated handshakes)
/// falls back to a generic 500 without leaking detail.
fn convert_handshake_without_normalization(
    canonical: eggserve_primitives::canonical::Response,
    file_stream_semaphore: &Arc<tokio::sync::Semaphore>,
    stream_chunk_size: usize,
    error_policy: eggserve_primitives::policy::ErrorRepresentationPolicy,
    ops: &crate::ops::OpsContext,
) -> hyper::Response<BoxBodyInner> {
    match crate::adapters::to_hyper_response_with_file_stream_semaphore_and_chunk_size(
        canonical,
        file_stream_semaphore,
        stream_chunk_size,
        Some(ops),
    ) {
        Ok(r) => r,
        Err(eggserve_primitives::canonical::ResponseConstructionError::FileStreamLimit) => {
            crate::response::service_unavailable_with_policy(error_policy)
        }
        Err(_) => crate::response::internal_error_with_policy(error_policy),
    }
}

/// One-shot tunnel invocation state for a single request.
///
/// Created by the pipeline when H1 classification yields a transport-backed
/// candidate; moved into [`invoke_service`] alongside the request. The
/// capability reaches the service via `Service::call_with_tunnel`; the
/// shared/sidecar/lifecycle stay pipeline-owned for commitment and
/// admission after the service returns.
pub(crate) struct TunnelInvocation {
    pub(crate) capability: crate::tunnel::TunnelCapability,
    pub(crate) shared: Arc<crate::tunnel::TunnelShared>,
    pub(crate) sidecar: Arc<std::sync::Mutex<Option<crate::tunnel::TunnelAcceptance>>>,
    pub(crate) lifecycle: eggserve_primitives::request_lifecycle::RequestLifecycle,
}

/// Execute the protocol-neutral service kernel after a body policy has
/// prepared a canonical request. Body acquisition stays outside this helper;
/// admission, panic containment, timeout, error conversion, normalization, and
/// response conversion are deliberately shared by Reject, Buffer, and Stream.
///
/// Interim commitment is owned here: the request's interim sender (if any) is
/// cloned before the service consumes the request and marked committed
/// once the final outcome is known, so no interim can follow final commitment.
/// Tunnel commitment is owned here too: the invocation's shared state is
/// marked committed once the final outcome is known, so a background task
/// holding a taken capability cannot accept after commitment.
/// H1 trailer policy is also owned here: responses carrying trailers are
/// suppressed when the request version/TE forbids them (HTTP/1.0 never,
/// H1.1 only with `TE: trailers`; H2/H3 always allow protocol-native terminal
/// fields).
#[allow(clippy::too_many_arguments)]
async fn invoke_service<S>(
    guard: &mut InFlightGuard,
    service: &S,
    request: eggserve_primitives::request::Request,
    is_head: bool,
    timeout: std::time::Duration,
    stream_body: Option<Arc<eggserve_primitives::request_lifecycle::RequestShared>>,
    service_semaphore: &Arc<tokio::sync::Semaphore>,
    file_stream_semaphore: &Arc<tokio::sync::Semaphore>,
    stream_chunk_size: usize,
    error_policy: eggserve_primitives::policy::ErrorRepresentationPolicy,
    conn_id: u64,
    ops: &crate::ops::OpsContext,
    activity: &Arc<ConnectionActivity>,
    tunnel_semaphore: &Arc<tokio::sync::Semaphore>,
    tunnel: Option<TunnelInvocation>,
) -> hyper::Response<BoxBodyInner>
where
    S: Service + 'static,
{
    if let Some(unavailable) = guard.admit(service_semaphore, conn_id, error_policy) {
        // Admission rejection commits implicitly: no service ran, but mark
        // interim + tunnel committed so late sends/accepts cannot follow 503.
        if let Some(interim) = request.context().interim() {
            interim.mark_committed();
        }
        if let Some(ref invocation) = tunnel {
            invocation.shared.mark_committed();
        }
        return unavailable;
    }

    // Capture trailer policy + interim + tunnel commitment/lifecycle before the
    // request moves into the service.
    let trailer_allowed = h1_trailers_allowed(request.head());
    let interim = request.context().interim().cloned();
    let tunnel_shared = tunnel.as_ref().map(|inv| inv.shared.clone());
    let tunnel_sidecar = tunnel.as_ref().map(|inv| inv.sidecar.clone());
    let tunnel_lifecycle = tunnel.as_ref().map(|inv| inv.lifecycle.clone());
    let capability = tunnel.map(|inv| inv.capability);
    let result = tokio::time::timeout(
        timeout,
        contain_service_panic(service.call_with_tunnel(request, capability)),
    )
    .await;
    // Final commitment: no interim/tunnel after this point regardless of outcome.
    if let Some(ref sender) = interim {
        sender.mark_committed();
    }
    if let Some(ref shared) = tunnel_shared {
        shared.mark_committed();
    }
    match result {
        Ok(Ok(mut canonical)) => {
            if canonical.has_response_trailers() && !trailer_allowed {
                ops.emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Debug,
                        crate::ops::EventKind::ResponseTrailerSuppressed,
                        "response trailers suppressed by H1 policy",
                    )
                    .connection_id(conn_id),
                );
                canonical.strip_response_trailers();
            }
            // Tunnel acceptance: when the service consumed the capability,
            // the sidecar holds exactly one staged acceptance. Admit via the
            // server-wide budget and spawn the tracked duplex task before
            // sending the validated handshake. Ordinary denial (no staged
            // acceptance) uses the normal path.
            if let Some(sidecar) = tunnel_sidecar {
                let acceptance = sidecar.lock().ok().and_then(|mut slot| slot.take());
                if let Some(acceptance) = acceptance {
                    match tunnel_lifecycle {
                        Some(lifecycle) => {
                            let admitted = crate::tunnel::admit_and_spawn(
                                activity,
                                tunnel_semaphore,
                                ops,
                                conn_id,
                                lifecycle,
                                acceptance,
                            )
                            .await;
                            if !admitted {
                                return crate::response::service_unavailable_with_policy(
                                    error_policy,
                                );
                            }
                            return convert_handshake_without_normalization(
                                canonical,
                                file_stream_semaphore,
                                stream_chunk_size,
                                error_policy,
                                ops,
                            );
                        }
                        None => {
                            // Impossible by construction (acceptance implies an
                            // invocation carried a lifecycle). Drop the staged
                            // acceptance fail-safe and use the normal path;
                            // the handler never runs and `OnUpgrade` fails safe.
                        }
                    }
                }
            }
            normalize_then_convert(
                canonical,
                is_head,
                file_stream_semaphore,
                stream_chunk_size,
                error_policy,
                Some(ops),
            )
        }
        Ok(Err(service_err)) => {
            let severity = if service_err.is_panic() || !service_err.is_timeout() {
                crate::ops::Severity::Error
            } else {
                crate::ops::Severity::Warn
            };
            ops.emit(
                crate::ops::Event::new(
                    severity,
                    crate::ops::EventKind::ServiceError,
                    crate::ops::sanitize_text_field(&service_err.to_string()),
                )
                .connection_id(conn_id),
            );
            super::response::service_error_to_response(&service_err, is_head, error_policy)
        }
        Err(_elapsed) => {
            let body_pending = stream_body
                .as_ref()
                .is_some_and(|shared| shared.is_body_active());
            if body_pending {
                ops.counters()
                    .body_read_timeouts
                    .fetch_add(1, Ordering::Relaxed);
                ops.emit(crate::ops::Event::new(
                    crate::ops::Severity::Warn,
                    crate::ops::EventKind::BodyReadTimeout,
                    "body read timeout",
                ));
                super::response::service_error_to_response(
                    &ServiceError::timeout("body read timeout".to_string()),
                    is_head,
                    error_policy,
                )
            } else {
                ops.emit(crate::ops::Event::new(
                    crate::ops::Severity::Warn,
                    crate::ops::EventKind::ServiceTimeout,
                    "handler timed out",
                ));
                super::response::service_error_to_response(
                    &ServiceError::timeout("handler timed out".to_string()),
                    is_head,
                    error_policy,
                )
            }
        }
    }
}

/// Build the canonical request plus an optional one-shot tunnel invocation.
///
/// When `candidate` is `Some` (validated H1 intent + transport handoff), the
/// intent is recorded cloneably on the request context for routing, and a
/// server-owned capability (with shared commitment state + pipeline sidecar)
/// is returned for `Service::call_with_tunnel`. Otherwise an ordinary
/// request with no capability is returned. The body lifecycle always shares
/// the body's allocation so completion/cancellation observations converge.
fn build_request_with_tunnel(
    head: eggserve_primitives::request_head::RequestHead,
    body: eggserve_primitives::request_body::RequestBody,
    connection: eggserve_primitives::connection_info::ConnectionInfo,
    candidate: Option<(
        eggserve_primitives::tunnel::TunnelRequest,
        hyper::upgrade::OnUpgrade,
    )>,
) -> (
    eggserve_primitives::request::Request,
    Option<TunnelInvocation>,
) {
    let Some((intent, upgrade)) = candidate else {
        return (
            eggserve_primitives::request::Request::new(head, body, connection),
            None,
        );
    };
    let version = head.version();
    let lifecycle = body.lifecycle();
    let ctx = eggserve_primitives::request_context::RequestContext::new_with_version(
        connection,
        lifecycle.clone(),
        version,
    )
    .with_tunnel_request(intent.clone());
    let request = eggserve_primitives::request::Request::new_with_context(head, body, ctx);
    let shared = Arc::new(crate::tunnel::TunnelShared::new());
    let sidecar = Arc::new(std::sync::Mutex::new(None));
    let capability = crate::tunnel::TunnelCapability::new(
        intent,
        Some(upgrade),
        shared.clone(),
        sidecar.clone(),
    );
    (
        request,
        Some(TunnelInvocation {
            capability,
            shared,
            sidecar,
            lifecycle,
        }),
    )
}

/// Concrete wrapper type for the canonical Hyper service returned by
/// [`make_canonical_hyper_service`].
///
/// Using a named type (rather than `impl Service`) preserves the `Send`
/// bound on the `Future` associated type, which is required by Hyper's
/// `serve_connection` when the task is spawned on a multi-threaded runtime.
#[allow(clippy::type_complexity)]
pub(crate) struct CanonicalHyperService<F> {
    inner: F,
}

impl<F: Clone> Clone for CanonicalHyperService<F> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<F> hyper::service::Service<hyper::Request<hyper::body::Incoming>> for CanonicalHyperService<F>
where
    F: Fn(
            hyper::Request<hyper::body::Incoming>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<hyper::Response<BoxBodyInner>, Infallible>>
                    + Send,
            >,
        > + Send
        + Sync
        + 'static,
{
    type Response = hyper::Response<BoxBodyInner>;
    type Error = Infallible;
    type Future = std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<hyper::Response<BoxBodyInner>, Infallible>>
                + Send,
        >,
    >;

    fn call(&self, req: hyper::Request<hyper::body::Incoming>) -> Self::Future {
        (self.inner)(req)
    }
}

type CanonicalHyperFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<hyper::Response<BoxBodyInner>, Infallible>> + Send>,
>;

pub(crate) trait ReadyCanonicalHyperService:
    hyper::service::Service<
        hyper::Request<hyper::body::Incoming>,
        Response = hyper::Response<BoxBodyInner>,
        Error = Infallible,
        Future = CanonicalHyperFuture,
    > + Clone
    + Send
    + 'static
where
    Self::Future: Send + 'static,
{
}

impl<T> ReadyCanonicalHyperService for T
where
    T: hyper::service::Service<
            hyper::Request<hyper::body::Incoming>,
            Response = hyper::Response<BoxBodyInner>,
            Error = Infallible,
            Future = CanonicalHyperFuture,
        > + Clone
        + Send
        + 'static,
    T::Future: Send + 'static,
{
}

/// Immutable values shared by every request on one connection.
///
/// Keeping these values behind one connection-level handle avoids cloning a
/// dozen independent `Arc`s in the Hyper service closure for every request.
/// Request-local body/lifecycle state remains owned by the request pipeline.
struct PipelineState<S> {
    service: Arc<S>,
    config: Arc<RuntimeConfig>,
    file_stream_semaphore: Arc<tokio::sync::Semaphore>,
    service_semaphore: Arc<tokio::sync::Semaphore>,
    tunnel_semaphore: Arc<tokio::sync::Semaphore>,
    activity: Arc<ConnectionActivity>,
    requests: Arc<ConnectionRequests>,
    stream_chunk_size: usize,
    handler_timeout: std::time::Duration,
    body_read_timeout: std::time::Duration,
    max_body_bytes: u64,
    context: ConnectionContext,
    conn_id: u64,
    ops: crate::ops::OpsContext,
}

/// Build the shared per-request canonical H1 pipeline as a Hyper service.
///
/// This is the single source of truth for the H1 request lifecycle:
/// Hyper parsing, EggServe parser ceilings (header count/size,
/// request-target length), TRACE check, body policy, service admission,
/// service invocation, normalization, framing, incomplete-body close, and
/// response finalization. Both the TCP accept loop and the
/// transport-neutral driver ([`serve_http1_connection`](super::serve_http1_connection))
/// share this pipeline.
///
/// Every response handed to Hyper — including parse rejections and policy
/// errors — passes through [`InFlightGuard::finish`], which counts it
/// toward `max_requests_per_connection`, arms the write-progress budget,
/// and wraps the body so completion releases the outstanding slot.
#[allow(clippy::too_many_arguments)]
pub(crate) fn make_canonical_hyper_service<S>(
    service: Arc<S>,
    config: Arc<RuntimeConfig>,
    file_stream_semaphore: Arc<tokio::sync::Semaphore>,
    service_semaphore: Arc<tokio::sync::Semaphore>,
    tunnel_semaphore: Arc<tokio::sync::Semaphore>,
    activity: Arc<ConnectionActivity>,
    requests: Arc<ConnectionRequests>,
    stream_chunk_size: usize,
    handler_timeout: std::time::Duration,
    body_read_timeout: std::time::Duration,
    max_body_bytes: u64,
    context: ConnectionContext,
    conn_id: u64,
    ops: crate::ops::OpsContext,
) -> impl ReadyCanonicalHyperService
where
    S: Service + 'static,
{
    let state = Arc::new(PipelineState {
        service,
        config,
        file_stream_semaphore,
        service_semaphore,
        tunnel_semaphore,
        activity,
        requests,
        stream_chunk_size,
        handler_timeout,
        body_read_timeout,
        max_body_bytes,
        context,
        conn_id,
        ops,
    });
    let handler = move |req: Request<Incoming>| -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<hyper::Response<BoxBodyInner>, Infallible>>
                + Send,
        >,
    > {
        let state = Arc::clone(&state);
        Box::pin(async move {
            let service = &state.service;
            let config = &state.config;
            let file_stream_semaphore = &state.file_stream_semaphore;
            let service_semaphore = &state.service_semaphore;
            let tunnel_semaphore = &state.tunnel_semaphore;
            let activity = &state.activity;
            let requests = &state.requests;
            let context = &state.context;
            let stream_chunk_size = state.stream_chunk_size;
            let handler_timeout = state.handler_timeout;
            let body_read_timeout = state.body_read_timeout;
            let max_body_bytes = state.max_body_bytes;
            let conn_id = state.conn_id;
            let ops = &state.ops;
            let mut guard = InFlightGuard::new(Arc::clone(activity));
            // Convert Hyper request to canonical RequestHead, enforcing the
            // EggServe-owned request-target and aggregate header ceilings
            // before any service work.
            let head = match convert_request_head(
                &req,
                config.max_request_target_bytes,
                config.max_header_bytes,
                context.scheme,
                conn_id,
                ops,
            ) {
                Ok(h) => h,
                Err(e) => {
                    return Ok::<_, Infallible>(finish_response(
                        guard,
                        super::response::service_error_to_response(
                            &e,
                            false,
                            config.response_policy.error_policy,
                        ),
                        config,
                        conn_id,
                        LifecycleDisposition::KEEP_ALIVE,
                    ));
                }
            };

            // TRACE content remains a transport-level rejection. Other
            // methods, including GET, HEAD, and DELETE, are governed by the
            // service-declared policy below.
            if head.method().as_str() == "TRACE"
                && (req
                    .headers()
                    .get(hyper::header::CONTENT_LENGTH)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .is_some_and(|length| length > 0)
                    || req.headers().contains_key(hyper::header::TRANSFER_ENCODING))
            {
                let response = crate::response::bad_request_with_policy(
                    false,
                    config.response_policy.error_policy,
                );
                return Ok::<_, Infallible>(finish_response(
                    guard,
                    response,
                    config,
                    conn_id,
                    LifecycleDisposition::close_and_cancel_body(),
                ));
            }

            let is_head = head.method().is_head();

            // Plan 202 Track D: trusted header-derived effective metadata.
            // Raw peer/local endpoints stay preserved in `context`; accepted
            // values populate the provenance-tagged effective layer only.
            let connection_template = apply_forwarded_policy(
                context.connection_info(),
                &head,
                config,
                context,
                conn_id,
                ops,
            );

            // Select effective body policy.
            let service_policy = service.request_body_policy(&head);
            let effective_policy = select_body_policy(service_policy, max_body_bytes);

            // Extract body from Hyper request.
            //
            // Transport-backed upgrade capability (Plan 216): `OnUpgrade`
            // (includes buffered H1 read-ahead) is removed here so the
            // canonical pipeline owns it; ordinary denial drops it safely
            // (pending sender fails safe). Without a validated candidate
            // below, intent follows the ordinary HTTP path.
            let (mut parts, body) = req.into_parts();
            let on_upgrade: Option<hyper::upgrade::OnUpgrade> =
                parts.extensions.remove::<hyper::upgrade::OnUpgrade>();

            // Validate body framing (TE+CL conflict, duplicate CL) for all methods.
            {
                if let Err(e) = validate_body_framing(&parts.headers) {
                    ops.counters()
                        .parser_rejects
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    ops.emit(
                        crate::ops::Event::new(
                            crate::ops::Severity::Debug,
                            crate::ops::EventKind::ParserRejection,
                            format!("parser rejection: {e}"),
                        )
                        .connection_id(conn_id),
                    );
                    let is_head = head.method().is_head();
                    return Ok::<_, Infallible>(finish_response(
                        guard,
                        super::response::service_error_to_response(
                            &e,
                            is_head,
                            config.response_policy.error_policy,
                        ),
                        config,
                        conn_id,
                        LifecycleDisposition::KEEP_ALIVE,
                    ));
                }
            }

            let declared_length = parts
                .headers
                .get(hyper::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());

            // Validate Content-Length against effective limit.
            if let Some(len) = declared_length {
                if let Some(limit) = effective_policy.max_bytes() {
                    if len > limit {
                        ops.counters()
                            .body_rejections
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        ops.emit(
                            crate::ops::Event::new(
                                crate::ops::Severity::Debug,
                                crate::ops::EventKind::BodyPolicyRejection,
                                "body too large",
                            )
                            .connection_id(conn_id)
                            .field(crate::ops::Field::U64("declared_bytes".into(), len))
                            .field(crate::ops::Field::U64("limit_bytes".into(), limit)),
                        );
                        let err = eggserve_primitives::request_body_error::RequestBodyError::DeclaredLengthTooLarge {
                            declared: len,
                            limit,
                        };
                        let disposition = body_error_disposition(&err);
                        return Ok::<_, Infallible>(finish_response(
                            guard,
                            body_error_to_response(err, &head, config.response_policy.error_policy),
                            config,
                            conn_id,
                            disposition,
                        ));
                    }
                }
            }

            // Expect handling (Plan 198 Track F): deterministic with body policy.
            // - Unknown (non-100-continue) expectations fail with 417 without
            //   inviting the body.
            // - `Reject` + `100-continue` is rejected early (413) without
            //   encouraging the client to send the body.
            // - `Buffer`/`Stream` + `100-continue` is accepted: Hyper owns wire
            //   `100` emission when the body is polled; EggServe owns the policy
            //   decision. App-generated 100s via the interim capability never
            //   duplicate the runtime `100` on the wire (interims are validated
            //   and recorded; Hyper server APIs own emission where permitted).
            if let Some(expect) = parts.headers.get(hyper::header::EXPECT) {
                let value = expect.to_str().ok().map(str::trim).unwrap_or("");
                if !value.eq_ignore_ascii_case("100-continue") && !value.is_empty() {
                    ops.emit(
                        crate::ops::Event::new(
                            crate::ops::Severity::Debug,
                            crate::ops::EventKind::ExpectationFailed,
                            "unknown Expect header",
                        )
                        .connection_id(conn_id),
                    );
                    let response = crate::response::expectation_failed_with_policy(
                        false,
                        config.response_policy.error_policy,
                    );
                    return Ok::<_, Infallible>(finish_response(
                        guard,
                        response,
                        config,
                        conn_id,
                        LifecycleDisposition::KEEP_ALIVE,
                    ));
                }
                if effective_policy.is_reject() && value.eq_ignore_ascii_case("100-continue") {
                    ops.counters()
                        .body_rejections
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    ops.emit(
                        crate::ops::Event::new(
                            crate::ops::Severity::Debug,
                            crate::ops::EventKind::BodyPolicyRejection,
                            "100-continue rejected by body policy",
                        )
                        .connection_id(conn_id),
                    );
                    let response = crate::response::payload_too_large_with_policy(
                        is_head,
                        config.response_policy.error_policy,
                    );
                    return Ok::<_, Infallible>(finish_response(
                        guard,
                        response,
                        config,
                        conn_id,
                        LifecycleDisposition::close_and_cancel_body(),
                    ));
                }
            }

            // Handle Reject policy — reject without invoking the service,
            // but only if the request actually carries a body.
            // A `Transfer-Encoding` header is treated as has-body even for
            // zero-length chunked input (`0\r\n\r\n`), since framing is
            // unknown until the stream is consumed. Size enforcement for
            // chunked bodies without `Content-Length` is deferred to the
            // streaming limit. H1 Reject stays framing-based (Plan 189).
            let has_body = declared_length.is_some_and(|len| len > 0)
                || parts.headers.contains_key(hyper::header::TRANSFER_ENCODING);
            if effective_policy.is_reject() && has_body {
                ops.counters()
                    .body_rejections
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                ops.emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Debug,
                        crate::ops::EventKind::BodyPolicyRejection,
                        "request body rejected by policy",
                    )
                    .connection_id(conn_id),
                );
                ops.emit(
                    crate::ops::Event::new(
                        crate::ops::Severity::Debug,
                        crate::ops::EventKind::ServiceInvocationSuppressed,
                        "service invocation suppressed: body rejected by policy",
                    )
                    .connection_id(conn_id),
                );
                let response = crate::response::payload_too_large_with_policy(
                    is_head,
                    config.response_policy.error_policy,
                );
                // Do not drain the body — drop it and close the connection to
                // prevent unread bytes from being interpreted as a subsequent
                // request. Hyper handles cleanup of the unconsumed body when
                // the connection is dropped.
                return Ok::<_, Infallible>(finish_response(
                    guard,
                    response,
                    config,
                    conn_id,
                    LifecycleDisposition::close_and_cancel_body(),
                ));
            }

            // For Buffer/Stream policies, create RequestBody with proper limits.
            // For Reject with no body, create an empty body (nothing to reject).
            // B-01: `declared_length > limit` is rejected above before `RequestBody`
            // construction. For `Transfer-Encoding: chunked` (no declared length)
            // enforcement is via `max_bytes` only. `Buffer` pre-buffers with
            // `read_all()` and fails fast; `Stream` delegates to the handler
            // under `min(body_read_timeout, handler_timeout)` and fails lazily
            // as `RequestBody` is consumed — intentional behavioral difference.
            // Trailers ride a wire slot populated only from protocol trailer
            // frames (H1 chunked trailers); H1 without valid framing cannot
            // inject.
            //
            // Tunnel classification (Plan 216): validated H1 Upgrade /
            // CONNECT intent becomes intent metadata plus a one-shot
            // transport-backed capability. `has_body` true => no capability
            // (smuggled body never crosses the transition). `OnUpgrade`
            // presence required; H2 Extended CONNECT stays
            // compatibility-owned until Plan 217.
            let mut tunnel_candidate = crate::tunnel::classify_tunnel(&head, has_body, on_upgrade);
            let request_body = match &effective_policy {
                RequestBodyPolicy::Reject => {
                    eggserve_primitives::request_body::RequestBody::empty()
                }
                RequestBodyPolicy::Buffer { max_bytes }
                | RequestBodyPolicy::Stream { max_bytes } => {
                    let slot = eggserve_primitives::request_body::new_wire_slot();
                    let (stream, slot) = wrap_incoming_body_with_trailers(body, slot);
                    // Shared allocation so `RequestBody` and `RequestLifecycle`
                    // observe the same ownership state.
                    let shared =
                        eggserve_primitives::request_lifecycle::RequestShared::new_active();
                    // `requests` registry needs the shared observer; register
                    // after construction below via the body's shared clone.
                    eggserve_primitives::request_body::RequestBody::from_incoming_with_shared_and_wire_slot(
                        stream,
                        declared_length,
                        *max_bytes,
                        shared,
                        slot,
                    )
                }
            };

            // For Buffer policy, pre-buffer the body under timeout.
            match &effective_policy {
                RequestBodyPolicy::Reject => {
                    let connection = connection_template.clone();
                    requests.register(&request_body.shared());
                    let (request, tunnel) = build_request_with_tunnel(
                        head,
                        request_body,
                        connection,
                        tunnel_candidate.take(),
                    );
                    let response = invoke_service(
                        &mut guard,
                        service.as_ref(),
                        request,
                        is_head,
                        handler_timeout,
                        None,
                        service_semaphore,
                        file_stream_semaphore,
                        stream_chunk_size,
                        config.response_policy.error_policy,
                        conn_id,
                        ops,
                        activity,
                        tunnel_semaphore,
                        tunnel,
                    )
                    .await;
                    Ok::<_, Infallible>(finish_response(
                        guard,
                        response,
                        config,
                        conn_id,
                        LifecycleDisposition::KEEP_ALIVE,
                    ))
                }
                RequestBodyPolicy::Buffer { .. } => {
                    // Buffer: body is fully consumed during pre-buffering.
                    // No incomplete body handling needed. Trailers are
                    // preserved via `read_all_with_trailers` (not discarded).
                    let body_limit = match effective_policy {
                        RequestBodyPolicy::Buffer { max_bytes } => max_bytes,
                        _ => unreachable!("buffer branch requires a buffer policy"),
                    };
                    let request_body = match tokio::time::timeout(
                        body_read_timeout,
                        request_body.read_all_with_trailers(),
                    )
                    .await
                    {
                        Ok(Ok((bytes, trailers))) => {
                            match trailers {
                                Some(t) => eggserve_primitives::request_body::RequestBody::from_bytes_with_trailers(
                                    bytes, body_limit, t,
                                ),
                                None => eggserve_primitives::request_body::RequestBody::from_bytes(
                                    bytes, body_limit,
                                ),
                            }
                        }
                        Ok(Err(err)) => {
                            let disposition = body_error_disposition(&err);
                            return Ok::<_, Infallible>(finish_response(
                                guard,
                                body_error_to_response(
                                    err,
                                    &head,
                                    config.response_policy.error_policy,
                                ),
                                config,
                                conn_id,
                                disposition,
                            ));
                        }
                        Err(_elapsed) => {
                            ops.counters()
                                .body_read_timeouts
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            ops.emit(crate::ops::Event::new(
                                crate::ops::Severity::Warn,
                                crate::ops::EventKind::BodyReadTimeout,
                                "body read timeout",
                            ));
                            let err = eggserve_primitives::request_body_error::RequestBodyError::ReadTimeout;
                            return Ok::<_, Infallible>(finish_response(
                                guard,
                                body_error_to_response(
                                    err,
                                    &head,
                                    config.response_policy.error_policy,
                                ),
                                config,
                                conn_id,
                                LifecycleDisposition::close_and_cancel_body(),
                            ));
                        }
                    };
                    let connection = connection_template.clone();
                    requests.register(&request_body.shared());
                    let (request, tunnel) = build_request_with_tunnel(
                        head,
                        request_body,
                        connection,
                        tunnel_candidate.take(),
                    );
                    let response = invoke_service(
                        &mut guard,
                        service.as_ref(),
                        request,
                        is_head,
                        handler_timeout,
                        None,
                        service_semaphore,
                        file_stream_semaphore,
                        stream_chunk_size,
                        config.response_policy.error_policy,
                        conn_id,
                        ops,
                        activity,
                        tunnel_semaphore,
                        tunnel,
                    )
                    .await;
                    Ok::<_, Infallible>(finish_response(
                        guard,
                        response,
                        config,
                        conn_id,
                        LifecycleDisposition::KEEP_ALIVE,
                    ))
                }
                RequestBodyPolicy::Stream { .. } => {
                    // Plan 174 Track C (compatibility-preserving split):
                    // during `Service::call` the two deadlines remain
                    // collapsed as `min(body_read_timeout, handler_timeout)`
                    // with body-vs-handler disambiguation, preserving the
                    // documented pre-174 behavior for conventional handlers.
                    // Once response-start is available with an Active
                    // deferred body, `body_read_timeout` continues as a total
                    // deadline via the watchdog below while `handler_timeout`
                    // no longer applies to the downstream task.
                    let effective_timeout = body_read_timeout.min(handler_timeout);
                    // Total body deadline from ingestion start for the
                    // post-return watchdog.
                    let body_deadline = tokio::time::Instant::now() + body_read_timeout;
                    let connection = connection_template.clone();
                    // Shared lifecycle observer retained by the runtime while
                    // the service owns/moves the actual body (Track A/B1).
                    let body_shared = request_body.shared();
                    requests.register(&body_shared);
                    let (request, tunnel) = build_request_with_tunnel(
                        head,
                        request_body,
                        connection,
                        tunnel_candidate.take(),
                    );

                    let response = invoke_service(
                        &mut guard,
                        service.as_ref(),
                        request,
                        is_head,
                        effective_timeout,
                        Some(body_shared.clone()),
                        service_semaphore,
                        file_stream_semaphore,
                        stream_chunk_size,
                        config.response_policy.error_policy,
                        conn_id,
                        ops,
                        activity,
                        tunnel_semaphore,
                        tunnel,
                    )
                    .await;

                    // Ownership-derived reuse safety (Track B): returning the
                    // Response does not force a decision while the body is
                    // still Active (deferred to a downstream task). Hyper
                    // prevents next-request parsing until the framing
                    // boundary is complete (pinned by regression tests); EggServe
                    // forces close only on abandonment/failure.
                    use eggserve_primitives::request_lifecycle::BodyLifecycleState;
                    match body_shared.body_state() {
                        BodyLifecycleState::Complete => Ok::<_, Infallible>(finish_response(
                            guard,
                            response,
                            config,
                            conn_id,
                            LifecycleDisposition::KEEP_ALIVE,
                        )),
                        BodyLifecycleState::Active => {
                            // Deferred: response-start available while a valid
                            // downstream task still owns the body. Do NOT add
                            // `Connection: close` merely because response-start
                            // came first. Track for idle accounting and
                            // completion observability; permits remain distinct
                            // (Track F): service admission already released by
                            // `finish`, downstream owns its own budget.
                            ops.counters()
                                .deferred_bodies_delegated
                                .fetch_add(1, Ordering::Relaxed);
                            ops.emit(
                                crate::ops::Event::new(
                                    crate::ops::Severity::Debug,
                                    crate::ops::EventKind::DeferredBodyDelegated,
                                    "request body delegated past service return",
                                )
                                .connection_id(conn_id),
                            );
                            activity.deferred_started();
                            spawn_deferred_tracker(
                                body_shared.clone(),
                                activity.clone(),
                                conn_id,
                                ops.clone(),
                            );
                            // Arm the remaining body deadline for deferred
                            // consumption. If already past deadline, the
                            // watchdog fires immediately.
                            {
                                let now = tokio::time::Instant::now();
                                let deadline = if body_deadline > now {
                                    body_deadline
                                } else {
                                    now
                                };
                                spawn_body_timeout_watchdog(
                                    body_shared.clone(),
                                    activity.clone(),
                                    deadline,
                                    conn_id,
                                    ops.clone(),
                                );
                            }
                            Ok::<_, Infallible>(finish_response(
                                guard,
                                response,
                                config,
                                conn_id,
                                LifecycleDisposition::KEEP_ALIVE,
                            ))
                        }
                        BodyLifecycleState::Abandoned | BodyLifecycleState::Failed => {
                            ops.emit(
                                crate::ops::Event::new(
                                    crate::ops::Severity::Debug,
                                    crate::ops::EventKind::IncompleteBodyClose,
                                    "service returned with abandoned/failed body; connection will close",
                                )
                                .connection_id(conn_id),
                            );
                            Ok::<_, Infallible>(finish_response(
                                guard,
                                response,
                                config,
                                conn_id,
                                LifecycleDisposition::close_and_cancel_body(),
                            ))
                        }
                    }
                }
            }
        })
    };
    CanonicalHyperService { inner: handler }
}
