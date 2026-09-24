//! Canonical response construction: normalization, panic containment,
//! body-error mapping, and final-boundary privacy.
//!
//! The runtime is the only framing authority: every service response
//! converges on `normalize_then_convert` (idempotent `normalize_response`
//! then Hyper conversion). `finalize_runtime_response` applies the Plan 165
//! privacy boundary (denylist, `Server` subordination, sole `Date` authority,
//! `Last-Modified <= Date`) at the one Hyper boundary.

use crate::config::{H1ConnectionPolicy, RuntimeConfig};
use crate::response::BoxBodyInner;
use crate::service::ServiceError;

use super::lifecycle::LifecycleDisposition;

/// Present an EggServe-selected rejection while retaining status, framing,
/// and response privacy authority. Invalid output or presenter panics use the
/// existing fixed runtime representation.
pub(crate) fn present_runtime_rejection(
    status: hyper::StatusCode,
    kind: crate::rejection::RuntimeRejectionKind,
    is_head: bool,
    policy: &H1ConnectionPolicy,
) -> hyper::Response<BoxBodyInner> {
    let fallback = || {
        crate::response::runtime_error_with_policy(
            status,
            is_head,
            policy.response_policy.error_policy,
        )
    };
    let Some(presenter) = &policy.runtime_rejection_presenter else {
        return fallback();
    };
    let status_value = eggserve_primitives::StatusCode::new(status.as_u16())
        .unwrap_or(eggserve_primitives::StatusCode::INTERNAL_SERVER_ERROR);
    let rejection = crate::rejection::RuntimeRejection::new(kind, status_value);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        presenter.present(&rejection)
    }));
    let Ok(Some(presentation)) = result else {
        return fallback();
    };
    if presentation.body.len() > crate::rejection::MAX_RUNTIME_REJECTION_BODY_BYTES {
        return fallback();
    }
    let mut headers = hyper::HeaderMap::new();
    let mut bytes = 0usize;
    for field in presentation.headers.iter().take(65) {
        let name = field.name.as_str();
        let lower = name.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "content-length"
                | "transfer-encoding"
                | "connection"
                | "keep-alive"
                | "upgrade"
                | "trailer"
                | "date"
                | "server"
        ) {
            continue;
        }
        let (Ok(name), Ok(value)) = (
            hyper::header::HeaderName::from_bytes(name.as_bytes()),
            hyper::header::HeaderValue::from_bytes(field.value.as_bytes()),
        ) else {
            return fallback();
        };
        bytes = bytes
            .saturating_add(name.as_str().len())
            .saturating_add(value.as_bytes().len());
        if bytes > 16 * 1024 || headers.len() >= 64 {
            return fallback();
        }
        headers.append(name, value);
    }
    let body_allowed = !is_head
        && eggserve_primitives::StatusCode::new(status.as_u16())
            .is_ok_and(|s| s.permits_payload_body());
    let representation_len = presentation.body.len();
    let body = if body_allowed {
        presentation.body
    } else {
        Vec::new()
    };
    let mut response = hyper::Response::builder()
        .status(status)
        .body(crate::response::full_body_bytes(body))
        .unwrap_or_else(|_| fallback());
    *response.headers_mut() = headers;
    // Recompute framing from the selected status and owned bytes. Body-forbidden
    // and HEAD responses intentionally carry no payload.
    response.headers_mut().remove(hyper::header::CONTENT_LENGTH);
    if status != hyper::StatusCode::NO_CONTENT
        && status != hyper::StatusCode::RESET_CONTENT
        && (status != hyper::StatusCode::NOT_MODIFIED || representation_len > 0)
    {
        let length = if is_head || body_allowed {
            representation_len
        } else {
            0
        };
        if let Ok(value) = hyper::header::HeaderValue::from_str(&length.to_string()) {
            response
                .headers_mut()
                .insert(hyper::header::CONTENT_LENGTH, value);
        }
    }
    response
}

