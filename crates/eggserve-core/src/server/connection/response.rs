//! Canonical response construction: normalization, panic containment,
//! body-error mapping, and final-boundary privacy.
//!
//! The runtime is the only framing authority: every service response
//! converges on `normalize_then_convert` (idempotent `normalize_response`
//! then Hyper conversion). `finalize_runtime_response` applies the Plan 165
//! privacy boundary (denylist, `Server` subordination, sole `Date` authority,
//! `Last-Modified <= Date`) at the one Hyper boundary.

use crate::response::BoxBodyInner;
use crate::server::config::RuntimeConfig;
use crate::server::service::ServiceError;

use super::lifecycle::LifecycleDisposition;

/// Normalize a service response then convert to Hyper.
///
/// The runtime is the only framing authority: every service response
/// (static or custom, buffered or streaming) converges here. Normalization
/// is idempotent so eagerly normalized static responses are preserved
/// (HEAD equivalent-GET lengths, unknown-length omission). Conversion
/// failures become generic 500/503 without leaking details.
pub(crate) fn normalize_then_convert(
    canonical: crate::primitives::canonical::Response,
    is_head: bool,
    file_stream_semaphore: &std::sync::Arc<tokio::sync::Semaphore>,
    stream_chunk_size: usize,
    error_policy: crate::policy::ErrorRepresentationPolicy,
    ops: Option<&crate::ops::OpsContext>,
) -> hyper::Response<BoxBodyInner> {
    let normalized = match crate::primitives::canonical::normalize_response(
        canonical,
        &crate::primitives::canonical::NormalizeRequest::new(is_head),
    ) {
        Ok(r) => r,
        Err(_) => return crate::response::internal_error_with_policy(error_policy),
    };
    match crate::primitives::canonical::to_hyper_response_with_file_stream_semaphore_and_chunk_size(
        normalized,
        file_stream_semaphore,
        stream_chunk_size,
        ops,
    ) {
        Ok(r) => r,
        Err(crate::primitives::canonical::ResponseConstructionError::FileStreamLimit) => {
            crate::response::service_unavailable_with_policy(error_policy)
        }
        Err(_) => crate::response::internal_error_with_policy(error_policy),
    }
}

/// Contain panics raised while polling a service future.
///
/// On panic, the payload is converted into [`ServiceError::panic`] so the
/// connection produces a 500 response instead of being dropped.
pub(crate) async fn contain_service_panic<F>(
    future: F,
) -> Result<crate::primitives::canonical::Response, ServiceError>
where
    F: std::future::Future<Output = Result<crate::primitives::canonical::Response, ServiceError>>,
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

/// Convert a RequestBodyError to an HTTP response.
///
/// Status selection lives here; representation is owned by
/// [`crate::response::runtime_error_with_policy`] so wire status and body
/// can never disagree (Plan 178 Track C). Transport consequences are returned
/// separately by [`body_error_disposition`].
pub(crate) fn body_error_to_response(
    err: crate::primitives::request_body_error::RequestBodyError,
    _head: &crate::primitives::request_head::RequestHead,
    error_policy: crate::policy::ErrorRepresentationPolicy,
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
    crate::response::runtime_error_with_policy(status, is_head, error_policy)
}

/// Return the protocol-neutral lifecycle consequence of a body failure.
pub(crate) fn body_error_disposition(
    err: &crate::primitives::request_body_error::RequestBodyError,
) -> LifecycleDisposition {
    let raw_status = err.to_status_code();
    if raw_status == 499 || err.is_transport() || matches!(raw_status, 400 | 408 | 413 | 505) {
        LifecycleDisposition::close_and_cancel_body()
    } else {
        LifecycleDisposition::KEEP_ALIVE
    }
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
    config: &RuntimeConfig,
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
        let response = finalize_runtime_response(response, &config);
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
        fn head() -> crate::primitives::request_head::RequestHead {
            crate::primitives::request_head::RequestHead::new(
                crate::primitives::method::Method::get(),
                crate::primitives::request_target::RequestTarget::parse("/x").unwrap(),
                crate::primitives::version::HttpVersion::Http11,
                crate::primitives::header_block::HeaderBlock::new(),
            )
        }

        // Transport failures (500) require a close disposition, but the
        // response itself remains free of HTTP/1-only headers.
        let transport = body_error_to_response(
            crate::primitives::request_body_error::RequestBodyError::Transport("io".into()),
            &head(),
            crate::policy::ErrorRepresentationPolicy::Minimal,
        );
        assert_eq!(transport.status(), hyper::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(transport.headers().get(hyper::header::CONNECTION).is_none());
        assert!(body_error_disposition(
            &crate::primitives::request_body_error::RequestBodyError::Transport("io".into())
        )
        .close_after_response_required());

        // Application-state 500s have no wire anomaly and stay reusable.
        let consumed = body_error_to_response(
            crate::primitives::request_body_error::RequestBodyError::AlreadyConsumed,
            &head(),
            crate::policy::ErrorRepresentationPolicy::Minimal,
        );
        assert_eq!(consumed.status(), hyper::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(consumed.headers().get(hyper::header::CONNECTION).is_none());

        // 499-collapsed disconnects still require close.
        let disconnected = body_error_to_response(
            crate::primitives::request_body_error::RequestBodyError::Disconnected,
            &head(),
            crate::policy::ErrorRepresentationPolicy::Minimal,
        );
        assert!(disconnected
            .headers()
            .get(hyper::header::CONNECTION)
            .is_none());
        assert!(body_error_disposition(
            &crate::primitives::request_body_error::RequestBodyError::Disconnected
        )
        .close_after_response_required());
    }
}
