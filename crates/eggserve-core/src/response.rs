//! Final Hyper response helpers owned by the runtime boundary.

use bytes::Bytes;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use hyper::{Response, StatusCode};
use std::time::SystemTime;

pub type BoxBodyInner = UnsyncBoxBody<Bytes, std::io::Error>;

/// Add the origin server's single authoritative Date header.
///
/// This is the direct-primitives default (system clock). The server
/// connection pipeline overrides per [`crate::server::response_policy::DatePolicy`]
/// in `finalize_runtime_response`, which is the sole Date authority when a
/// `RuntimeConfig` is present (Hyper automatic Date is disabled).
pub(crate) fn finalize_origin_headers(response: &mut Response<BoxBodyInner>, now: SystemTime) {
    response.headers_mut().remove(hyper::header::DATE);
    if let Ok(value) = hyper::header::HeaderValue::from_str(&httpdate::fmt_http_date(now)) {
        response.headers_mut().insert(hyper::header::DATE, value);
    }
}

fn finalize(mut response: Response<BoxBodyInner>) -> Response<BoxBodyInner> {
    finalize_origin_headers(&mut response, SystemTime::now());
    response
}

#[allow(dead_code)]
pub(crate) fn canonical_error(
    status: StatusCode,
    body: &'static str,
    is_head: bool,
) -> Response<BoxBodyInner> {
    canonical_error_with_policy(
        status,
        body,
        is_head,
        crate::policy::ErrorRepresentationPolicy::Minimal,
    )
}

/// Canonical error with an explicit representation policy.
///
/// `Minimal` emits the fixed generic plain-text body; `Empty` emits no body
/// bytes (`Content-Length: 0`, no `Content-Type`) for runtime-generated
/// errors. `Allow` for 405 is retained under both variants. `HEAD`
/// suppression remains correct (no body bytes). Body-forbidden statuses
/// (1xx/204/205/304) never emit body bytes regardless of policy.
pub(crate) fn canonical_error_with_policy(
    status: StatusCode,
    body: &'static str,
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    canonical_error_owned_with_policy(status, body, is_head, policy)
}

/// Owned-body variant of [`canonical_error_with_policy`] for dynamically
/// derived error bodies (Plan 178 Track C). Behavior is identical; `body`
/// is copied into the transport body.
pub(crate) fn canonical_error_owned_with_policy(
    status: StatusCode,
    body: &str,
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    let code = crate::primitives::canonical::StatusCode::new(status.as_u16())
        .unwrap_or(crate::primitives::canonical::StatusCode::INTERNAL_SERVER_ERROR);
    // Body-forbidden statuses never carry a payload (RFC 9110); suppress
    // even when a caller supplies a representation. HEAD and Empty likewise
    // emit no bytes; only Minimal + payload-permitting + GET emits.
    let body_forbidden = !code.permits_payload_body();
    let effective_body: &str = match policy {
        crate::policy::ErrorRepresentationPolicy::Minimal => {
            if is_head || body_forbidden {
                ""
            } else {
                body
            }
        }
        crate::policy::ErrorRepresentationPolicy::Empty => "",
    };
    let mut headers = crate::primitives::header_block::HeaderBlock::new();
    // These headers are static, valid HTTP metadata; failure would indicate
    // an implementation change rather than a runtime input problem.
    // `Empty` omits Content-Type (no body emitted); `Allow` is retained.
    if policy == crate::policy::ErrorRepresentationPolicy::Minimal {
        headers
            .push_str("content-type", "text/plain; charset=utf-8")
            .expect("canonical error content type is valid");
    }
    if status == StatusCode::METHOD_NOT_ALLOWED {
        headers
            .push_str("allow", "GET, HEAD")
            .expect("canonical error Allow value is valid");
    }
    crate::primitives::canonical::normalize_metadata(
        code,
        &mut headers,
        effective_body.len() as u64,
    )
    // All current canonical error statuses permit a payload and therefore
    // cannot trigger the normalizer's body-forbidden metadata error.
    .expect("canonical error metadata is valid");
    let mut builder = Response::builder().status(status);
    for field in headers.iter() {
        let name = hyper::header::HeaderName::from_bytes(field.name.as_str().as_bytes())
            .map_err(|_| {
                crate::primitives::canonical::ResponseConstructionError::InvalidHeader(
                    crate::primitives::header_block::HeaderError::InvalidName,
                )
            })
            .expect("canonical error header name is valid");
        let value = hyper::header::HeaderValue::from_bytes(field.value.as_bytes())
            .map_err(|_| {
                crate::primitives::canonical::ResponseConstructionError::InvalidHeader(
                    crate::primitives::header_block::HeaderError::InvalidValue,
                )
            })
            .expect("canonical error header value is valid");
        builder = builder.header(name, value);
    }
    finalize(
        builder
            .body(full_body(effective_body))
            .expect("canonical error response headers and body are valid"),
    )
}