/// Normalize a service response then convert to Hyper.
///
/// The runtime is the only framing authority: every service response
/// (static or custom, buffered or streaming) converges here. Normalization
/// is idempotent so eagerly normalized static responses are preserved
/// (HEAD equivalent-GET lengths, unknown-length omission). Conversion
/// failures become generic 500/503 without leaking details.
pub(crate) fn normalize_then_convert(
    canonical: eggserve_primitives::canonical::Response,
    is_head: bool,
    file_stream_semaphore: &std::sync::Arc<tokio::sync::Semaphore>,
    stream_chunk_size: usize,
    error_policy: eggserve_primitives::policy::ErrorRepresentationPolicy,
    ops: Option<&crate::ops::OpsContext>,
) -> hyper::Response<BoxBodyInner> {
    let normalized = match eggserve_primitives::canonical::normalize_response(
        canonical,
        &eggserve_primitives::canonical::NormalizeRequest::new(is_head),
    ) {
        Ok(r) => r,
        Err(_) => return crate::response::internal_error_with_policy(error_policy),
    };
    match crate::adapters::to_hyper_response_with_file_stream_semaphore_and_chunk_size(
        normalized,
        file_stream_semaphore,
        stream_chunk_size,
        ops,
    ) {
        Ok(r) => r,
        Err(eggserve_primitives::canonical::ResponseConstructionError::FileStreamLimit) => {
            crate::response::service_unavailable_with_policy(error_policy)
        }
        Err(_) => crate::response::internal_error_with_policy(error_policy),
    }
}

/// Contain panics raised while polling a service future.
///
/// On panic, the payload is converted into [`ServiceError::panic`] so the
/// connection produces a 500 response instead of being dropped.
///
/// Shared by the H1 pipeline and the experimental H3 adapter (Plan 220).
pub async fn contain_service_panic<F>(
    future: F,
) -> Result<eggserve_primitives::canonical::Response, ServiceError>
where
    F: std::future::Future<Output = Result<eggserve_primitives::canonical::Response, ServiceError>>,
{
    match futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(future)).await {
        Ok(result) => result,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "service panicked".to_string());
            Err(ServiceError::panic(message))
        }
    }
}

/// Invoke a canonical service with handler-timeout and panic containment.
///
/// Single shared kernel for direct H1 and the experimental H3 adapter
/// (Plan 220): `handler_timeout` bounds `Service::call`, panics become
/// `ServiceError::panic`, and a timeout while the request body is still
/// active observes `BodyReadTimeout` (otherwise `ServiceTimeout`).
/// H3/QUIC transport policy (Alt-Svc, QUIC windows) stays H3-owned.
pub async fn invoke_canonical_service<S>(
    service: &S,
    request: eggserve_primitives::request::Request,
    timeout: std::time::Duration,
    ops: &crate::ops::OpsContext,
) -> Result<eggserve_primitives::canonical::Response, ServiceError>
where
    S: crate::service::Service,
{
    let lifecycle = request.lifecycle_clone();
    match tokio::time::timeout(timeout, contain_service_panic(service.call(request))).await {
        Ok(result) => result,
        Err(_) if lifecycle.is_body_active() => {
            ops.counters()
                .body_read_timeouts
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ops.emit(crate::ops::Event::new(
                crate::ops::Severity::Warn,
                crate::ops::EventKind::BodyReadTimeout,
                "body read timeout",
            ));
            Err(ServiceError::timeout("body read timeout"))
        }
        Err(_) => {
            ops.emit(crate::ops::Event::new(
                crate::ops::Severity::Warn,
                crate::ops::EventKind::ServiceTimeout,
                "handler timed out",
            ));
            Err(ServiceError::timeout("handler timed out"))
        }
    }
}

/// Apply the final-boundary response privacy policy to a canonical response.
///
/// Generic authority shared by H1 and the experimental H3 adapter
/// (Plan 220): strips denylisted application headers, subordinates `Server`,
/// applies sole `Date` authority, and drops future `Last-Modified`.
/// H3 `Alt-Svc` advertisement stays H3-owned (applied by `eggserve-h3`
/// after this call) so `RuntimeConfig` never gains QUIC types.
pub fn finalize_canonical_response(
    mut response: eggserve_primitives::canonical::Response,
    config: &RuntimeConfig,
) -> eggserve_primitives::canonical::Response {
    let policy = &config.response_policy;
    let now = policy.date_policy.now();
    let last_modified = response
        .headers()
        .get_first("last-modified")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| httpdate::parse_http_date(value).ok());
    let future_last_modified = now.zip(last_modified).is_some_and(|(now, last)| last > now);

    {
        let headers = response.head_mut().headers_mut();
        headers.retain(|field| {
            let stripped = policy
                .stripped_response_headers
                .iter()
                .any(|name| name.eq_ignore_ascii_case(field.name.as_str()));
            let protected = field.name.as_str().eq_ignore_ascii_case("server")
                || field.name.as_str().eq_ignore_ascii_case("date")
                || (future_last_modified
                    && field.name.as_str().eq_ignore_ascii_case("last-modified"));
            !(stripped || protected)
        });
        if let Some(server) = &policy.server_identification {
            let _ = headers.push_str("server", server.clone());
        }
        if let Some(now) = now {
            let _ = headers.push_str("date", httpdate::fmt_http_date(now));
        }
    }
    response
}

