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

use crate::primitives::request_body_policy::RequestBodyPolicy;
use crate::response::BoxBodyInner;
use crate::server::config::RuntimeConfig;
use crate::server::service::{Service, ServiceError};

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
    is_h2: bool,
    disposition: LifecycleDisposition,
) -> hyper::Response<BoxBodyInner> {
    let (response, disposition) = guard.finish(response, config, conn_id, disposition);
    if is_h2 {
        response
    } else {
        apply_http1_disposition(response, disposition)
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
fn h1_trailers_allowed(head: &crate::primitives::request_head::RequestHead) -> bool {
    use crate::primitives::version::HttpVersion;
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
    }
}

/// Execute the protocol-neutral service kernel after a body policy has
/// prepared a canonical request. Body acquisition stays outside this helper;
/// admission, panic containment, timeout, error conversion, normalization, and
/// response conversion are deliberately shared by Reject, Buffer, and Stream.
///
/// Interim commitment is owned here: the request's interim sender (if any) is
/// cloned before `Service::call` consumes the request and marked committed
/// once the final outcome is known, so no interim can follow final commitment.
/// H1 trailer policy is also owned here: responses carrying trailers are
/// suppressed when the request version/TE forbids them (HTTP/1.0 never,
/// H1.1 only with `TE: trailers`; H2/H3 always allow protocol-native terminal
/// fields).
#[allow(clippy::too_many_arguments)]
async fn invoke_service<S>(
    guard: &mut InFlightGuard,
    service: &S,
    request: crate::primitives::request::Request,
    is_head: bool,
    timeout: std::time::Duration,
    stream_body: Option<Arc<crate::primitives::request_lifecycle::RequestShared>>,
    service_semaphore: &Arc<tokio::sync::Semaphore>,
    file_stream_semaphore: &Arc<tokio::sync::Semaphore>,
    stream_chunk_size: usize,
    error_policy: crate::policy::ErrorRepresentationPolicy,
    conn_id: u64,
    ops: &crate::ops::OpsContext,
) -> hyper::Response<BoxBodyInner>
where
    S: Service + 'static,
{
    if let Some(unavailable) = guard.admit(service_semaphore, conn_id, error_policy) {
        // Admission rejection commits implicitly: no service ran, but mark
        // interim committed so late sends cannot follow the 503.
        if let Some(interim) = request.context().interim() {
            interim.mark_committed();
        }
        return unavailable;
    }

    // Capture trailer policy + interim before the request moves into the service.
    let trailer_allowed = h1_trailers_allowed(request.head());
    let interim = request.context().interim().cloned();
    let result = tokio::time::timeout(timeout, contain_service_panic(service.call(request))).await;
    // Final commitment: no interim after this point regardless of outcome.
    if let Some(ref sender) = interim {
        sender.mark_committed();
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
            service_err.to_response_with_head_and_policy(is_head, error_policy)
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
                ServiceError::timeout("body read timeout".to_string())
                    .to_response_with_head_and_policy(is_head, error_policy)
            } else {
                ops.emit(crate::ops::Event::new(
                    crate::ops::Severity::Warn,
                    crate::ops::EventKind::ServiceTimeout,
                    "handler timed out",
                ));
                ServiceError::timeout("handler timed out".to_string())
                    .to_response_with_head_and_policy(is_head, error_policy)
            }
        }
    }
}