/// Hyper conversion wrapper for the transport-neutral runtime-error builder
/// (Plan 178 Track C, corrected by Plan 189).
///
/// The canonical module owns the status/reason/body representation. This
/// wrapper applies the existing Hyper body type and leaves status selection to
/// its callers; no application detail is reflected.
pub(crate) fn runtime_error_with_policy(
    status: StatusCode,
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    let canonical_status = crate::primitives::canonical::StatusCode::new(status.as_u16())
        .unwrap_or(crate::primitives::canonical::StatusCode::INTERNAL_SERVER_ERROR);
    let canonical =
        crate::primitives::canonical::runtime_error_with_policy(canonical_status, is_head, policy);
    let normalized = crate::primitives::canonical::normalize_response(
        canonical,
        &crate::primitives::canonical::NormalizeRequest::new(is_head),
    )
    .expect("canonical runtime error normalizes");
    crate::primitives::canonical::to_hyper_response(normalized)
        .expect("canonical runtime error converts to Hyper")
        .map(|body| body.boxed_unsync())
}

#[allow(dead_code)]
pub fn bad_request(is_head: bool) -> Response<BoxBodyInner> {
    canonical_error(StatusCode::BAD_REQUEST, "400 Bad Request\n", is_head)
}

pub fn bad_request_with_policy(
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    canonical_error_with_policy(
        StatusCode::BAD_REQUEST,
        "400 Bad Request\n",
        is_head,
        policy,
    )
}

#[allow(dead_code)]
pub fn payload_too_large(is_head: bool) -> Response<BoxBodyInner> {
    canonical_error(
        StatusCode::PAYLOAD_TOO_LARGE,
        "413 Payload Too Large\n",
        is_head,
    )
}

pub fn payload_too_large_with_policy(
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    canonical_error_with_policy(
        StatusCode::PAYLOAD_TOO_LARGE,
        "413 Payload Too Large\n",
        is_head,
        policy,
    )
}

#[allow(dead_code)]
pub fn internal_error() -> Response<BoxBodyInner> {
    canonical_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "500 Internal Server Error\n",
        false,
    )
}

pub fn internal_error_with_policy(
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    canonical_error_with_policy(
        StatusCode::INTERNAL_SERVER_ERROR,
        "500 Internal Server Error\n",
        false,
        policy,
    )
}

#[allow(dead_code)]
pub fn service_unavailable() -> Response<BoxBodyInner> {
    canonical_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "503 Service Unavailable\n",
        false,
    )
}

pub fn service_unavailable_with_policy(
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    canonical_error_with_policy(
        StatusCode::SERVICE_UNAVAILABLE,
        "503 Service Unavailable\n",
        false,
        policy,
    )
}

pub fn expectation_failed_with_policy(
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    canonical_error_with_policy(
        StatusCode::EXPECTATION_FAILED,
        "417 Expectation Failed\n",
        is_head,
        policy,
    )
}

#[cfg(test)]
pub fn not_found(is_head: bool) -> Response<BoxBodyInner> {
    canonical_error(StatusCode::NOT_FOUND, "404 Not Found\n", is_head)
}

fn full_body(s: &str) -> BoxBodyInner {
    Full::new(Bytes::copy_from_slice(s.as_bytes()))
        .map_err(|never| match never {})
        .boxed_unsync()
}