/// Convert a RequestBodyError to an HTTP response.
///
/// Status selection lives here; representation is owned by
/// [`crate::response::runtime_error_with_policy`] so wire status and body
/// can never disagree (Plan 178 Track C). Transport consequences are returned
/// separately by [`body_error_disposition`].
pub(crate) fn body_error_to_response(
    err: eggserve_primitives::request_body_error::RequestBodyError,
    _head: &eggserve_primitives::request_head::RequestHead,
    policy: &H1ConnectionPolicy,
) -> hyper::Response<BoxBodyInner> {
    let raw_status = err.to_status_code();
    // Cancelled/disconnected reads report the non-standard 499, which has
    // no standard reason phrase and must not appear on the wire; collapse
    // to 500 but still close because the request ended mid-body.
    // Transport failures (raw_status 500) also end the request mid-body
    // with wire framing unknown, so they force close too; consumption-
    // state 500s are application bugs with no wire anomaly and stay alive.
    let wire_status = if raw_status == 499 { 500 } else { raw_status };
    let status = hyper::StatusCode::from_u16(wire_status)
        .unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR);
    let is_head = _head.method().is_head();
    let kind = if wire_status == 408 {
        crate::rejection::RuntimeRejectionKind::RequestBodyTimeout
    } else if wire_status == 413 {
        crate::rejection::RuntimeRejectionKind::RequestBodyTooLarge
    } else {
        crate::rejection::RuntimeRejectionKind::RequestBodyRejected
    };
    present_runtime_rejection(status, kind, is_head, policy)
}

/// Return the protocol-neutral lifecycle consequence of a body failure.
pub(crate) fn body_error_disposition(
    err: &eggserve_primitives::request_body_error::RequestBodyError,
) -> LifecycleDisposition {
    let raw_status = err.to_status_code();
    if raw_status == 499 || err.is_transport() || matches!(raw_status, 400 | 408 | 413 | 505) {
        LifecycleDisposition::close_and_cancel_body()
    } else {
        LifecycleDisposition::KEEP_ALIVE
    }
}

/// Convert a [`ServiceError`] into an HTTP response with an explicit policy.
///
/// Status selection lives on the error; representation is owned by
/// [`crate::response::runtime_error_with_policy`] so wire status and body
/// can never disagree. Internal and panic errors map to 500, timeouts to
/// 504, rejections to their status. No internal details are reflected.
pub(crate) fn service_error_to_response(
    err: &ServiceError,
    is_head: bool,
    policy: &H1ConnectionPolicy,
) -> hyper::Response<BoxBodyInner> {
    let code = err.status_code().as_u16();
    let status =
        hyper::StatusCode::from_u16(code).unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR);
    let kind = if code == 414 {
        crate::rejection::RuntimeRejectionKind::RequestTargetTooLong
    } else if code == 431 {
        crate::rejection::RuntimeRejectionKind::RequestHeadersTooLarge
    } else if err.is_timeout() {
        crate::rejection::RuntimeRejectionKind::HandlerTimeout
    } else if err.is_panic() {
        crate::rejection::RuntimeRejectionKind::ServicePanic
    } else {
        crate::rejection::RuntimeRejectionKind::ServiceRejected
    };
    present_runtime_rejection(status, kind, is_head, policy)
}