/// Concrete wrapper type for the canonical Hyper service returned by
/// [`make_canonical_hyper_service`].
///
/// Using a named type (rather than `impl Service`) preserves the `Send`
/// bound on the `Future` associated type, which is required by Hyper's
/// `serve_connection` when the task is spawned on a multi-threaded runtime.
#[allow(clippy::type_complexity)]
pub(crate) struct CanonicalHyperService {
    inner: std::sync::Arc<
        dyn Fn(
                hyper::Request<hyper::body::Incoming>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<hyper::Response<BoxBodyInner>, Infallible>,
                        > + Send,
                >,
            > + Send
            + Sync,
    >,
}

impl Clone for CanonicalHyperService {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl hyper::service::Service<hyper::Request<hyper::body::Incoming>> for CanonicalHyperService {
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

/// Build the shared per-request canonical pipeline as a Hyper service.
///
/// This is the single source of truth for the request lifecycle:
/// Hyper parsing, EggServe parser ceilings (header count/size,
/// request-target length), TRACE check, body policy, service admission,
/// service invocation, normalization, framing, incomplete-body close, and
/// response finalization. Both the TCP/TLS accept loop and the
/// transport-neutral driver ([`serve_http1_connection`]) share this
/// pipeline.
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
    activity: Arc<ConnectionActivity>,
    requests: Arc<ConnectionRequests>,
    stream_chunk_size: usize,
    handler_timeout: std::time::Duration,
    body_read_timeout: std::time::Duration,
    max_body_bytes: u64,
    context: ConnectionContext,
    conn_id: u64,
    ops: crate::ops::OpsContext,
) -> CanonicalHyperService
where
    S: Service + 'static,
{
    #[allow(clippy::type_complexity)]
    let handler: std::sync::Arc<
        dyn Fn(
                hyper::Request<hyper::body::Incoming>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<hyper::Response<BoxBodyInner>, Infallible>,
                        > + Send,
                >,
            > + Send
            + Sync,
    > = std::sync::Arc::new(move |req: Request<Incoming>| {
        let service = service.clone();
        let context = context.clone();
        let file_stream_semaphore = file_stream_semaphore.clone();
        let service_semaphore = service_semaphore.clone();
        let activity = activity.clone();
        let requests = requests.clone();
        let config = config.clone();
        let ops = ops.clone();
        Box::pin(async move {
            let mut guard = InFlightGuard::new(activity.clone());
            // Convert Hyper request to canonical RequestHead, enforcing the
            // EggServe-owned request-target and aggregate header ceilings
            // before any service work.
            let head = match convert_request_head(
                &req,
                config.max_request_target_bytes,
                config.max_header_bytes,
                context.scheme,
                conn_id,
                &ops,
            ) {
                Ok(h) => h,
                Err(e) => {
                    return Ok::<_, Infallible>(finish_response(
                        guard,
                        e.to_response_with_head_and_policy(
                            false,
                            config.response_policy.error_policy,
                        ),
                        &config,
                        conn_id,
                        req.version() == hyper::Version::HTTP_2,
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
                    || (head.version() != crate::primitives::version::HttpVersion::Http2
                        && req.headers().contains_key(hyper::header::TRANSFER_ENCODING)))
            {
                let response = crate::response::bad_request_with_policy(
                    false,
                    config.response_policy.error_policy,
                );
                return Ok::<_, Infallible>(finish_response(
                    guard,
                    response,
                    &config,
                    conn_id,
                    head.version() == crate::primitives::version::HttpVersion::Http2,
                    LifecycleDisposition::close_and_cancel_body(),
                ));
            }

            let is_head = head.method().is_head();
            let is_h2 = head.version() == crate::primitives::version::HttpVersion::Http2;

            // Select effective body policy.
            let service_policy = service.request_body_policy(&head);
            let effective_policy = select_body_policy(service_policy, max_body_bytes);

            // Extract body from Hyper request.
            let (parts, body) = req.into_parts();
            // `Content-Length` and `Transfer-Encoding` are HTTP/1 framing
            // signals. HTTP/2 has neither framing header requirement and can
            // carry DATA without Content-Length, so use Hyper's public body
            // state for the header-time Reject decision. `is_end_stream()`
            // is bounded: it observes the protocol stream state and never
            // polls or buffers request DATA.
            let body_is_end_stream = http_body::Body::is_end_stream(&body);

            // Validate body framing (TE+CL conflict, duplicate CL) for all methods.
            if head.version() != crate::primitives::version::HttpVersion::Http2 {
                if let Err(e) = validate_body_framing(&parts.headers) {
                    ops.counters()
                        .parser_rejects
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    ops.emit(
                        crate::ops::Event::new(
                            crate::ops::Severity::Debug,
                            crate::ops::EventKind::ParserRejection,
                            format!("parser rejection: {}", e),
                        )
                        .connection_id(conn_id),
                    );
                    let is_head = head.method().is_head();
                    return Ok::<_, Infallible>(finish_response(
                        guard,
                        e.to_response_with_head_and_policy(
                            is_head,
                            config.response_policy.error_policy,
                        ),
                        &config,
                        conn_id,
                        is_h2,
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
                        let err = crate::primitives::request_body_error::RequestBodyError::DeclaredLengthTooLarge {
                            declared: len,
                            limit,
                        };
                        let disposition = body_error_disposition(&err);
                        return Ok::<_, Infallible>(finish_response(
                            guard,
                            body_error_to_response(err, &head, config.response_policy.error_policy),
                            &config,
                            conn_id,
                            is_h2,
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
                        &config,
                        conn_id,
                        is_h2,
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
                        &config,
                        conn_id,
                        is_h2,
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
            // streaming limit.
            let has_body = if is_h2 {
                !body_is_end_stream
            } else {
                declared_length.is_some_and(|len| len > 0)
                    || parts.headers.contains_key(hyper::header::TRANSFER_ENCODING)
            };
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
                    &config,
                    conn_id,
                    is_h2,
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
            // frames (H1 chunked trailers, H2 terminal HEADERS); H1 without
            // valid framing cannot inject.
            let request_body = match &effective_policy {
                RequestBodyPolicy::Reject => crate::primitives::request_body::RequestBody::empty(),
                RequestBodyPolicy::Buffer { max_bytes }
                | RequestBodyPolicy::Stream { max_bytes } => {
                    let slot = crate::primitives::request_body::new_wire_slot();
                    let (stream, slot) = wrap_incoming_body_with_trailers(body, slot);
                    // Shared allocation so `RequestBody` and `RequestLifecycle`
                    // observe the same ownership state.
                    let shared = crate::primitives::request_lifecycle::RequestShared::new_active();
                    // `requests` registry needs the shared observer; register
                    // after construction below via the body's shared clone.
                    crate::primitives::request_body::RequestBody::from_incoming_with_shared_and_wire_slot(
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
                    let connection = context.connection_info();
                    requests.register(&request_body.shared());
                    let request =
                        crate::primitives::request::Request::new(head, request_body, connection);
                    let response = invoke_service(
                        &mut guard,
                        service.as_ref(),
                        request,
                        is_head,
                        handler_timeout,
                        None,
                        &service_semaphore,
                        &file_stream_semaphore,
                        stream_chunk_size,
                        config.response_policy.error_policy,
                        conn_id,
                        &ops,
                    )
                    .await;
                    Ok::<_, Infallible>(finish_response(
                        guard,
                        response,
                        &config,
                        conn_id,
                        is_h2,
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
                                Some(t) => crate::primitives::request_body::RequestBody::from_bytes_with_trailers(
                                    bytes, body_limit, t,
                                ),
                                None => crate::primitives::request_body::RequestBody::from_bytes(
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
                                &config,
                                conn_id,
                                is_h2,
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
                            let err = crate::primitives::request_body_error::RequestBodyError::ReadTimeout;
                            return Ok::<_, Infallible>(finish_response(
                                guard,
                                body_error_to_response(
                                    err,
                                    &head,
                                    config.response_policy.error_policy,
                                ),
                                &config,
                                conn_id,
                                is_h2,
                                LifecycleDisposition::close_and_cancel_body(),
                            ));
                        }
                    };
                    let connection = context.connection_info();
                    requests.register(&request_body.shared());
                    let request =
                        crate::primitives::request::Request::new(head, request_body, connection);
                    let response = invoke_service(
                        &mut guard,
                        service.as_ref(),
                        request,
                        is_head,
                        handler_timeout,
                        None,
                        &service_semaphore,
                        &file_stream_semaphore,
                        stream_chunk_size,
                        config.response_policy.error_policy,
                        conn_id,
                        &ops,
                    )
                    .await;
                    Ok::<_, Infallible>(finish_response(
                        guard,
                        response,
                        &config,
                        conn_id,
                        is_h2,
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
                    let connection = context.connection_info();
                    // Shared lifecycle observer retained by the runtime while
                    // the service owns/moves the actual body (Track A/B1).
                    let body_shared = request_body.shared();
                    requests.register(&body_shared);
                    let request =
                        crate::primitives::request::Request::new(head, request_body, connection);

                    let response = invoke_service(
                        &mut guard,
                        service.as_ref(),
                        request,
                        is_head,
                        effective_timeout,
                        Some(body_shared.clone()),
                        &service_semaphore,
                        &file_stream_semaphore,
                        stream_chunk_size,
                        config.response_policy.error_policy,
                        conn_id,
                        &ops,
                    )
                    .await;

                    // Ownership-derived reuse safety (Track B): returning the
                    // Response does not force a decision while the body is
                    // still Active (deferred to a downstream task). Hyper
                    // prevents next-request parsing until the framing
                    // boundary is complete (pinned by regression tests); EggServe
                    // forces close only on abandonment/failure.
                    use crate::primitives::request_lifecycle::BodyLifecycleState;
                    match body_shared.body_state() {
                        BodyLifecycleState::Complete => Ok::<_, Infallible>(finish_response(
                            guard,
                            response,
                            &config,
                            conn_id,
                            is_h2,
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
                                &config,
                                conn_id,
                                is_h2,
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
                                &config,
                                conn_id,
                                is_h2,
                                LifecycleDisposition::close_and_cancel_body(),
                            ))
                        }
                    }
                }
            }
        })
    });
    CanonicalHyperService { inner: handler }
}