/// HTTP/1 adapter for protocol-neutral lifecycle dispositions.
pub(crate) fn apply_http1_disposition(
    mut response: hyper::Response<BoxBodyInner>,
    disposition: LifecycleDisposition,
) -> hyper::Response<BoxBodyInner> {
    if disposition.close_after_response_required() {
        response.headers_mut().insert(
            hyper::header::CONNECTION,
            hyper::header::HeaderValue::from_static("close"),
        );
    }
    response
}

/// Apply the final-boundary response privacy policy at the one Hyper boundary.
///
/// Order: strip denylisted application headers first (so applications cannot
/// re-add stripped identifiers), then apply `Server` identification (a fixed
/// value survives a `server` denylist entry as explicit operator intent),
/// then apply `Date` policy as the sole authority (Hyper automatic `Date` is
/// disabled in [`hyper_builder`]). Duplicates are all removed. Framing and
/// hop-by-hop headers are never stripped (rejected at validation).
/// `Last-Modified` later than `Date` is dropped to preserve the RFC
/// invariant. No transport peer metadata is copied into response headers.
/// Client responses never contain log or service error text (callers pass
/// only fixed generic bodies here).
pub(crate) fn finalize_runtime_response(
    mut response: hyper::Response<BoxBodyInner>,
    config: &H1ConnectionPolicy,
) -> hyper::Response<BoxBodyInner> {
    let policy = &config.response_policy;
    // 1. Denylist after service construction.
    for name in &policy.stripped_response_headers {
        // Validation guarantees these are not framing/hop-by-hop/date, so
        // removal cannot break framing invariants. `HeaderMap::remove`
        // removes all occurrences.
        response.headers_mut().remove(name.as_str());
    }
    // 2. Server identification: application values are always subordinate.
    response.headers_mut().remove(hyper::header::SERVER);
    if let Some(value) = &policy.server_identification {
        if let Ok(value) = hyper::header::HeaderValue::from_str(value) {
            response.headers_mut().insert(hyper::header::SERVER, value);
        }
    }
    // 3. Date: EggServe is the sole authority.
    response.headers_mut().remove(hyper::header::DATE);
    if let Some(now) = policy.date_policy.now() {
        // `now()` already guarantees formattability; the `from_str` guard
        // protects against a future formatting change.
        let date_str = httpdate::fmt_http_date(now);
        if let Ok(value) = hyper::header::HeaderValue::from_str(&date_str) {
            response.headers_mut().insert(hyper::header::DATE, value);
        }
    }
    // 4. Last-Modified must not be later than Date (RFC). When Date is
    // suppressed there is no reference to compare against, so retain
    // Last-Modified as-is; when both are present and Last-Modified is in
    // the future relative to Date, drop Last-Modified.
    if let (Some(date_val), Some(lm_val)) = (
        response.headers().get(hyper::header::DATE),
        response.headers().get(hyper::header::LAST_MODIFIED),
    ) {
        let date_ok = date_val
            .to_str()
            .ok()
            .and_then(|s| httpdate::parse_http_date(s).ok());
        let lm_ok = lm_val
            .to_str()
            .ok()
            .and_then(|s| httpdate::parse_http_date(s).ok());
        if let (Some(date_time), Some(lm_time)) = (date_ok, lm_ok) {
            if lm_time > date_time {
                response.headers_mut().remove(hyper::header::LAST_MODIFIED);
            }
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestPresenter;
    impl crate::rejection::RuntimeRejectionPresenter for TestPresenter {
        fn present(
            &self,
            _: &crate::rejection::RuntimeRejection,
        ) -> Option<crate::rejection::RuntimeErrorPresentation> {
            let mut headers = eggserve_primitives::HeaderBlock::new();
            headers.push_str("x-brand", "custom").unwrap();
            headers.push_str("connection", "close").unwrap();
            Some(crate::rejection::RuntimeErrorPresentation {
                headers,
                body: b"custom body".to_vec(),
            })
        }
    }

    #[test]
    fn presenter_changes_only_bounded_presentation_and_runtime_framing() {
        let config = RuntimeConfig::builder()
            .runtime_rejection_presenter(std::sync::Arc::new(TestPresenter))
            .build()
            .unwrap();
        let policy = config.h1_connection_policy().unwrap();
        let response = present_runtime_rejection(
            hyper::StatusCode::SERVICE_UNAVAILABLE,
            crate::rejection::RuntimeRejectionKind::ServiceAdmissionSaturated,
            false,
            &policy,
        );
        assert_eq!(response.status(), hyper::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()["x-brand"], "custom");
        assert!(response.headers().get(hyper::header::CONNECTION).is_none());
        assert_eq!(response.headers()[hyper::header::CONTENT_LENGTH], "11");
        let head = present_runtime_rejection(
            hyper::StatusCode::REQUEST_TIMEOUT,
            crate::rejection::RuntimeRejectionKind::HandlerTimeout,
            true,
            &policy,
        );
        assert_eq!(head.status(), hyper::StatusCode::REQUEST_TIMEOUT);
        assert_eq!(head.headers()[hyper::header::CONTENT_LENGTH], "11");
    }

    #[derive(Debug)]
    struct PanickingPresenter;
    impl crate::rejection::RuntimeRejectionPresenter for PanickingPresenter {
        fn present(
            &self,
            _: &crate::rejection::RuntimeRejection,
        ) -> Option<crate::rejection::RuntimeErrorPresentation> {
            panic!("private")
        }
    }

    #[test]
    fn presenter_panic_uses_generic_fallback() {
        let config = RuntimeConfig::builder()
            .runtime_rejection_presenter(std::sync::Arc::new(PanickingPresenter))
            .build()
            .unwrap();
        let response = present_runtime_rejection(
            hyper::StatusCode::GATEWAY_TIMEOUT,
            crate::rejection::RuntimeRejectionKind::HandlerTimeout,
            false,
            &config.h1_connection_policy().unwrap(),
        );
        assert_eq!(response.status(), hyper::StatusCode::GATEWAY_TIMEOUT);
    }

    #[test]
    fn runtime_server_header_replaces_service_value() {
        let config = RuntimeConfig::builder()
            .server_header("eggserve-test".into())
            .build()
            .unwrap();
        let mut response = crate::response::not_found(false);
        response.headers_mut().insert(
            hyper::header::SERVER,
            hyper::header::HeaderValue::from_static("spoofed"),
        );
        let response = finalize_runtime_response(response, &config.h1_connection_policy().unwrap());
        assert_eq!(
            response.headers().get(hyper::header::SERVER).unwrap(),
            "eggserve-test"
        );
        assert_eq!(
            response
                .headers()
                .get_all(hyper::header::SERVER)
                .iter()
                .count(),
            1
        );
    }

    #[test]
    fn body_error_transport_forces_connection_close() {
        fn head() -> eggserve_primitives::request_head::RequestHead {
            eggserve_primitives::request_head::RequestHead::new(
                eggserve_primitives::method::Method::get(),
                eggserve_primitives::request_target::RequestTarget::parse("/x").unwrap(),
                eggserve_primitives::version::HttpVersion::Http11,
                eggserve_primitives::header_block::HeaderBlock::new(),
            )
        }
        let policy = RuntimeConfig::default().h1_connection_policy().unwrap();

        // Transport failures (500) require a close disposition, but the
        // response itself remains free of HTTP/1-only headers.
        let transport = body_error_to_response(
            eggserve_primitives::request_body_error::RequestBodyError::Transport("io".into()),
            &head(),
            &policy,
        );
        assert_eq!(transport.status(), hyper::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(transport.headers().get(hyper::header::CONNECTION).is_none());
        assert!(body_error_disposition(
            &eggserve_primitives::request_body_error::RequestBodyError::Transport("io".into())
        )
        .close_after_response_required());

        // Application-state 500s have no wire anomaly and stay reusable.
        let consumed = body_error_to_response(
            eggserve_primitives::request_body_error::RequestBodyError::AlreadyConsumed,
            &head(),
            &policy,
        );
        assert_eq!(consumed.status(), hyper::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(consumed.headers().get(hyper::header::CONNECTION).is_none());

        // 499-collapsed disconnects still require close.
        let disconnected = body_error_to_response(
            eggserve_primitives::request_body_error::RequestBodyError::Disconnected,
            &head(),
            &policy,
        );
        assert!(disconnected
            .headers()
            .get(hyper::header::CONNECTION)
            .is_none());
        assert!(body_error_disposition(
            &eggserve_primitives::request_body_error::RequestBodyError::Disconnected
        )
        .close_after_response_required());
    }
}
